use actix_web::{web, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{
    api::extractors::AuthRequired,
    application::integration_service::{
        CreateConnectorInput, ExportInput, ImportInput, IntegrationService, UpdateConnectorInput,
    },
    shared::{
        app_state::AppState,
        errors::{AppError, FieldError},
        idempotency::idempotency_info,
        response::{ApiResponse, PaginatedEnvelope, PaginationMeta},
    },
};

// ============================================================
// Query params
// ============================================================

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_per_page")]
    pub per_page: u32,
}
fn default_page() -> u32 {
    1
}
fn default_per_page() -> u32 {
    25
}

// ============================================================
// Request bodies
// ============================================================

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct CreateConnectorRequest {
    #[validate(length(min = 1, max = 200))]
    pub name: String,
    pub connector_type: String,
    pub base_url: Option<String>,
    pub auth_config: Option<serde_json::Value>,
    #[serde(default = "default_true")]
    pub is_enabled: bool,
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateConnectorRequest {
    pub name: Option<String>,
    pub base_url: Option<serde_json::Value>, // null = clear, string = set, absent = no change
    pub auth_config: Option<serde_json::Value>, // null = clear, object = set, absent = no change
    pub is_enabled: Option<bool>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct TriggerSyncRequest {
    #[validate(length(min = 1, max = 100))]
    pub entity_type: String,
    /// When `true`, bypasses the concurrent-running guard and re-executes
    /// even if a `running` state already exists for this connector/entity pair.
    /// Defaults to `false`.
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ImportRequest {
    pub connector_id: Option<Uuid>,
    pub entity_type: String,
    #[serde(default)]
    pub records: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ExportRequest {
    pub connector_id: Option<Uuid>,
    pub entity_type: String,
    #[serde(default)]
    pub field_map: std::collections::HashMap<String, String>,
}

// ============================================================
// Handlers
// ============================================================

pub async fn list_connectors(
    state: web::Data<AppState>,
    auth: AuthRequired,
    query: web::Query<ListQuery>,
) -> Result<HttpResponse, AppError> {
    let ctx = auth.into_inner();
    let page = query.page.max(1) as i64;
    let per_page = crate::shared::pagination::clamp_per_page(query.per_page);

    let mut conn = state
        .db_pool
        .get()
        .map_err(|e| AppError::Internal(format!("db pool: {e}")))?;

    let (items, total) =
        web::block(move || IntegrationService::list_connectors(&mut conn, &ctx, page, per_page))
            .await
            .map_err(|e| AppError::Internal(format!("blocking: {e}")))?
            .map_err(|e| e)?;

    Ok(HttpResponse::Ok().json(PaginatedEnvelope {
        data: items,
        pagination: PaginationMeta {
            page: query.page,
            per_page: query.per_page,
            total,
        },
        meta: None,
    }))
}

pub async fn create_connector(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    body: web::Json<CreateConnectorRequest>,
) -> Result<HttpResponse, AppError> {
    body.validate().map_err(validation_error)?;
    let ctx = auth.into_inner();
    let encryption_key = state.config.encryption_key.clone();
    let body_bytes = serde_json::to_vec(&*body).unwrap_or_default();
    let (idempotency_key, request_hash) = idempotency_info(&req, &body_bytes);
    let input = CreateConnectorInput {
        name: body.name.clone(),
        connector_type: body.connector_type.clone(),
        base_url: body.base_url.clone(),
        auth_config: body.auth_config.clone(),
        is_enabled: body.is_enabled,
        idempotency_key,
        request_hash,
    };

    let mut conn = state
        .db_pool
        .get()
        .map_err(|e| AppError::Internal(format!("db pool: {e}")))?;

    let connector = web::block(move || {
        IntegrationService::create_connector(&mut conn, &ctx, input, &encryption_key)
    })
    .await
    .map_err(|e| AppError::Internal(format!("blocking: {e}")))?
    .map_err(|e| e)?;

    Ok(HttpResponse::Created().json(ApiResponse::ok(connector)))
}

pub async fn get_connector(
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let ctx = auth.into_inner();
    let id = path.into_inner();

    let mut conn = state
        .db_pool
        .get()
        .map_err(|e| AppError::Internal(format!("db pool: {e}")))?;

    let connector = web::block(move || IntegrationService::get_connector(&mut conn, &ctx, id))
        .await
        .map_err(|e| AppError::Internal(format!("blocking: {e}")))?
        .map_err(|e| e)?;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(connector)))
}

pub async fn update_connector(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<Uuid>,
    body: web::Json<UpdateConnectorRequest>,
) -> Result<HttpResponse, AppError> {
    let ctx = auth.into_inner();
    let id = path.into_inner();
    let encryption_key = state.config.encryption_key.clone();
    let body_bytes = serde_json::to_vec(&*body).unwrap_or_default();
    let (idempotency_key, request_hash) = idempotency_info(&req, &body_bytes);

    // Interpret nullable optional fields
    let base_url: Option<Option<String>> = match &body.base_url {
        Some(serde_json::Value::Null) => Some(None),
        Some(serde_json::Value::String(s)) => Some(Some(s.clone())),
        None => None,
        _ => None,
    };

    let auth_config: Option<Option<serde_json::Value>> = match &body.auth_config {
        Some(serde_json::Value::Null) => Some(None),
        Some(v) if v.is_object() => Some(Some(v.clone())),
        None => None,
        _ => None,
    };

    let input = UpdateConnectorInput {
        name: body.name.clone(),
        base_url,
        auth_config,
        is_enabled: body.is_enabled,
        idempotency_key,
        request_hash,
    };

    let mut conn = state
        .db_pool
        .get()
        .map_err(|e| AppError::Internal(format!("db pool: {e}")))?;

    let connector = web::block(move || {
        IntegrationService::update_connector(&mut conn, &ctx, id, input, &encryption_key)
    })
    .await
    .map_err(|e| AppError::Internal(format!("blocking: {e}")))?
    .map_err(|e| e)?;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(connector)))
}

pub async fn trigger_sync(
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<Uuid>,
    body: web::Json<TriggerSyncRequest>,
) -> Result<HttpResponse, AppError> {
    body.validate().map_err(validation_error)?;
    let ctx = auth.into_inner();
    let connector_id = path.into_inner();
    let entity_type = body.entity_type.clone();
    let force = body.force;
    let storage_path = state.config.storage_path.clone();

    let mut conn = state
        .db_pool
        .get()
        .map_err(|e| AppError::Internal(format!("db pool: {e}")))?;

    let sync_state = web::block(move || {
        IntegrationService::trigger_sync(
            &mut conn,
            &ctx,
            connector_id,
            &entity_type,
            &storage_path,
            force,
        )
    })
    .await
    .map_err(|e| AppError::Internal(format!("blocking: {e}")))?
    .map_err(|e| e)?;

    Ok(HttpResponse::Accepted().json(ApiResponse::ok(sync_state)))
}

pub async fn get_sync_state(
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let ctx = auth.into_inner();
    let connector_id = path.into_inner();

    let mut conn = state
        .db_pool
        .get()
        .map_err(|e| AppError::Internal(format!("db pool: {e}")))?;

    let states =
        web::block(move || IntegrationService::get_sync_state(&mut conn, &ctx, connector_id))
            .await
            .map_err(|e| AppError::Internal(format!("blocking: {e}")))?
            .map_err(|e| e)?;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(states)))
}

pub async fn import_data(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    body: web::Json<ImportRequest>,
) -> Result<HttpResponse, AppError> {
    let ctx = auth.into_inner();
    let storage_path = state.config.storage_path.clone();
    let body_bytes = serde_json::to_vec(&*body).unwrap_or_default();
    let (idempotency_key, request_hash) = idempotency_info(&req, &body_bytes);
    let input = ImportInput {
        connector_id: body.connector_id,
        entity_type: body.entity_type.clone(),
        records: body.records.clone(),
        storage_path,
        idempotency_key,
        request_hash,
    };

    let mut conn = state
        .db_pool
        .get()
        .map_err(|e| AppError::Internal(format!("db pool: {e}")))?;

    let result = web::block(move || IntegrationService::import_data(&mut conn, &ctx, input))
        .await
        .map_err(|e| AppError::Internal(format!("blocking: {e}")))?
        .map_err(|e| e)?;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(result)))
}

pub async fn export_data(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    body: web::Json<ExportRequest>,
) -> Result<HttpResponse, AppError> {
    let ctx = auth.into_inner();
    let storage_path = state.config.storage_path.clone();
    let body_bytes = serde_json::to_vec(&*body).unwrap_or_default();
    let (idempotency_key, request_hash) = idempotency_info(&req, &body_bytes);
    let input = ExportInput {
        connector_id: body.connector_id,
        entity_type: body.entity_type.clone(),
        field_map: body.field_map.clone(),
        storage_path,
        idempotency_key,
        request_hash,
    };

    let mut conn = state
        .db_pool
        .get()
        .map_err(|e| AppError::Internal(format!("db pool: {e}")))?;

    let result = web::block(move || IntegrationService::export_data(&mut conn, &ctx, input))
        .await
        .map_err(|e| AppError::Internal(format!("blocking: {e}")))?
        .map_err(|e| e)?;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(result)))
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
