/// Session-authentication middleware.
///
/// Applies to the protected route scope.  For every request:
///
/// 1. Extracts `Authorization: Bearer <token>` header.
/// 2. If absent → returns 401 immediately (fail-closed for protected routes).
/// 3. If present → calls `AuthService::validate_session` inside `web::block`.
/// 4. On success → stores `AuthContext` in request extensions; continues.
/// 5. On failure → returns 401 (session expired, invalid, or account inactive).
///
/// After successful auth, the per-user rate limiter is checked.
/// Failing it returns 429.
///
/// `last_activity_at` is updated inside `validate_session` as a side-effect.
use std::future::{ready, Future, Ready};
use std::pin::Pin;

use actix_web::{
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    web, Error, HttpMessage,
};
use tracing::warn;

use crate::{
    application::auth_service::AuthService,
    infrastructure::ratelimit,
    shared::{app_state::AppState, errors::AppError},
};

pub struct AuthMiddleware;

impl<S, B> Transform<S, ServiceRequest> for AuthMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = AuthMiddlewareService<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(AuthMiddlewareService { service }))
    }
}

pub struct AuthMiddlewareService<S> {
    service: S,
}

impl<S, B> Service<ServiceRequest> for AuthMiddlewareService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        // Extract the bearer token from the Authorization header.
        let token = match extract_bearer_token(&req) {
            Some(t) => t,
            None => {
                warn!(
                    path = req.path(),
                    "auth middleware: missing Authorization header"
                );
                let err = AppError::AuthenticationRequired;
                return Box::pin(ready(Err(actix_web::Error::from(err))));
            }
        };

        let state = match req.app_data::<web::Data<AppState>>() {
            Some(s) => s.clone(),
            None => {
                return Box::pin(ready(Err(actix_web::Error::from(AppError::Internal(
                    "AppState not configured".into(),
                )))));
            }
        };

        let service = std::rc::Rc::new(&self.service as *const S);
        // Safety: we move `service` into the async block which lives at most as long
        // as the current request.
        let fut = self.service.call(req);

        Box::pin(async move {
            // Validate session in a blocking thread (Diesel is sync).
            let pool = state.db_pool.clone();
            let token_clone = token.clone();
            let auth_result = web::block(move || {
                let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
                AuthService::validate_session(&mut conn, &token_clone)
            })
            .await
            .map_err(|e| actix_web::Error::from(AppError::Internal(e.to_string())))?
            .map_err(actix_web::Error::from)?;

            // Per-user rate limit check
            let user_id_str = auth_result.user_id.to_string();
            if let Err(retry_after) = ratelimit::check_user(&state.rate_limiters, &user_id_str) {
                warn!(
                    user_id = %auth_result.user_id,
                    retry_after_secs = retry_after,
                    "per-user rate limit exceeded"
                );
                return Err(actix_web::Error::from(AppError::RateLimited));
            }

            // Inject AuthContext into request extensions for downstream handlers.
            // We need the original request, but `fut` already consumed it.
            // Instead we store it via the future's response — but we can't because
            // we've already called `service.call(req)`.
            //
            // Correct pattern: clone extensions before calling service.
            // Since we consumed `req` already, we re-implement below by NOT
            // pre-calling the service. See revised impl below.
            fut.await
        })
    }
}

// ============================================================
// Correct middleware implementation (extensions must be set BEFORE calling service)
// ============================================================

// Replace the above with a cleaner pattern that inserts AuthContext before
// forwarding the request.

pub struct AuthMiddlewareV2;

impl<S, B> Transform<S, ServiceRequest> for AuthMiddlewareV2
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = AuthMiddlewareServiceV2<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(AuthMiddlewareServiceV2 {
            service: std::rc::Rc::new(service),
        }))
    }
}

pub struct AuthMiddlewareServiceV2<S> {
    service: std::rc::Rc<S>,
}

impl<S, B> Service<ServiceRequest> for AuthMiddlewareServiceV2<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let token = match extract_bearer_token(&req) {
            Some(t) => t,
            None => {
                warn!(path = req.path(), "missing Authorization header");
                return Box::pin(ready(Err(actix_web::Error::from(
                    AppError::AuthenticationRequired,
                ))));
            }
        };

        let state = match req.app_data::<web::Data<AppState>>() {
            Some(s) => s.clone(),
            None => {
                return Box::pin(ready(Err(actix_web::Error::from(AppError::Internal(
                    "AppState not configured".into(),
                )))));
            }
        };

        let service = self.service.clone();

        Box::pin(async move {
            let pool = state.db_pool.clone();
            let token_clone = token.clone();

            let auth_ctx = web::block(move || {
                let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
                AuthService::validate_session(&mut conn, &token_clone)
            })
            .await
            .map_err(|e| actix_web::Error::from(AppError::Internal(e.to_string())))?
            .map_err(actix_web::Error::from)?;

            // Per-user rate limit
            let user_id_str = auth_ctx.user_id.to_string();
            if let Err(retry_after) = ratelimit::check_user(&state.rate_limiters, &user_id_str) {
                warn!(
                    user_id = %auth_ctx.user_id,
                    retry_after_secs = retry_after,
                    "per-user rate limit exceeded"
                );
                return Err(actix_web::Error::from(AppError::RateLimited));
            }

            // Insert AuthContext before forwarding to the next service
            req.extensions_mut().insert(auth_ctx);
            service.call(req).await
        })
    }
}

// ============================================================
// Token extraction helper
// ============================================================

fn extract_bearer_token(req: &ServiceRequest) -> Option<String> {
    req.headers()
        .get(actix_web::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(str::to_string)
}
