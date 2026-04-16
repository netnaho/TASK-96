use diesel::prelude::*;
use uuid::Uuid;

use crate::infrastructure::db::{
    models::{
        DbCandidate, DbControlledVocabulary, DbHistoricalQuery, DbOffer, NewDbHistoricalQuery,
    },
    schema::{candidates, controlled_vocabularies, historical_queries, offers},
};
use crate::shared::errors::AppError;

fn db_err(e: diesel::result::Error) -> AppError {
    AppError::Internal(format!("database error: {e}"))
}

pub struct PgSearchRepository;

impl PgSearchRepository {
    // ── Vocabulary ──────────────────────────────────────────────────────────

    /// Return all active vocabulary entries for a given category.
    pub fn list_vocabulary(
        conn: &mut PgConnection,
        category: &str,
    ) -> Result<Vec<DbControlledVocabulary>, AppError> {
        controlled_vocabularies::table
            .filter(controlled_vocabularies::category.eq(category))
            .filter(controlled_vocabularies::is_active.eq(true))
            .order(controlled_vocabularies::sort_order.asc())
            .select(DbControlledVocabulary::as_select())
            .load(conn)
            .map_err(db_err)
    }

    /// Return all active vocabulary labels for the spell-correction dictionary.
    /// Vocabulary terms are higher-quality signal than free-text history and are
    /// always checked first when building the correction corpus.
    pub fn all_vocabulary_labels(conn: &mut PgConnection) -> Result<Vec<String>, AppError> {
        controlled_vocabularies::table
            .select(controlled_vocabularies::label)
            .filter(controlled_vocabularies::is_active.eq(true))
            .load::<String>(conn)
            .map_err(db_err)
    }

    /// Return all active `(category, value)` pairs for the given categories.
    /// Used by the `categories` search facet to resolve vocabulary-driven filters.
    pub fn vocabulary_values_for_categories(
        conn: &mut PgConnection,
        categories: &[&str],
    ) -> Result<Vec<(String, String)>, AppError> {
        controlled_vocabularies::table
            .select((
                controlled_vocabularies::category,
                controlled_vocabularies::value,
            ))
            .filter(
                controlled_vocabularies::category
                    .eq_any(categories)
                    .and(controlled_vocabularies::is_active.eq(true)),
            )
            .load::<(String, String)>(conn)
            .map_err(db_err)
    }

    /// Return all distinct active categories.
    pub fn list_vocabulary_categories(conn: &mut PgConnection) -> Result<Vec<String>, AppError> {
        controlled_vocabularies::table
            .select(controlled_vocabularies::category)
            .filter(controlled_vocabularies::is_active.eq(true))
            .distinct()
            .order(controlled_vocabularies::category.asc())
            .load::<String>(conn)
            .map_err(db_err)
    }

    // ── Historical queries ──────────────────────────────────────────────────

    /// Persist a new historical query record.
    pub fn insert_query(
        conn: &mut PgConnection,
        user_id: Uuid,
        query_text: String,
        filters: serde_json::Value,
        result_count: Option<i32>,
    ) -> Result<(), AppError> {
        let record = NewDbHistoricalQuery {
            id: Uuid::new_v4(),
            user_id,
            query_text,
            filters,
            result_count,
            executed_at: chrono::Utc::now(),
        };
        diesel::insert_into(historical_queries::table)
            .values(&record)
            .execute(conn)
            .map(|_| ())
            .map_err(db_err)
    }

    /// Return the most recent queries for a user (newest first), up to `limit`.
    /// Pass `Uuid::nil()` for system-wide history (used by spell correction).
    pub fn list_user_history(
        conn: &mut PgConnection,
        user_id: Uuid,
        limit: i64,
    ) -> Result<Vec<DbHistoricalQuery>, AppError> {
        if user_id == Uuid::nil() {
            historical_queries::table
                .order(historical_queries::executed_at.desc())
                .limit(limit)
                .select(DbHistoricalQuery::as_select())
                .load(conn)
                .map_err(db_err)
        } else {
            historical_queries::table
                .filter(historical_queries::user_id.eq(user_id))
                .order(historical_queries::executed_at.desc())
                .limit(limit)
                .select(DbHistoricalQuery::as_select())
                .load(conn)
                .map_err(db_err)
        }
    }

    /// Top N most-frequently used query texts matching a prefix (for autocomplete).
    pub fn top_query_texts(
        conn: &mut PgConnection,
        prefix: &str,
        limit: i64,
    ) -> Result<Vec<String>, AppError> {
        let pattern = format!("{}%", prefix.to_lowercase());
        diesel::sql_query(
            "SELECT query_text FROM historical_queries \
             WHERE LOWER(query_text) LIKE $1 \
             GROUP BY query_text \
             ORDER BY COUNT(*) DESC \
             LIMIT $2",
        )
        .bind::<diesel::sql_types::Text, _>(pattern)
        .bind::<diesel::sql_types::BigInt, _>(limit)
        .load::<QueryTextRow>(conn)
        .map(|rows| rows.into_iter().map(|r| r.query_text).collect())
        .map_err(db_err)
    }

    // ── Candidate search ────────────────────────────────────────────────────

    /// Full-text + tag + source candidate search.
    ///
    /// Query parameters:
    /// - `q`: optional keyword — matched against first_name, last_name, email, notes (ILIKE)
    /// - `tag_filter`: optional tags — matched with PostgreSQL array overlap (&&)
    /// - `source_filter`: optional source — exact match on candidates.source (case-insensitive)
    /// - `page`/`per_page`: 1-based pagination
    ///
    /// Uses parameterized raw SQL throughout.  The tag array is embedded as a
    /// safe literal (single quotes doubled in each tag value).
    pub fn search_candidates(
        conn: &mut PgConnection,
        q: Option<&str>,
        tag_filter: Option<&[String]>,
        source_filter: Option<&str>,
        page: i64,
        per_page: i64,
    ) -> Result<(Vec<DbCandidate>, i64), AppError> {
        let offset = (page - 1).max(0) * per_page;
        let kw_pattern = q.map(|k| format!("%{}%", k.to_lowercase()));

        // Build tag literal (safe escaping of each value)
        let tag_literal = tag_filter
            .filter(|t| !t.is_empty())
            .map(build_pg_text_array);

        // Build WHERE clause using positional parameters
        let where_clause =
            build_candidate_where(kw_pattern.is_some(), &tag_literal, source_filter.is_some());

        // Track how many bind params precede LIMIT/OFFSET
        let mut bind_count: usize = 0;
        if kw_pattern.is_some() {
            bind_count += 1;
        }
        if source_filter.is_some() {
            bind_count += 1;
        }

        // COUNT
        let count_sql = format!("SELECT COUNT(*) AS count FROM candidates{where_clause}");
        let total: i64 =
            count_candidates(conn, &count_sql, kw_pattern.as_deref(), source_filter)?;

        let lim_pos = bind_count + 1;
        let off_pos = bind_count + 2;

        let data_sql = format!(
            "SELECT * FROM candidates{where_clause} \
             ORDER BY last_name, first_name \
             LIMIT ${lim_pos} OFFSET ${off_pos}"
        );

        let rows: Vec<DbCandidate> = select_candidates(
            conn,
            &data_sql,
            kw_pattern.as_deref(),
            source_filter,
            per_page,
            offset,
        )?;

        Ok((rows, total))
    }

    // ── Offer search ────────────────────────────────────────────────────────

    /// Keyword + status + department + salary + total-comp filtered offer search.
    pub fn search_offers(
        conn: &mut PgConnection,
        q: Option<&str>,
        status_filter: Option<&str>,
        department_filter: Option<&str>,
        salary_min_cents: Option<i64>,
        salary_max_cents: Option<i64>,
        price_min_cents: Option<i64>,
        price_max_cents: Option<i64>,
        page: i64,
        per_page: i64,
    ) -> Result<(Vec<DbOffer>, i64), AppError> {
        let offset = (page - 1).max(0) * per_page;
        let kw_pattern = q.map(|k| format!("%{}%", k.to_lowercase()));

        let where_clause = build_offer_where(
            kw_pattern.is_some(),
            status_filter.is_some(),
            department_filter.is_some(),
            salary_min_cents,
            salary_max_cents,
            price_min_cents,
            price_max_cents,
        );

        // Count how many text bind params are used before LIMIT/OFFSET
        // (salary bounds are inlined as literals, not bound)
        let mut bind_count: usize = 0;
        if kw_pattern.is_some() {
            bind_count += 1;
        }
        if status_filter.is_some() {
            bind_count += 1;
        }
        if department_filter.is_some() {
            bind_count += 1;
        }

        // COUNT
        let count_sql = format!("SELECT COUNT(*) AS count FROM offers{where_clause}");
        let total: i64 = count_offers_ext(
            conn,
            &count_sql,
            kw_pattern.as_deref(),
            status_filter,
            department_filter,
        )?;

        let lim_pos = bind_count + 1;
        let off_pos = bind_count + 2;

        let data_sql = format!(
            "SELECT * FROM offers{where_clause} \
             ORDER BY created_at DESC \
             LIMIT ${lim_pos} OFFSET ${off_pos}"
        );

        let rows: Vec<DbOffer> = select_offers_ext(
            conn,
            &data_sql,
            kw_pattern.as_deref(),
            status_filter,
            department_filter,
            per_page,
            offset,
        )?;

        Ok((rows, total))
    }

    // ── Autocomplete ────────────────────────────────────────────────────────

    /// Return up to `limit` autocomplete suggestions for a given prefix.
    ///
    /// Sources (in order):
    /// 1. Active vocabulary labels matching the prefix.
    /// 2. Most-popular historical query texts matching the prefix.
    ///
    /// Deduplicated, vocabulary first, truncated to `limit`.
    pub fn autocomplete(
        conn: &mut PgConnection,
        prefix: &str,
        categories: &[&str],
        limit: i64,
    ) -> Result<Vec<String>, AppError> {
        let pattern = format!("{}%", prefix.to_lowercase());

        let vocab: Vec<String> = controlled_vocabularies::table
            .select(controlled_vocabularies::label)
            .filter(
                controlled_vocabularies::category
                    .eq_any(categories)
                    .and(controlled_vocabularies::is_active.eq(true))
                    .and(controlled_vocabularies::label.ilike(pattern)),
            )
            .order(controlled_vocabularies::sort_order.asc())
            .limit(limit)
            .load::<String>(conn)
            .map_err(db_err)?;

        let history = Self::top_query_texts(conn, prefix, limit)?;

        let mut seen = std::collections::HashSet::new();
        let mut results: Vec<String> = Vec::new();
        for s in vocab.into_iter().chain(history) {
            let key = s.to_lowercase();
            if seen.insert(key) {
                results.push(s);
                if results.len() >= limit as usize {
                    break;
                }
            }
        }
        Ok(results)
    }
}

// ── SQL builder helpers ───────────────────────────────────────────────────────

/// Build the WHERE clause for candidate search.
///
/// - Keyword filter uses `$1` (if `has_kw`).
/// - Tag filter is embedded as a safe literal (no extra bind parameter).
/// - Source filter uses the next positional param after keyword.
fn build_candidate_where(
    has_kw: bool,
    tag_literal: &Option<String>,
    has_source: bool,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut next_param = 1usize;
    if has_kw {
        parts.push(format!(
            "(LOWER(first_name) LIKE ${next_param} OR LOWER(last_name) LIKE ${next_param} \
             OR LOWER(email) LIKE ${next_param} OR LOWER(COALESCE(notes,'')) LIKE ${next_param})"
        ));
        next_param += 1;
    }
    if let Some(arr) = tag_literal {
        parts.push(format!("tags && {arr}"));
    }
    if has_source {
        parts.push(format!("LOWER(COALESCE(source,'')) = LOWER(${next_param})"));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", parts.join(" AND "))
    }
}

/// Build the WHERE clause for offer search.
///
/// - Keyword, status, department use sequential positional bind params.
/// - Salary bounds are inlined as integer literals (safe — they originate
///   from parsed `i64` values, never from user text) to avoid Diesel's
///   typed-bind combinatorial explosion.
fn build_offer_where(
    has_kw: bool,
    has_status: bool,
    has_department: bool,
    salary_min_cents: Option<i64>,
    salary_max_cents: Option<i64>,
    price_min_cents: Option<i64>,
    price_max_cents: Option<i64>,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut next_param = 1usize;
    if has_kw {
        parts.push(format!(
            "(LOWER(title) LIKE ${next_param} OR LOWER(COALESCE(department,'')) LIKE ${next_param})"
        ));
        next_param += 1;
    }
    if has_status {
        parts.push(format!("status = ${next_param}"));
        next_param += 1;
    }
    if has_department {
        parts.push(format!(
            "LOWER(COALESCE(department,'')) = LOWER(${next_param})"
        ));
    }
    if let Some(min_c) = salary_min_cents {
        parts.push(format!(
            "salary_cents IS NOT NULL AND salary_cents >= {min_c}"
        ));
    }
    if let Some(max_c) = salary_max_cents {
        parts.push(format!(
            "salary_cents IS NOT NULL AND salary_cents <= {max_c}"
        ));
    }
    // Total compensation = salary + bonus target.  Inlined as integer literal
    // (safe — value originates from parsed i64, never from user text).
    if let Some(min_p) = price_min_cents {
        parts.push(format!(
            "salary_cents IS NOT NULL AND \
             (salary_cents::float8 * (1.0 + COALESCE(bonus_target_pct, 0.0) / 100.0)) >= {min_p}"
        ));
    }
    if let Some(max_p) = price_max_cents {
        parts.push(format!(
            "salary_cents IS NOT NULL AND \
             (salary_cents::float8 * (1.0 + COALESCE(bonus_target_pct, 0.0) / 100.0)) <= {max_p}"
        ));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", parts.join(" AND "))
    }
}

fn count_candidates(
    conn: &mut PgConnection,
    sql: &str,
    kw: Option<&str>,
    source: Option<&str>,
) -> Result<i64, AppError> {
    // Diesel's sql_query builder requires static bind chain, so we branch.
    let rows: Vec<CountRow> = match (kw, source) {
        (Some(k), Some(s)) => diesel::sql_query(sql)
            .bind::<diesel::sql_types::Text, _>(k)
            .bind::<diesel::sql_types::Text, _>(s)
            .load(conn),
        (Some(k), None) => diesel::sql_query(sql)
            .bind::<diesel::sql_types::Text, _>(k)
            .load(conn),
        (None, Some(s)) => diesel::sql_query(sql)
            .bind::<diesel::sql_types::Text, _>(s)
            .load(conn),
        (None, None) => diesel::sql_query(sql).load(conn),
    }
    .map_err(db_err)?;
    Ok(rows.first().map_or(0, |r| r.count))
}

fn select_candidates(
    conn: &mut PgConnection,
    sql: &str,
    kw: Option<&str>,
    source: Option<&str>,
    per_page: i64,
    offset: i64,
) -> Result<Vec<DbCandidate>, AppError> {
    match (kw, source) {
        (Some(k), Some(s)) => diesel::sql_query(sql)
            .bind::<diesel::sql_types::Text, _>(k)
            .bind::<diesel::sql_types::Text, _>(s)
            .bind::<diesel::sql_types::BigInt, _>(per_page)
            .bind::<diesel::sql_types::BigInt, _>(offset)
            .load(conn),
        (Some(k), None) => diesel::sql_query(sql)
            .bind::<diesel::sql_types::Text, _>(k)
            .bind::<diesel::sql_types::BigInt, _>(per_page)
            .bind::<diesel::sql_types::BigInt, _>(offset)
            .load(conn),
        (None, Some(s)) => diesel::sql_query(sql)
            .bind::<diesel::sql_types::Text, _>(s)
            .bind::<diesel::sql_types::BigInt, _>(per_page)
            .bind::<diesel::sql_types::BigInt, _>(offset)
            .load(conn),
        (None, None) => diesel::sql_query(sql)
            .bind::<diesel::sql_types::BigInt, _>(per_page)
            .bind::<diesel::sql_types::BigInt, _>(offset)
            .load(conn),
    }
    .map_err(db_err)
}

/// Extended offer count — binds text params (kw, status, department) in order.
/// Salary and price bounds are already inlined as literals by `build_offer_where`.
fn count_offers_ext(
    conn: &mut PgConnection,
    sql: &str,
    kw: Option<&str>,
    status: Option<&str>,
    department: Option<&str>,
) -> Result<i64, AppError> {
    let rows: Vec<CountRow> = match (kw, status, department) {
        (Some(k), Some(s), Some(d)) => diesel::sql_query(sql)
            .bind::<diesel::sql_types::Text, _>(k)
            .bind::<diesel::sql_types::Text, _>(s)
            .bind::<diesel::sql_types::Text, _>(d)
            .load(conn),
        (Some(k), Some(s), None) => diesel::sql_query(sql)
            .bind::<diesel::sql_types::Text, _>(k)
            .bind::<diesel::sql_types::Text, _>(s)
            .load(conn),
        (Some(k), None, Some(d)) => diesel::sql_query(sql)
            .bind::<diesel::sql_types::Text, _>(k)
            .bind::<diesel::sql_types::Text, _>(d)
            .load(conn),
        (Some(k), None, None) => diesel::sql_query(sql)
            .bind::<diesel::sql_types::Text, _>(k)
            .load(conn),
        (None, Some(s), Some(d)) => diesel::sql_query(sql)
            .bind::<diesel::sql_types::Text, _>(s)
            .bind::<diesel::sql_types::Text, _>(d)
            .load(conn),
        (None, Some(s), None) => diesel::sql_query(sql)
            .bind::<diesel::sql_types::Text, _>(s)
            .load(conn),
        (None, None, Some(d)) => diesel::sql_query(sql)
            .bind::<diesel::sql_types::Text, _>(d)
            .load(conn),
        (None, None, None) => diesel::sql_query(sql).load(conn),
    }
    .map_err(db_err)?;
    Ok(rows.first().map_or(0, |r| r.count))
}

fn select_offers_ext(
    conn: &mut PgConnection,
    sql: &str,
    kw: Option<&str>,
    status: Option<&str>,
    department: Option<&str>,
    per_page: i64,
    offset: i64,
) -> Result<Vec<DbOffer>, AppError> {
    match (kw, status, department) {
        (Some(k), Some(s), Some(d)) => diesel::sql_query(sql)
            .bind::<diesel::sql_types::Text, _>(k)
            .bind::<diesel::sql_types::Text, _>(s)
            .bind::<diesel::sql_types::Text, _>(d)
            .bind::<diesel::sql_types::BigInt, _>(per_page)
            .bind::<diesel::sql_types::BigInt, _>(offset)
            .load(conn),
        (Some(k), Some(s), None) => diesel::sql_query(sql)
            .bind::<diesel::sql_types::Text, _>(k)
            .bind::<diesel::sql_types::Text, _>(s)
            .bind::<diesel::sql_types::BigInt, _>(per_page)
            .bind::<diesel::sql_types::BigInt, _>(offset)
            .load(conn),
        (Some(k), None, Some(d)) => diesel::sql_query(sql)
            .bind::<diesel::sql_types::Text, _>(k)
            .bind::<diesel::sql_types::Text, _>(d)
            .bind::<diesel::sql_types::BigInt, _>(per_page)
            .bind::<diesel::sql_types::BigInt, _>(offset)
            .load(conn),
        (Some(k), None, None) => diesel::sql_query(sql)
            .bind::<diesel::sql_types::Text, _>(k)
            .bind::<diesel::sql_types::BigInt, _>(per_page)
            .bind::<diesel::sql_types::BigInt, _>(offset)
            .load(conn),
        (None, Some(s), Some(d)) => diesel::sql_query(sql)
            .bind::<diesel::sql_types::Text, _>(s)
            .bind::<diesel::sql_types::Text, _>(d)
            .bind::<diesel::sql_types::BigInt, _>(per_page)
            .bind::<diesel::sql_types::BigInt, _>(offset)
            .load(conn),
        (None, Some(s), None) => diesel::sql_query(sql)
            .bind::<diesel::sql_types::Text, _>(s)
            .bind::<diesel::sql_types::BigInt, _>(per_page)
            .bind::<diesel::sql_types::BigInt, _>(offset)
            .load(conn),
        (None, None, Some(d)) => diesel::sql_query(sql)
            .bind::<diesel::sql_types::Text, _>(d)
            .bind::<diesel::sql_types::BigInt, _>(per_page)
            .bind::<diesel::sql_types::BigInt, _>(offset)
            .load(conn),
        (None, None, None) => diesel::sql_query(sql)
            .bind::<diesel::sql_types::BigInt, _>(per_page)
            .bind::<diesel::sql_types::BigInt, _>(offset)
            .load(conn),
    }
    .map_err(db_err)
}

/// Build a safe PostgreSQL array literal from a slice of tag strings.
/// Single quotes in each tag value are escaped (doubled) to prevent injection.
fn build_pg_text_array(tags: &[String]) -> String {
    let escaped: Vec<String> = tags
        .iter()
        .map(|t| format!("'{}'", t.replace('\'', "''")))
        .collect();
    format!("ARRAY[{}]::text[]", escaped.join(","))
}

// ── Private QueryableByName helpers ──────────────────────────────────────────

#[derive(QueryableByName)]
struct QueryTextRow {
    #[diesel(sql_type = diesel::sql_types::Text)]
    query_text: String,
}

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    count: i64,
}
