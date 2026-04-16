use chrono::NaiveDate;
use diesel::prelude::*;
use diesel::sql_types::Uuid as DieselUuid;
use uuid::Uuid;

use crate::infrastructure::db::models::{DbBookingSlot, NewDbBookingSlot};
use crate::infrastructure::db::schema::booking_slots;
use crate::shared::errors::AppError;

pub struct PgInventoryRepository;

impl PgInventoryRepository {
    pub fn find_by_id(
        conn: &mut PgConnection,
        id: Uuid,
    ) -> Result<Option<DbBookingSlot>, AppError> {
        booking_slots::table
            .filter(booking_slots::id.eq(id))
            .select(DbBookingSlot::as_select())
            .first(conn)
            .optional()
            .map_err(db_err)
    }

    pub fn create(
        conn: &mut PgConnection,
        new: NewDbBookingSlot,
    ) -> Result<DbBookingSlot, AppError> {
        diesel::insert_into(booking_slots::table)
            .values(&new)
            .returning(DbBookingSlot::as_returning())
            .get_result(conn)
            .map_err(db_err)
    }

    /// List available slots for a site on a given date.
    pub fn list_available(
        conn: &mut PgConnection,
        site_id: Uuid,
        date: NaiveDate,
    ) -> Result<Vec<DbBookingSlot>, AppError> {
        booking_slots::table
            .filter(booking_slots::site_id.eq(site_id))
            .filter(booking_slots::slot_date.eq(date))
            .filter(booking_slots::booked_count.lt(booking_slots::capacity))
            .select(DbBookingSlot::as_select())
            .order(booking_slots::start_time.asc())
            .load(conn)
            .map_err(db_err)
    }

    /// Atomically reserve one unit of capacity on a slot.
    ///
    /// Uses `SELECT ... FOR UPDATE` to prevent race conditions.
    /// Returns `Ok(true)` if the reservation succeeded, `Ok(false)` if the slot was full.
    ///
    /// MUST be called inside a `conn.transaction(...)` block.
    pub fn reserve_slot(conn: &mut PgConnection, slot_id: Uuid) -> Result<bool, AppError> {
        // SELECT FOR UPDATE — locks the row for the duration of the enclosing transaction.
        // This prevents two concurrent callers from both seeing available capacity and
        // double-booking.
        let maybe_slot: Option<DbBookingSlot> =
            diesel::sql_query("SELECT * FROM booking_slots WHERE id = $1 FOR UPDATE")
                .bind::<DieselUuid, _>(slot_id)
                .get_result(conn)
                .optional()
                .map_err(db_err)?;

        let slot = match maybe_slot {
            Some(s) => s,
            None => return Err(AppError::NotFound("booking_slot".into())),
        };

        if slot.booked_count >= slot.capacity {
            return Ok(false); // slot is full — caller should not proceed
        }

        // Atomically increment booked_count. The CHECK constraint (booked_count <= capacity)
        // provides a database-level safety net.
        diesel::update(booking_slots::table.filter(booking_slots::id.eq(slot_id)))
            .set(booking_slots::booked_count.eq(booking_slots::booked_count + 1))
            .execute(conn)
            .map_err(db_err)?;

        Ok(true)
    }

    /// Release one unit of capacity on a slot (e.g. on hold expiry or cancellation).
    ///
    /// MUST be called inside a `conn.transaction(...)` block.
    pub fn release_slot(conn: &mut PgConnection, slot_id: Uuid) -> Result<(), AppError> {
        // Use GREATEST to prevent going below zero even under race conditions.
        diesel::sql_query(
            "UPDATE booking_slots SET booked_count = GREATEST(booked_count - 1, 0), \
             updated_at = now() WHERE id = $1",
        )
        .bind::<DieselUuid, _>(slot_id)
        .execute(conn)
        .map(|_| ())
        .map_err(db_err)
    }
}

fn db_err(e: diesel::result::Error) -> AppError {
    AppError::Internal(format!("database error: {e}"))
}
