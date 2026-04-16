use chrono::{DateTime, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use crate::infrastructure::db::models::{DbSession, NewDbSession};
use crate::infrastructure::db::schema::sessions;
use crate::shared::errors::AppError;

pub struct PgSessionRepository;

impl PgSessionRepository {
    pub fn create(
        conn: &mut PgConnection,
        new_session: NewDbSession,
    ) -> Result<DbSession, AppError> {
        diesel::insert_into(sessions::table)
            .values(&new_session)
            .returning(DbSession::as_returning())
            .get_result(conn)
            .map_err(db_err)
    }

    /// Look up a non-expired session by its stored token hash.
    /// Returns `None` if not found or already expired.
    pub fn find_valid_by_token_hash(
        conn: &mut PgConnection,
        token_hash: &str,
    ) -> Result<Option<DbSession>, AppError> {
        sessions::table
            .filter(sessions::token_hash.eq(token_hash))
            .filter(sessions::expires_at.gt(Utc::now()))
            .select(DbSession::as_select())
            .first(conn)
            .optional()
            .map_err(db_err)
    }

    /// Touch last_activity_at for idle-timeout tracking.
    pub fn touch(conn: &mut PgConnection, session_id: Uuid) -> Result<(), AppError> {
        diesel::update(sessions::table.filter(sessions::id.eq(session_id)))
            .set(sessions::last_activity_at.eq(Some(Utc::now())))
            .execute(conn)
            .map(|_| ())
            .map_err(db_err)
    }

    /// Delete a session by its ID (logout).
    pub fn delete(conn: &mut PgConnection, session_id: Uuid) -> Result<(), AppError> {
        diesel::delete(sessions::table.filter(sessions::id.eq(session_id)))
            .execute(conn)
            .map(|_| ())
            .map_err(db_err)
    }

    /// Count live (unexpired) sessions belonging to a user.
    pub fn count_active_for_user(conn: &mut PgConnection, user_id: Uuid) -> Result<i64, AppError> {
        sessions::table
            .filter(sessions::user_id.eq(user_id))
            .filter(sessions::expires_at.gt(Utc::now()))
            .count()
            .get_result(conn)
            .map_err(db_err)
    }

    /// Evict the oldest session when the per-user cap is exceeded.
    pub fn evict_oldest_for_user(conn: &mut PgConnection, user_id: Uuid) -> Result<(), AppError> {
        // Find the ID of the oldest session for this user
        let oldest_id: Option<Uuid> = sessions::table
            .filter(sessions::user_id.eq(user_id))
            .order(sessions::created_at.asc())
            .select(sessions::id)
            .first(conn)
            .optional()
            .map_err(db_err)?;

        if let Some(id) = oldest_id {
            diesel::delete(sessions::table.filter(sessions::id.eq(id)))
                .execute(conn)
                .map(|_| ())
                .map_err(db_err)?;
        }
        Ok(())
    }

    /// Prune all expired sessions (called by background job).
    pub fn delete_expired(conn: &mut PgConnection) -> Result<usize, AppError> {
        diesel::delete(sessions::table.filter(sessions::expires_at.le(Utc::now())))
            .execute(conn)
            .map_err(db_err)
    }
}

fn db_err(e: diesel::result::Error) -> AppError {
    AppError::Internal(format!("database error: {e}"))
}
