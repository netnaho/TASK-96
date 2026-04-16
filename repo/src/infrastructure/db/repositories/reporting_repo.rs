use chrono::{DateTime, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use crate::infrastructure::db::{
    models::{
        DbDashboardVersion, DbReportingAlert, DbReportingSubscription, NewDbDashboardVersion,
        NewDbReportingAlert, NewDbReportingSubscription,
    },
    schema::{dashboard_versions, reporting_alerts, reporting_subscriptions},
};
use crate::shared::errors::AppError;

fn db_err(e: diesel::result::Error) -> AppError {
    AppError::Internal(format!("database error: {e}"))
}

pub struct PgReportingRepository;

impl PgReportingRepository {
    // ── Subscriptions ────────────────────────────────────────────────────────

    pub fn insert_subscription(
        conn: &mut PgConnection,
        record: NewDbReportingSubscription,
    ) -> Result<DbReportingSubscription, AppError> {
        diesel::insert_into(reporting_subscriptions::table)
            .values(&record)
            .get_result(conn)
            .map_err(db_err)
    }

    pub fn find_subscription(
        conn: &mut PgConnection,
        id: Uuid,
    ) -> Result<Option<DbReportingSubscription>, AppError> {
        reporting_subscriptions::table
            .filter(reporting_subscriptions::id.eq(id))
            .first(conn)
            .optional()
            .map_err(db_err)
    }

    pub fn list_subscriptions(
        conn: &mut PgConnection,
        user_id: Option<Uuid>,
        page: i64,
        per_page: i64,
    ) -> Result<(Vec<DbReportingSubscription>, i64), AppError> {
        let offset = (page - 1).max(0) * per_page;
        let (rows, total) = if let Some(uid) = user_id {
            let rows = reporting_subscriptions::table
                .filter(reporting_subscriptions::user_id.eq(uid))
                .order(reporting_subscriptions::created_at.desc())
                .limit(per_page)
                .offset(offset)
                .load(conn)
                .map_err(db_err)?;
            let total: i64 = reporting_subscriptions::table
                .filter(reporting_subscriptions::user_id.eq(uid))
                .count()
                .get_result(conn)
                .map_err(db_err)?;
            (rows, total)
        } else {
            let rows = reporting_subscriptions::table
                .order(reporting_subscriptions::created_at.desc())
                .limit(per_page)
                .offset(offset)
                .load(conn)
                .map_err(db_err)?;
            let total: i64 = reporting_subscriptions::table
                .count()
                .get_result(conn)
                .map_err(db_err)?;
            (rows, total)
        };
        Ok((rows, total))
    }

    pub fn update_subscription(
        conn: &mut PgConnection,
        id: Uuid,
        report_type: Option<String>,
        parameters: Option<serde_json::Value>,
        cron_expression: Option<Option<String>>,
        is_active: Option<bool>,
    ) -> Result<DbReportingSubscription, AppError> {
        // Build the SET clause manually since Diesel doesn't do dynamic partial updates easily
        diesel::sql_query(
            "UPDATE reporting_subscriptions \
             SET report_type      = COALESCE($2, report_type), \
                 parameters       = COALESCE($3, parameters), \
                 cron_expression  = CASE WHEN $4::boolean THEN $5 ELSE cron_expression END, \
                 is_active        = COALESCE($6, is_active), \
                 updated_at       = now() \
             WHERE id = $1 \
             RETURNING *",
        )
        .bind::<diesel::sql_types::Uuid, _>(id)
        .bind::<diesel::sql_types::Nullable<diesel::sql_types::Text>, _>(report_type)
        .bind::<diesel::sql_types::Nullable<diesel::sql_types::Jsonb>, _>(parameters)
        .bind::<diesel::sql_types::Bool, _>(cron_expression.is_some())
        .bind::<diesel::sql_types::Nullable<diesel::sql_types::Text>, _>(cron_expression.flatten())
        .bind::<diesel::sql_types::Nullable<diesel::sql_types::Bool>, _>(is_active)
        .get_result(conn)
        .map_err(|e| match e {
            diesel::result::Error::NotFound => AppError::NotFound("subscription".into()),
            other => db_err(other),
        })
    }

    pub fn delete_subscription(conn: &mut PgConnection, id: Uuid) -> Result<(), AppError> {
        let deleted = diesel::delete(
            reporting_subscriptions::table.filter(reporting_subscriptions::id.eq(id)),
        )
        .execute(conn)
        .map_err(db_err)?;
        if deleted == 0 {
            return Err(AppError::NotFound("subscription".into()));
        }
        Ok(())
    }

    pub fn mark_subscription_run(
        conn: &mut PgConnection,
        id: Uuid,
        ran_at: DateTime<Utc>,
    ) -> Result<(), AppError> {
        diesel::update(reporting_subscriptions::table.filter(reporting_subscriptions::id.eq(id)))
            .set(reporting_subscriptions::last_run_at.eq(Some(ran_at)))
            .execute(conn)
            .map(|_| ())
            .map_err(db_err)
    }

    /// Return all active subscriptions that should run on the daily schedule.
    pub fn list_active_subscriptions_for_snapshot(
        conn: &mut PgConnection,
    ) -> Result<Vec<DbReportingSubscription>, AppError> {
        reporting_subscriptions::table
            .filter(reporting_subscriptions::is_active.eq(true))
            .load(conn)
            .map_err(db_err)
    }

    // ── Dashboard versions ───────────────────────────────────────────────────

    pub fn insert_dashboard_version(
        conn: &mut PgConnection,
        record: NewDbDashboardVersion,
    ) -> Result<DbDashboardVersion, AppError> {
        diesel::insert_into(dashboard_versions::table)
            .values(&record)
            .get_result(conn)
            .map_err(db_err)
    }

    pub fn find_dashboard_version_by_id(
        conn: &mut PgConnection,
        id: Uuid,
    ) -> Result<Option<DbDashboardVersion>, AppError> {
        dashboard_versions::table
            .find(id)
            .first(conn)
            .optional()
            .map_err(db_err)
    }

    pub fn list_dashboard_versions(
        conn: &mut PgConnection,
        dashboard_key: &str,
    ) -> Result<Vec<DbDashboardVersion>, AppError> {
        dashboard_versions::table
            .filter(dashboard_versions::dashboard_key.eq(dashboard_key))
            .order(dashboard_versions::version.desc())
            .load(conn)
            .map_err(db_err)
    }

    /// Return the next version number for a given dashboard key.
    pub fn next_dashboard_version(
        conn: &mut PgConnection,
        dashboard_key: &str,
    ) -> Result<i32, AppError> {
        let max: Option<i32> = dashboard_versions::table
            .filter(dashboard_versions::dashboard_key.eq(dashboard_key))
            .select(diesel::dsl::max(dashboard_versions::version))
            .first(conn)
            .map_err(db_err)?;
        Ok(max.unwrap_or(0) + 1)
    }

    // ── Alerts ───────────────────────────────────────────────────────────────

    pub fn insert_alert(
        conn: &mut PgConnection,
        record: NewDbReportingAlert,
    ) -> Result<DbReportingAlert, AppError> {
        diesel::insert_into(reporting_alerts::table)
            .values(&record)
            .get_result(conn)
            .map_err(db_err)
    }

    /// List alerts with pagination.
    ///
    /// When `owner_user_id` is `Some`, only alerts whose parent subscription
    /// belongs to that user are returned (security: non-admin ownership scoping).
    pub fn list_alerts(
        conn: &mut PgConnection,
        acknowledged: Option<bool>,
        owner_user_id: Option<Uuid>,
        page: i64,
        per_page: i64,
    ) -> Result<(Vec<DbReportingAlert>, i64), AppError> {
        let offset = (page - 1).max(0) * per_page;

        // Build a base query joining through subscriptions when owner-scoped.
        // Diesel's type system requires separate branches for the filtered vs
        // unfiltered query shapes, similar to `list_subscriptions`.
        match (acknowledged, owner_user_id) {
            (Some(ack), Some(uid)) => {
                let rows = reporting_alerts::table
                    .inner_join(reporting_subscriptions::table)
                    .filter(reporting_alerts::acknowledged.eq(ack))
                    .filter(reporting_subscriptions::user_id.eq(uid))
                    .order(reporting_alerts::created_at.desc())
                    .select(reporting_alerts::all_columns)
                    .limit(per_page)
                    .offset(offset)
                    .load(conn)
                    .map_err(db_err)?;
                let total: i64 = reporting_alerts::table
                    .inner_join(reporting_subscriptions::table)
                    .filter(reporting_alerts::acknowledged.eq(ack))
                    .filter(reporting_subscriptions::user_id.eq(uid))
                    .count()
                    .get_result(conn)
                    .map_err(db_err)?;
                Ok((rows, total))
            }
            (None, Some(uid)) => {
                let rows = reporting_alerts::table
                    .inner_join(reporting_subscriptions::table)
                    .filter(reporting_subscriptions::user_id.eq(uid))
                    .order(reporting_alerts::created_at.desc())
                    .select(reporting_alerts::all_columns)
                    .limit(per_page)
                    .offset(offset)
                    .load(conn)
                    .map_err(db_err)?;
                let total: i64 = reporting_alerts::table
                    .inner_join(reporting_subscriptions::table)
                    .filter(reporting_subscriptions::user_id.eq(uid))
                    .count()
                    .get_result(conn)
                    .map_err(db_err)?;
                Ok((rows, total))
            }
            (Some(ack), None) => {
                let rows = reporting_alerts::table
                    .filter(reporting_alerts::acknowledged.eq(ack))
                    .order(reporting_alerts::created_at.desc())
                    .limit(per_page)
                    .offset(offset)
                    .load(conn)
                    .map_err(db_err)?;
                let total: i64 = reporting_alerts::table
                    .filter(reporting_alerts::acknowledged.eq(ack))
                    .count()
                    .get_result(conn)
                    .map_err(db_err)?;
                Ok((rows, total))
            }
            (None, None) => {
                let rows = reporting_alerts::table
                    .order(reporting_alerts::created_at.desc())
                    .limit(per_page)
                    .offset(offset)
                    .load(conn)
                    .map_err(db_err)?;
                let total: i64 = reporting_alerts::table
                    .count()
                    .get_result(conn)
                    .map_err(db_err)?;
                Ok((rows, total))
            }
        }
    }

    pub fn find_alert(
        conn: &mut PgConnection,
        id: Uuid,
    ) -> Result<Option<DbReportingAlert>, AppError> {
        reporting_alerts::table
            .filter(reporting_alerts::id.eq(id))
            .first(conn)
            .optional()
            .map_err(db_err)
    }

    pub fn acknowledge_alert(
        conn: &mut PgConnection,
        id: Uuid,
        actor_id: Uuid,
    ) -> Result<DbReportingAlert, AppError> {
        diesel::update(reporting_alerts::table.filter(reporting_alerts::id.eq(id)))
            .set((
                reporting_alerts::acknowledged.eq(true),
                reporting_alerts::acknowledged_by.eq(Some(actor_id)),
                reporting_alerts::acknowledged_at.eq(Some(Utc::now())),
            ))
            .get_result(conn)
            .map_err(|e| match e {
                diesel::result::Error::NotFound => AppError::NotFound("alert".into()),
                other => db_err(other),
            })
    }

    /// Count offers expiring within the next `days` days.
    pub fn count_expiring_offers(conn: &mut PgConnection, days: i64) -> Result<i64, AppError> {
        diesel::sql_query(
            "SELECT COUNT(*) AS count FROM offers \
             WHERE status NOT IN ('withdrawn','expired','declined','accepted') \
             AND expires_at IS NOT NULL \
             AND expires_at <= now() + ($1 * INTERVAL '1 day')",
        )
        .bind::<diesel::sql_types::BigInt, _>(days)
        .load::<CountRow>(conn)
        .map_err(db_err)
        .map(|rows| rows.first().map_or(0, |r| r.count))
    }

    /// Compute breach rate over the last `days` days.
    /// breach_rate = breach_cancellations / total_cancellations (as percentage).
    pub fn compute_breach_rate(conn: &mut PgConnection, days: i64) -> Result<f64, AppError> {
        diesel::sql_query(
            "SELECT \
               COUNT(*) FILTER (WHERE breach_reason_code IS NOT NULL) AS breach_count, \
               COUNT(*) AS total_count \
             FROM booking_orders \
             WHERE status = 'cancelled' \
             AND updated_at >= now() - ($1 * INTERVAL '1 day')",
        )
        .bind::<diesel::sql_types::BigInt, _>(days)
        .load::<BreachRateRow>(conn)
        .map_err(db_err)
        .map(|rows| {
            rows.first().map_or(0.0, |r| {
                if r.total_count == 0 {
                    0.0
                } else {
                    (r.breach_count as f64 / r.total_count as f64) * 100.0
                }
            })
        })
    }
}

// ── Private QueryableByName helpers ──────────────────────────────────────────

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    count: i64,
}

#[derive(QueryableByName)]
struct BreachRateRow {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    breach_count: i64,
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    total_count: i64,
}
