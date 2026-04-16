use actix_web::web;

use crate::api::handlers::reporting as handlers;

/// GET    /api/v1/reporting/subscriptions
/// POST   /api/v1/reporting/subscriptions
/// GET    /api/v1/reporting/subscriptions/{id}
/// PUT    /api/v1/reporting/subscriptions/{id}
/// DELETE /api/v1/reporting/subscriptions/{id}
/// GET    /api/v1/reporting/dashboards/{key}/versions
/// POST   /api/v1/reporting/dashboards/{key}/versions
/// GET    /api/v1/reporting/alerts
/// PUT    /api/v1/reporting/alerts/{id}/acknowledge
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/reporting")
            .route(
                "/subscriptions",
                web::get().to(handlers::list_subscriptions),
            )
            .route(
                "/subscriptions",
                web::post().to(handlers::create_subscription),
            )
            .route(
                "/subscriptions/{id}",
                web::get().to(handlers::get_subscription),
            )
            .route(
                "/subscriptions/{id}",
                web::put().to(handlers::update_subscription),
            )
            .route(
                "/subscriptions/{id}",
                web::delete().to(handlers::delete_subscription),
            )
            .route(
                "/dashboards/{key}/versions",
                web::get().to(handlers::list_dashboard_versions),
            )
            .route(
                "/dashboards/{key}/versions",
                web::post().to(handlers::publish_dashboard),
            )
            .route("/alerts", web::get().to(handlers::list_alerts))
            .route(
                "/alerts/{id}/acknowledge",
                web::put().to(handlers::acknowledge_alert),
            ),
    );
}
