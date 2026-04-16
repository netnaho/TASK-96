use serde::Deserialize;
use validator::Validate;

/// Query parameters for paginated list endpoints.
#[derive(Debug, Deserialize, Validate)]
pub struct PaginationParams {
    #[validate(range(min = 1))]
    #[serde(default = "default_page")]
    pub page: u32,

    #[validate(range(min = 1, max = 100))]
    #[serde(default = "default_per_page")]
    pub per_page: u32,
}

fn default_page() -> u32 {
    1
}

fn default_per_page() -> u32 {
    25
}

impl PaginationParams {
    pub fn offset(&self) -> i64 {
        ((self.page - 1) * self.per_page) as i64
    }

    pub fn limit(&self) -> i64 {
        self.per_page as i64
    }
}

/// Clamp a raw per_page value to the safe range [1, 100].
/// Use this in handlers that don't go through PaginationParams.
pub fn clamp_per_page(raw: u32) -> i64 {
    raw.clamp(1, 100) as i64
}
