use chrono::{DateTime, Duration, NaiveDate, NaiveTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

// ============================================================
// Booking Status — state machine
// ============================================================

/// Booking lifecycle states.
///
/// ```text
/// PendingConfirmation → Confirmed       (eligibility gate passes)
/// PendingConfirmation → Canceled        (hold expires or user cancels)
/// Confirmed           → InProgress      (start time reached)
/// Confirmed           → Canceled        (user cancels; breach rules apply)
/// Confirmed           → Exception       (unexpected issue)
/// InProgress          → Completed       (normal finish)
/// InProgress          → Exception       (unexpected issue)
/// Exception           → Completed       (resolved)
/// Exception           → Canceled        (unresolvable)
/// Completed / Canceled                  → (terminal)
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BookingStatus {
    PendingConfirmation,
    Confirmed,
    InProgress,
    Completed,
    Canceled,
    Exception,
}

impl BookingStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PendingConfirmation => "pending_confirmation",
            Self::Confirmed => "confirmed",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Canceled => "cancelled",
            Self::Exception => "exception",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending_confirmation" | "draft" => Some(Self::PendingConfirmation),
            "confirmed" => Some(Self::Confirmed),
            "in_progress" => Some(Self::InProgress),
            "completed" => Some(Self::Completed),
            "cancelled" | "canceled" => Some(Self::Canceled),
            "exception" => Some(Self::Exception),
            _ => None,
        }
    }

    pub fn valid_transitions(&self) -> &[BookingStatus] {
        match self {
            Self::PendingConfirmation => &[Self::Confirmed, Self::Canceled],
            Self::Confirmed => &[Self::InProgress, Self::Canceled, Self::Exception],
            Self::InProgress => &[Self::Completed, Self::Exception],
            Self::Exception => &[Self::Completed, Self::Canceled],
            Self::Completed | Self::Canceled => &[],
        }
    }

    pub fn can_transition_to(&self, target: BookingStatus) -> bool {
        self.valid_transitions().contains(&target)
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Canceled)
    }
}

// ============================================================
// Inventory Slot
// ============================================================

#[derive(Debug, Clone)]
pub struct InventorySlot {
    pub id: Uuid,
    pub site_id: Uuid,
    pub slot_date: NaiveDate,
    pub start_time: NaiveTime,
    pub end_time: NaiveTime,
    pub capacity: i32,
    pub booked_count: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl InventorySlot {
    pub fn available_capacity(&self) -> i32 {
        self.capacity - self.booked_count
    }

    pub fn is_available(&self) -> bool {
        self.available_capacity() > 0
    }
}

// ============================================================
// Booking Order
// ============================================================

/// Hold expiration window: 15 minutes from creation.
pub const HOLD_DURATION_MINUTES: i64 = 15;

/// Reschedule/cancel cutoff: 24 hours before the booking's start time.
pub const CANCEL_CUTOFF_HOURS: i64 = 24;

#[derive(Debug, Clone)]
pub struct BookingOrder {
    pub id: Uuid,
    pub candidate_id: Uuid,
    pub site_id: Uuid,
    pub status: BookingStatus,
    pub scheduled_date: NaiveDate,
    pub scheduled_time_start: Option<NaiveTime>,
    pub scheduled_time_end: Option<NaiveTime>,
    pub notes: Option<String>,
    pub slot_id: Option<Uuid>,
    pub hold_expires_at: Option<DateTime<Utc>>,
    pub agreement_evidence: Option<AgreementEvidence>,
    pub breach_reason: Option<String>,
    pub breach_reason_code: Option<String>,
    pub exception_detail: Option<String>,
    pub idempotency_key: Option<String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl BookingOrder {
    /// Returns the booking start as a full UTC datetime for 24-hour rule checks.
    /// Falls back to midnight on scheduled_date if no time is set.
    pub fn start_datetime_utc(&self) -> DateTime<Utc> {
        let time = self
            .scheduled_time_start
            .unwrap_or(NaiveTime::from_hms_opt(0, 0, 0).unwrap());
        self.scheduled_date.and_time(time).and_utc()
    }

    /// True if `now` is within 24 hours of the booking start.
    pub fn is_within_cancel_cutoff(&self, now: DateTime<Utc>) -> bool {
        let start = self.start_datetime_utc();
        let cutoff = start - Duration::hours(CANCEL_CUTOFF_HOURS);
        now >= cutoff
    }

    /// True if the hold has expired.
    pub fn is_hold_expired(&self, now: DateTime<Utc>) -> bool {
        match self.hold_expires_at {
            Some(expires) => now >= expires,
            None => false,
        }
    }
}

// ============================================================
// Office Site
// ============================================================

#[derive(Debug, Clone)]
pub struct OfficeSite {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub address: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub timezone: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

// ============================================================
// Agreement Evidence
// ============================================================

/// Electronic agreement confirmation evidence.
///
/// A bare boolean is NOT sufficient — we capture who signed, when, and a SHA-256
/// hash that binds the signer's name, timestamp, candidate, and booking together.
///
/// ## Hash composition
///
/// ```text
/// SHA-256( "{typed_name}:{signed_at_rfc3339}:{candidate_id}:{booking_id}" )
/// ```
///
/// The hash proves that these four values were present at signing time.
/// It does not contain secrets — the inputs are non-sensitive identifiers and the
/// signer's typed name. The hash is stored for tamper-evidence, not encryption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgreementEvidence {
    /// Full legal name typed by the signer.
    pub typed_name: String,
    /// Timestamp when the agreement was signed.
    pub signed_at: DateTime<Utc>,
    /// SHA-256 hex digest binding (typed_name, signed_at, candidate_id, booking_id).
    pub hash: String,
}

impl AgreementEvidence {
    /// Compute a new agreement evidence hash.
    pub fn new(
        typed_name: String,
        signed_at: DateTime<Utc>,
        candidate_id: Uuid,
        booking_id: Uuid,
    ) -> Self {
        let hash = Self::compute_hash(&typed_name, signed_at, candidate_id, booking_id);
        Self {
            typed_name,
            signed_at,
            hash,
        }
    }

    /// Recompute the hash to verify integrity.
    pub fn verify(&self, candidate_id: Uuid, booking_id: Uuid) -> bool {
        let expected =
            Self::compute_hash(&self.typed_name, self.signed_at, candidate_id, booking_id);
        self.hash == expected
    }

    fn compute_hash(
        typed_name: &str,
        signed_at: DateTime<Utc>,
        candidate_id: Uuid,
        booking_id: Uuid,
    ) -> String {
        let input = format!(
            "{}:{}:{}:{}",
            typed_name,
            signed_at.to_rfc3339(),
            candidate_id,
            booking_id,
        );
        let digest = Sha256::digest(input.as_bytes());
        hex::encode(digest)
    }

    pub fn is_present(&self) -> bool {
        !self.typed_name.is_empty() && !self.hash.is_empty()
    }
}

// ============================================================
// Booking Restriction
// ============================================================

#[derive(Debug, Clone)]
pub struct BookingRestriction {
    pub id: Uuid,
    pub candidate_id: Uuid,
    pub restriction_type: String,
    pub reason: Option<String>,
    pub is_active: bool,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl BookingRestriction {
    /// A restriction is blocking if it is active and has not expired.
    pub fn is_blocking(&self, now: DateTime<Utc>) -> bool {
        if !self.is_active {
            return false;
        }
        match self.expires_at {
            Some(exp) => now < exp,
            None => true,
        }
    }
}

// ============================================================
// Eligibility Check — composable pre-booking validation
// ============================================================

/// Individual eligibility check result.
#[derive(Debug, Clone, Serialize)]
pub struct EligibilityCheck {
    pub name: &'static str,
    pub passed: bool,
    pub detail: String,
}

/// Aggregated eligibility result.
#[derive(Debug, Clone, Serialize)]
pub struct EligibilityResult {
    pub eligible: bool,
    pub checks: Vec<EligibilityCheck>,
}

impl EligibilityResult {
    pub fn from_checks(checks: Vec<EligibilityCheck>) -> Self {
        let eligible = checks.iter().all(|c| c.passed);
        Self { eligible, checks }
    }

    pub fn failed_checks(&self) -> Vec<&EligibilityCheck> {
        self.checks.iter().filter(|c| !c.passed).collect()
    }
}

/// Standard breach reason codes for late cancellations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BreachReasonCode {
    LateCancellation,
    NoShow,
    PolicyViolation,
    Other,
}

impl BreachReasonCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LateCancellation => "late_cancellation",
            Self::NoShow => "no_show",
            Self::PolicyViolation => "policy_violation",
            Self::Other => "other",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "late_cancellation" => Some(Self::LateCancellation),
            "no_show" => Some(Self::NoShow),
            "policy_violation" => Some(Self::PolicyViolation),
            "other" => Some(Self::Other),
            _ => None,
        }
    }
}
