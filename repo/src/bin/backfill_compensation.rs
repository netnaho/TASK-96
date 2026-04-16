use diesel::prelude::*;
use tracing::{error, info};

use talentflow::infrastructure::{config::AppConfig, crypto, db, logging};

/// Backfill structured compensation columns from the encrypted blob.
///
/// Run after migration 00000000000007_compensation_structured_fields has been
/// applied. Reads each offer that has `compensation_encrypted IS NOT NULL` but
/// `salary_cents IS NULL`, decrypts the blob, and populates the five structured
/// columns.
///
/// This binary is idempotent — it only touches rows where the structured
/// columns are still NULL. Safe to re-run if interrupted.
///
/// ```bash
/// DATABASE_URL=postgres://... ENCRYPTION_KEY=... cargo run --bin backfill_compensation
/// ```
fn main() {
    dotenvy::dotenv().ok();
    logging::init();

    let config = AppConfig::from_env();
    let pool = db::create_pool(&config.database_url);
    let mut conn = pool.get().expect("failed to get database connection");

    info!("starting compensation backfill");

    // Find all offers that have an encrypted blob but no structured data yet
    let rows: Vec<OfferRow> = diesel::sql_query(
        "SELECT id, compensation_encrypted \
         FROM offers \
         WHERE compensation_encrypted IS NOT NULL \
           AND salary_cents IS NULL \
         ORDER BY created_at ASC",
    )
    .load(&mut conn)
    .expect("failed to query offers");

    info!(count = rows.len(), "found offers to backfill");

    let mut success = 0u64;
    let mut failed = 0u64;

    for row in &rows {
        match crypto::decrypt(&row.compensation_encrypted, &config.encryption_key) {
            Ok(plaintext) => {
                match serde_json::from_slice::<CompensationData>(&plaintext) {
                    Ok(comp) => {
                        let salary_cents = comp.base_salary_usd as i64 * 100;
                        let result = diesel::sql_query(
                            "UPDATE offers \
                             SET salary_cents = $1, \
                                 bonus_target_pct = $2, \
                                 equity_units = $3, \
                                 pto_days = $4, \
                                 k401_match_pct = $5 \
                             WHERE id = $6 AND salary_cents IS NULL",
                        )
                        .bind::<diesel::sql_types::BigInt, _>(salary_cents)
                        .bind::<diesel::sql_types::Double, _>(comp.bonus_target_pct)
                        .bind::<diesel::sql_types::Integer, _>(comp.equity_units as i32)
                        .bind::<diesel::sql_types::SmallInt, _>(comp.pto_days as i16)
                        .bind::<diesel::sql_types::Double, _>(comp.k401_match_pct)
                        .bind::<diesel::sql_types::Uuid, _>(row.id)
                        .execute(&mut conn);

                        match result {
                            Ok(_) => success += 1,
                            Err(e) => {
                                error!(offer_id = %row.id, error = %e, "failed to update offer");
                                failed += 1;
                            }
                        }
                    }
                    Err(e) => {
                        error!(offer_id = %row.id, error = %e, "failed to deserialize compensation");
                        failed += 1;
                    }
                }
            }
            Err(e) => {
                error!(offer_id = %row.id, error = %e, "failed to decrypt compensation");
                failed += 1;
            }
        }
    }

    info!(success, failed, total = rows.len() as u64, "backfill complete");

    if failed > 0 {
        std::process::exit(1);
    }
}

#[derive(diesel::QueryableByName)]
struct OfferRow {
    #[diesel(sql_type = diesel::sql_types::Uuid)]
    id: uuid::Uuid,
    #[diesel(sql_type = diesel::sql_types::Binary)]
    compensation_encrypted: Vec<u8>,
}

/// Mirror of the domain struct — kept here so the backfill binary doesn't depend
/// on the full domain module (avoids coupling issues if the domain struct evolves).
#[derive(serde::Deserialize)]
struct CompensationData {
    base_salary_usd: u64,
    bonus_target_pct: f64,
    equity_units: u32,
    pto_days: u16,
    k401_match_pct: f64,
}
