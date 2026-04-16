use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ReportingSubscription {
    pub id: Uuid,
    pub user_id: Uuid,
    pub report_type: String,
    pub parameters: serde_json::Value,
    pub cron_expression: Option<String>,
    pub is_active: bool,
    pub last_run_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DashboardVersion {
    pub id: Uuid,
    pub dashboard_key: String,
    pub version: i32,
    pub layout: serde_json::Value,
    pub published_by: Uuid,
    pub published_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SeverityLevel {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone)]
pub struct ReportingAlert {
    pub id: Uuid,
    pub subscription_id: Uuid,
    pub severity: SeverityLevel,
    pub message: String,
    pub acknowledged: bool,
    pub acknowledged_by: Option<Uuid>,
    pub acknowledged_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}
