use actix_web::web;

use crate::api::handlers::search as handlers;

/// GET  /api/v1/search
/// GET  /api/v1/search/autocomplete
/// GET  /api/v1/search/history
/// GET  /api/v1/vocabularies
/// GET  /api/v1/vocabularies/{category}
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/search")
            .route("", web::get().to(handlers::search))
            .route("/autocomplete", web::get().to(handlers::autocomplete))
            .route("/history", web::get().to(handlers::search_history)),
    )
    .service(
        web::scope("/vocabularies")
            .route("", web::get().to(handlers::list_vocabularies))
            .route("/{category}", web::get().to(handlers::get_vocabulary)),
    );
}
