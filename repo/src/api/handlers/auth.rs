/// Authentication handlers: login, logout, current-session, captcha.
///
/// ## Request/Response DTOs
///
/// All request bodies are validated before reaching the service layer.
/// Passwords are zeroized after use (via `Drop` impl on `LoginRequest`).
///
/// ## Rate limiting on auth endpoints
///
/// Login and captcha endpoints are subject to an additional per-IP limit
/// of 10 req/min (enforced here, before any DB access).  The global IP
/// limit of 300 req/min is enforced by the outer middleware.
use actix_web::{web, HttpResponse};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{
    api::extractors::{AuthRequired, ClientIp},
    application::auth_service::{AuthService, LoginInput},
    infrastructure::{captcha, ratelimit},
    shared::{
        app_state::AppState,
        errors::{AppError, FieldError},
        response::ApiResponse,
    },
};

// ============================================================
// DTOs
// ============================================================

#[derive(Debug, Deserialize, Validate)]
pub struct LoginRequest {
    #[validate(length(min = 1, max = 128, message = "username is required"))]
    pub username: String,
    #[validate(length(min = 1, message = "password is required"))]
    pub password: String,
    #[validate(length(max = 512))]
    pub device_fingerprint: Option<String>,
    /// CAPTCHA challenge token issued by GET /auth/captcha.
    pub captcha_token: Option<String>,
    /// Client's answer to the CAPTCHA arithmetic question.
    pub captcha_answer: Option<u32>,
}

// Zeroize password after use to limit plaintext lifetime in memory.
impl Drop for LoginRequest {
    fn drop(&mut self) {
        use std::ptr;
        for b in unsafe { self.password.as_bytes_mut() } {
            unsafe { ptr::write_volatile(b, 0) };
        }
    }
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    /// Bearer token the client must include in subsequent `Authorization` headers.
    pub token: String,
    pub expires_at: DateTime<Utc>,
    pub user: AuthUserSummary,
}

#[derive(Debug, Serialize)]
pub struct AuthUserSummary {
    pub id: Uuid,
    pub username: String,
    pub roles: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SessionResponse {
    pub session_id: Uuid,
    pub user_id: Uuid,
    pub username: String,
    pub expires_at: DateTime<Utc>,
    pub roles: Vec<String>,
}

// ============================================================
// Handlers
// ============================================================

/// POST /api/v1/auth/login
///
/// Rate-limited at 10 req/min per IP (auth_ip limiter).
///
/// **CAPTCHA policy**: CAPTCHA becomes mandatory when the target account has
/// `>= CAPTCHA_REQUIRED_AFTER_FAILURES` consecutive failed login attempts
/// (default: `LOCKOUT_THRESHOLD - 2`, i.e. 3 with threshold 5).  When required,
/// the request must include valid `captcha_token` + `captcha_answer` fields or
/// a 422 validation error is returned before credential verification.  When
/// CAPTCHA is not required, it is still validated if provided.
pub async fn login(
    state: web::Data<AppState>,
    client_ip: ClientIp,
    body: web::Json<LoginRequest>,
) -> Result<HttpResponse, AppError> {
    // Validate the request body shape
    body.validate().map_err(|e| {
        let fields: Vec<FieldError> = e
            .field_errors()
            .iter()
            .flat_map(|(field, errs)| {
                errs.iter().map(move |err| FieldError {
                    field: field.to_string(),
                    message: err.message.as_deref().unwrap_or("invalid").to_string(),
                })
            })
            .collect();
        AppError::Validation(fields)
    })?;

    // Per-IP auth rate limit (10 req/min)
    ratelimit::check_auth_ip(&state.rate_limiters, &client_ip.0)
        .map_err(|_| AppError::RateLimited)?;

    let pool = state.db_pool.clone();
    let captcha_key = state.captcha_key;
    let config = state.config.clone();

    let input = LoginInput {
        username: body.username.clone(),
        password: body.password.clone(),
        device_fingerprint: body.device_fingerprint.clone(),
        ip_address: Some(client_ip.0.clone()),
        captcha_token: body.captcha_token.clone(),
        captcha_answer: body.captcha_answer,
    };

    let output = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        AuthService::login(&mut conn, input, &config, &captcha_key)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    let roles: Vec<String> = {
        let pool2 = state.db_pool.clone();
        let user_id = output.user_id;
        web::block(move || {
            use crate::infrastructure::db::repositories::role_repo::PgRoleRepository;
            let mut conn = pool2.get().map_err(|e| AppError::Internal(e.to_string()))?;
            let user_roles = PgRoleRepository::find_user_roles(&mut conn, user_id)?;
            let role_ids: Vec<Uuid> = user_roles.iter().map(|r| r.role_id).collect();
            let all_roles = PgRoleRepository::list_roles(&mut conn)?;
            let names = role_ids
                .iter()
                .filter_map(|rid| all_roles.iter().find(|r| r.id == *rid))
                .map(|r| r.name.clone())
                .collect::<Vec<_>>();
            Ok::<_, AppError>(names)
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))??
    };

    Ok(HttpResponse::Ok().json(ApiResponse::ok(LoginResponse {
        token: output.token,
        expires_at: output.session.expires_at,
        user: AuthUserSummary {
            id: output.user_id,
            username: output.username,
            roles,
        },
    })))
}

/// POST /api/v1/auth/logout
///
/// Requires: valid session (AuthMiddleware). Revokes the current session.
pub async fn logout(
    state: web::Data<AppState>,
    auth: AuthRequired,
    client_ip: ClientIp,
) -> Result<HttpResponse, AppError> {
    let pool = state.db_pool.clone();
    let session_id = auth.0.session_id;
    let user_id = auth.0.user_id;
    let ip = client_ip.0.clone();

    web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        AuthService::logout(&mut conn, session_id, user_id, Some(&ip))
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(HttpResponse::NoContent().finish())
}

/// GET /api/v1/auth/session
///
/// Returns the authenticated caller's current session and role information.
pub async fn current_session(auth: AuthRequired) -> Result<HttpResponse, AppError> {
    let ctx = auth.into_inner();
    let roles: Vec<String> = ctx.roles.iter().map(|r| r.role_name.clone()).collect();

    // We don't have the expiry here without a DB round-trip; the token carries it.
    // Return user + role info, which is what clients typically need.
    Ok(HttpResponse::Ok().json(ApiResponse::ok(serde_json::json!({
        "session_id": ctx.session_id,
        "user_id": ctx.user_id,
        "username": ctx.username,
        "roles": roles,
    }))))
}

/// GET /api/v1/auth/captcha
///
/// Issue a new offline CAPTCHA challenge.
/// Rate-limited at 10 req/min per IP (same auth_ip limiter).
pub async fn get_captcha(
    state: web::Data<AppState>,
    client_ip: ClientIp,
) -> Result<HttpResponse, AppError> {
    ratelimit::check_auth_ip(&state.rate_limiters, &client_ip.0)
        .map_err(|_| AppError::RateLimited)?;

    let challenge = captcha::issue_challenge(&state.captcha_key);
    Ok(HttpResponse::Ok().json(ApiResponse::ok(challenge)))
}
