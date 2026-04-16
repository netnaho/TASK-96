use actix_web::web;

use crate::api::handlers::bookings as handlers;

/// POST   /api/v1/bookings                   — create hold on slot
/// GET    /api/v1/bookings                   — list bookings
/// GET    /api/v1/bookings/{id}              — get booking detail
/// POST   /api/v1/bookings/{id}/agreement    — submit agreement evidence
/// POST   /api/v1/bookings/{id}/confirm      — run eligibility gate + confirm
/// POST   /api/v1/bookings/{id}/start        — Confirmed → InProgress
/// POST   /api/v1/bookings/{id}/complete     — InProgress → Completed
/// POST   /api/v1/bookings/{id}/cancel       — cancel with breach rules
/// POST   /api/v1/bookings/{id}/reschedule   — reschedule to new slot
/// POST   /api/v1/bookings/{id}/exception    — mark exception
/// GET    /api/v1/sites
/// GET    /api/v1/sites/{id}
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/bookings")
            .route("", web::post().to(handlers::create_booking))
            .route("", web::get().to(handlers::list_bookings))
            .route("/{id}", web::get().to(handlers::get_booking))
            .route(
                "/{id}/agreement",
                web::post().to(handlers::submit_agreement),
            )
            .route("/{id}/confirm", web::post().to(handlers::confirm_booking))
            .route("/{id}/start", web::post().to(handlers::start_booking))
            .route("/{id}/complete", web::post().to(handlers::complete_booking))
            .route("/{id}/cancel", web::post().to(handlers::cancel_booking))
            .route(
                "/{id}/reschedule",
                web::post().to(handlers::reschedule_booking),
            )
            .route("/{id}/exception", web::post().to(handlers::mark_exception)),
    )
    .service(
        web::scope("/sites")
            .route("", web::get().to(handlers::list_sites))
            .route("/{id}", web::get().to(handlers::get_site)),
    );
}
