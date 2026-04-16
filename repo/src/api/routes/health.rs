use actix_web::web;

use crate::api::handlers::health as handlers;

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(web::resource("/health").route(web::get().to(handlers::health_check)));
}
