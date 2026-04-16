use actix_web::{web, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{
    api::extractors::AuthRequired,
    application::reporting_service::{
        CreateSubscriptionInput, ListAlertsInput, PublishDashboardInput, ReportingService,
        UpdateSubscriptionInput,
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
pub struct ListSubscriptionsQuery {
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_per_page")]
    pub per_page: u32,
}

#[derive(Debug, Deserialize)]
pub struct ListAlertsQuery {
    pub acknowledged: Option<bool>,
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
pub struct CreateSubscriptionRequest {
    #[validate(length(min = 1, max = 50))]
    pub report_type: String,
    #[serde(default)]
    pub parameters: serde_json::Value,
    pub cron_expression: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateSubscriptionRequest {
    pub report_type: Option<String>,
    pub parameters: Option<serde_json::Value>,
    /// `null` means "clear the cron expression"; absent means "no change".
    pub cron_expression: Option<serde_json::Value>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PublishDashboardRequest {
    pub layout: serde_json::Value,
}

// ============================================================
// Handlers
// ============================================================

pub async fn list_subscriptions(
    state: web::Data<AppState>,
    auth: AuthRequired,
    query: web::Query<ListSubscriptionsQuery>,
) -> Result<HttpResponse, AppError> {
    let ctx = auth.into_inner();
    let page = query.page.max(1) as i64;
    let per_page = crate::shared::pagination::clamp_per_page(query.per_page);

    let mut conn = state
        .db_pool
        .get()
        .map_err(|e| AppError::Internal(format!("db pool: {e}")))?;

    let (items, total) =
        web::block(move || ReportingService::list_subscriptions(&mut conn, &ctx, page, per_page))
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

pub async fn create_subscription(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    body: web::Json<CreateSubscriptionRequest>,
) -> Result<HttpResponse, AppError> {
    body.validate().map_err(validation_error)?;
    let ctx = auth.into_inner();
    let body_bytes = serde_json::to_vec(&*body).unwrap_or_default();
    let (idempotency_key, request_hash) = idempotency_info(&req, &body_bytes);
    let input = CreateSubscriptionInput {
        report_type: body.report_type.clone(),
        parameters: body.parameters.clone(),
        cron_expression: body.cron_expression.clone(),
        idempotency_key,
        request_hash,
    };

    let mut conn = state
        .db_pool
        .get()
        .map_err(|e| AppError::Internal(format!("db pool: {e}")))?;

    let sub = web::block(move || ReportingService::create_subscription(&mut conn, &ctx, input))
        .await
        .map_err(|e| AppError::Internal(format!("blocking: {e}")))?
        .map_err(|e| e)?;

    Ok(HttpResponse::Created().json(ApiResponse::ok(sub)))
}

pub async fn get_subscription(
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

    let sub = web::block(move || ReportingService::get_subscription(&mut conn, &ctx, id))
        .await
        .map_err(|e| AppError::Internal(format!("blocking: {e}")))?
        .map_err(|e| e)?;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(sub)))
}

pub async fn update_subscription(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<Uuid>,
    body: web::Json<UpdateSubscriptionRequest>,
) -> Result<HttpResponse, AppError> {
    let ctx = auth.into_inner();
    let id = path.into_inner();
    let body_bytes = serde_json::to_vec(&*body).unwrap_or_default();
    let (idempotency_key, request_hash) = idempotency_info(&req, &body_bytes);

    // Interpret cron_expression: null → Some(None), string → Some(Some(s)), absent → None
    let cron = match &body.cron_expression {
        Some(serde_json::Value::Null) => Some(None),
        Some(serde_json::Value::String(s)) => Some(Some(s.clone())),
        None => None,
        _ => None,
    };

    let input = UpdateSubscriptionInput {
        report_type: body.report_type.clone(),
        parameters: body.parameters.clone(),
        cron_expression: cron,
        is_active: body.is_active,
        idempotency_key,
        request_hash,
    };

    let mut conn = state
        .db_pool
        .get()
        .map_err(|e| AppError::Internal(format!("db pool: {e}")))?;

    let sub = web::block(move || ReportingService::update_subscription(&mut conn, &ctx, id, input))
        .await
        .map_err(|e| AppError::Internal(format!("blocking: {e}")))?
        .map_err(|e| e)?;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(sub)))
}

pub async fn delete_subscription(
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

    web::block(move || ReportingService::delete_subscription(&mut conn, &ctx, id))
        .await
        .map_err(|e| AppError::Internal(format!("blocking: {e}")))?
        .map_err(|e| e)?;

    Ok(HttpResponse::NoContent().finish())
}

pub async fn list_dashboard_versions(
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let ctx = auth.into_inner();
    let key = path.into_inner();
    let mut conn = state
        .db_pool
        .get()
        .map_err(|e| AppError::Internal(format!("db pool: {e}")))?;

    let versions =
        web::block(move || ReportingService::list_dashboard_versions(&mut conn, &ctx, &key))
            .await
            .map_err(|e| AppError::Internal(format!("blocking: {e}")))?
            .map_err(|e| e)?;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(versions)))
}

pub async fn publish_dashboard(
    req: HttpRequest,
    state: web::Data<AppState>,
    auth: AuthRequired,
    path: web::Path<String>,
    body: web::Json<PublishDashboardRequest>,
) -> Result<HttpResponse, AppError> {
    let ctx = auth.into_inner();
    let dashboard_key = path.into_inner();
    let body_bytes = serde_json::to_vec(&*body).unwrap_or_default();
    let (idempotency_key, request_hash) = idempotency_info(&req, &body_bytes);
    let input = PublishDashboardInput {
        dashboard_key,
        layout: body.layout.clone(),
        idempotency_key,
        request_hash,
    };

    let mut conn = state
        .db_pool
        .get()
        .map_err(|e| AppError::Internal(format!("db pool: {e}")))?;

    let version = web::block(move || ReportingService::publish_dashboard(&mut conn, &ctx, input))
        .await
        .map_err(|e| AppError::Internal(format!("blocking: {e}")))?
        .map_err(|e| e)?;

    Ok(HttpResponse::Created().json(ApiResponse::ok(version)))
}

pub async fn list_alerts(
    state: web::Data<AppState>,
    auth: AuthRequired,
    query: web::Query<ListAlertsQuery>,
) -> Result<HttpResponse, AppError> {
    let ctx = auth.into_inner();
    let input = ListAlertsInput {
        acknowledged: query.acknowledged,
        page: query.page.max(1) as i64,
        per_page: crate::shared::pagination::clamp_per_page(query.per_page),
    };

    let mut conn = state
        .db_pool
        .get()
        .map_err(|e| AppError::Internal(format!("db pool: {e}")))?;

    let (items, total) = web::block(move || ReportingService::list_alerts(&mut conn, &ctx, input))
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

pub async fn acknowledge_alert(
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

    let alert = web::block(move || ReportingService::acknowledge_alert(&mut conn, id, &ctx))
        .await
        .map_err(|e| AppError::Internal(format!("blocking: {e}")))?
        .map_err(|e| e)?;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(alert)))
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
