use uuid::Uuid;

use crate::shared::errors::AppError;

use super::models::{BookingOrder, OfficeSite};

pub trait BookingRepository: Send + Sync {
    fn find_order_by_id(&self, id: Uuid) -> Result<Option<BookingOrder>, AppError>;
    fn create_order(&self, order: &BookingOrder) -> Result<BookingOrder, AppError>;
    fn update_order(&self, order: &BookingOrder) -> Result<BookingOrder, AppError>;
    fn list_orders(&self, offset: i64, limit: i64) -> Result<(Vec<BookingOrder>, i64), AppError>;
}

pub trait SiteRepository: Send + Sync {
    fn find_by_id(&self, id: Uuid) -> Result<Option<OfficeSite>, AppError>;
    fn find_by_code(&self, code: &str) -> Result<Option<OfficeSite>, AppError>;
    fn list_active(&self) -> Result<Vec<OfficeSite>, AppError>;
}
