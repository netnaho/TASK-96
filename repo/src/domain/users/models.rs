use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum AccountStatus {
    Active,
    Locked,
    Suspended,
    Deactivated,
}

impl AccountStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Locked => "locked",
            Self::Suspended => "suspended",
            Self::Deactivated => "deactivated",
        }
    }
}

/// Core user domain model.
#[derive(Debug, Clone)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub display_name: String,
    pub account_status: AccountStatus,
    pub failed_login_count: i32,
    pub locked_until: Option<DateTime<Utc>>,
    pub last_login_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Role assignment with optional scope narrowing.
#[derive(Debug, Clone, Serialize)]
pub struct UserRole {
    pub user_id: Uuid,
    pub role_id: Uuid,
    pub role_name: String,
    pub scope_type: Option<String>,
    pub scope_id: Option<Uuid>,
}

/// Permission tuple: (resource, action).
#[derive(Debug, Clone, Serialize)]
pub struct Permission {
    pub id: Uuid,
    pub resource: String,
    pub action: String,
}
