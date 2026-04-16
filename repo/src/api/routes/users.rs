use actix_web::web;

use crate::api::handlers::users as handlers;

/// GET    /api/v1/users
/// POST   /api/v1/users
/// GET    /api/v1/users/{id}
/// PUT    /api/v1/users/{id}
/// GET    /api/v1/users/{id}/roles
/// POST   /api/v1/users/{id}/roles
/// DELETE /api/v1/users/{id}/roles/{role_id}
/// GET    /api/v1/roles
/// GET    /api/v1/permissions
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/users")
            .route("", web::get().to(handlers::list_users))
            .route("", web::post().to(handlers::create_user))
            .route("/{id}", web::get().to(handlers::get_user))
            .route("/{id}", web::put().to(handlers::update_user))
            .route("/{id}/roles", web::get().to(handlers::list_user_roles))
            .route("/{id}/roles", web::post().to(handlers::assign_role))
            .route(
                "/{id}/roles/{role_id}",
                web::delete().to(handlers::revoke_role),
            ),
    )
    .service(web::resource("/roles").route(web::get().to(handlers::list_roles)))
    .service(web::resource("/permissions").route(web::get().to(handlers::list_permissions)));
}
