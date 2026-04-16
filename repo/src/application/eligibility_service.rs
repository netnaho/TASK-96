/// Composable pre-booking eligibility checks.
///
/// Each check is an explicit, individually-testable function.
/// `run_all` composes them into an `EligibilityResult` with per-check detail.
///
/// Required before transitioning PendingConfirmation → Confirmed:
/// 1. Onboarding checklist 100% complete
/// 2. All required documents present (items with requires_upload must have upload_storage_key)
/// 3. Health/eligibility attestation signed within last 30 days
/// 4. No active booking restrictions (e.g. pending background check)
/// 5. Electronic agreement confirmation exists (non-boolean evidence)
use chrono::{DateTime, Duration, Utc};
use diesel::PgConnection;
use uuid::Uuid;

use crate::{
    domain::bookings::models::{
        AgreementEvidence, BookingOrder, EligibilityCheck, EligibilityResult,
    },
    infrastructure::db::{
        models::{DbBookingOrder, DbBookingRestriction, DbOnboardingChecklist, DbOnboardingItem},
        repositories::{
            onboarding_repo::PgOnboardingRepository, restriction_repo::PgRestrictionRepository,
        },
    },
    shared::errors::AppError,
};

/// Maximum age for a health attestation to be considered current.
const ATTESTATION_MAX_AGE_DAYS: i64 = 30;

pub struct EligibilityService;

impl EligibilityService {
    /// Run all eligibility checks for a booking. Returns a structured result with
    /// per-check pass/fail and detail messages.
    pub fn run_all(
        conn: &mut PgConnection,
        booking: &DbBookingOrder,
        now: DateTime<Utc>,
    ) -> Result<EligibilityResult, AppError> {
        let mut checks = Vec::with_capacity(5);

        checks.push(Self::check_onboarding_complete(conn, booking.candidate_id)?);
        checks.push(Self::check_required_documents(conn, booking.candidate_id)?);
        checks.push(Self::check_health_attestation(
            conn,
            booking.candidate_id,
            now,
        )?);
        checks.push(Self::check_no_restrictions(
            conn,
            booking.candidate_id,
            now,
        )?);
        checks.push(Self::check_agreement_confirmed(booking));

        Ok(EligibilityResult::from_checks(checks))
    }

    /// Check 1: Onboarding checklist readiness must be 100%.
    pub fn check_onboarding_complete(
        conn: &mut PgConnection,
        candidate_id: Uuid,
    ) -> Result<EligibilityCheck, AppError> {
        // Find all checklists for this candidate
        let (checklists, _) =
            PgOnboardingRepository::list_checklists(conn, Some(candidate_id), None, 0, 100)?;

        if checklists.is_empty() {
            return Ok(EligibilityCheck {
                name: "onboarding_complete",
                passed: false,
                detail: "no onboarding checklist found for candidate".into(),
            });
        }

        // Check all checklists — every required item must be completed
        for checklist in &checklists {
            let items = PgOnboardingRepository::find_items_for_checklist(conn, checklist.id)?;
            let total_required = items.iter().filter(|i| i.required).count();
            let completed_required = items
                .iter()
                .filter(|i| i.required && i.status == "completed")
                .count();

            if total_required > 0 && completed_required < total_required {
                return Ok(EligibilityCheck {
                    name: "onboarding_complete",
                    passed: false,
                    detail: format!(
                        "checklist {} has {}/{} required items completed",
                        checklist.id, completed_required, total_required
                    ),
                });
            }
        }

        Ok(EligibilityCheck {
            name: "onboarding_complete",
            passed: true,
            detail: "all onboarding checklists complete".into(),
        })
    }

    /// Check 2: All items that require an upload must have an upload_storage_key.
    pub fn check_required_documents(
        conn: &mut PgConnection,
        candidate_id: Uuid,
    ) -> Result<EligibilityCheck, AppError> {
        let (checklists, _) =
            PgOnboardingRepository::list_checklists(conn, Some(candidate_id), None, 0, 100)?;

        for checklist in &checklists {
            let items = PgOnboardingRepository::find_items_for_checklist(conn, checklist.id)?;
            let missing: Vec<&DbOnboardingItem> = items
                .iter()
                .filter(|i| i.requires_upload && i.upload_storage_key.is_none())
                .collect();

            if !missing.is_empty() {
                let names: Vec<&str> = missing.iter().map(|i| i.title.as_str()).collect();
                return Ok(EligibilityCheck {
                    name: "required_documents",
                    passed: false,
                    detail: format!("missing uploads: {}", names.join(", ")),
                });
            }
        }

        Ok(EligibilityCheck {
            name: "required_documents",
            passed: true,
            detail: "all required documents uploaded".into(),
        })
    }

    /// Check 3: Health/eligibility attestation signed within the last 30 days.
    /// An attestation is an onboarding item with a non-null health_attestation_encrypted
    /// and completed_at within the attestation window.
    pub fn check_health_attestation(
        conn: &mut PgConnection,
        candidate_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<EligibilityCheck, AppError> {
        let (checklists, _) =
            PgOnboardingRepository::list_checklists(conn, Some(candidate_id), None, 0, 100)?;

        let cutoff = now - Duration::days(ATTESTATION_MAX_AGE_DAYS);

        for checklist in &checklists {
            let items = PgOnboardingRepository::find_items_for_checklist(conn, checklist.id)?;
            for item in &items {
                if item.health_attestation_encrypted.is_some() {
                    // Every branch returns — no fallthrough needed.
                    return Ok(match item.completed_at {
                        Some(completed) if completed >= cutoff => EligibilityCheck {
                            name: "health_attestation",
                            passed: true,
                            detail: format!(
                                "attestation '{}' completed {} (within 30 days)",
                                item.title,
                                completed.format("%Y-%m-%d")
                            ),
                        },
                        Some(completed) => EligibilityCheck {
                            name: "health_attestation",
                            passed: false,
                            detail: format!(
                                "attestation '{}' completed {} (older than 30 days)",
                                item.title,
                                completed.format("%Y-%m-%d")
                            ),
                        },
                        None => EligibilityCheck {
                            name: "health_attestation",
                            passed: false,
                            detail: format!(
                                "attestation '{}' has not been completed",
                                item.title
                            ),
                        },
                    });
                }
            }
        }

        Ok(EligibilityCheck {
            name: "health_attestation",
            passed: false,
            detail: "no health attestation item found for candidate".into(),
        })
    }

    /// Check 4: No active booking restrictions block the candidate.
    pub fn check_no_restrictions(
        conn: &mut PgConnection,
        candidate_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<EligibilityCheck, AppError> {
        let restrictions =
            PgRestrictionRepository::find_active_for_candidate(conn, candidate_id, now)?;

        if restrictions.is_empty() {
            return Ok(EligibilityCheck {
                name: "no_restrictions",
                passed: true,
                detail: "no active restrictions".into(),
            });
        }

        let types: Vec<&str> = restrictions
            .iter()
            .map(|r| r.restriction_type.as_str())
            .collect();
        Ok(EligibilityCheck {
            name: "no_restrictions",
            passed: false,
            detail: format!("active restrictions: {}", types.join(", ")),
        })
    }

    /// Check 5: Electronic agreement confirmation exists with proper evidence
    /// (typed name + timestamp + hash — not a bare boolean).
    pub fn check_agreement_confirmed(booking: &DbBookingOrder) -> EligibilityCheck {
        let has_signed_by = booking
            .agreement_signed_by
            .as_ref()
            .map_or(false, |s| !s.is_empty());
        let has_signed_at = booking.agreement_signed_at.is_some();
        let has_hash = booking
            .agreement_hash
            .as_ref()
            .map_or(false, |s| !s.is_empty());

        if has_signed_by && has_signed_at && has_hash {
            EligibilityCheck {
                name: "agreement_confirmed",
                passed: true,
                detail: format!(
                    "agreement signed by '{}' at {}",
                    booking.agreement_signed_by.as_deref().unwrap_or(""),
                    booking
                        .agreement_signed_at
                        .map(|t| t.to_rfc3339())
                        .unwrap_or_default()
                ),
            }
        } else {
            let mut missing = Vec::new();
            if !has_signed_by {
                missing.push("typed_name");
            }
            if !has_signed_at {
                missing.push("signed_at");
            }
            if !has_hash {
                missing.push("hash");
            }
            EligibilityCheck {
                name: "agreement_confirmed",
                passed: false,
                detail: format!(
                    "agreement evidence incomplete: missing {}",
                    missing.join(", ")
                ),
            }
        }
    }
}
