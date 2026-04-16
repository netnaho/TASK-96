/// Background job / scheduler infrastructure.
///
/// Uses `tokio-cron-scheduler` for fixed-interval jobs and a hand-rolled
/// tokio loop for the DST-safe daily snapshot:
///
/// - **Hold auto-release** (every 60 s): transitions expired
///   `pending_confirmation` bookings to `cancelled` and releases capacity.
/// - **Session expiry cleanup** (every 300 s): deletes expired session rows.
/// - **Idempotency key pruning** (every 300 s): deletes rows past 24 h TTL.
/// - **Daily reporting snapshot** (configurable local time + timezone):
///   fires at `SNAPSHOT_TIME_LOCAL` in `SNAPSHOT_TIMEZONE` (default 06:00 UTC).
///   Uses DST-safe scheduling — see [`time_helpers`] for details.
///
/// The scheduler is only started when `SCHEDULER_ENABLED=true`.
pub mod time_helpers;

use std::sync::Arc;

use chrono::Utc;
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing::{error, info};

use crate::infrastructure::{config::AppConfig, db::DbPool};

/// Start the background job scheduler.
///
/// This spawns a tokio task that runs the cron scheduler indefinitely.
/// The daily snapshot is additionally managed by a dedicated tokio task
/// so that its schedule can be computed dynamically (DST-safe).
pub async fn start_scheduler(pool: DbPool, config: AppConfig) {
    let sched = JobScheduler::new()
        .await
        .expect("failed to create job scheduler");

    let pool = Arc::new(pool);

    // ── Hold auto-release: every 60 seconds ──────────────────────────
    let hold_pool = pool.clone();
    let hold_job = Job::new_repeated(std::time::Duration::from_secs(60), move |_uuid, _lock| {
        let pool = hold_pool.clone();
        std::thread::spawn(move || match pool.get() {
            Ok(mut conn) => {
                match crate::application::booking_service::BookingService::release_expired_holds(
                    &mut conn,
                ) {
                    Ok(0) => {}
                    Ok(n) => info!(released = n, "hold auto-release sweep"),
                    Err(e) => error!(error = %e, "hold auto-release failed"),
                }
            }
            Err(e) => error!(error = %e, "hold auto-release: failed to get DB connection"),
        });
    })
    .expect("failed to create hold auto-release job");

    sched.add(hold_job).await.expect("failed to add hold job");

    // ── Session expiry cleanup: every 5 minutes ──────────────────────
    let session_pool = pool.clone();
    let session_job =
        Job::new_repeated(std::time::Duration::from_secs(300), move |_uuid, _lock| {
            let pool = session_pool.clone();
            std::thread::spawn(move || {
                match pool.get() {
                    Ok(mut conn) => {
                        match crate::infrastructure::db::repositories::session_repo::PgSessionRepository::delete_expired(
                            &mut conn,
                        ) {
                            Ok(0) => {}
                            Ok(n) => info!(deleted = n, "expired sessions cleaned"),
                            Err(e) => error!(error = %e, "session cleanup failed"),
                        }
                    }
                    Err(e) => error!(error = %e, "session cleanup: failed to get DB connection"),
                }
            });
        })
        .expect("failed to create session cleanup job");

    sched
        .add(session_job)
        .await
        .expect("failed to add session cleanup job");

    // ── Idempotency key pruning: every 5 minutes ─────────────────────
    let idem_pool = pool.clone();
    let idem_job =
        Job::new_repeated(std::time::Duration::from_secs(300), move |_uuid, _lock| {
            let pool = idem_pool.clone();
            std::thread::spawn(move || {
                match pool.get() {
                    Ok(mut conn) => {
                        match crate::infrastructure::db::repositories::idempotency_repo::PgIdempotencyRepository::delete_expired(
                            &mut conn,
                        ) {
                            Ok(0) => {}
                            Ok(n) => info!(deleted = n, "expired idempotency keys pruned"),
                            Err(e) => error!(error = %e, "idempotency key pruning failed"),
                        }
                    }
                    Err(e) => error!(error = %e, "idempotency pruning: failed to get DB connection"),
                }
            });
        })
        .expect("failed to create idempotency pruning job");

    sched
        .add(idem_job)
        .await
        .expect("failed to add idempotency pruning job");

    // ── Daily reporting snapshot: DST-safe local-time loop ───────────────
    //
    // Instead of a fixed UTC cron expression, we compute the next local-time
    // occurrence of SNAPSHOT_TIME_LOCAL in SNAPSHOT_TIMEZONE on each iteration.
    // This keeps the snapshot at the configured *local* clock time across DST
    // transitions without requiring any external time-zone service.
    let snap_pool = pool.clone();
    let snap_tz_str = config.scheduler.snapshot_timezone.clone();
    let snap_time_str = config.scheduler.snapshot_time_local.clone();
    let snap_delivery = config.reporting_delivery.clone();

    tokio::spawn(async move {
        let tz: chrono_tz::Tz = snap_tz_str.parse().unwrap_or_else(|_| {
            error!(
                timezone = %snap_tz_str,
                "unknown SNAPSHOT_TIMEZONE — falling back to UTC"
            );
            chrono_tz::UTC
        });
        let target_time = time_helpers::parse_hhmm(&snap_time_str);

        loop {
            let next_utc = time_helpers::next_local_run_utc(tz, target_time);
            let next_local = next_utc.with_timezone(&tz);

            info!(
                snapshot_timezone = %snap_tz_str,
                next_local_run = %next_local.format("%Y-%m-%d %H:%M:%S %Z"),
                next_utc_run = %next_utc.format("%Y-%m-%d %H:%M:%S UTC"),
                "daily snapshot scheduled"
            );

            let now_utc = Utc::now();
            let delta = (next_utc - now_utc)
                .to_std()
                .unwrap_or(std::time::Duration::from_secs(1));
            tokio::time::sleep(delta).await;

            let pool = snap_pool.clone();
            let delivery = snap_delivery.clone();
            std::thread::spawn(move || {
                match pool.get() {
                    Ok(mut conn) => {
                        match crate::application::reporting_service::ReportingService::run_daily_snapshot(
                            &mut conn,
                            &delivery,
                        ) {
                            Ok(0) => info!("daily snapshot: no alerts fired"),
                            Ok(n) => info!(alerts_fired = n, "daily snapshot complete"),
                            Err(e) => error!(error = %e, "daily snapshot failed"),
                        }
                    }
                    Err(e) => error!(error = %e, "daily snapshot: failed to get DB connection"),
                }
            });
        }
    });

    info!(
        "background scheduler started (hold release: 60s, session cleanup: 300s, \
         idempotency pruning: 300s, daily snapshot: {} {})",
        config.scheduler.snapshot_time_local, config.scheduler.snapshot_timezone,
    );

    sched.start().await.expect("failed to start scheduler");
}
