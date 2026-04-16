/// Search and discovery service.
///
/// ## Scoring function (deterministic)
///
/// Each candidate result is scored as a float in [0.0, 1.0]:
///
/// ```text
/// score = (exact_match_bonus * 0.40)
///       + (tag_overlap_ratio  * 0.35)
///       + (recency_score      * 0.25)
/// ```
///
/// Where:
/// - `exact_match_bonus`: 1.0 if query matches full name/email exactly, else 0.0
/// - `tag_overlap_ratio`: |requested_tags ∩ candidate_tags| / |requested_tags| (0.0 if no tags requested)
/// - `recency_score`: linear decay over 365 days — candidate created within last year scores 1.0 → 0.0
///
/// Offer results use a simpler two-factor score:
/// - `exact_match_bonus * 0.60 + recency_score * 0.40`
///
/// Ties are broken by resource ID (lexicographic) to produce a stable order.
///
/// ## Derived fields
///
/// - `rating`: domain-native when available, relevance-derived fallback.
///   - **Offers with salary_cents**: log-linear map of annual salary into
///     [0.0, 5.0] using a $30k–$300k reference band.
///   - **All other results**: `score * 5.0` (relevance-derived).
///   See [`domain_rating`] for the deterministic formula.
/// - `popularity_score`: `tag_density * 0.6 + recency * 0.4` — proxy engagement metric
/// - `distance_miles`: Haversine distance from a reference site (requires `site_code` param and
///   coordinates on the resource — returns `None` when no location basis exists)
/// - `recommended`: `true` when `score >= 0.5` — marks high-confidence results for interleaving
///
/// ## Interleaving
///
/// When results contain both recommended and regular items, they are interleaved at a 3:1 ratio:
/// three regular items followed by one recommended item, cycling until one list is exhausted,
/// then appending the remainder.  The ordering within each group is stable (sort field +
/// resource_type + id tie-break), making the final sequence fully deterministic.
///
/// ## Spell correction
///
/// The service provides best-effort spell correction by finding the closest
/// vocabulary label or historical query within edit distance 2.  If a corrected
/// form is found the suggestion is included in the response.
///
/// ## Business-native facets (additive, backward-compatible)
///
/// All new facets are optional query parameters.  When omitted, behavior is
/// identical to the pre-facet API.
///
/// | Param         | Type   | Applies to  | Semantics                                  |
/// |---------------|--------|-------------|--------------------------------------------|
/// | `department`  | string | offers      | Exact match on offers.department (case-insensitive) |
/// | `source`      | string | candidates  | Exact match on candidates.source (case-insensitive) |
/// | `salary_min`  | i64    | offers      | Minimum base salary in dollars (converted to cents) |
/// | `salary_max`  | i64    | offers      | Maximum base salary in dollars (converted to cents) |
/// | `categories`  | string | both        | Comma-separated vocabulary categories; resolves values from `controlled_vocabularies` and filters candidates by tag match / offers by department match |
/// | `price_min`   | i64    | offers      | Minimum **total comp** in dollars (salary + bonus target) |
/// | `price_max`   | i64    | offers      | Maximum **total comp** in dollars (salary + bonus target) |
/// | `quality_min` | f64    | domain-rated only | Minimum domain-native quality score (0–5); **excludes** items without domain rating |
/// | `quality_max` | f64    | domain-rated only | Maximum domain-native quality score (0–5); **excludes** items without domain rating |
///
/// Salary/price filters require offers to have a non-NULL `salary_cents` value;
/// offers without salary data are excluded when these filters are active.
///
/// `quality_min`/`quality_max` are **exclusive gates** — unlike `min_rating`/`max_rating`
/// (which pass items without a rating through), quality filters exclude items
/// that lack a domain-native quality signal (e.g., candidates, offers without
/// salary data).
///
/// ## Autocomplete
///
/// Returns merged suggestions from approved vocabulary labels and the user's
/// historical query dictionary, deduplicated, vocabulary first.
use chrono::Utc;
use diesel::PgConnection;
use serde::Serialize;
use uuid::Uuid;

use crate::{
    domain::auth::models::AuthContext,
    infrastructure::db::{
        models::{DbCandidate, DbOffer},
        repositories::{search_repo::PgSearchRepository, site_repo::PgSiteRepository},
    },
    shared::errors::AppError,
};

// ============================================================
// Request/response types
// ============================================================

pub struct SearchInput {
    pub q: Option<String>,
    pub tags: Option<Vec<String>>,
    pub status: Option<String>,
    pub sort_by: Option<SortField>,
    pub page: i64,
    pub per_page: i64,
    // Additive filters (all optional — omitting preserves existing behavior)
    pub min_rating: Option<f64>,
    pub max_rating: Option<f64>,
    pub max_distance_miles: Option<f64>,
    pub site_code: Option<String>,
    // Business-native facets (all optional — omitting preserves existing behavior)
    /// Filter offers by department (exact match, case-insensitive).
    pub department: Option<String>,
    /// Filter candidates by acquisition source (exact match, case-insensitive).
    pub source: Option<String>,
    /// Minimum annual salary in cents (filters offers by salary_cents >= this value).
    pub salary_min_cents: Option<i64>,
    /// Maximum annual salary in cents (filters offers by salary_cents <= this value).
    pub salary_max_cents: Option<i64>,
    /// Vocabulary categories for vocabulary-driven filtering.
    /// Candidates pass if their tags include values from the resolved `candidate_tag` category.
    /// Offers pass if their department matches a value from the resolved `department` category.
    pub categories: Option<Vec<String>>,
    /// Minimum total compensation in cents (salary + bonus target).
    /// Distinct from `salary_min_cents` which is base salary only.
    pub price_min_cents: Option<i64>,
    /// Maximum total compensation in cents (salary + bonus target).
    pub price_max_cents: Option<i64>,
    /// Minimum domain-native quality score (0.0–5.0).
    /// Unlike `min_rating` (which passes items without a rating through),
    /// this is an exclusive gate: items without a domain-native quality
    /// score are excluded.
    pub quality_min: Option<f64>,
    /// Maximum domain-native quality score (0.0–5.0), exclusive gate.
    pub quality_max: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortField {
    /// Sort by computed relevance score (default)
    Relevance,
    /// Sort by created_at descending
    Recency,
    /// Sort by tag overlap (for candidate searches)
    TagOverlap,
    /// Sort by derived popularity score descending
    Popularity,
    /// Sort by derived rating descending
    Rating,
    /// Sort by distance_miles ascending (None values sort last)
    Distance,
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub resource_type: String,
    pub id: Uuid,
    pub title: String,
    pub subtitle: Option<String>,
    pub score: f64,
    pub tags: Vec<String>,
    pub status: Option<String>,
    pub created_at: String,
    // Additive fields — always present in new responses, None when no basis exists
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance_miles: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub popularity_score: Option<f64>,
    pub recommended: bool,
    /// True when `rating` is derived from a domain-native signal (e.g.,
    /// salary-based for offers).  False for relevance-only fallback.
    /// Not serialized — used internally by the `quality_min`/`quality_max` gate.
    #[serde(skip_serializing)]
    pub domain_rated: bool,
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
    /// Non-null when a spelling correction was applied.
    pub spell_correction: Option<String>,
}

pub struct AutocompleteInput {
    pub prefix: String,
    /// Vocabulary categories to include (defaults to all if empty).
    pub categories: Vec<String>,
    pub limit: i64,
}

// ============================================================
// Service
// ============================================================

pub struct SearchService;

impl SearchService {
    // ── Unified search ────────────────────────────────────────────────────────

    /// Execute a unified search across candidates and offers.
    ///
    /// Results are scored, merged, and sorted by the requested sort field.
    /// Recommended items (score ≥ 0.5) are interleaved with regular items at
    /// a 3:1 ratio.  The query is persisted to `historical_queries` for future
    /// autocomplete.
    pub fn search(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        input: SearchInput,
    ) -> Result<SearchResponse, AppError> {
        let q_ref = input.q.as_deref();
        let tags_ref: Option<&[String]> = input.tags.as_deref();

        // Resolve reference site coordinates for distance (best-effort)
        let ref_coords: Option<(f64, f64)> = if let Some(ref code) = input.site_code {
            match PgSiteRepository::find_by_code(conn, code)? {
                Some(site) => match (site.latitude, site.longitude) {
                    (Some(lat), Some(lng)) => Some((lat, lng)),
                    _ => None,
                },
                None => None,
            }
        } else {
            None
        };

        // Resolve vocabulary-driven category filter (if requested).
        // Returns sets of allowed values per resource type; empty set = no filter.
        let (cat_tag_values, cat_dept_values) = if let Some(ref cats) = input.categories {
            let cat_refs: Vec<&str> = cats.iter().map(String::as_str).collect();
            let vocab_pairs =
                PgSearchRepository::vocabulary_values_for_categories(conn, &cat_refs)
                    .unwrap_or_default();
            let tags: std::collections::HashSet<String> = vocab_pairs
                .iter()
                .filter(|(c, _)| c == "candidate_tag")
                .map(|(_, v)| v.to_lowercase())
                .collect();
            let depts: std::collections::HashSet<String> = vocab_pairs
                .iter()
                .filter(|(c, _)| c == "department")
                .map(|(_, v)| v.to_lowercase())
                .collect();
            (tags, depts)
        } else {
            (
                std::collections::HashSet::new(),
                std::collections::HashSet::new(),
            )
        };

        // Search candidates
        let (cand_rows, cand_total) = PgSearchRepository::search_candidates(
            conn,
            q_ref,
            tags_ref,
            input.source.as_deref(),
            input.page,
            input.per_page,
        )?;

        // Apply vocabulary category filter on raw candidate rows (before scoring)
        let cand_rows: Vec<DbCandidate> = if !cat_tag_values.is_empty() {
            cand_rows
                .into_iter()
                .filter(|c| {
                    c.tags
                        .iter()
                        .any(|t| cat_tag_values.contains(&t.to_lowercase()))
                })
                .collect()
        } else {
            cand_rows
        };

        // Search offers (price filter is SQL-level)
        let (offer_rows, offer_total) = PgSearchRepository::search_offers(
            conn,
            q_ref,
            input.status.as_deref(),
            input.department.as_deref(),
            input.salary_min_cents,
            input.salary_max_cents,
            input.price_min_cents,
            input.price_max_cents,
            input.page,
            input.per_page,
        )?;

        // Apply vocabulary category filter on raw offer rows (before scoring)
        let offer_rows: Vec<DbOffer> = if !cat_dept_values.is_empty() {
            offer_rows
                .into_iter()
                .filter(|o| {
                    o.department
                        .as_ref()
                        .map_or(false, |d| cat_dept_values.contains(&d.to_lowercase()))
                })
                .collect()
        } else {
            offer_rows
        };

        // Score and convert
        let now = Utc::now();
        let mut results: Vec<SearchResult> = Vec::new();

        for c in cand_rows {
            let recency = Self::recency_score(c.created_at, &now);
            let score = Self::score_candidate(&c, q_ref, tags_ref, &now);
            // Candidates have no persisted domain rating — derive from relevance score.
            let rating = Some(score * 5.0);
            let popularity_score = Some(Self::popularity_from_tags(c.tags.len(), recency));
            // Compute Haversine distance when both the reference site and the
            // candidate's home coordinates are available.  If either is absent
            // (site_code not provided, site has no coordinates, or candidate
            // has no stored latitude/longitude), distance_miles stays None and
            // the item passes through any max_distance_miles filter unchanged.
            let distance_miles: Option<f64> =
                ref_coords.and_then(|(ref_lat, ref_lng)| match (c.latitude, c.longitude) {
                    (Some(lat), Some(lng)) => {
                        Some(Self::haversine_miles(ref_lat, ref_lng, lat, lng))
                    }
                    _ => None,
                });
            let recommended = score >= 0.5;

            results.push(SearchResult {
                resource_type: "candidate".into(),
                id: c.id,
                title: format!("{} {}", c.first_name, c.last_name),
                subtitle: Some(c.email.clone()),
                score,
                tags: c.tags.clone(),
                status: None,
                created_at: c.created_at.to_rfc3339(),
                rating,
                distance_miles,
                popularity_score,
                recommended,
                domain_rated: false, // candidates have no domain-native rating
            });
        }

        for o in offer_rows {
            let recency = Self::recency_score(o.created_at, &now);
            let score = Self::score_offer(&o, q_ref, &now);
            let has_salary = o.salary_cents.map_or(false, |c| c > 0);
            // Rating precedence for offers:
            //   1. Domain-native: when salary_cents is present, derive a 0–5
            //      rating from compensation positioning within a reference band
            //      ($30k–$300k).  This surfaces real business signal.
            //   2. Fallback: score * 5.0 (relevance-derived) when no salary data.
            let rating = Some(Self::offer_rating(&o, score));
            let popularity_score = Some(Self::popularity_from_tags(0, recency));
            // Offers have no physical location (they describe roles, not sites),
            // so distance_miles is always None.  A None value passes through
            // the max_distance_miles filter unchanged (map_or(true, ...)) and
            // sorts last under sort_by=distance — both are correct by design.
            let distance_miles: Option<f64> = None;
            let recommended = score >= 0.5;

            results.push(SearchResult {
                resource_type: "offer".into(),
                id: o.id,
                title: o.title.clone(),
                subtitle: o.department.clone(),
                score,
                tags: vec![],
                status: Some(o.status.clone()),
                created_at: o.created_at.to_rfc3339(),
                rating,
                distance_miles,
                popularity_score,
                recommended,
                domain_rated: has_salary,
            });
        }

        // Apply additive filters (items without a basis value pass through)
        if let Some(min_r) = input.min_rating {
            results.retain(|r| r.rating.map_or(true, |v| v >= min_r));
        }
        if let Some(max_r) = input.max_rating {
            results.retain(|r| r.rating.map_or(true, |v| v <= max_r));
        }
        if let Some(max_d) = input.max_distance_miles {
            results.retain(|r| r.distance_miles.map_or(true, |v| v <= max_d));
        }

        // Quality gate: unlike min_rating/max_rating (which pass items without a
        // rating through), quality_min/quality_max EXCLUDE items that are not
        // domain-rated.  This lets callers request "only results with a proven
        // business quality metric that meets this threshold."
        if let Some(qmin) = input.quality_min {
            results.retain(|r| r.domain_rated && r.rating.map_or(false, |v| v >= qmin));
        }
        if let Some(qmax) = input.quality_max {
            results.retain(|r| r.domain_rated && r.rating.map_or(false, |v| v <= qmax));
        }

        // Sort each cohort independently, then interleave
        let sort_by = input.sort_by.unwrap_or(SortField::Relevance);
        let (mut regular, mut recommended_items): (Vec<SearchResult>, Vec<SearchResult>) =
            results.into_iter().partition(|r| !r.recommended);

        Self::sort_results(&mut regular, sort_by);
        Self::sort_results(&mut recommended_items, sort_by);

        let results = Self::interleave(regular, recommended_items);

        // Persist query to history (best-effort, do not fail the request on error)
        let filters = serde_json::json!({
            "tags": input.tags,
            "status": input.status,
            "sort_by": format!("{:?}", sort_by),
            "min_rating": input.min_rating,
            "max_rating": input.max_rating,
            "max_distance_miles": input.max_distance_miles,
            "site_code": input.site_code,
            "department": input.department,
            "source": input.source,
            "salary_min_cents": input.salary_min_cents,
            "salary_max_cents": input.salary_max_cents,
            "categories": input.categories,
            "price_min_cents": input.price_min_cents,
            "price_max_cents": input.price_max_cents,
            "quality_min": input.quality_min,
            "quality_max": input.quality_max,
        });
        let total = cand_total + offer_total;
        let _ = PgSearchRepository::insert_query(
            conn,
            ctx.user_id,
            input.q.clone().unwrap_or_default(),
            filters,
            Some(total as i32),
        );

        // Spell correction (best-effort)
        let spell_correction = if let Some(ref kw) = input.q {
            Self::suggest_correction(conn, kw)
        } else {
            None
        };

        Ok(SearchResponse {
            results,
            total,
            page: input.page,
            per_page: input.per_page,
            spell_correction,
        })
    }

    // ── Autocomplete ──────────────────────────────────────────────────────────

    pub fn autocomplete(
        conn: &mut PgConnection,
        input: AutocompleteInput,
    ) -> Result<Vec<String>, AppError> {
        let default_categories = &["tags", "department", "source"];
        let categories: Vec<&str> = if input.categories.is_empty() {
            default_categories.to_vec()
        } else {
            input.categories.iter().map(String::as_str).collect()
        };
        PgSearchRepository::autocomplete(conn, &input.prefix, &categories, input.limit)
    }

    // ── History ───────────────────────────────────────────────────────────────

    pub fn search_history(
        conn: &mut PgConnection,
        user_id: Uuid,
        limit: i64,
    ) -> Result<Vec<HistoryEntry>, AppError> {
        let rows = PgSearchRepository::list_user_history(conn, user_id, limit)?;
        Ok(rows
            .into_iter()
            .map(|r| HistoryEntry {
                id: r.id,
                query_text: r.query_text,
                filters: r.filters,
                result_count: r.result_count,
                executed_at: r.executed_at.to_rfc3339(),
            })
            .collect())
    }

    // ── Vocabulary ────────────────────────────────────────────────────────────

    pub fn list_vocabulary_categories(conn: &mut PgConnection) -> Result<Vec<String>, AppError> {
        PgSearchRepository::list_vocabulary_categories(conn)
    }

    pub fn get_vocabulary(
        conn: &mut PgConnection,
        category: &str,
    ) -> Result<Vec<VocabularyEntry>, AppError> {
        let rows = PgSearchRepository::list_vocabulary(conn, category)?;
        if rows.is_empty() {
            return Err(AppError::NotFound(format!(
                "vocabulary category '{category}'"
            )));
        }
        Ok(rows
            .into_iter()
            .map(|r| VocabularyEntry {
                id: r.id,
                category: r.category,
                value: r.value,
                label: r.label,
                sort_order: r.sort_order,
            })
            .collect())
    }

    // ── Scoring ───────────────────────────────────────────────────────────────

    /// Score a candidate result (see module-level documentation for formula).
    fn score_candidate(
        c: &DbCandidate,
        q: Option<&str>,
        tags: Option<&[String]>,
        now: &chrono::DateTime<Utc>,
    ) -> f64 {
        let exact_bonus = q.map_or(0.0, |kw| {
            let full_name = format!("{} {}", c.first_name, c.last_name).to_lowercase();
            if full_name == kw.to_lowercase() || c.email.to_lowercase() == kw.to_lowercase() {
                1.0
            } else {
                0.0
            }
        });

        let tag_ratio = tags.map_or(0.0, |requested| {
            if requested.is_empty() {
                return 0.0;
            }
            let overlap = c.tags.iter().filter(|t| requested.contains(t)).count() as f64;
            overlap / requested.len() as f64
        });

        let recency = Self::recency_score(c.created_at, now);

        (exact_bonus * 0.40) + (tag_ratio * 0.35) + (recency * 0.25)
    }

    /// Score an offer result.
    fn score_offer(o: &DbOffer, q: Option<&str>, now: &chrono::DateTime<Utc>) -> f64 {
        let exact_bonus = q.map_or(0.0, |kw| {
            if o.title.to_lowercase() == kw.to_lowercase() {
                1.0
            } else {
                0.0
            }
        });
        let recency = Self::recency_score(o.created_at, now);
        (exact_bonus * 0.60) + (recency * 0.40)
    }

    /// Linear decay over 365 days: 1.0 at creation, 0.0 at 365+ days old.
    fn recency_score(created_at: chrono::DateTime<Utc>, now: &chrono::DateTime<Utc>) -> f64 {
        let age_days = (*now - created_at).num_days().max(0) as f64;
        (1.0 - age_days / 365.0).clamp(0.0, 1.0)
    }

    /// Popularity proxy: tag density (0.6) + recency (0.4).
    ///
    /// `tag_count` is the number of tags on the resource.  Saturates at 10 tags.
    fn popularity_from_tags(tag_count: usize, recency: f64) -> f64 {
        let tag_density = (tag_count as f64 / 10.0).min(1.0);
        tag_density * 0.6 + recency * 0.4
    }

    /// Derive an offer's rating by delegating to the standalone
    /// [`domain_rating`] function.  See its documentation for the formula.
    pub fn offer_rating(o: &DbOffer, relevance_score: f64) -> f64 {
        domain_rating(o.salary_cents, relevance_score)
    }

    fn sort_results(results: &mut Vec<SearchResult>, sort_by: SortField) {
        match sort_by {
            SortField::Relevance => {
                results.sort_by(|a, b| {
                    b.score
                        .partial_cmp(&a.score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| a.resource_type.cmp(&b.resource_type))
                        .then_with(|| a.id.cmp(&b.id))
                });
            }
            SortField::Recency => {
                results.sort_by(|a, b| {
                    b.created_at
                        .cmp(&a.created_at)
                        .then_with(|| a.resource_type.cmp(&b.resource_type))
                        .then_with(|| a.id.cmp(&b.id))
                });
            }
            SortField::TagOverlap => {
                results.sort_by(|a, b| {
                    b.tags
                        .len()
                        .cmp(&a.tags.len())
                        .then_with(|| a.resource_type.cmp(&b.resource_type))
                        .then_with(|| a.id.cmp(&b.id))
                });
            }
            SortField::Popularity => {
                results.sort_by(|a, b| {
                    let pa = a.popularity_score.unwrap_or(0.0);
                    let pb = b.popularity_score.unwrap_or(0.0);
                    pb.partial_cmp(&pa)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| a.resource_type.cmp(&b.resource_type))
                        .then_with(|| a.id.cmp(&b.id))
                });
            }
            SortField::Rating => {
                results.sort_by(|a, b| {
                    let ra = a.rating.unwrap_or(0.0);
                    let rb = b.rating.unwrap_or(0.0);
                    rb.partial_cmp(&ra)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| a.resource_type.cmp(&b.resource_type))
                        .then_with(|| a.id.cmp(&b.id))
                });
            }
            SortField::Distance => {
                // Ascending distance; None (no location basis) sorts last
                results.sort_by(|a, b| {
                    match (a.distance_miles, b.distance_miles) {
                        (Some(da), Some(db)) => {
                            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                        }
                        (Some(_), None) => std::cmp::Ordering::Less,
                        (None, Some(_)) => std::cmp::Ordering::Greater,
                        (None, None) => std::cmp::Ordering::Equal,
                    }
                    .then_with(|| a.resource_type.cmp(&b.resource_type))
                    .then_with(|| a.id.cmp(&b.id))
                });
            }
        }
    }

    // ── Interleaving ──────────────────────────────────────────────────────────

    /// Interleave regular and recommended result lists at a 3:1 ratio.
    ///
    /// For every `RATIO` items from `regular`, one item from `recommended` is
    /// inserted.  When either list is exhausted the remainder of the other is
    /// appended.  Both input lists must already be sorted; the output ordering
    /// is fully deterministic.
    pub fn interleave(
        regular: Vec<SearchResult>,
        recommended: Vec<SearchResult>,
    ) -> Vec<SearchResult> {
        const RATIO: usize = 3;

        if recommended.is_empty() {
            return regular;
        }
        if regular.is_empty() {
            return recommended;
        }

        let mut out = Vec::with_capacity(regular.len() + recommended.len());
        let mut reg = regular.into_iter();
        let mut rec = recommended.into_iter();

        loop {
            // Take RATIO regular items
            let mut exhausted = false;
            for _ in 0..RATIO {
                match reg.next() {
                    Some(item) => out.push(item),
                    None => {
                        exhausted = true;
                        break;
                    }
                }
            }
            if exhausted {
                out.extend(rec);
                return out;
            }
            // Take 1 recommended item
            match rec.next() {
                Some(item) => out.push(item),
                None => {
                    out.extend(reg);
                    return out;
                }
            }
        }
    }

    // ── Haversine distance ────────────────────────────────────────────────────

    /// Compute the great-circle distance in miles between two (lat, lng) pairs.
    ///
    /// Uses the Haversine formula.  Input coordinates are in decimal degrees.
    pub fn haversine_miles(lat1: f64, lng1: f64, lat2: f64, lng2: f64) -> f64 {
        const EARTH_RADIUS_MILES: f64 = 3_958.8;
        let dlat = (lat2 - lat1).to_radians();
        let dlng = (lng2 - lng1).to_radians();
        let lat1r = lat1.to_radians();
        let lat2r = lat2.to_radians();
        let a = (dlat / 2.0).sin().powi(2) + lat1r.cos() * lat2r.cos() * (dlng / 2.0).sin().powi(2);
        let c = 2.0 * a.sqrt().asin();
        EARTH_RADIUS_MILES * c
    }

    // ── Spell correction ──────────────────────────────────────────────────────

    /// Best-effort spell correction.
    ///
    /// Finds the vocabulary label or recent historical query that is closest
    /// to `kw` by edit distance.  Returns `Some(suggestion)` if the closest
    /// match has edit distance ≤ 2 and differs from `kw`.  Returns `None`
    /// otherwise (no correction needed or no dictionary entry found).
    fn suggest_correction(conn: &mut PgConnection, kw: &str) -> Option<String> {
        // Build a local correction dictionary from two sources (in priority order):
        // 1. All active vocabulary labels — structured, curated terms.
        // 2. Recent system-wide query history (last 200) — captures domain-specific
        //    free-text that may not yet be in the controlled vocabulary.
        // Both are best-effort; errors are silently ignored.
        let vocab_labels = PgSearchRepository::all_vocabulary_labels(conn).unwrap_or_default();
        let history = PgSearchRepository::list_user_history(
            conn,
            Uuid::nil(), // system-wide, not per-user
            200,
        )
        .unwrap_or_default();
        let history_terms: Vec<String> = history.into_iter().map(|r| r.query_text).collect();

        // Merge: vocabulary first, history second; deduplicate case-insensitively.
        let mut seen = std::collections::HashSet::new();
        let dictionary: Vec<String> = vocab_labels
            .into_iter()
            .chain(history_terms)
            .filter(|s| !s.is_empty() && seen.insert(s.to_lowercase()))
            .collect();

        dictionary
            .iter()
            .filter_map(|candidate| {
                let dist = edit_distance(kw, candidate);
                if dist > 0 && dist <= 2 {
                    Some((dist, candidate.clone()))
                } else {
                    None
                }
            })
            .min_by_key(|(dist, _)| *dist)
            .map(|(_, s)| s)
    }
}

// ── Shared response types ────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct HistoryEntry {
    pub id: Uuid,
    pub query_text: String,
    pub filters: serde_json::Value,
    pub result_count: Option<i32>,
    pub executed_at: String,
}

#[derive(Debug, Serialize)]
pub struct VocabularyEntry {
    pub id: Uuid,
    pub category: String,
    pub value: String,
    pub label: String,
    pub sort_order: i32,
}

// ── Edit distance (Wagner-Fischer, O(mn)) ────────────────────────────────────

/// Derive a domain-native rating for an offer (or fall back to relevance).
///
/// **Precedence:**
/// 1. **Domain-native** (preferred): when `salary_cents` is `Some(n)` with `n > 0`,
///    map the annual salary into a 0.0–5.0 band using a linear scale anchored at
///    $30 000 (floor → 0.0) and $300 000 (ceiling → 5.0).  Values outside this
///    range are clamped.
/// 2. **Fallback**: `relevance_score * 5.0` — derived from the search relevance
///    score when no salary data is available.
///
/// This function is deterministic, pure, and free of I/O.  It is the single
/// source of truth for offer rating logic — both the service layer and the unit
/// tests call it directly, eliminating formula-drift risk.
pub fn domain_rating(salary_cents: Option<i64>, relevance_score: f64) -> f64 {
    const FLOOR_CENTS: f64 = 30_000_00.0; // $30,000
    const CEILING_CENTS: f64 = 300_000_00.0; // $300,000

    match salary_cents {
        Some(cents) if cents > 0 => {
            let clamped = (cents as f64).clamp(FLOOR_CENTS, CEILING_CENTS);
            let ratio = (clamped - FLOOR_CENTS) / (CEILING_CENTS - FLOOR_CENTS);
            (ratio * 5.0).clamp(0.0, 5.0)
        }
        _ => relevance_score * 5.0,
    }
}

/// Compute the Levenshtein edit distance between two strings (ASCII, case-insensitive).
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.to_lowercase().chars().collect();
    let b: Vec<char> = b.to_lowercase().chars().collect();
    let m = a.len();
    let n = b.len();

    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 0..=m {
        dp[i][0] = i;
    }
    for j in 0..=n {
        dp[0][j] = j;
    }
    for i in 1..=m {
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }
    dp[m][n]
}
