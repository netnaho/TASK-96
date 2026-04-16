use actix_web::web;

use crate::api::handlers::auth as handlers;

/// Public auth routes — no AuthMiddleware wrapping.
///
/// | Method | Path                 | Auth | Rate limit    |
/// |--------|----------------------|------|---------------|
/// | POST   | /auth/login          | No   | 10/min per IP |
/// | GET    | /auth/captcha        | No   | 10/min per IP |
/// | POST   | /auth/logout         | Yes  | 60/min per user (via AuthMiddleware) |
/// | GET    | /auth/session        | Yes  | 60/min per user (via AuthMiddleware) |
///
/// Note: logout and session are registered in `routes::protected` scope,
/// not here, so they get AuthMiddleware automatically.
pub fn configure_public(cfg: &mut web::ServiceConfig) {
    cfg.route("/auth/login", web::post().to(handlers::login))
        .route("/auth/captcha", web::get().to(handlers::get_captcha));
}

/// Protected auth routes — registered inside the AuthMiddleware scope.
/// Uses direct route registration instead of a scope to avoid ambiguity
/// with the public `/auth` scope (actix-web matches the first scope).
pub fn configure_protected(cfg: &mut web::ServiceConfig) {
    cfg.route("/auth/logout", web::post().to(handlers::logout))
        .route("/auth/session", web::get().to(handlers::current_session));
}
