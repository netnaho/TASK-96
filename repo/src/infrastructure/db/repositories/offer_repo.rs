use chrono::{DateTime, NaiveDate, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use crate::infrastructure::db::models::{DbApprovalStep, DbOffer, NewDbApprovalStep, NewDbOffer};
use crate::infrastructure::db::schema::{approval_steps, offers};
use crate::shared::errors::AppError;

pub struct PgOfferRepository;

impl PgOfferRepository {
    pub fn find_by_id(conn: &mut PgConnection, id: Uuid) -> Result<Option<DbOffer>, AppError> {
        offers::table
            .filter(offers::id.eq(id))
            .select(DbOffer::as_select())
            .first(conn)
            .optional()
            .map_err(db_err)
    }

    pub fn create(conn: &mut PgConnection, new_offer: NewDbOffer) -> Result<DbOffer, AppError> {
        diesel::insert_into(offers::table)
            .values(&new_offer)
            .returning(DbOffer::as_returning())
            .get_result(conn)
            .map_err(db_err)
    }

    /// Update mutable fields. Only valid while offer is in draft; callers enforce state rules.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        conn: &mut PgConnection,
        id: Uuid,
        title: &str,
        department: Option<&str>,
        compensation_encrypted: Option<&[u8]>,
        start_date: Option<NaiveDate>,
        status: &str,
        expires_at: Option<DateTime<Utc>>,
        template_id: Option<Uuid>,
        clause_version: Option<&str>,
        salary_cents: Option<i64>,
        bonus_target_pct: Option<f64>,
        equity_units: Option<i32>,
        pto_days: Option<i16>,
        k401_match_pct: Option<f64>,
        currency: &str,
    ) -> Result<DbOffer, AppError> {
        diesel::update(offers::table.filter(offers::id.eq(id)))
            .set((
                offers::title.eq(title),
                offers::department.eq(department),
                offers::compensation_encrypted.eq(compensation_encrypted),
                offers::start_date.eq(start_date),
                offers::status.eq(status),
                offers::expires_at.eq(expires_at),
                offers::template_id.eq(template_id),
                offers::clause_version.eq(clause_version),
                offers::salary_cents.eq(salary_cents),
                offers::bonus_target_pct.eq(bonus_target_pct),
                offers::equity_units.eq(equity_units),
                offers::pto_days.eq(pto_days),
                offers::k401_match_pct.eq(k401_match_pct),
                offers::currency.eq(currency),
            ))
            .returning(DbOffer::as_returning())
            .get_result(conn)
            .map_err(db_err)
    }

    /// Update only the status column (used for state transitions).
    pub fn set_status(conn: &mut PgConnection, id: Uuid, status: &str) -> Result<(), AppError> {
        diesel::update(offers::table.filter(offers::id.eq(id)))
            .set(offers::status.eq(status))
            .execute(conn)
            .map(|_| ())
            .map_err(db_err)
    }

    /// Paginated list with optional candidate and ownership filters.
    /// Returns (rows, total). Both filters apply to count and rows.
    pub fn list(
        conn: &mut PgConnection,
        candidate_id_filter: Option<Uuid>,
        created_by_filter: Option<Uuid>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<DbOffer>, i64), AppError> {
        use diesel::dsl::count_star;

        let mut base = offers::table.into_boxed();
        let mut count_q = offers::table.into_boxed();

        if let Some(cid) = candidate_id_filter {
            base = base.filter(offers::candidate_id.eq(cid));
            count_q = count_q.filter(offers::candidate_id.eq(cid));
        }

        if let Some(uid) = created_by_filter {
            base = base.filter(offers::created_by.eq(uid));
            count_q = count_q.filter(offers::created_by.eq(uid));
        }

        let total: i64 = count_q.select(count_star()).first(conn).map_err(db_err)?;

        let rows = base
            .select(DbOffer::as_select())
            .order(offers::created_at.desc())
            .offset(offset)
            .limit(limit)
            .load(conn)
            .map_err(db_err)?;

        Ok((rows, total))
    }
}

pub struct PgApprovalRepository;

impl PgApprovalRepository {
    pub fn find_steps_for_offer(
        conn: &mut PgConnection,
        offer_id: Uuid,
    ) -> Result<Vec<DbApprovalStep>, AppError> {
        approval_steps::table
            .filter(approval_steps::offer_id.eq(offer_id))
            .select(DbApprovalStep::as_select())
            .order(approval_steps::step_order.asc())
            .load(conn)
            .map_err(db_err)
    }

    pub fn find_step_by_id(
        conn: &mut PgConnection,
        step_id: Uuid,
    ) -> Result<Option<DbApprovalStep>, AppError> {
        approval_steps::table
            .filter(approval_steps::id.eq(step_id))
            .select(DbApprovalStep::as_select())
            .first(conn)
            .optional()
            .map_err(db_err)
    }

    pub fn create_step(
        conn: &mut PgConnection,
        new_step: NewDbApprovalStep,
    ) -> Result<DbApprovalStep, AppError> {
        diesel::insert_into(approval_steps::table)
            .values(&new_step)
            .returning(DbApprovalStep::as_returning())
            .get_result(conn)
            .map_err(db_err)
    }

    /// Record a decision (approved/rejected/escalated) on a step.
    pub fn record_decision(
        conn: &mut PgConnection,
        step_id: Uuid,
        decision: &str,
        decided_at: DateTime<Utc>,
        comments: Option<&str>,
    ) -> Result<DbApprovalStep, AppError> {
        diesel::update(approval_steps::table.filter(approval_steps::id.eq(step_id)))
            .set((
                approval_steps::decision.eq(decision),
                approval_steps::decided_at.eq(Some(decided_at)),
                approval_steps::comments.eq(comments),
            ))
            .returning(DbApprovalStep::as_returning())
            .get_result(conn)
            .map_err(db_err)
    }

    /// Returns the next pending step (lowest step_order where decision = 'pending').
    pub fn find_next_pending(
        conn: &mut PgConnection,
        offer_id: Uuid,
    ) -> Result<Option<DbApprovalStep>, AppError> {
        approval_steps::table
            .filter(approval_steps::offer_id.eq(offer_id))
            .filter(approval_steps::decision.eq("pending"))
            .select(DbApprovalStep::as_select())
            .order(approval_steps::step_order.asc())
            .first(conn)
            .optional()
            .map_err(db_err)
    }
}

fn db_err(e: diesel::result::Error) -> AppError {
    AppError::Internal(format!("database error: {e}"))
}
