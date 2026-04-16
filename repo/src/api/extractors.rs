/// Request extractors for authentication and authorization.
///
/// ## Usage
///
/// ```rust
/// // Require any authenticated user
/// async fn my_handler(auth: AuthRequired) -> Result<HttpResponse, AppError> {
///     let ctx = auth.into_inner();
///     ctx.require_permission("offers", "read")?;
///     // ...
/// }
///
/// // Inline permission check (fails with 403 if missing)
/// async fn admin_handler(auth: AuthRequired) -> Result<HttpResponse, AppError> {
///     auth.require_permission("users", "create")?;
///     // ...
/// }
/// ```
use std::future::{ready, Ready};

use actix_web::{web, FromRequest, HttpMessage, HttpRequest};

use crate::{
    domain::auth::models::AuthContext,
    shared::{app_state::AppState, errors::AppError},
};

/// Extractor that requires a valid, loaded `AuthContext` in the request extensions.
///
/// `AuthMiddleware` must be in the middleware chain for any route using this extractor.
/// If no `AuthContext` is present, returns 401.
pub struct AuthRequired(pub AuthContext);

impl AuthRequired {
    pub fn into_inner(self) -> AuthContext {
        self.0
    }

    /// Convenience: delegate to `AuthContext::require_permission`.
    pub fn require_permission(&self, resource: &str, action: &str) -> Result<(), AppError> {
        self.0.require_permission(resource, action)
    }

    /// Convenience: delegate to `AuthContext::require_self_or_admin`.
    pub fn require_self_or_admin(&self, resource_owner_id: uuid::Uuid) -> Result<(), AppError> {
        self.0.require_self_or_admin(resource_owner_id)
    }

    /// Convenience: delegate to `AuthContext::require_self_or_platform_admin`.
    pub fn require_self_or_platform_admin(
        &self,
        resource_owner_id: uuid::Uuid,
    ) -> Result<(), AppError> {
        self.0.require_self_or_platform_admin(resource_owner_id)
    }
}

impl FromRequest for AuthRequired {
    type Error = actix_web::Error;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut actix_web::dev::Payload) -> Self::Future {
        match req.extensions().get::<AuthContext>().cloned() {
            Some(ctx) => ready(Ok(AuthRequired(ctx))),
            None => ready(Err(actix_web::Error::from(
                AppError::AuthenticationRequired,
            ))),
        }
    }
}

/// Extractor for the client's IP address, consistent with the rate-limit middleware.
pub struct ClientIp(pub String);

impl FromRequest for ClientIp {
    type Error = actix_web::Error;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _: &mut actix_web::dev::Payload) -> Self::Future {
        let ip = req
            .headers()
            .get("X-Forwarded-For")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(',').next())
            .map(str::trim)
            .map(str::to_string)
            .or_else(|| {
                req.connection_info()
                    .realip_remote_addr()
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "unknown".to_string());
        ready(Ok(ClientIp(ip)))
    }
}
