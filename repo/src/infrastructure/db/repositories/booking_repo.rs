use chrono::{DateTime, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use crate::infrastructure::db::models::{DbBookingOrder, NewDbBookingOrder};
use crate::infrastructure::db::schema::booking_orders;
use crate::shared::errors::AppError;

pub struct PgBookingRepository;

impl PgBookingRepository {
    pub fn find_by_id(
        conn: &mut PgConnection,
        id: Uuid,
    ) -> Result<Option<DbBookingOrder>, AppError> {
        booking_orders::table
            .filter(booking_orders::id.eq(id))
            .select(DbBookingOrder::as_select())
            .first(conn)
            .optional()
            .map_err(db_err)
    }

    pub fn create(
        conn: &mut PgConnection,
        new: NewDbBookingOrder,
    ) -> Result<DbBookingOrder, AppError> {
        diesel::insert_into(booking_orders::table)
            .values(&new)
            .returning(DbBookingOrder::as_returning())
            .get_result(conn)
            .map_err(db_err)
    }

    /// Find an existing booking by idempotency key (non-expired).
    pub fn find_by_idempotency_key(
        conn: &mut PgConnection,
        key: &str,
    ) -> Result<Option<DbBookingOrder>, AppError> {
        booking_orders::table
            .filter(booking_orders::idempotency_key.eq(key))
            .select(DbBookingOrder::as_select())
            .first(conn)
            .optional()
            .map_err(db_err)
    }

    pub fn set_status(conn: &mut PgConnection, id: Uuid, status: &str) -> Result<(), AppError> {
        diesel::update(booking_orders::table.filter(booking_orders::id.eq(id)))
            .set(booking_orders::status.eq(status))
            .execute(conn)
            .map(|_| ())
            .map_err(db_err)
    }

    pub fn set_agreement(
        conn: &mut PgConnection,
        id: Uuid,
        signed_by: &str,
        signed_at: DateTime<Utc>,
        hash: &str,
    ) -> Result<(), AppError> {
        diesel::update(booking_orders::table.filter(booking_orders::id.eq(id)))
            .set((
                booking_orders::agreement_signed_by.eq(Some(signed_by)),
                booking_orders::agreement_signed_at.eq(Some(signed_at)),
                booking_orders::agreement_hash.eq(Some(hash)),
            ))
            .execute(conn)
            .map(|_| ())
            .map_err(db_err)
    }

    pub fn set_breach(
        conn: &mut PgConnection,
        id: Uuid,
        reason: &str,
        reason_code: &str,
    ) -> Result<(), AppError> {
        diesel::update(booking_orders::table.filter(booking_orders::id.eq(id)))
            .set((
                booking_orders::breach_reason.eq(Some(reason)),
                booking_orders::breach_reason_code.eq(Some(reason_code)),
            ))
            .execute(conn)
            .map(|_| ())
            .map_err(db_err)
    }

    pub fn set_exception(conn: &mut PgConnection, id: Uuid, detail: &str) -> Result<(), AppError> {
        diesel::update(booking_orders::table.filter(booking_orders::id.eq(id)))
            .set((
                booking_orders::status.eq("exception"),
                booking_orders::exception_detail.eq(Some(detail)),
            ))
            .execute(conn)
            .map(|_| ())
            .map_err(db_err)
    }

    /// Update the slot_id and hold_expires_at (used during reschedule to swap slots).
    pub fn update_slot(
        conn: &mut PgConnection,
        id: Uuid,
        slot_id: Uuid,
        scheduled_date: chrono::NaiveDate,
        start_time: Option<chrono::NaiveTime>,
        end_time: Option<chrono::NaiveTime>,
        hold_expires_at: DateTime<Utc>,
    ) -> Result<(), AppError> {
        diesel::update(booking_orders::table.filter(booking_orders::id.eq(id)))
            .set((
                booking_orders::slot_id.eq(Some(slot_id)),
                booking_orders::scheduled_date.eq(scheduled_date),
                booking_orders::scheduled_time_start.eq(start_time),
                booking_orders::scheduled_time_end.eq(end_time),
                booking_orders::hold_expires_at.eq(Some(hold_expires_at)),
                booking_orders::status.eq("pending_confirmation"),
                // Clear previous agreement since slot changed
                booking_orders::agreement_signed_by.eq(None::<String>),
                booking_orders::agreement_signed_at.eq(None::<DateTime<Utc>>),
                booking_orders::agreement_hash.eq(None::<String>),
            ))
            .execute(conn)
            .map(|_| ())
            .map_err(db_err)
    }

    /// Paginated list with optional candidate and ownership filters.
    /// Both filters apply to count and rows.
    pub fn list(
        conn: &mut PgConnection,
        candidate_id_filter: Option<Uuid>,
        created_by_filter: Option<Uuid>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<DbBookingOrder>, i64), AppError> {
        use diesel::dsl::count_star;

        let mut base = booking_orders::table.into_boxed();
        let mut count_q = booking_orders::table.into_boxed();

        if let Some(cid) = candidate_id_filter {
            base = base.filter(booking_orders::candidate_id.eq(cid));
            count_q = count_q.filter(booking_orders::candidate_id.eq(cid));
        }

        if let Some(uid) = created_by_filter {
            base = base.filter(booking_orders::created_by.eq(uid));
            count_q = count_q.filter(booking_orders::created_by.eq(uid));
        }

        let total: i64 = count_q.select(count_star()).first(conn).map_err(db_err)?;

        let rows = base
            .select(DbBookingOrder::as_select())
            .order(booking_orders::created_at.desc())
            .offset(offset)
            .limit(limit)
            .load(conn)
            .map_err(db_err)?;

        Ok((rows, total))
    }

    /// Find all booking_orders with status=pending_confirmation and hold_expires_at < now.
    pub fn find_expired_holds(
        conn: &mut PgConnection,
        now: DateTime<Utc>,
    ) -> Result<Vec<DbBookingOrder>, AppError> {
        booking_orders::table
            .filter(booking_orders::status.eq("pending_confirmation"))
            .filter(booking_orders::hold_expires_at.le(now))
            .select(DbBookingOrder::as_select())
            .load(conn)
            .map_err(db_err)
    }
}

fn db_err(e: diesel::result::Error) -> AppError {
    AppError::Internal(format!("database error: {e}"))
}
