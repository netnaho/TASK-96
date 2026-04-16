use actix_web::web;

use crate::api::handlers::audit as handlers;

/// GET /api/v1/audit
/// GET /api/v1/audit/{id}
///
/// Audit events are read-only. No create/update/delete endpoints.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/audit")
            .route("", web::get().to(handlers::list_events))
            .route("/{id}", web::get().to(handlers::get_event)),
    );
}
