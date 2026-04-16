use actix_web::{HttpResponse, ResponseError};
use diesel::result::Error as DieselError;
use serde::Serialize;

/// Unified application error type.
///
/// Each variant maps to a specific HTTP status and a stable `code` string
/// that clients can match on programmatically.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("validation failed")]
    Validation(Vec<FieldError>),

    #[error("authentication required")]
    AuthenticationRequired,

    #[error("forbidden")]
    Forbidden,

    #[error("{0} not found")]
    NotFound(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("rate limit exceeded")]
    RateLimited,

    #[error("invalid state transition: {0}")]
    InvalidStateTransition(String),

    #[error("idempotency conflict")]
    IdempotencyConflict,

    #[error("internal error: {0}")]
    Internal(String),
}

/// A single field-level validation error.
#[derive(Debug, Serialize, Clone)]
pub struct FieldError {
    pub field: String,
    pub message: String,
}

/// Stable JSON error envelope returned to clients.
///
/// ```json
/// {
///   "error": {
///     "code": "validation_error",
///     "message": "validation failed",
///     "details": [...]
///   }
/// }
/// ```
#[derive(Serialize)]
pub struct ErrorEnvelope {
    pub error: ErrorBody,
}

#[derive(Serialize)]
pub struct ErrorBody {
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl ResponseError for AppError {
    fn error_response(&self) -> HttpResponse {
        let (status, code, details) = match self {
            AppError::Validation(fields) => (
                actix_web::http::StatusCode::UNPROCESSABLE_ENTITY,
                "validation_error",
                Some(serde_json::to_value(fields).unwrap_or_default()),
            ),
            AppError::AuthenticationRequired => (
                actix_web::http::StatusCode::UNAUTHORIZED,
                "authentication_required",
                None,
            ),
            AppError::Forbidden => (actix_web::http::StatusCode::FORBIDDEN, "forbidden", None),
            AppError::NotFound(_) => (actix_web::http::StatusCode::NOT_FOUND, "not_found", None),
            AppError::Conflict(_) => (actix_web::http::StatusCode::CONFLICT, "conflict", None),
            AppError::RateLimited => (
                actix_web::http::StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                None,
            ),
            AppError::InvalidStateTransition(_) => (
                actix_web::http::StatusCode::CONFLICT,
                "invalid_state_transition",
                None,
            ),
            AppError::IdempotencyConflict => (
                actix_web::http::StatusCode::CONFLICT,
                "idempotency_conflict",
                None,
            ),
            AppError::Internal(_) => (
                actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                None,
            ),
        };

        let envelope = ErrorEnvelope {
            error: ErrorBody {
                code,
                message: self.to_string(),
                details,
            },
        };

        HttpResponse::build(status).json(envelope)
    }
}

/// Allow `?` inside Diesel `transaction()` closures that return `Result<_, AppError>`.
impl From<DieselError> for AppError {
    fn from(e: DieselError) -> Self {
        AppError::Internal(format!("database error: {e}"))
    }
}
