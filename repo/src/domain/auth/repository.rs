use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::shared::errors::AppError;

use super::models::Session;

/// Data-access contract for session management.
pub trait SessionRepository: Send + Sync {
    fn create_session(
        &self,
        user_id: Uuid,
        token_hash: &str,
        device_fingerprint: Option<&str>,
        ip_address: Option<&str>,
        expires_at: DateTime<Utc>,
    ) -> Result<Session, AppError>;

    fn find_by_token_hash(&self, token_hash: &str) -> Result<Option<Session>, AppError>;

    fn delete_session(&self, session_id: Uuid) -> Result<(), AppError>;

    fn delete_expired(&self) -> Result<u64, AppError>;

    fn count_active_for_user(&self, user_id: Uuid) -> Result<i64, AppError>;
}
