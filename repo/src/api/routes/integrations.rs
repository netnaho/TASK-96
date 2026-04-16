use actix_web::web;

use crate::api::handlers::integrations as handlers;

/// GET    /api/v1/integrations/connectors
/// POST   /api/v1/integrations/connectors
/// GET    /api/v1/integrations/connectors/{id}
/// PUT    /api/v1/integrations/connectors/{id}
/// POST   /api/v1/integrations/connectors/{id}/sync
/// GET    /api/v1/integrations/connectors/{id}/sync-state
/// POST   /api/v1/integrations/import
/// POST   /api/v1/integrations/export
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/integrations")
            .route("/connectors", web::get().to(handlers::list_connectors))
            .route("/connectors", web::post().to(handlers::create_connector))
            .route("/connectors/{id}", web::get().to(handlers::get_connector))
            .route(
                "/connectors/{id}",
                web::put().to(handlers::update_connector),
            )
            .route(
                "/connectors/{id}/sync",
                web::post().to(handlers::trigger_sync),
            )
            .route(
                "/connectors/{id}/sync-state",
                web::get().to(handlers::get_sync_state),
            )
            .route("/import", web::post().to(handlers::import_data))
            .route("/export", web::post().to(handlers::export_data)),
    );
}
