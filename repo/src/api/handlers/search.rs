use actix_web::{web, HttpResponse};
use serde::{Deserialize, Serialize};

use crate::{
    api::extractors::AuthRequired,
    application::search_service::{AutocompleteInput, SearchInput, SearchService, SortField},
    shared::{
        app_state::AppState,
        errors::AppError,
        response::{ApiResponse, PaginationMeta, ResponseMeta},
    },
};

// ============================================================
// Query params
// ============================================================

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: Option<String>,
    pub tags: Option<String>, // comma-separated
    pub status: Option<String>,
    pub sort_by: Option<String>,
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_per_page")]
    pub per_page: u32,
    // Additive filters — omitting them preserves existing behavior
    pub min_rating: Option<f64>,
    pub max_rating: Option<f64>,
    pub max_distance_miles: Option<f64>,
    pub site_code: Option<String>,
    // Business-native facets (additive, all optional)
    pub department: Option<String>,
    pub source: Option<String>,
    /// Minimum annual salary in whole dollars (compared against offers.salary_cents).
    pub salary_min: Option<i64>,
    /// Maximum annual salary in whole dollars (compared against offers.salary_cents).
    pub salary_max: Option<i64>,
    /// Comma-separated vocabulary categories for domain-vocabulary filtering.
    pub categories: Option<String>,
    /// Minimum total compensation in whole dollars (salary + bonus target).
    pub price_min: Option<i64>,
    /// Maximum total compensation in whole dollars (salary + bonus target).
    pub price_max: Option<i64>,
    /// Minimum domain-native quality score (0.0–5.0). Excludes non-domain-rated items.
    pub quality_min: Option<f64>,
    /// Maximum domain-native quality score (0.0–5.0). Excludes non-domain-rated items.
    pub quality_max: Option<f64>,
}
fn default_page() -> u32 {
    1
}
fn default_per_page() -> u32 {
    25
}

#[derive(Debug, Deserialize)]
pub struct AutocompleteQuery {
    pub prefix: Option<String>,
    pub categories: Option<String>, // comma-separated
    #[serde(default = "default_autocomplete_limit")]
    pub limit: u32,
}
fn default_autocomplete_limit() -> u32 {
    10
}

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    #[serde(default = "default_history_limit")]
    pub limit: u32,
}
fn default_history_limit() -> u32 {
    20
}

// ============================================================
// Search-specific response type (extends pagination with spell_correction)
// ============================================================

#[derive(Serialize)]
struct SearchEnvelope<T: Serialize> {
    data: Vec<T>,
    pagination: PaginationMeta,
    #[serde(skip_serializing_if = "Option::is_none")]
    spell_correction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    meta: Option<ResponseMeta>,
}

// ============================================================
// Handlers
// ============================================================

pub async fn search(
    state: web::Data<AppState>,
    auth: AuthRequired,
    query: web::Query<SearchQuery>,
) -> Result<HttpResponse, AppError> {
    let ctx = auth.into_inner();

    let tags: Option<Vec<String>> = query.tags.as_ref().map(|s| {
        s.split(',')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect()
    });

    let categories: Option<Vec<String>> = query.categories.as_ref().map(|s| {
        s.split(',')
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
            .collect()
    });

    let sort_by = match query.sort_by.as_deref() {
        Some("relevance") | None => Some(SortField::Relevance),
        Some("recency") => Some(SortField::Recency),
        Some("tag_overlap") => Some(SortField::TagOverlap),
        Some("popularity") => Some(SortField::Popularity),
        Some("rating") => Some(SortField::Rating),
        Some("distance") => Some(SortField::Distance),
        Some(other) => {
            return Err(AppError::Validation(vec![
                crate::shared::errors::FieldError {
                    field: "sort_by".into(),
                    message: format!(
                        "unknown sort field '{other}'; valid: relevance, recency, tag_overlap, popularity, rating, distance"
                    ),
                },
            ]));
        }
    };

    let input = SearchInput {
        q: query.q.clone(),
        tags,
        status: query.status.clone(),
        sort_by,
        page: query.page.max(1) as i64,
        per_page: crate::shared::pagination::clamp_per_page(query.per_page),
        min_rating: query.min_rating,
        max_rating: query.max_rating,
        max_distance_miles: query.max_distance_miles,
        site_code: query.site_code.clone(),
        department: query.department.clone(),
        source: query.source.clone(),
        salary_min_cents: query.salary_min.map(|d| d * 100),
        salary_max_cents: query.salary_max.map(|d| d * 100),
        categories,
        price_min_cents: query.price_min.map(|d| d * 100),
        price_max_cents: query.price_max.map(|d| d * 100),
        quality_min: query.quality_min,
        quality_max: query.quality_max,
    };

    let mut conn = state
        .db_pool
        .get()
        .map_err(|e| AppError::Internal(format!("db pool: {e}")))?;

    let result = web::block(move || SearchService::search(&mut conn, &ctx, input))
        .await
        .map_err(|e| AppError::Internal(format!("blocking error: {e}")))?
        .map_err(|e| e)?;

    Ok(HttpResponse::Ok().json(SearchEnvelope {
        data: result.results,
        pagination: PaginationMeta {
            page: query.page,
            per_page: query.per_page,
            total: result.total,
        },
        spell_correction: result.spell_correction,
        meta: None,
    }))
}

pub async fn search_history(
    state: web::Data<AppState>,
    auth: AuthRequired,
    query: web::Query<HistoryQuery>,
) -> Result<HttpResponse, AppError> {
    let ctx = auth.into_inner();
    let user_id = ctx.user_id;
    let limit = query.limit as i64;

    let mut conn = state
        .db_pool
        .get()
        .map_err(|e| AppError::Internal(format!("db pool: {e}")))?;

    let entries = web::block(move || SearchService::search_history(&mut conn, user_id, limit))
        .await
        .map_err(|e| AppError::Internal(format!("blocking error: {e}")))?
        .map_err(|e| e)?;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(entries)))
}

pub async fn autocomplete(
    state: web::Data<AppState>,
    _auth: AuthRequired,
    query: web::Query<AutocompleteQuery>,
) -> Result<HttpResponse, AppError> {
    let prefix = query.prefix.clone().unwrap_or_default();
    let categories: Vec<String> = query
        .categories
        .as_ref()
        .map(|s| {
            s.split(',')
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let limit = query.limit as i64;

    let mut conn = state
        .db_pool
        .get()
        .map_err(|e| AppError::Internal(format!("db pool: {e}")))?;

    let suggestions = web::block(move || {
        SearchService::autocomplete(
            &mut conn,
            AutocompleteInput {
                prefix,
                categories,
                limit,
            },
        )
    })
    .await
    .map_err(|e| AppError::Internal(format!("blocking error: {e}")))?
    .map_err(|e| e)?;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(suggestions)))
}

pub async fn list_vocabularies(
    state: web::Data<AppState>,
    _auth: AuthRequired,
) -> Result<HttpResponse, AppError> {
    let mut conn = state
        .db_pool
        .get()
        .map_err(|e| AppError::Internal(format!("db pool: {e}")))?;

    let categories = web::block(move || SearchService::list_vocabulary_categories(&mut conn))
        .await
        .map_err(|e| AppError::Internal(format!("blocking error: {e}")))?
        .map_err(|e| e)?;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(categories)))
}

pub async fn get_vocabulary(
    state: web::Data<AppState>,
    _auth: AuthRequired,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let category = path.into_inner();

    let mut conn = state
        .db_pool
        .get()
        .map_err(|e| AppError::Internal(format!("db pool: {e}")))?;

    let entries = web::block(move || SearchService::get_vocabulary(&mut conn, &category))
        .await
        .map_err(|e| AppError::Internal(format!("blocking error: {e}")))?
        .map_err(|e| e)?;

    Ok(HttpResponse::Ok().json(ApiResponse::ok(entries)))
}
