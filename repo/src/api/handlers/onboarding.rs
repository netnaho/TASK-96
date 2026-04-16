use actix_web::{web, HttpRequest, HttpResponse};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{
    api::extractors::AuthRequired,
    application::onboarding_service::{
        AddItemInput, CreateChecklistInput, ListChecklistsInput, OnboardingService, UpdateItemInput,
    },
    domain::onboarding::models::OnboardingItemStatus,
    shared::{
        app_state::AppState,
        errors::{AppError, FieldError},
        idempotency::idempotency_info,
        response::{ApiResponse, PaginatedEnvelope, PaginationMeta},
    },
};

// ============================================================
// Request DTOs
// ============================================================

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct CreateChecklistRequest {
    pub offer_id: Uuid,
    pub candidate_id: Uuid,
    pub assigned_to: Option<Uuid>,
    pub due_date: Option<NaiveDate>,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct AddItemRequest {
    #[validate(length(min = 1, max = 200))]
    pub title: String,
    #[validate(length(max = 2000))]
    pub description: Option<String>,
    pub item_order: i32,
    #[serde(default)]
    pub requires_upload: bool,
    #[serde(default)]
    pub required: bool,
    pub item_due_date: Option<NaiveDate>,
    pub health_attestation: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct UpdateItemRequest {
    pub status: String,
    pub upload_storage_key: Option<String>,
    pub health_attestation: Option<String>,
}

// ============================================================
// Query params
// ============================================================

#[derive(Debug, Deserialize)]
pub struct ListChecklistsQuery {
    pub candidate_id: Option<Uuid>,
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
// Response DTOs
// ============================================================

#[derive(Debug, Serialize)]
pub struct ChecklistResponse {
    pub id: Uuid,
    pub offer_id: Uuid,
    pub candidate_id: Uuid,
    pub assigned_to: Option<Uuid>,
    pub due_date: Option<NaiveDate>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct OnboardingItemResponse {
    pub id: Uuid,
    pub checklist_id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub item_order: i32,
    pub status: String,
    pub requires_upload: bool,
    pub upload_storage_key: Option<String>,
    pub required: bool,
    pub item_due_date: Option<NaiveDate>,
    pub completed_at: Option<DateTime<Utc>>,
    pub completed_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct ReadinessResponse {
    pub checklist_id: Uuid,
    pub total_required: u32,
    pub required_completed: u32,
    pub readiness_pct: u8,
}

// ============================================================
// Handlers
// ============================================================

/// GET /api/v1/onboarding/checklists?candidate_id=...&page=1&per_page=25
pub async fn list_checklists(
    state: web::Data<AppState>,
    auth: AuthRequired,
    query: web::Query<ListChecklistsQuery>,
) -> Result<HttpResponse, AppError> {
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    let candidate_id = query.candidate_id;
    let page = query.page.max(1) as i64;
    let per_page = crate::shared::pagination::clamp_per_page(query.per_page);

    let (checklists, total) = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        OnboardingService::list_checklists(
            &mut conn,
            &ctx,
            ListChecklistsInput {
                candidate_id,
                page,
                per_page,
            },
        )
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    let data: Vec<ChecklistResponse> = checklists.into_iter().map(checklist_to_response).collect();

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

/// POST /api/v1/onboarding/checklists
pub async fn create_checklist(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    body: web::Json<CreateChecklistRequest>,
) -> Result<HttpResponse, AppError> {
    body.validate().map_err(validation_error)?;

    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    let body_bytes = serde_json::to_vec(&*body).unwrap_or_default();
    let (idempotency_key, request_hash) = idempotency_info(&req, &body_bytes);

    let input = CreateChecklistInput {
        offer_id: body.offer_id,
        candidate_id: body.candidate_id,
        assigned_to: body.assigned_to,
        due_date: body.due_date,
        idempotency_key,
        request_hash,
    };

    let checklist = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        OnboardingService::create_checklist(&mut conn, &ctx, input)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(HttpResponse::Created().json(ApiResponse::ok(checklist_to_response(checklist))))
}

/// GET /api/v1/onboarding/checklists/{id}
pub async fn get_checklist(
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let id = path.into_inner();
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();

    let checklist = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        OnboardingService::get_checklist(&mut conn, &ctx, id)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(checklist_to_response(checklist))))
}

/// GET /api/v1/onboarding/checklists/{id}/items
pub async fn list_items(
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let checklist_id = path.into_inner();
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();

    let items = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        OnboardingService::list_items(&mut conn, &ctx, checklist_id)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    let data: Vec<OnboardingItemResponse> = items.into_iter().map(item_to_response).collect();
    Ok(HttpResponse::Ok().json(ApiResponse::ok(data)))
}

/// POST /api/v1/onboarding/checklists/{id}/items
pub async fn create_item(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<Uuid>,
    body: web::Json<AddItemRequest>,
) -> Result<HttpResponse, AppError> {
    body.validate().map_err(validation_error)?;

    let checklist_id = path.into_inner();
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    let encryption_key = state.config.encryption_key.clone();
    let body_bytes = serde_json::to_vec(&*body).unwrap_or_default();
    let (idempotency_key, request_hash) = idempotency_info(&req, &body_bytes);

    let input = AddItemInput {
        title: body.title.clone(),
        description: body.description.clone(),
        item_order: body.item_order,
        requires_upload: body.requires_upload,
        required: body.required,
        item_due_date: body.item_due_date,
        health_attestation: body.health_attestation.clone(),
        idempotency_key,
        request_hash,
    };

    let item = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        OnboardingService::add_item(&mut conn, &ctx, checklist_id, input, &encryption_key)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(HttpResponse::Created().json(ApiResponse::ok(item_to_response(item))))
}

/// PUT /api/v1/onboarding/checklists/{id}/items/{item_id}
pub async fn update_item(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<(Uuid, Uuid)>,
    body: web::Json<UpdateItemRequest>,
) -> Result<HttpResponse, AppError> {
    body.validate().map_err(validation_error)?;

    let (checklist_id, item_id) = path.into_inner();
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    let encryption_key = state.config.encryption_key.clone();
    let body_bytes = serde_json::to_vec(&*body).unwrap_or_default();
    let (idempotency_key, request_hash) = idempotency_info(&req, &body_bytes);

    let status = parse_item_status(&body.status)?;

    let input = UpdateItemInput {
        status,
        upload_storage_key: body.upload_storage_key.clone(),
        health_attestation: body.health_attestation.clone(),
        idempotency_key,
        request_hash,
    };

    let item = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        OnboardingService::update_item(
            &mut conn,
            &ctx,
            checklist_id,
            item_id,
            input,
            &encryption_key,
        )
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(item_to_response(item))))
}

// ============================================================
// Helpers
// ============================================================

fn checklist_to_response(
    c: crate::domain::onboarding::models::OnboardingChecklist,
) -> ChecklistResponse {
    ChecklistResponse {
        id: c.id,
        offer_id: c.offer_id,
        candidate_id: c.candidate_id,
        assigned_to: c.assigned_to,
        due_date: c.due_date,
        created_at: c.created_at,
        updated_at: c.updated_at,
    }
}

fn item_to_response(
    item: crate::domain::onboarding::models::OnboardingItem,
) -> OnboardingItemResponse {
    OnboardingItemResponse {
        id: item.id,
        checklist_id: item.checklist_id,
        title: item.title,
        description: item.description,
        item_order: item.item_order,
        status: item.status.as_str().to_string(),
        requires_upload: item.requires_upload,
        upload_storage_key: item.upload_storage_key,
        required: item.required,
        item_due_date: item.item_due_date,
        completed_at: item.completed_at,
        completed_by: item.completed_by,
        created_at: item.created_at,
        updated_at: item.updated_at,
    }
}

fn parse_item_status(s: &str) -> Result<OnboardingItemStatus, AppError> {
    match s {
        "not_started" => Ok(OnboardingItemStatus::NotStarted),
        "in_progress" => Ok(OnboardingItemStatus::InProgress),
        "completed" => Ok(OnboardingItemStatus::Completed),
        "blocked" => Ok(OnboardingItemStatus::Blocked),
        "skipped" => Ok(OnboardingItemStatus::Skipped),
        other => Err(AppError::Validation(vec![FieldError {
            field: "status".into(),
            message: format!(
                "unknown status '{other}'; must be not_started, in_progress, completed, blocked, or skipped"
            ),
        }])),
    }
}

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
