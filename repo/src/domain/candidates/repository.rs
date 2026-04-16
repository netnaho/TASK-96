use uuid::Uuid;

use crate::shared::errors::AppError;

use super::models::Candidate;

pub trait CandidateRepository: Send + Sync {
    fn find_by_id(&self, id: Uuid) -> Result<Option<Candidate>, AppError>;
    fn create(&self, candidate: &Candidate) -> Result<Candidate, AppError>;
    fn update(&self, candidate: &Candidate) -> Result<Candidate, AppError>;
    fn list(&self, offset: i64, limit: i64) -> Result<(Vec<Candidate>, i64), AppError>;
}
