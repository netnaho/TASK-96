use actix_web::web;

use crate::api::handlers::onboarding as handlers;

/// GET    /api/v1/onboarding/checklists
/// POST   /api/v1/onboarding/checklists
/// GET    /api/v1/onboarding/checklists/{id}
/// GET    /api/v1/onboarding/checklists/{id}/items
/// POST   /api/v1/onboarding/checklists/{id}/items
/// PUT    /api/v1/onboarding/checklists/{id}/items/{item_id}
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/onboarding/checklists")
            .route("", web::get().to(handlers::list_checklists))
            .route("", web::post().to(handlers::create_checklist))
            .route("/{id}", web::get().to(handlers::get_checklist))
            .route("/{id}/items", web::get().to(handlers::list_items))
            .route("/{id}/items", web::post().to(handlers::create_item))
            .route(
                "/{id}/items/{item_id}",
                web::put().to(handlers::update_item),
            ),
    );
}
