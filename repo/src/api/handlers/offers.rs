use actix_web::{web, HttpRequest, HttpResponse};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{
    api::extractors::AuthRequired,
    application::offer_service::{
        AddApprovalStepInput, ApprovalService, CreateOfferInput, ListOffersInput, OfferService,
        RecordApprovalInput, UpdateOfferInput,
    },
    domain::offers::models::{ApprovalDecision, CompensationData, OfferStatus},
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
pub struct CreateOfferRequest {
    pub candidate_id: Uuid,
    #[validate(length(min = 1, max = 200))]
    pub title: String,
    #[validate(length(max = 100))]
    pub department: Option<String>,
    pub compensation: Option<CompensationRequest>,
    pub start_date: Option<NaiveDate>,
    pub expires_at: Option<DateTime<Utc>>,
    pub template_id: Option<Uuid>,
    #[validate(length(max = 32))]
    pub clause_version: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct UpdateOfferRequest {
    #[validate(length(min = 1, max = 200))]
    pub title: String,
    #[validate(length(max = 100))]
    pub department: Option<String>,
    pub compensation: Option<CompensationRequest>,
    pub start_date: Option<NaiveDate>,
    pub expires_at: Option<DateTime<Utc>>,
    pub template_id: Option<Uuid>,
    #[validate(length(max = 32))]
    pub clause_version: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CompensationRequest {
    pub base_salary_usd: u64,
    pub bonus_target_pct: f64,
    pub equity_units: u32,
    pub pto_days: u16,
    pub k401_match_pct: f64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AddApprovalStepRequest {
    pub approver_id: Uuid,
    pub step_order: i32,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RecordApprovalRequest {
    pub decision: String,
    pub comments: Option<String>,
}

// ============================================================
// Query params
// ============================================================

#[derive(Debug, Deserialize)]
pub struct GetOfferQuery {
    #[serde(default)]
    pub reveal_compensation: bool,
}

#[derive(Debug, Deserialize)]
pub struct ListOffersQuery {
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
pub struct OfferResponse {
    pub id: Uuid,
    pub candidate_id: Uuid,
    pub title: String,
    pub department: Option<String>,
    /// Only present when `reveal_compensation=true` was requested.
    pub compensation: Option<CompensationResponse>,
    pub start_date: Option<NaiveDate>,
    pub status: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub template_id: Option<Uuid>,
    pub clause_version: Option<String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct CompensationResponse {
    pub base_salary_usd: u64,
    pub bonus_target_pct: f64,
    pub equity_units: u32,
    pub pto_days: u16,
    pub k401_match_pct: f64,
}

#[derive(Debug, Serialize)]
pub struct ApprovalStepResponse {
    pub id: Uuid,
    pub offer_id: Uuid,
    pub step_order: i32,
    pub approver_id: Uuid,
    pub decision: String,
    pub decided_at: Option<DateTime<Utc>>,
    pub comments: Option<String>,
    pub created_at: DateTime<Utc>,
}

// ============================================================
// Handlers
// ============================================================

/// GET /api/v1/offers?candidate_id=...&page=1&per_page=25
pub async fn list_offers(
    state: web::Data<AppState>,
    auth: AuthRequired,
    query: web::Query<ListOffersQuery>,
) -> Result<HttpResponse, AppError> {
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    let candidate_id = query.candidate_id;
    let page = query.page.max(1) as i64;
    let per_page = crate::shared::pagination::clamp_per_page(query.per_page);

    let (offers, total) = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        OfferService::list(
            &mut conn,
            &ctx,
            ListOffersInput {
                candidate_id,
                page,
                per_page,
            },
        )
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    let data: Vec<OfferResponse> = offers
        .into_iter()
        .map(|o| offer_to_response(o, None))
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

/// POST /api/v1/offers
pub async fn create_offer(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    body: web::Json<CreateOfferRequest>,
) -> Result<HttpResponse, AppError> {
    body.validate().map_err(validation_error)?;

    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    let encryption_key = state.config.encryption_key.clone();
    let body_bytes = serde_json::to_vec(&*body).unwrap_or_default();
    let (idempotency_key, request_hash) = idempotency_info(&req, &body_bytes);

    let compensation = body.compensation.as_ref().map(comp_req_to_domain);

    let input = CreateOfferInput {
        candidate_id: body.candidate_id,
        title: body.title.clone(),
        department: body.department.clone(),
        compensation,
        start_date: body.start_date,
        expires_at: body.expires_at,
        template_id: body.template_id,
        clause_version: body.clause_version.clone(),
        idempotency_key,
        request_hash,
    };

    let offer = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        OfferService::create(&mut conn, &ctx, input, &encryption_key)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(HttpResponse::Created().json(ApiResponse::ok(offer_to_response(offer, None))))
}

/// GET /api/v1/offers/{id}?reveal_compensation=true
pub async fn get_offer(
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<Uuid>,
    query: web::Query<GetOfferQuery>,
) -> Result<HttpResponse, AppError> {
    let id = path.into_inner();
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    let encryption_key = state.config.encryption_key.clone();
    let reveal = query.reveal_compensation;

    let (offer, compensation) = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        OfferService::get(&mut conn, &ctx, id, reveal, &encryption_key)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    let comp_resp = compensation.map(comp_domain_to_response);

    Ok(HttpResponse::Ok().json(ApiResponse::ok(offer_to_response(offer, comp_resp))))
}

/// PUT /api/v1/offers/{id}
pub async fn update_offer(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<Uuid>,
    body: web::Json<UpdateOfferRequest>,
) -> Result<HttpResponse, AppError> {
    body.validate().map_err(validation_error)?;

    let id = path.into_inner();
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    let encryption_key = state.config.encryption_key.clone();
    let body_bytes = serde_json::to_vec(&*body).unwrap_or_default();
    let (idempotency_key, request_hash) = idempotency_info(&req, &body_bytes);

    let compensation = body.compensation.as_ref().map(comp_req_to_domain);

    let input = UpdateOfferInput {
        title: body.title.clone(),
        department: body.department.clone(),
        compensation,
        start_date: body.start_date,
        expires_at: body.expires_at,
        template_id: body.template_id,
        clause_version: body.clause_version.clone(),
        idempotency_key,
        request_hash,
    };

    let offer = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        OfferService::update(&mut conn, &ctx, id, input, &encryption_key)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(offer_to_response(offer, None))))
}

/// POST /api/v1/offers/{id}/submit
pub async fn submit_offer(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let id = path.into_inner();
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    let (idempotency_key, request_hash) = idempotency_info(&req, b"");

    let offer = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        OfferService::transition(
            &mut conn,
            &ctx,
            id,
            OfferStatus::PendingApproval,
            idempotency_key,
            request_hash,
        )
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(offer_to_response(offer, None))))
}

/// POST /api/v1/offers/{id}/withdraw
pub async fn withdraw_offer(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let id = path.into_inner();
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    let (idempotency_key, request_hash) = idempotency_info(&req, b"");

    let offer = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        OfferService::transition(
            &mut conn,
            &ctx,
            id,
            OfferStatus::Withdrawn,
            idempotency_key,
            request_hash,
        )
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(offer_to_response(offer, None))))
}

/// GET /api/v1/offers/{id}/approvals
pub async fn list_approvals(
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let offer_id = path.into_inner();
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();

    let steps = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        ApprovalService::list_steps(&mut conn, &ctx, offer_id)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    let data: Vec<ApprovalStepResponse> = steps.into_iter().map(step_to_response).collect();
    Ok(HttpResponse::Ok().json(ApiResponse::ok(data)))
}

/// POST /api/v1/offers/{id}/approvals
pub async fn create_approval_step(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<Uuid>,
    body: web::Json<AddApprovalStepRequest>,
) -> Result<HttpResponse, AppError> {
    let offer_id = path.into_inner();
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    let body_bytes = serde_json::to_vec(&*body).unwrap_or_default();
    let (idempotency_key, request_hash) = idempotency_info(&req, &body_bytes);

    let input = AddApprovalStepInput {
        approver_id: body.approver_id,
        step_order: body.step_order,
        idempotency_key,
        request_hash,
    };

    let step = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        ApprovalService::add_step(&mut conn, &ctx, offer_id, input)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(HttpResponse::Created().json(ApiResponse::ok(step_to_response(step))))
}

/// PUT /api/v1/offers/{offer_id}/approvals/{step_id}
pub async fn decide_approval(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<(Uuid, Uuid)>,
    body: web::Json<RecordApprovalRequest>,
) -> Result<HttpResponse, AppError> {
    let (offer_id, step_id) = path.into_inner();
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    let body_bytes = serde_json::to_vec(&*body).unwrap_or_default();
    let (idempotency_key, request_hash) = idempotency_info(&req, &body_bytes);

    let decision = parse_decision(&body.decision)?;

    let input = RecordApprovalInput {
        step_id,
        decision,
        comments: body.comments.clone(),
        idempotency_key,
        request_hash,
    };

    let step = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        ApprovalService::record_decision(&mut conn, &ctx, offer_id, input)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(step_to_response(step))))
}

// ============================================================
// Helpers
// ============================================================

fn comp_req_to_domain(c: &CompensationRequest) -> CompensationData {
    CompensationData {
        base_salary_usd: c.base_salary_usd,
        bonus_target_pct: c.bonus_target_pct,
        equity_units: c.equity_units,
        pto_days: c.pto_days,
        k401_match_pct: c.k401_match_pct,
    }
}

fn comp_domain_to_response(c: CompensationData) -> CompensationResponse {
    CompensationResponse {
        base_salary_usd: c.base_salary_usd,
        bonus_target_pct: c.bonus_target_pct,
        equity_units: c.equity_units,
        pto_days: c.pto_days,
        k401_match_pct: c.k401_match_pct,
    }
}

fn offer_to_response(
    o: crate::domain::offers::models::Offer,
    compensation: Option<CompensationResponse>,
) -> OfferResponse {
    OfferResponse {
        id: o.id,
        candidate_id: o.candidate_id,
        title: o.title,
        department: o.department,
        compensation,
        start_date: o.start_date,
        status: o.status.as_str().to_string(),
        expires_at: o.expires_at,
        template_id: o.template_id,
        clause_version: o.clause_version,
        created_by: o.created_by,
        created_at: o.created_at,
        updated_at: o.updated_at,
    }
}

fn step_to_response(s: crate::domain::offers::models::ApprovalStep) -> ApprovalStepResponse {
    ApprovalStepResponse {
        id: s.id,
        offer_id: s.offer_id,
        step_order: s.step_order,
        approver_id: s.approver_id,
        decision: s.decision.as_str().to_string(),
        decided_at: s.decided_at,
        comments: s.comments,
        created_at: s.created_at,
    }
}

fn parse_decision(s: &str) -> Result<ApprovalDecision, AppError> {
    ApprovalDecision::from_str(s).ok_or_else(|| {
        AppError::Validation(vec![crate::shared::errors::FieldError {
            field: "decision".into(),
            message: format!("unknown decision '{s}'; must be approved, rejected, or escalated"),
        }])
    })
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
