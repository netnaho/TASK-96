use uuid::Uuid;

use crate::shared::errors::AppError;

use super::models::User;

/// Data-access contract for user management.
pub trait UserRepository: Send + Sync {
    fn find_by_id(&self, id: Uuid) -> Result<Option<User>, AppError>;
    fn find_by_username(&self, username: &str) -> Result<Option<User>, AppError>;
    fn create(&self, user: &User) -> Result<User, AppError>;
    fn update(&self, user: &User) -> Result<User, AppError>;
    fn increment_failed_logins(&self, id: Uuid) -> Result<(), AppError>;
    fn reset_failed_logins(&self, id: Uuid) -> Result<(), AppError>;
}
