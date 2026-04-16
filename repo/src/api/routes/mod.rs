use actix_web::web;

use crate::api::middleware::{AuthMiddleware, IpRateLimitMiddleware};

pub mod audit;
pub mod auth;
pub mod bookings;
pub mod candidates;
pub mod health;
pub mod integrations;
pub mod offers;
pub mod onboarding;
pub mod reporting;
pub mod search;
pub mod users;

/// Register all routes under `/api/v1`.
///
/// ## Scope layout
///
/// ```
/// /api/v1
///   ├── [IpRateLimitMiddleware]        ← applied to ALL routes
///   ├── /health                        ← public, no auth
///   ├── /auth/login                    ← public, no auth
///   ├── /auth/captcha                  ← public, no auth
///   └── [AuthMiddleware]               ← all routes below require valid session
///       ├── /auth/logout
///       ├── /auth/session
///       ├── /users/**
///       ├── /candidates/**
///       ├── /offers/**
///       ├── /onboarding/**
///       ├── /bookings/**
///       ├── /sites/**
///       ├── /search/**
///       ├── /vocabularies/**
///       ├── /reporting/**
///       ├── /integrations/**
///       └── /audit/**
/// ```
///
/// The `AuthMiddleware` is fail-closed: any missing or invalid token in the
/// protected scope returns 401 before the handler is reached.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/v1")
            // IP rate limit wraps everything
            .wrap(IpRateLimitMiddleware)
            // Public routes (no auth required)
            .configure(health::configure)
            .configure(auth::configure_public)
            // Protected routes (auth required)
            .service(
                web::scope("")
                    .wrap(AuthMiddleware)
                    .configure(auth::configure_protected)
                    .configure(users::configure)
                    .configure(candidates::configure)
                    .configure(offers::configure)
                    .configure(onboarding::configure)
                    .configure(bookings::configure)
                    .configure(search::configure)
                    .configure(reporting::configure)
                    .configure(integrations::configure)
                    .configure(audit::configure),
            ),
    );
}
