use actix_web::{web, HttpResponse};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    api::extractors::AuthRequired,
    infrastructure::db::repositories::audit_repo::PgAuditRepository,
    shared::{
        app_state::AppState,
        errors::AppError,
        pagination::PaginationParams,
        response::{ApiResponse, PaginatedEnvelope, PaginationMeta},
    },
};

// ============================================================
// Response DTOs
// ============================================================

#[derive(Debug, Serialize)]
pub struct AuditEventResponse {
    pub id: Uuid,
    pub actor_id: Option<Uuid>,
    pub actor_ip: Option<String>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<Uuid>,
    pub old_value: Option<serde_json::Value>,
    pub new_value: Option<serde_json::Value>,
    pub metadata: serde_json::Value,
    pub correlation_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

// ============================================================
// Handlers
// ============================================================

/// GET /api/v1/audit?page=1&per_page=25
pub async fn list_events(
    state: web::Data<AppState>,
    auth: AuthRequired,
    query: web::Query<PaginationParams>,
) -> Result<HttpResponse, AppError> {
    let ctx = auth.into_inner();
    if !ctx.has_role("platform_admin") {
        return Err(AppError::Forbidden);
    }

    let pool = state.db_pool.clone();
    let page = query.page.max(1) as i64;
    let per_page = crate::shared::pagination::clamp_per_page(query.per_page);

    let (events, total) = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        PgAuditRepository::list_events(&mut conn, page, per_page)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    let data: Vec<AuditEventResponse> = events
        .into_iter()
        .map(|e| AuditEventResponse {
            id: e.id,
            actor_id: e.actor_id,
            actor_ip: e.actor_ip,
            action: e.action,
            resource_type: e.resource_type,
            resource_id: e.resource_id,
            old_value: e.old_value,
            new_value: e.new_value,
            metadata: e.metadata,
            correlation_id: e.correlation_id,
            created_at: e.created_at,
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

/// GET /api/v1/audit/{id}
pub async fn get_event(
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let ctx = auth.into_inner();
    if !ctx.has_role("platform_admin") {
        return Err(AppError::Forbidden);
    }

    let id = path.into_inner();
    let pool = state.db_pool.clone();

    let event = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        PgAuditRepository::find_event(&mut conn, id)?
            .ok_or_else(|| AppError::NotFound("audit_event".into()))
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(AuditEventResponse {
        id: event.id,
        actor_id: event.actor_id,
        actor_ip: event.actor_ip,
        action: event.action,
        resource_type: event.resource_type,
        resource_id: event.resource_id,
        old_value: event.old_value,
        new_value: event.new_value,
        metadata: event.metadata,
        correlation_id: event.correlation_id,
        created_at: event.created_at,
    })))
}
