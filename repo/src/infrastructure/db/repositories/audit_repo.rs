use diesel::prelude::*;
use uuid::Uuid;

use crate::infrastructure::db::models::{DbAuditEvent, NewDbAuditEvent};
use crate::infrastructure::db::schema::audit_events;
use crate::shared::errors::AppError;

pub struct PgAuditRepository;

impl PgAuditRepository {
    /// Insert a single audit event. This is the ONLY write path for audit_events.
    /// The DB trigger prevents UPDATE and DELETE, so this insert is append-only.
    pub fn insert(conn: &mut PgConnection, event: NewDbAuditEvent) -> Result<(), AppError> {
        diesel::insert_into(audit_events::table)
            .values(&event)
            .execute(conn)
            .map(|_| ())
            .map_err(|e| AppError::Internal(format!("audit insert failed: {e}")))
    }

    /// Paginated list of audit events, ordered by created_at descending.
    pub fn list_events(
        conn: &mut PgConnection,
        page: i64,
        per_page: i64,
    ) -> Result<(Vec<DbAuditEvent>, i64), AppError> {
        let total: i64 = audit_events::table
            .count()
            .get_result(conn)
            .map_err(db_err)?;

        let offset = (page.saturating_sub(1)) * per_page;
        let rows = audit_events::table
            .select(DbAuditEvent::as_select())
            .order(audit_events::created_at.desc())
            .offset(offset)
            .limit(per_page)
            .load(conn)
            .map_err(db_err)?;

        Ok((rows, total))
    }

    /// Find a single audit event by ID.
    pub fn find_event(conn: &mut PgConnection, id: Uuid) -> Result<Option<DbAuditEvent>, AppError> {
        audit_events::table
            .filter(audit_events::id.eq(id))
            .select(DbAuditEvent::as_select())
            .first(conn)
            .optional()
            .map_err(db_err)
    }
}

fn db_err(e: diesel::result::Error) -> AppError {
    AppError::Internal(format!("database error: {e}"))
}
