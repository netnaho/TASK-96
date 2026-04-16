use diesel::prelude::*;
use uuid::Uuid;

use crate::infrastructure::db::models::{DbCandidate, NewDbCandidate};
use crate::infrastructure::db::schema::candidates;
use crate::shared::errors::AppError;

pub struct PgCandidateRepository;

impl PgCandidateRepository {
    pub fn find_by_id(conn: &mut PgConnection, id: Uuid) -> Result<Option<DbCandidate>, AppError> {
        candidates::table
            .filter(candidates::id.eq(id))
            .select(DbCandidate::as_select())
            .first(conn)
            .optional()
            .map_err(db_err)
    }

    pub fn create(
        conn: &mut PgConnection,
        new_candidate: NewDbCandidate,
    ) -> Result<DbCandidate, AppError> {
        diesel::insert_into(candidates::table)
            .values(&new_candidate)
            .returning(DbCandidate::as_returning())
            .get_result(conn)
            .map_err(|e| match e {
                diesel::result::Error::DatabaseError(
                    diesel::result::DatabaseErrorKind::UniqueViolation,
                    info,
                ) => AppError::Conflict(info.message().to_string()),
                other => db_err(other),
            })
    }

    /// Update mutable fields on a candidate row. Only fields that may change after creation
    /// are included here; id, created_by, created_at are immutable.
    pub fn update(
        conn: &mut PgConnection,
        id: Uuid,
        first_name: &str,
        last_name: &str,
        email: &str,
        phone_encrypted: Option<&[u8]>,
        ssn_last4_encrypted: Option<&[u8]>,
        resume_storage_key: Option<&str>,
        source: Option<&str>,
        tags: &Vec<String>,
        notes: Option<&str>,
        organization_id: Option<Uuid>,
    ) -> Result<DbCandidate, AppError> {
        diesel::update(candidates::table.filter(candidates::id.eq(id)))
            .set((
                candidates::first_name.eq(first_name),
                candidates::last_name.eq(last_name),
                candidates::email.eq(email),
                candidates::phone_encrypted.eq(phone_encrypted),
                candidates::ssn_last4_encrypted.eq(ssn_last4_encrypted),
                candidates::resume_storage_key.eq(resume_storage_key),
                candidates::source.eq(source),
                candidates::tags.eq(tags),
                candidates::notes.eq(notes),
                candidates::organization_id.eq(organization_id),
            ))
            .returning(DbCandidate::as_returning())
            .get_result(conn)
            .map_err(db_err)
    }

    /// Paginated list. Returns (rows, total_count).
    ///
    /// Both `organization_id_filter` and `created_by_filter` are applied to
    /// the count query as well as the row query, so the total accurately
    /// reflects only the records visible to the caller.
    pub fn list(
        conn: &mut PgConnection,
        organization_id_filter: Option<Uuid>,
        created_by_filter: Option<Uuid>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<DbCandidate>, i64), AppError> {
        use diesel::dsl::count_star;

        let mut base = candidates::table.into_boxed();
        let mut count_q = candidates::table.into_boxed();

        if let Some(org_id) = organization_id_filter {
            base = base.filter(candidates::organization_id.eq(org_id));
            count_q = count_q.filter(candidates::organization_id.eq(org_id));
        }

        if let Some(uid) = created_by_filter {
            base = base.filter(candidates::created_by.eq(uid));
            count_q = count_q.filter(candidates::created_by.eq(uid));
        }

        let total: i64 = count_q.select(count_star()).first(conn).map_err(db_err)?;

        let rows = base
            .select(DbCandidate::as_select())
            .order(candidates::created_at.desc())
            .offset(offset)
            .limit(limit)
            .load(conn)
            .map_err(db_err)?;

        Ok((rows, total))
    }
}

fn db_err(e: diesel::result::Error) -> AppError {
    AppError::Internal(format!("database error: {e}"))
}
