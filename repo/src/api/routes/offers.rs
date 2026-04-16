use actix_web::web;

use crate::api::handlers::offers as handlers;

/// GET    /api/v1/offers
/// POST   /api/v1/offers
/// GET    /api/v1/offers/{id}
/// PUT    /api/v1/offers/{id}
/// POST   /api/v1/offers/{id}/submit
/// POST   /api/v1/offers/{id}/withdraw
/// GET    /api/v1/offers/{id}/approvals
/// POST   /api/v1/offers/{id}/approvals
/// PUT    /api/v1/offers/{id}/approvals/{step_id}
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/offers")
            .route("", web::get().to(handlers::list_offers))
            .route("", web::post().to(handlers::create_offer))
            .route("/{id}", web::get().to(handlers::get_offer))
            .route("/{id}", web::put().to(handlers::update_offer))
            .route("/{id}/submit", web::post().to(handlers::submit_offer))
            .route("/{id}/withdraw", web::post().to(handlers::withdraw_offer))
            .route("/{id}/approvals", web::get().to(handlers::list_approvals))
            .route(
                "/{id}/approvals",
                web::post().to(handlers::create_approval_step),
            )
            .route(
                "/{id}/approvals/{step_id}",
                web::put().to(handlers::decide_approval),
            ),
    );
}
