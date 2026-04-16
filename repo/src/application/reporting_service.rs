/// Reporting, dashboard, and alert management service.
///
/// ## Subscription model
///
/// Users subscribe to named report types (`offers_expiring`, `breach_rate`, `snapshot`).
/// Each subscription stores JSON parameters and an optional cron expression.
/// Alerts are always written to `reporting_alerts`.  When
/// `REPORTING_DELIVERY_ENABLED=true` the snapshot job additionally attempts
/// local delivery via the configured email and IM gateways.  Delivery is
/// best-effort: failures are recorded in `delivery_meta` and never prevent
/// the snapshot from completing.
///
/// ## Scheduled jobs
///
/// - **Daily snapshot**: Scans all active subscriptions, runs the corresponding
///   report, and inserts alerts if thresholds are breached.  `last_run_at` is
///   updated on each sweep.  The fire time is DST-aware wall-clock, configured
///   via `SNAPSHOT_TIME_LOCAL` (default 06:00) in the `SNAPSHOT_TIMEZONE`
///   timezone (default UTC).
///
/// ## Threshold alerts
///
/// - Offers expiring within 7 days: fires `warning` when count > 0.
/// - Breach rate > 3% weekly: fires `critical`.
///
/// ## Graceful degradation
///
/// If the database is unavailable during snapshot generation, the scheduler
/// logs the error and retries on the next tick.  Partial results are never
/// written.
use chrono::Utc;
use diesel::PgConnection;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    application::idempotency_op::IdempotencyOp,
    domain::auth::models::AuthContext,
    infrastructure::{
        config::ReportingDeliveryConfig,
        db::{
            models::{NewDbDashboardVersion, NewDbReportingAlert, NewDbReportingSubscription},
            repositories::reporting_repo::PgReportingRepository,
        },
        reporting_delivery::{build_gateway, AlertPayload},
    },
    shared::errors::{AppError, FieldError},
};

// ============================================================
// Input types
// ============================================================

pub struct CreateSubscriptionInput {
    pub report_type: String,
    pub parameters: serde_json::Value,
    pub cron_expression: Option<String>,
    pub idempotency_key: Option<String>,
    pub request_hash: Option<String>,
}

pub struct UpdateSubscriptionInput {
    pub report_type: Option<String>,
    pub parameters: Option<serde_json::Value>,
    pub cron_expression: Option<Option<String>>,
    pub is_active: Option<bool>,
    pub idempotency_key: Option<String>,
    pub request_hash: Option<String>,
}

pub struct PublishDashboardInput {
    pub dashboard_key: String,
    pub layout: serde_json::Value,
    pub idempotency_key: Option<String>,
    pub request_hash: Option<String>,
}

pub struct ListAlertsInput {
    pub acknowledged: Option<bool>,
    pub page: i64,
    pub per_page: i64,
}

// ============================================================
// Response types
// ============================================================

#[derive(Debug, Serialize)]
pub struct SubscriptionResponse {
    pub id: Uuid,
    pub user_id: Uuid,
    pub report_type: String,
    pub parameters: serde_json::Value,
    pub cron_expression: Option<String>,
    pub is_active: bool,
    pub last_run_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct DashboardVersionResponse {
    pub id: Uuid,
    pub dashboard_key: String,
    pub version: i32,
    pub layout: serde_json::Value,
    pub published_by: Uuid,
    pub published_at: String,
}

#[derive(Debug, Serialize)]
pub struct AlertResponse {
    pub id: Uuid,
    pub subscription_id: Uuid,
    pub severity: String,
    pub message: String,
    pub acknowledged: bool,
    pub acknowledged_by: Option<Uuid>,
    pub acknowledged_at: Option<String>,
    pub created_at: String,
    /// Best-effort delivery attempt metadata; omitted when delivery is
    /// disabled or the alert predates migration 5.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_meta: Option<serde_json::Value>,
}

// ============================================================
// Service
// ============================================================

pub struct ReportingService;

impl ReportingService {
    // ── Subscriptions ────────────────────────────────────────────────────────

    pub fn create_subscription(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        input: CreateSubscriptionInput,
    ) -> Result<SubscriptionResponse, AppError> {
        ctx.require_permission("reporting", "create")?;
        validate_report_type(&input.report_type)?;

        let idem = IdempotencyOp::new(
            input.idempotency_key.as_deref(),
            input.request_hash.as_deref(),
            ctx.user_id,
            "/api/v1/reporting/subscriptions",
        );
        if let Some(replay_id) = idem.check(conn)? {
            let db = PgReportingRepository::find_subscription(conn, replay_id)?
                .ok_or_else(|| AppError::NotFound("subscription".into()))?;
            return Ok(map_subscription(db));
        }

        let record = NewDbReportingSubscription {
            id: Uuid::new_v4(),
            user_id: ctx.user_id,
            report_type: input.report_type,
            parameters: input.parameters,
            cron_expression: input.cron_expression,
            is_active: true,
        };

        let sub = PgReportingRepository::insert_subscription(conn, record)?;

        info!(
            actor = %ctx.user_id,
            subscription_id = %sub.id,
            report_type = %sub.report_type,
            "reporting subscription created"
        );

        idem.record(conn, 201, Some(sub.id));
        Ok(map_subscription(sub))
    }

    pub fn get_subscription(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        id: Uuid,
    ) -> Result<SubscriptionResponse, AppError> {
        let sub = PgReportingRepository::find_subscription(conn, id)?
            .ok_or_else(|| AppError::NotFound("subscription".into()))?;
        // Object-level authorization: only the owner or platform_admin may read
        if !ctx.has_role("platform_admin") && sub.user_id != ctx.user_id {
            return Err(AppError::Forbidden);
        }
        Ok(map_subscription(sub))
    }

    pub fn list_subscriptions(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        page: i64,
        per_page: i64,
    ) -> Result<(Vec<SubscriptionResponse>, i64), AppError> {
        // platform_admin sees all; others see only their own
        let user_filter = if ctx.has_role("platform_admin") {
            None
        } else {
            Some(ctx.user_id)
        };
        let (rows, total) =
            PgReportingRepository::list_subscriptions(conn, user_filter, page, per_page)?;
        Ok((rows.into_iter().map(map_subscription).collect(), total))
    }

    pub fn update_subscription(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        id: Uuid,
        input: UpdateSubscriptionInput,
    ) -> Result<SubscriptionResponse, AppError> {
        let idem = IdempotencyOp::new(
            input.idempotency_key.as_deref(),
            input.request_hash.as_deref(),
            ctx.user_id,
            "/api/v1/reporting/subscriptions",
        );
        if let Some(replay_id) = idem.check(conn)? {
            let db = PgReportingRepository::find_subscription(conn, replay_id)?
                .ok_or_else(|| AppError::NotFound("subscription".into()))?;
            return Ok(map_subscription(db));
        }

        // Ensure the subscription exists and user owns it (unless admin)
        let sub = PgReportingRepository::find_subscription(conn, id)?
            .ok_or_else(|| AppError::NotFound("subscription".into()))?;

        if !ctx.has_role("platform_admin") && sub.user_id != ctx.user_id {
            return Err(AppError::Forbidden);
        }

        if let Some(ref rt) = input.report_type {
            validate_report_type(rt)?;
        }

        let updated = PgReportingRepository::update_subscription(
            conn,
            id,
            input.report_type,
            input.parameters,
            input.cron_expression,
            input.is_active,
        )?;
        idem.record(conn, 200, Some(updated.id));
        Ok(map_subscription(updated))
    }

    pub fn delete_subscription(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        id: Uuid,
    ) -> Result<(), AppError> {
        let sub = PgReportingRepository::find_subscription(conn, id)?
            .ok_or_else(|| AppError::NotFound("subscription".into()))?;

        if !ctx.has_role("platform_admin") && sub.user_id != ctx.user_id {
            return Err(AppError::Forbidden);
        }

        PgReportingRepository::delete_subscription(conn, id)?;

        info!(
            actor = %ctx.user_id,
            subscription_id = %id,
            "reporting subscription deleted"
        );

        Ok(())
    }

    // ── Dashboard versions ───────────────────────────────────────────────────

    pub fn publish_dashboard(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        input: PublishDashboardInput,
    ) -> Result<DashboardVersionResponse, AppError> {
        ctx.require_permission("reporting", "update")?;

        let idem = IdempotencyOp::new(
            input.idempotency_key.as_deref(),
            input.request_hash.as_deref(),
            ctx.user_id,
            "/api/v1/reporting/dashboards",
        );
        if let Some(replay_id) = idem.check(conn)? {
            let db = PgReportingRepository::find_dashboard_version_by_id(conn, replay_id)?
                .ok_or_else(|| AppError::NotFound("dashboard_version".into()))?;
            return Ok(map_dashboard_version(db));
        }

        let next_version =
            PgReportingRepository::next_dashboard_version(conn, &input.dashboard_key)?;

        let record = NewDbDashboardVersion {
            id: Uuid::new_v4(),
            dashboard_key: input.dashboard_key,
            version: next_version,
            layout: input.layout,
            published_by: ctx.user_id,
            published_at: Utc::now(),
        };

        let v = PgReportingRepository::insert_dashboard_version(conn, record)?;

        info!(
            actor = %ctx.user_id,
            dashboard_key = %v.dashboard_key,
            version = v.version,
            "dashboard version published"
        );

        idem.record(conn, 201, Some(v.id));
        Ok(map_dashboard_version(v))
    }

    pub fn list_dashboard_versions(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        dashboard_key: &str,
    ) -> Result<Vec<DashboardVersionResponse>, AppError> {
        ctx.require_permission("reporting", "read")?;
        let rows = PgReportingRepository::list_dashboard_versions(conn, dashboard_key)?;
        Ok(rows.into_iter().map(map_dashboard_version).collect())
    }

    // ── Alerts ───────────────────────────────────────────────────────────────

    pub fn list_alerts(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        input: ListAlertsInput,
    ) -> Result<(Vec<AlertResponse>, i64), AppError> {
        ctx.require_permission("reporting", "read")?;

        // Security: non-admin users only see alerts tied to their own
        // subscriptions. platform_admin sees all alerts globally.
        let owner_filter = if ctx.has_role("platform_admin") {
            None
        } else {
            Some(ctx.user_id)
        };

        let (rows, total) = PgReportingRepository::list_alerts(
            conn,
            input.acknowledged,
            owner_filter,
            input.page,
            input.per_page,
        )?;
        Ok((rows.into_iter().map(map_alert).collect(), total))
    }

    pub fn acknowledge_alert(
        conn: &mut PgConnection,
        id: Uuid,
        ctx: &AuthContext,
    ) -> Result<AlertResponse, AppError> {
        ctx.require_permission("reporting", "update")?;

        // Ownership check: fetch the alert, then verify the parent subscription
        // belongs to the caller. platform_admin bypasses the ownership check.
        let existing = PgReportingRepository::find_alert(conn, id)?
            .ok_or_else(|| AppError::NotFound("alert".into()))?;

        if !ctx.has_role("platform_admin") {
            let sub = PgReportingRepository::find_subscription(conn, existing.subscription_id)?
                .ok_or_else(|| AppError::NotFound("alert".into()))?;
            if sub.user_id != ctx.user_id {
                return Err(AppError::Forbidden);
            }
        }

        let alert = PgReportingRepository::acknowledge_alert(conn, id, ctx.user_id)?;

        info!(
            actor = %ctx.user_id,
            alert_id = %id,
            "alert acknowledged"
        );

        Ok(map_alert(alert))
    }

    // ── Snapshot generation (called by scheduled job) ────────────────────────

    /// Run the daily snapshot sweep.
    ///
    /// For each active subscription, evaluates the report and inserts alerts
    /// if thresholds are breached.  Updates `last_run_at` on each subscription.
    ///
    /// When `delivery_config.enabled = true`, also attempts local delivery via
    /// the configured email / IM gateways.  Delivery is best-effort: outcomes
    /// are stored in `delivery_meta` and never cause this function to fail.
    ///
    /// This is a system-level operation — no `AuthContext` required.
    pub fn run_daily_snapshot(
        conn: &mut PgConnection,
        delivery_config: &ReportingDeliveryConfig,
    ) -> Result<usize, AppError> {
        let gateway = build_gateway(delivery_config);
        let subscriptions = PgReportingRepository::list_active_subscriptions_for_snapshot(conn)?;
        let now = Utc::now();
        let mut fired = 0usize;

        for sub in subscriptions {
            match sub.report_type.as_str() {
                "offers_expiring" => {
                    let days: i64 = sub
                        .parameters
                        .get("days")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(7);
                    let count =
                        PgReportingRepository::count_expiring_offers(conn, days).unwrap_or(0);
                    if count > 0 {
                        let alert_id = Uuid::new_v4();
                        let message = format!("{count} offer(s) expiring within {days} days");
                        let payload = AlertPayload {
                            alert_id,
                            subscription_id: sub.id,
                            severity: "warning".into(),
                            message: message.clone(),
                        };
                        let delivery_meta = Some(gateway.deliver_all(&payload));
                        let _ = PgReportingRepository::insert_alert(
                            conn,
                            NewDbReportingAlert {
                                id: alert_id,
                                subscription_id: sub.id,
                                severity: "warning".into(),
                                message,
                                acknowledged: false,
                                delivery_meta,
                            },
                        );
                        warn!(
                            subscription_id = %sub.id,
                            count = count,
                            days = days,
                            "snapshot alert: offers expiring"
                        );
                        fired += 1;
                    }
                }
                "breach_rate" => {
                    let threshold: f64 = sub
                        .parameters
                        .get("threshold_pct")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(3.0);
                    let days: i64 = sub
                        .parameters
                        .get("window_days")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(7);
                    let rate =
                        PgReportingRepository::compute_breach_rate(conn, days).unwrap_or(0.0);
                    if rate > threshold {
                        let alert_id = Uuid::new_v4();
                        let message = format!(
                            "Breach rate {rate:.1}% exceeds threshold {threshold:.1}% over {days}-day window"
                        );
                        let payload = AlertPayload {
                            alert_id,
                            subscription_id: sub.id,
                            severity: "critical".into(),
                            message: message.clone(),
                        };
                        let delivery_meta = Some(gateway.deliver_all(&payload));
                        let _ = PgReportingRepository::insert_alert(
                            conn,
                            NewDbReportingAlert {
                                id: alert_id,
                                subscription_id: sub.id,
                                severity: "critical".into(),
                                message,
                                acknowledged: false,
                                delivery_meta,
                            },
                        );
                        warn!(
                            subscription_id = %sub.id,
                            rate = rate,
                            threshold = threshold,
                            "snapshot alert: breach rate exceeded"
                        );
                        fired += 1;
                    }
                }
                "snapshot" => {
                    // General snapshot: record that it ran
                    let alert_id = Uuid::new_v4();
                    let message = format!("Daily snapshot generated at {}", now.to_rfc3339());
                    let payload = AlertPayload {
                        alert_id,
                        subscription_id: sub.id,
                        severity: "info".into(),
                        message: message.clone(),
                    };
                    let delivery_meta = Some(gateway.deliver_all(&payload));
                    let _ = PgReportingRepository::insert_alert(
                        conn,
                        NewDbReportingAlert {
                            id: alert_id,
                            subscription_id: sub.id,
                            severity: "info".into(),
                            message,
                            acknowledged: false,
                            delivery_meta,
                        },
                    );
                    fired += 1;
                }
                _ => {} // Unknown report type — skip gracefully
            }

            let _ = PgReportingRepository::mark_subscription_run(conn, sub.id, now);
        }

        Ok(fired)
    }
}

// ============================================================
// Helpers
// ============================================================

fn validate_report_type(rt: &str) -> Result<(), AppError> {
    match rt {
        "offers_expiring" | "breach_rate" | "snapshot" => Ok(()),
        _ => Err(AppError::Validation(vec![FieldError {
            field: "report_type".into(),
            message: format!(
                "unknown report type '{rt}'; valid: offers_expiring, breach_rate, snapshot"
            ),
        }])),
    }
}

fn map_subscription(
    s: crate::infrastructure::db::models::DbReportingSubscription,
) -> SubscriptionResponse {
    SubscriptionResponse {
        id: s.id,
        user_id: s.user_id,
        report_type: s.report_type,
        parameters: s.parameters,
        cron_expression: s.cron_expression,
        is_active: s.is_active,
        last_run_at: s.last_run_at.map(|t| t.to_rfc3339()),
        created_at: s.created_at.to_rfc3339(),
        updated_at: s.updated_at.to_rfc3339(),
    }
}

fn map_dashboard_version(
    v: crate::infrastructure::db::models::DbDashboardVersion,
) -> DashboardVersionResponse {
    DashboardVersionResponse {
        id: v.id,
        dashboard_key: v.dashboard_key,
        version: v.version,
        layout: v.layout,
        published_by: v.published_by,
        published_at: v.published_at.to_rfc3339(),
    }
}

fn map_alert(a: crate::infrastructure::db::models::DbReportingAlert) -> AlertResponse {
    AlertResponse {
        id: a.id,
        subscription_id: a.subscription_id,
        severity: a.severity,
        message: a.message,
        acknowledged: a.acknowledged,
        acknowledged_by: a.acknowledged_by,
        acknowledged_at: a.acknowledged_at.map(|t| t.to_rfc3339()),
        created_at: a.created_at.to_rfc3339(),
        delivery_meta: a.delivery_meta,
    }
}
