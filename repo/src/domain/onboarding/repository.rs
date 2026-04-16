use uuid::Uuid;

use crate::shared::errors::AppError;

use super::models::{OnboardingChecklist, OnboardingItem};

pub trait OnboardingRepository: Send + Sync {
    fn find_checklist(&self, id: Uuid) -> Result<Option<OnboardingChecklist>, AppError>;
    fn create_checklist(
        &self,
        checklist: &OnboardingChecklist,
    ) -> Result<OnboardingChecklist, AppError>;
    fn find_items_for_checklist(&self, checklist_id: Uuid)
        -> Result<Vec<OnboardingItem>, AppError>;
    fn create_item(&self, item: &OnboardingItem) -> Result<OnboardingItem, AppError>;
    fn update_item(&self, item: &OnboardingItem) -> Result<OnboardingItem, AppError>;
}
