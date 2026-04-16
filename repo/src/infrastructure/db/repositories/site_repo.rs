use diesel::prelude::*;
use uuid::Uuid;

use crate::{
    infrastructure::db::{models::DbOfficeSite, schema::office_sites},
    shared::errors::AppError,
};

pub struct PgSiteRepository;

impl PgSiteRepository {
    /// Return all active office sites, ordered by name.
    pub fn list_active(conn: &mut PgConnection) -> Result<Vec<DbOfficeSite>, AppError> {
        office_sites::table
            .filter(office_sites::is_active.eq(true))
            .order(office_sites::name.asc())
            .load::<DbOfficeSite>(conn)
            .map_err(|e| AppError::Internal(format!("list sites: {e}")))
    }

    /// Find a single site by ID.
    pub fn find_by_id(conn: &mut PgConnection, id: Uuid) -> Result<Option<DbOfficeSite>, AppError> {
        office_sites::table
            .find(id)
            .first::<DbOfficeSite>(conn)
            .optional()
            .map_err(|e| AppError::Internal(format!("find site: {e}")))
    }

    /// Find a single active site by its short code.
    pub fn find_by_code(
        conn: &mut PgConnection,
        code: &str,
    ) -> Result<Option<DbOfficeSite>, AppError> {
        office_sites::table
            .filter(office_sites::code.eq(code))
            .filter(office_sites::is_active.eq(true))
            .first::<DbOfficeSite>(conn)
            .optional()
            .map_err(|e| AppError::Internal(format!("find site by code: {e}")))
    }
}
