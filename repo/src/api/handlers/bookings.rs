use actix_web::{web, HttpRequest, HttpResponse};
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{
    api::extractors::AuthRequired,
    application::booking_service::{
        BookingService, CancelInput, CreateHoldInput, ExceptionInput, ListBookingsInput,
        RescheduleInput, SubmitAgreementInput,
    },
    domain::bookings::models::{AgreementEvidence, BookingStatus, EligibilityResult},
    shared::{
        app_state::AppState,
        errors::{AppError, FieldError},
        idempotency::body_hash,
        response::{ApiResponse, PaginatedEnvelope, PaginationMeta},
    },
};

// ============================================================
// Request DTOs
// ============================================================

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct CreateHoldRequest {
    pub candidate_id: Uuid,
    pub site_id: Uuid,
    pub slot_id: Uuid,
    #[validate(length(max = 4096))]
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct SubmitAgreementRequest {
    #[validate(length(min = 1, max = 200))]
    pub typed_name: String,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct CancelRequest {
    #[validate(length(max = 4096))]
    pub reason: Option<String>,
    #[validate(length(max = 50))]
    pub reason_code: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RescheduleRequest {
    pub new_slot_id: Uuid,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct ExceptionRequest {
    #[validate(length(min = 1, max = 4096))]
    pub detail: String,
}

// ============================================================
// Query params
// ============================================================

#[derive(Debug, Deserialize)]
pub struct ListBookingsQuery {
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
pub struct BookingResponse {
    pub id: Uuid,
    pub candidate_id: Uuid,
    pub site_id: Uuid,
    pub slot_id: Option<Uuid>,
    pub status: String,
    pub scheduled_date: NaiveDate,
    pub scheduled_time_start: Option<NaiveTime>,
    pub scheduled_time_end: Option<NaiveTime>,
    pub hold_expires_at: Option<DateTime<Utc>>,
    pub agreement: Option<AgreementResponse>,
    pub breach_reason: Option<String>,
    pub breach_reason_code: Option<String>,
    pub exception_detail: Option<String>,
    pub notes: Option<String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct AgreementResponse {
    pub signed_by: String,
    pub signed_at: DateTime<Utc>,
    pub hash: String,
}

#[derive(Debug, Serialize)]
pub struct ConfirmResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub booking: Option<BookingResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eligibility: Option<EligibilityResult>,
}

// ============================================================
// Idempotency key extraction
// ============================================================

fn extract_idempotency_key(req: &HttpRequest) -> Option<String> {
    req.headers()
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

// ============================================================
// Handlers
// ============================================================

/// POST /api/v1/bookings — create a hold on an inventory slot
pub async fn create_booking(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    body: web::Json<CreateHoldRequest>,
) -> Result<HttpResponse, AppError> {
    body.validate().map_err(validation_error)?;
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    let idempotency_key = extract_idempotency_key(&req);

    // Compute request hash from the serialised body for conflict detection
    let request_hash = idempotency_key.as_ref().map(|_| {
        let bytes = serde_json::to_vec(&*body).unwrap_or_default();
        body_hash(&bytes)
    });

    let input = CreateHoldInput {
        candidate_id: body.candidate_id,
        site_id: body.site_id,
        slot_id: body.slot_id,
        notes: body.notes.clone(),
        idempotency_key,
        request_hash,
    };

    let order = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        BookingService::create_hold(&mut conn, &ctx, input)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(HttpResponse::Created().json(ApiResponse::ok(to_response(order))))
}

/// GET /api/v1/bookings
pub async fn list_bookings(
    state: web::Data<AppState>,
    auth: AuthRequired,
    query: web::Query<ListBookingsQuery>,
) -> Result<HttpResponse, AppError> {
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    let candidate_id = query.candidate_id;
    let page = query.page.max(1) as i64;
    let per_page = crate::shared::pagination::clamp_per_page(query.per_page);

    let (orders, total) = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        BookingService::list(
            &mut conn,
            &ctx,
            ListBookingsInput {
                candidate_id,
                page,
                per_page,
            },
        )
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    let data: Vec<BookingResponse> = orders.into_iter().map(to_response).collect();
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

/// GET /api/v1/bookings/{id}
pub async fn get_booking(
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let id = path.into_inner();
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();

    let order = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        BookingService::get(&mut conn, &ctx, id)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(to_response(order))))
}

/// POST /api/v1/bookings/{id}/agreement — submit agreement evidence
pub async fn submit_agreement(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<Uuid>,
    body: web::Json<SubmitAgreementRequest>,
) -> Result<HttpResponse, AppError> {
    body.validate().map_err(validation_error)?;
    let id = path.into_inner();
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    let body_bytes = serde_json::to_vec(&*body).unwrap_or_default();
    let idempotency_key = extract_idempotency_key(&req);
    let request_hash = idempotency_key.as_ref().map(|_| body_hash(&body_bytes));

    let input = SubmitAgreementInput {
        typed_name: body.typed_name.clone(),
        idempotency_key,
        request_hash,
    };

    let evidence = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        BookingService::submit_agreement(&mut conn, &ctx, id, input)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(AgreementResponse {
        signed_by: evidence.typed_name,
        signed_at: evidence.signed_at,
        hash: evidence.hash,
    })))
}

/// POST /api/v1/bookings/{id}/confirm — run eligibility gate and confirm
pub async fn confirm_booking(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let id = path.into_inner();
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    let idempotency_key = extract_idempotency_key(&req);

    let result = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        BookingService::confirm(&mut conn, &ctx, id, idempotency_key.as_deref())
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    match result {
        Ok(order) => Ok(HttpResponse::Ok().json(ApiResponse::ok(ConfirmResponse {
            booking: Some(to_response(order)),
            eligibility: None,
        }))),
        Err(eligibility) => {
            // 422 with eligibility failure details
            Ok(HttpResponse::UnprocessableEntity().json(serde_json::json!({
                "error": {
                    "code": "eligibility_failed",
                    "message": "booking confirmation blocked by eligibility checks",
                    "details": eligibility.checks,
                }
            })))
        }
    }
}

/// POST /api/v1/bookings/{id}/start
pub async fn start_booking(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let id = path.into_inner();
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    let idempotency_key = extract_idempotency_key(&req);
    let request_hash = idempotency_key.as_ref().map(|_| body_hash(b""));

    let order = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        BookingService::start(&mut conn, &ctx, id, idempotency_key, request_hash)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(to_response(order))))
}

/// POST /api/v1/bookings/{id}/complete
pub async fn complete_booking(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let id = path.into_inner();
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    let idempotency_key = extract_idempotency_key(&req);
    let request_hash = idempotency_key.as_ref().map(|_| body_hash(b""));

    let order = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        BookingService::complete(&mut conn, &ctx, id, idempotency_key, request_hash)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(to_response(order))))
}

/// POST /api/v1/bookings/{id}/cancel
pub async fn cancel_booking(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<Uuid>,
    body: web::Json<CancelRequest>,
) -> Result<HttpResponse, AppError> {
    body.validate().map_err(validation_error)?;
    let id = path.into_inner();
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    let idempotency_key = extract_idempotency_key(&req);
    let request_hash = idempotency_key.as_ref().map(|_| {
        let bytes = serde_json::to_vec(&*body).unwrap_or_default();
        body_hash(&bytes)
    });

    let input = CancelInput {
        reason: body.reason.clone(),
        reason_code: body.reason_code.clone(),
    };

    let order = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        BookingService::cancel(&mut conn, &ctx, id, input, idempotency_key, request_hash)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(to_response(order))))
}

/// POST /api/v1/bookings/{id}/reschedule
pub async fn reschedule_booking(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<Uuid>,
    body: web::Json<RescheduleRequest>,
) -> Result<HttpResponse, AppError> {
    let id = path.into_inner();
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    let idempotency_key = extract_idempotency_key(&req);
    let request_hash = idempotency_key.as_ref().map(|_| {
        let bytes = serde_json::to_vec(&*body).unwrap_or_default();
        body_hash(&bytes)
    });

    let input = RescheduleInput {
        new_slot_id: body.new_slot_id,
    };

    let order = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        BookingService::reschedule(&mut conn, &ctx, id, input, idempotency_key, request_hash)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(to_response(order))))
}

/// POST /api/v1/bookings/{id}/exception
pub async fn mark_exception(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<Uuid>,
    body: web::Json<ExceptionRequest>,
) -> Result<HttpResponse, AppError> {
    body.validate().map_err(validation_error)?;
    let id = path.into_inner();
    let ctx = auth.into_inner();
    let pool = state.db_pool.clone();
    let idempotency_key = extract_idempotency_key(&req);
    let request_hash = idempotency_key.as_ref().map(|_| {
        let bytes = serde_json::to_vec(&*body).unwrap_or_default();
        body_hash(&bytes)
    });

    let input = ExceptionInput {
        detail: body.detail.clone(),
    };

    let order = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        BookingService::mark_exception(&mut conn, &ctx, id, input, idempotency_key, request_hash)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(to_response(order))))
}

// ============================================================
// Site response DTO
// ============================================================

#[derive(Debug, Serialize)]
pub struct SiteResponse {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub address: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub timezone: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

fn to_site_response(s: crate::infrastructure::db::models::DbOfficeSite) -> SiteResponse {
    SiteResponse {
        id: s.id,
        code: s.code,
        name: s.name,
        address: s.address,
        latitude: s.latitude,
        longitude: s.longitude,
        timezone: s.timezone,
        is_active: s.is_active,
        created_at: s.created_at,
    }
}

/// GET /api/v1/sites — return all active sites
pub async fn list_sites(
    state: web::Data<AppState>,
    _auth: AuthRequired,
) -> Result<HttpResponse, AppError> {
    let pool = state.db_pool.clone();

    let sites = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        crate::infrastructure::db::repositories::site_repo::PgSiteRepository::list_active(&mut conn)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    let data: Vec<SiteResponse> = sites.into_iter().map(to_site_response).collect();
    Ok(HttpResponse::Ok().json(ApiResponse::ok(data)))
}

/// GET /api/v1/sites/{id} — return site or 404
pub async fn get_site(
    state: web::Data<AppState>,
    _auth: AuthRequired,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let id = path.into_inner();
    let pool = state.db_pool.clone();

    let site = web::block(move || {
        let mut conn = pool.get().map_err(|e| AppError::Internal(e.to_string()))?;
        crate::infrastructure::db::repositories::site_repo::PgSiteRepository::find_by_id(
            &mut conn, id,
        )
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??
    .ok_or_else(|| AppError::NotFound("site".into()))?;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(to_site_response(site))))
}

// ============================================================
// Response mapping
// ============================================================

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

fn to_response(o: crate::domain::bookings::models::BookingOrder) -> BookingResponse {
    BookingResponse {
        id: o.id,
        candidate_id: o.candidate_id,
        site_id: o.site_id,
        slot_id: o.slot_id,
        status: o.status.as_str().to_string(),
        scheduled_date: o.scheduled_date,
        scheduled_time_start: o.scheduled_time_start,
        scheduled_time_end: o.scheduled_time_end,
        hold_expires_at: o.hold_expires_at,
        agreement: o.agreement_evidence.map(|a| AgreementResponse {
            signed_by: a.typed_name,
            signed_at: a.signed_at,
            hash: a.hash,
        }),
        breach_reason: o.breach_reason,
        breach_reason_code: o.breach_reason_code,
        exception_detail: o.exception_detail,
        notes: o.notes,
        created_by: o.created_by,
        created_at: o.created_at,
        updated_at: o.updated_at,
    }
}
