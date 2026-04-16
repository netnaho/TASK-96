use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum OnboardingItemStatus {
    NotStarted,
    InProgress,
    Completed,
    Blocked,
    Skipped,
}

impl OnboardingItemStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NotStarted => "not_started",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Blocked => "blocked",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Debug, Clone)]
pub struct OnboardingChecklist {
    pub id: Uuid,
    pub offer_id: Uuid,
    pub candidate_id: Uuid,
    pub assigned_to: Option<Uuid>,
    pub due_date: Option<NaiveDate>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct OnboardingItem {
    pub id: Uuid,
    pub checklist_id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub item_order: i32,
    pub status: OnboardingItemStatus,
    pub requires_upload: bool,
    pub upload_storage_key: Option<String>,
    pub health_attestation_encrypted: Option<Vec<u8>>,
    /// When true, this item is counted in the readiness_pct denominator.
    pub required: bool,
    /// Optional per-item deadline within a checklist.
    pub item_due_date: Option<NaiveDate>,
    pub completed_at: Option<DateTime<Utc>>,
    pub completed_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Readiness summary for a checklist.
/// `readiness_pct` = (required_completed / total_required) × 100, or 100 when total_required = 0.
#[derive(Debug, Serialize)]
pub struct ReadinessReport {
    pub checklist_id: Uuid,
    pub total_required: u32,
    pub required_completed: u32,
    pub readiness_pct: u8,
}

impl ReadinessReport {
    pub fn compute(checklist_id: Uuid, items: &[OnboardingItem]) -> Self {
        let total_required = items.iter().filter(|i| i.required).count() as u32;
        let required_completed = items
            .iter()
            .filter(|i| i.required && i.status == OnboardingItemStatus::Completed)
            .count() as u32;
        let readiness_pct = if total_required == 0 {
            100
        } else {
            ((required_completed as f64 / total_required as f64) * 100.0).floor() as u8
        };
        Self {
            checklist_id,
            total_required,
            required_completed,
            readiness_pct,
        }
    }
}
