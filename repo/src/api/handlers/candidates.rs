use actix_web::{web, HttpRequest, HttpResponse};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{
    api::extractors::AuthRequired,
    application::candidate_service::{
        CandidateService, CreateCandidateInput, ListCandidatesInput, UpdateCandidateInput,
    },
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

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct CreateCandidateRequest {
    #[validate(length(min = 1, max = 100))]
    pub first_name: String,
    #[validate(length(min = 1, max = 100))]
    pub last_name: String,
    #[validate(email)]
    pub email: String,
    #[validate(length(max = 30))]
    pub phone: Option<String>,
    #[validate(length(min = 4, max = 4))]
    pub ssn_last4: Option<String>,
    pub resume_storage_key: Option<String>,
    #[validate(length(max = 64))]
    pub source: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[validate(length(max = 4096))]
    pub notes: Option<String>,
    pub organization_id: Option<Uuid>,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct UpdateCandidateRequest {
    #[validate(length(min = 1, max = 100))]
    pub first_name: String,
    #[validate(length(min = 1, max = 100))]
    pub last_name: String,
    #[validate(email)]
    pub email: String,
    #[validate(length(max = 30))]
    pub phone: Option<String>,
    #[validate(length(min = 4, max = 4))]
    pub ssn_last4: Option<String>,
    pub resume_storage_key: Option<String>,
    #[validate(length(max = 64))]
    pub source: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[validate(length(max = 4096))]
    pub notes: Option<String>,
    pub organization_id: Option<Uuid>,
}

// ============================================================
// Query params
// ============================================================

#[derive(Debug, Deserialize)]
pub struct GetCandidateQuery {
    /// Pass `reveal_sensitive=true` to decrypt phone and ssn_last4.
    #[serde(default)]
    pub reveal_sensitive: bool,
}

// ============================================================
// Response DTOs
// ============================================================

#[derive(Debug, Serialize)]
pub struct CandidateSummaryResponse {
    pub id: Uuid,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub source: Option<String>,
    pub tags: Vec<String>,
    pub organization_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct CandidateDetailResponse {
    pub id: Uuid,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub phone: Option<String>,
    pub ssn_last4: Option<String>,
    pub resume_storage_key: Option<String>,
    pub source: Option<String>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
    pub organization_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ============================================================
// Handlers
// ============================================================

/// GET /api/v1/candidates?page=1&per_page=25
pub async fn list_candidates(
    state: web::Data<AppState>,
    auth: AuthRequired,
    query: web::Query<PaginationParams>,
) -> Result<HttpResponse, AppError> {
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    let page = query.page as i64;
    let per_page = query.per_page as i64;

    let (summaries, total) = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        CandidateService::list(&mut conn, &ctx, ListCandidatesInput { page, per_page })
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    let data: Vec<CandidateSummaryResponse> = summaries
        .into_iter()
        .map(|c| CandidateSummaryResponse {
            id: c.id,
            first_name: c.first_name,
            last_name: c.last_name,
            email: c.email,
            source: c.source,
            tags: c.tags,
            organization_id: c.organization_id,
            created_at: c.created_at,
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

/// POST /api/v1/candidates
pub async fn create_candidate(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    body: web::Json<CreateCandidateRequest>,
) -> Result<HttpResponse, AppError> {
    body.validate().map_err(validation_error)?;

    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    let encryption_key = state.config.encryption_key.clone();
    let body_bytes = serde_json::to_vec(&*body).unwrap_or_default();
    let (idempotency_key, request_hash) = idempotency_info(&req, &body_bytes);

    let input = CreateCandidateInput {
        first_name: body.first_name.clone(),
        last_name: body.last_name.clone(),
        email: body.email.clone(),
        phone: body.phone.clone(),
        ssn_last4: body.ssn_last4.clone(),
        resume_storage_key: body.resume_storage_key.clone(),
        source: body.source.clone(),
        tags: body.tags.clone(),
        notes: body.notes.clone(),
        organization_id: body.organization_id,
        idempotency_key,
        request_hash,
    };

    let candidate = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        CandidateService::create(&mut conn, &ctx, input, &encryption_key)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(
        HttpResponse::Created().json(ApiResponse::ok(CandidateDetailResponse {
            id: candidate.id,
            first_name: candidate.first_name,
            last_name: candidate.last_name,
            email: candidate.email,
            phone: None, // plaintext is not echoed back
            ssn_last4: None,
            resume_storage_key: candidate.resume_storage_key,
            source: candidate.source,
            tags: candidate.tags,
            notes: candidate.notes,
            organization_id: candidate.organization_id,
            created_by: candidate.created_by,
            created_at: candidate.created_at,
            updated_at: candidate.updated_at,
        })),
    )
}

/// GET /api/v1/candidates/{id}?reveal_sensitive=true
pub async fn get_candidate(
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<Uuid>,
    query: web::Query<GetCandidateQuery>,
) -> Result<HttpResponse, AppError> {
    let id = path.into_inner();
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    let encryption_key = state.config.encryption_key.clone();
    let reveal = query.reveal_sensitive;

    let detail = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        CandidateService::get(&mut conn, &ctx, id, reveal, &encryption_key)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(
        HttpResponse::Ok().json(ApiResponse::ok(CandidateDetailResponse {
            id: detail.id,
            first_name: detail.first_name,
            last_name: detail.last_name,
            email: detail.email,
            phone: detail.phone,
            ssn_last4: detail.ssn_last4,
            resume_storage_key: detail.resume_storage_key,
            source: detail.source,
            tags: detail.tags,
            notes: detail.notes,
            organization_id: detail.organization_id,
            created_by: detail.created_by,
            created_at: detail.created_at,
            updated_at: detail.updated_at,
        })),
    )
}

/// PUT /api/v1/candidates/{id}
pub async fn update_candidate(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<Uuid>,
    body: web::Json<UpdateCandidateRequest>,
) -> Result<HttpResponse, AppError> {
    body.validate().map_err(validation_error)?;

    let id = path.into_inner();
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    let encryption_key = state.config.encryption_key.clone();
    let body_bytes = serde_json::to_vec(&*body).unwrap_or_default();
    let (idempotency_key, request_hash) = idempotency_info(&req, &body_bytes);

    let input = UpdateCandidateInput {
        first_name: body.first_name.clone(),
        last_name: body.last_name.clone(),
        email: body.email.clone(),
        phone: body.phone.clone(),
        ssn_last4: body.ssn_last4.clone(),
        resume_storage_key: body.resume_storage_key.clone(),
        source: body.source.clone(),
        tags: body.tags.clone(),
        notes: body.notes.clone(),
        organization_id: body.organization_id,
        idempotency_key,
        request_hash,
    };

    let candidate = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        CandidateService::update(&mut conn, &ctx, id, input, &encryption_key)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(
        HttpResponse::Ok().json(ApiResponse::ok(CandidateDetailResponse {
            id: candidate.id,
            first_name: candidate.first_name,
            last_name: candidate.last_name,
            email: candidate.email,
            phone: None,
            ssn_last4: None,
            resume_storage_key: candidate.resume_storage_key,
            source: candidate.source,
            tags: candidate.tags,
            notes: candidate.notes,
            organization_id: candidate.organization_id,
            created_by: candidate.created_by,
            created_at: candidate.created_at,
            updated_at: candidate.updated_at,
        })),
    )
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
