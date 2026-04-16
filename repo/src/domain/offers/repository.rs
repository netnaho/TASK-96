use uuid::Uuid;

use crate::shared::errors::AppError;

use super::models::{ApprovalStep, Offer};

pub trait OfferRepository: Send + Sync {
    fn find_by_id(&self, id: Uuid) -> Result<Option<Offer>, AppError>;
    fn create(&self, offer: &Offer) -> Result<Offer, AppError>;
    fn update(&self, offer: &Offer) -> Result<Offer, AppError>;
    fn list_for_candidate(&self, candidate_id: Uuid) -> Result<Vec<Offer>, AppError>;
}

pub trait ApprovalRepository: Send + Sync {
    fn find_steps_for_offer(&self, offer_id: Uuid) -> Result<Vec<ApprovalStep>, AppError>;
    fn create_step(&self, step: &ApprovalStep) -> Result<ApprovalStep, AppError>;
    fn update_step(&self, step: &ApprovalStep) -> Result<ApprovalStep, AppError>;
}
