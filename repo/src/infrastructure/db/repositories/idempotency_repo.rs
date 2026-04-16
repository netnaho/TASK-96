/// Canonical idempotency key store.
///
/// Backs the 24-hour deduplication window for mutating endpoints.
/// Keys expire after 24 hours; a background job prunes expired rows.
use chrono::Utc;
use diesel::prelude::*;
use uuid::Uuid;

use crate::{
    infrastructure::db::{
        models::{DbIdempotencyKey, NewDbIdempotencyKey},
        schema::idempotency_keys,
    },
    shared::errors::AppError,
};

fn db_err(e: diesel::result::Error) -> AppError {
    AppError::Internal(format!("idempotency_repo: {e}"))
}

pub struct PgIdempotencyRepository;

impl PgIdempotencyRepository {
    /// Look up an active (non-expired) idempotency record.
    pub fn find_active(
        conn: &mut PgConnection,
        key: &str,
    ) -> Result<Option<DbIdempotencyKey>, AppError> {
        let now = Utc::now();
        idempotency_keys::table
            .filter(
                idempotency_keys::key
                    .eq(key)
                    .and(idempotency_keys::expires_at.gt(now)),
            )
            .first::<DbIdempotencyKey>(conn)
            .optional()
            .map_err(db_err)
    }

    /// Insert a new idempotency record. ON CONFLICT DO NOTHING so concurrent
    /// requests racing on the same key never overwrite a completed record.
    pub fn insert(conn: &mut PgConnection, record: NewDbIdempotencyKey) -> Result<(), AppError> {
        diesel::insert_into(idempotency_keys::table)
            .values(&record)
            .on_conflict_do_nothing()
            .execute(conn)
            .map_err(db_err)?;
        Ok(())
    }

    /// Delete all expired keys. Called by the background pruning job.
    pub fn delete_expired(conn: &mut PgConnection) -> Result<usize, AppError> {
        let now = Utc::now();
        diesel::delete(idempotency_keys::table.filter(idempotency_keys::expires_at.le(now)))
            .execute(conn)
            .map_err(db_err)
    }

    /// Delete a specific key by (key, user_id) pair — used to clean up if the
    /// request we're about to record ultimately fails before we can store it.
    pub fn delete_by_key(
        conn: &mut PgConnection,
        key: &str,
        user_id: Uuid,
    ) -> Result<(), AppError> {
        diesel::delete(
            idempotency_keys::table.filter(
                idempotency_keys::key
                    .eq(key)
                    .and(idempotency_keys::user_id.eq(user_id)),
            ),
        )
        .execute(conn)
        .map_err(db_err)?;
        Ok(())
    }
}
