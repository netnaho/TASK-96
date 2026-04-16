use actix_web::{web, HttpRequest, HttpResponse};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{
    api::extractors::AuthRequired,
    application::user_service::{CreateUserInput, UpdateUserInput, UserService},
    shared::{
        app_state::AppState,
        errors::{AppError, FieldError},
        idempotency::idempotency_info,
        pagination::PaginationParams,
        response::{ApiResponse, PaginatedEnvelope, PaginationMeta},
    },
};

// ============================================================
// Request DTOs
// ============================================================

#[derive(Debug, Deserialize, Validate)]
pub struct CreateUserRequest {
    #[validate(length(min = 3, max = 64))]
    pub username: String,
    #[validate(email)]
    pub email: String,
    #[validate(length(min = 12, max = 128))]
    pub password: String,
    #[validate(length(min = 1, max = 128))]
    pub display_name: String,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct UpdateUserRequest {
    #[validate(length(min = 1, max = 128))]
    pub display_name: String,
    #[validate(email)]
    pub email: String,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct AssignRoleRequest {
    #[validate(length(min = 1, max = 64))]
    pub role_name: String,
    pub scope_type: Option<String>,
    pub scope_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct RevokeRolePath {
    pub id: Uuid,
    pub role_id: Uuid,
}

// ============================================================
// Response DTOs
// ============================================================

#[derive(Debug, Serialize)]
pub struct UserSummaryResponse {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub display_name: String,
    pub account_status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct UserDetailResponse {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub display_name: String,
    pub account_status: String,
    pub failed_login_count: i32,
    pub locked_until: Option<DateTime<Utc>>,
    pub last_login_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct UserRoleResponse {
    pub user_id: Uuid,
    pub role_id: Uuid,
    pub scope_type: Option<String>,
    pub scope_id: Option<Uuid>,
    pub granted_at: DateTime<Utc>,
    pub granted_by: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct RoleResponse {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub is_system_role: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct PermissionResponse {
    pub id: Uuid,
    pub resource: String,
    pub action: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
}

// ============================================================
// Handlers
// ============================================================

/// GET /api/v1/users?page=1&per_page=25
pub async fn list_users(
    state: web::Data<AppState>,
    auth: AuthRequired,
    query: web::Query<PaginationParams>,
) -> Result<HttpResponse, AppError> {
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    let page = query.page.max(1) as i64;
    let per_page = crate::shared::pagination::clamp_per_page(query.per_page);

    let (users, total) = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        UserService::list_users(&mut conn, &ctx, page, per_page)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    let data: Vec<UserSummaryResponse> = users
        .into_iter()
        .map(|u| UserSummaryResponse {
            id: u.id,
            username: u.username,
            email: u.email,
            display_name: u.display_name,
            account_status: u.account_status,
            created_at: u.created_at,
        })
        .collect();

    Ok(HttpResponse::Ok().json(PaginatedEnvelope {
        data,
        pagination: PaginationMeta {
            page: query.page,
            per_page: query.per_page,
            total,
        },
        meta: None,
    }))
}

/// POST /api/v1/users
pub async fn create_user(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    body: web::Json<CreateUserRequest>,
) -> Result<HttpResponse, AppError> {
    body.validate().map_err(validation_error)?;

    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    // Hash body without the password to avoid storing sensitive data in request_hash
    let body_bytes = serde_json::to_vec(&serde_json::json!({
        "username": body.username,
        "email": body.email,
        "display_name": body.display_name,
    }))
    .unwrap_or_default();
    let (idempotency_key, request_hash) = idempotency_info(&req, &body_bytes);

    let input = CreateUserInput {
        username: body.username.clone(),
        email: body.email.clone(),
        password: body.password.clone(),
        display_name: body.display_name.clone(),
        idempotency_key,
        request_hash,
    };

    let user = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        UserService::create_user(&mut conn, &ctx, input)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(
        HttpResponse::Created().json(ApiResponse::ok(UserDetailResponse {
            id: user.id,
            username: user.username,
            email: user.email,
            display_name: user.display_name,
            account_status: user.account_status,
            failed_login_count: user.failed_login_count,
            locked_until: user.locked_until,
            last_login_at: user.last_login_at,
            created_at: user.created_at,
            updated_at: user.updated_at,
        })),
    )
}

/// GET /api/v1/users/{id}
pub async fn get_user(
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let id = path.into_inner();
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();

    let user = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        UserService::get_user(&mut conn, &ctx, id)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(UserDetailResponse {
        id: user.id,
        username: user.username,
        email: user.email,
        display_name: user.display_name,
        account_status: user.account_status,
        failed_login_count: user.failed_login_count,
        locked_until: user.locked_until,
        last_login_at: user.last_login_at,
        created_at: user.created_at,
        updated_at: user.updated_at,
    })))
}

/// PUT /api/v1/users/{id}
pub async fn update_user(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<Uuid>,
    body: web::Json<UpdateUserRequest>,
) -> Result<HttpResponse, AppError> {
    body.validate().map_err(validation_error)?;

    let id = path.into_inner();
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    let body_bytes = serde_json::to_vec(&*body).unwrap_or_default();
    let (idempotency_key, request_hash) = idempotency_info(&req, &body_bytes);

    let input = UpdateUserInput {
        display_name: body.display_name.clone(),
        email: body.email.clone(),
        idempotency_key,
        request_hash,
    };

    let user = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        UserService::update_user(&mut conn, &ctx, id, input)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(UserDetailResponse {
        id: user.id,
        username: user.username,
        email: user.email,
        display_name: user.display_name,
        account_status: user.account_status,
        failed_login_count: user.failed_login_count,
        locked_until: user.locked_until,
        last_login_at: user.last_login_at,
        created_at: user.created_at,
        updated_at: user.updated_at,
    })))
}

/// GET /api/v1/users/{user_id}/roles
pub async fn list_user_roles(
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let user_id = path.into_inner();
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();

    let roles = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        UserService::list_user_roles(&mut conn, &ctx, user_id)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    let data: Vec<UserRoleResponse> = roles
        .into_iter()
        .map(|r| UserRoleResponse {
            user_id: r.user_id,
            role_id: r.role_id,
            scope_type: r.scope_type,
            scope_id: r.scope_id,
            granted_at: r.granted_at,
            granted_by: r.granted_by,
        })
        .collect();

    Ok(HttpResponse::Ok().json(ApiResponse::ok(data)))
}

/// POST /api/v1/users/{user_id}/roles
pub async fn assign_role(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<Uuid>,
    body: web::Json<AssignRoleRequest>,
) -> Result<HttpResponse, AppError> {
    body.validate().map_err(validation_error)?;

    let user_id = path.into_inner();
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    let body_bytes = serde_json::to_vec(&*body).unwrap_or_default();
    let (idempotency_key, request_hash) = idempotency_info(&req, &body_bytes);
    let role_name = body.role_name.clone();
    let scope_type = body.scope_type.clone();
    let scope_id = body.scope_id;

    web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        UserService::assign_role(
            &mut conn,
            &ctx,
            user_id,
            &role_name,
            scope_type,
            scope_id,
            idempotency_key,
            request_hash,
        )
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(HttpResponse::NoContent().finish())
}

/// DELETE /api/v1/users/{user_id}/roles/{role_id}
pub async fn revoke_role(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<RevokeRolePath>,
) -> Result<HttpResponse, AppError> {
    let ids = path.into_inner();
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    let (idempotency_key, request_hash) = idempotency_info(&req, &[]);

    web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        UserService::revoke_role(
            &mut conn,
            &ctx,
            ids.id,
            ids.role_id,
            idempotency_key,
            request_hash,
        )
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(HttpResponse::NoContent().finish())
}

/// GET /api/v1/roles
///
/// Restricted to platform_admin. Returns 403 for non-admin users.
pub async fn list_roles(
    state: web::Data<AppState>,
    auth: AuthRequired,
) -> Result<HttpResponse, AppError> {
    let pool = state.db_pool.clone();
    let ctx = auth.into_inner();

    let roles = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        UserService::list_roles(&mut conn, &ctx)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    let data: Vec<RoleResponse> = roles
        .into_iter()
        .map(|r| RoleResponse {
            id: r.id,
            name: r.name,
            description: r.description,
            is_system_role: r.is_system_role,
            created_at: r.created_at,
        })
        .collect();

    Ok(HttpResponse::Ok().json(ApiResponse::ok(data)))
}

/// GET /api/v1/permissions
///
/// Restricted to platform_admin. Returns 403 for non-admin users.
pub async fn list_permissions(
    state: web::Data<AppState>,
    auth: AuthRequired,
) -> Result<HttpResponse, AppError> {
    let pool = state.db_pool.clone();
    let ctx = auth.into_inner();

    let perms = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        UserService::list_permissions(&mut conn, &ctx)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    let data: Vec<PermissionResponse> = perms
        .into_iter()
        .map(|p| PermissionResponse {
            id: p.id,
            resource: p.resource,
            action: p.action,
            description: p.description,
            created_at: p.created_at,
        })
        .collect();

    Ok(HttpResponse::Ok().json(ApiResponse::ok(data)))
}

// ============================================================
// Validation helper
// ============================================================

fn validation_error(e: validator::ValidationErrors) -> AppError {
    let fields: Vec<FieldError> = e
        .field_errors()
        .iter()
        .flat_map(|(field, errs)| {
            errs.iter().map(move |err| FieldError {
                field: field.to_string(),
                message: err.message.as_deref().unwrap_or("invalid").to_string(),
            })
        })
        .collect();
    AppError::Validation(fields)
}
