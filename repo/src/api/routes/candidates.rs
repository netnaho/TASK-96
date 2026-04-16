use actix_web::web;

use crate::api::handlers::candidates as handlers;

/// GET    /api/v1/candidates
/// POST   /api/v1/candidates
/// GET    /api/v1/candidates/{id}
/// PUT    /api/v1/candidates/{id}
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/candidates")
            .route("", web::get().to(handlers::list_candidates))
            .route("", web::post().to(handlers::create_candidate))
            .route("/{id}", web::get().to(handlers::get_candidate))
            .route("/{id}", web::put().to(handlers::update_candidate)),
    );
}
