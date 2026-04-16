use chrono::{DateTime, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use crate::infrastructure::db::models::DbBookingRestriction;
use crate::infrastructure::db::schema::booking_restrictions;
use crate::shared::errors::AppError;

pub struct PgRestrictionRepository;

impl PgRestrictionRepository {
    /// Find all active restrictions for a candidate that have not yet expired.
    pub fn find_active_for_candidate(
        conn: &mut PgConnection,
        candidate_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<Vec<DbBookingRestriction>, AppError> {
        booking_restrictions::table
            .filter(booking_restrictions::candidate_id.eq(candidate_id))
            .filter(booking_restrictions::is_active.eq(true))
            .filter(
                booking_restrictions::expires_at
                    .is_null()
                    .or(booking_restrictions::expires_at.gt(now)),
            )
            .select(DbBookingRestriction::as_select())
            .load(conn)
            .map_err(db_err)
    }
}

fn db_err(e: diesel::result::Error) -> AppError {
    AppError::Internal(format!("database error: {e}"))
}
