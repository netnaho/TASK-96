use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Structured compensation stored as AES-256-GCM encrypted JSON in `compensation_encrypted`.
/// All monetary values are in USD cents to avoid floating-point precision issues.
/// Range constraints are enforced at the service layer before encryption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompensationData {
    /// Annual base salary in USD (whole dollars). Must be > 0.
    pub base_salary_usd: u64,
    /// Target bonus as a percentage of base salary (0.0–100.0).
    pub bonus_target_pct: f64,
    /// Number of equity units (RSUs/options). 0 if no equity.
    pub equity_units: u32,
    /// Paid time off in days per year. Must be in range 0–365.
    pub pto_days: u16,
    /// 401(k) employer match as a percentage (0.0–100.0).
    pub k401_match_pct: f64,
}

impl CompensationData {
    /// Validates all range constraints. Returns a list of field-level error messages.
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.base_salary_usd == 0 {
            errors.push("base_salary_usd must be greater than 0".to_string());
        }
        if !(0.0..=100.0).contains(&self.bonus_target_pct) {
            errors.push("bonus_target_pct must be between 0 and 100".to_string());
        }
        if self.pto_days > 365 {
            errors.push("pto_days must be between 0 and 365".to_string());
        }
        if !(0.0..=100.0).contains(&self.k401_match_pct) {
            errors.push("k401_match_pct must be between 0 and 100".to_string());
        }
        errors
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum OfferStatus {
    Draft,
    PendingApproval,
    Approved,
    Sent,
    Accepted,
    Declined,
    Withdrawn,
    Expired,
}

impl OfferStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::PendingApproval => "pending_approval",
            Self::Approved => "approved",
            Self::Sent => "sent",
            Self::Accepted => "accepted",
            Self::Declined => "declined",
            Self::Withdrawn => "withdrawn",
            Self::Expired => "expired",
        }
    }

    /// Returns the set of statuses this status can transition to.
    pub fn valid_transitions(&self) -> &[OfferStatus] {
        match self {
            Self::Draft => &[Self::PendingApproval, Self::Withdrawn],
            Self::PendingApproval => &[Self::Approved, Self::Withdrawn],
            Self::Approved => &[Self::Sent, Self::Withdrawn],
            Self::Sent => &[
                Self::Accepted,
                Self::Declined,
                Self::Withdrawn,
                Self::Expired,
            ],
            Self::Accepted | Self::Declined | Self::Withdrawn | Self::Expired => &[],
        }
    }

    pub fn can_transition_to(&self, target: OfferStatus) -> bool {
        self.valid_transitions().contains(&target)
    }
}

#[derive(Debug, Clone)]
pub struct Offer {
    pub id: Uuid,
    pub candidate_id: Uuid,
    pub title: String,
    pub department: Option<String>,
    pub compensation_encrypted: Option<Vec<u8>>,
    pub start_date: Option<NaiveDate>,
    pub status: OfferStatus,
    pub expires_at: Option<DateTime<Utc>>,
    /// Links this offer to a template for lineage tracking.
    pub template_id: Option<Uuid>,
    /// Records the version of the offer template clauses used.
    pub clause_version: Option<String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    // -- Structured compensation fields (queryable, dual-written with blob) --
    pub salary_cents: Option<i64>,
    pub bonus_target_pct: Option<f64>,
    pub equity_units: Option<i32>,
    pub pto_days: Option<i16>,
    pub k401_match_pct: Option<f64>,
    pub currency: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ApprovalDecision {
    Pending,
    Approved,
    Rejected,
    Escalated,
}

impl ApprovalDecision {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Escalated => "escalated",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "approved" => Some(Self::Approved),
            "rejected" => Some(Self::Rejected),
            "escalated" => Some(Self::Escalated),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ApprovalStep {
    pub id: Uuid,
    pub offer_id: Uuid,
    pub step_order: i32,
    pub approver_id: Uuid,
    pub decision: ApprovalDecision,
    pub decided_at: Option<DateTime<Utc>>,
    pub comments: Option<String>,
    pub created_at: DateTime<Utc>,
}
