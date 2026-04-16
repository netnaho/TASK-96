use serde::Serialize;

/// Standard success envelope for all API responses.
///
/// ```json
/// {
///   "data": { ... },
///   "meta": { "request_id": "..." }
/// }
/// ```
///
/// List responses use [`PaginatedEnvelope`] instead.
#[derive(Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub data: T,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<ResponseMeta>,
}

#[derive(Serialize)]
pub struct ResponseMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

/// Paginated list envelope.
///
/// ```json
/// {
///   "data": [...],
///   "pagination": { "page": 1, "per_page": 25, "total": 100 },
///   "meta": { "request_id": "..." }
/// }
/// ```
#[derive(Serialize)]
pub struct PaginatedEnvelope<T: Serialize> {
    pub data: Vec<T>,
    pub pagination: PaginationMeta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<ResponseMeta>,
}

#[derive(Serialize)]
pub struct PaginationMeta {
    pub page: u32,
    pub per_page: u32,
    pub total: i64,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn ok(data: T) -> Self {
        Self { data, meta: None }
    }
}
