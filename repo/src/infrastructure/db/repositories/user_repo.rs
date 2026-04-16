use chrono::{DateTime, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use crate::infrastructure::db::models::{DbUser, NewDbUser};
use crate::infrastructure::db::schema::users;
use crate::shared::errors::AppError;

/// Diesel-backed user repository.
/// All methods take a `&mut PgConnection` so callers control connection lifecycle.
pub struct PgUserRepository;

impl PgUserRepository {
    pub fn find_by_id(conn: &mut PgConnection, id: Uuid) -> Result<Option<DbUser>, AppError> {
        users::table
            .filter(users::id.eq(id))
            .select(DbUser::as_select())
            .first(conn)
            .optional()
            .map_err(db_err)
    }

    pub fn find_by_username(
        conn: &mut PgConnection,
        username: &str,
    ) -> Result<Option<DbUser>, AppError> {
        users::table
            .filter(users::username.eq(username))
            .select(DbUser::as_select())
            .first(conn)
            .optional()
            .map_err(db_err)
    }

    pub fn find_by_email(conn: &mut PgConnection, email: &str) -> Result<Option<DbUser>, AppError> {
        users::table
            .filter(users::email.eq(email))
            .select(DbUser::as_select())
            .first(conn)
            .optional()
            .map_err(db_err)
    }

    pub fn create(conn: &mut PgConnection, new_user: NewDbUser) -> Result<DbUser, AppError> {
        diesel::insert_into(users::table)
            .values(&new_user)
            .returning(DbUser::as_returning())
            .get_result(conn)
            .map_err(|e| match e {
                diesel::result::Error::DatabaseError(
                    diesel::result::DatabaseErrorKind::UniqueViolation,
                    info,
                ) => {
                    let msg = info.message().to_string();
                    AppError::Conflict(msg)
                }
                other => db_err(other),
            })
    }

    pub fn increment_failed_logins(conn: &mut PgConnection, id: Uuid) -> Result<(), AppError> {
        diesel::update(users::table.filter(users::id.eq(id)))
            .set(users::failed_login_count.eq(users::failed_login_count + 1))
            .execute(conn)
            .map(|_| ())
            .map_err(db_err)
    }

    pub fn apply_lockout(
        conn: &mut PgConnection,
        id: Uuid,
        locked_until: DateTime<Utc>,
    ) -> Result<(), AppError> {
        diesel::update(users::table.filter(users::id.eq(id)))
            .set((
                users::locked_until.eq(Some(locked_until)),
                users::account_status.eq("locked"),
            ))
            .execute(conn)
            .map(|_| ())
            .map_err(db_err)
    }

    pub fn reset_failed_logins(conn: &mut PgConnection, id: Uuid) -> Result<(), AppError> {
        diesel::update(users::table.filter(users::id.eq(id)))
            .set((
                users::failed_login_count.eq(0),
                users::locked_until.eq(None::<DateTime<Utc>>),
                users::account_status.eq("active"),
            ))
            .execute(conn)
            .map(|_| ())
            .map_err(db_err)
    }

    pub fn update_last_login(conn: &mut PgConnection, id: Uuid) -> Result<(), AppError> {
        diesel::update(users::table.filter(users::id.eq(id)))
            .set(users::last_login_at.eq(Some(Utc::now())))
            .execute(conn)
            .map(|_| ())
            .map_err(db_err)
    }

    pub fn username_exists(conn: &mut PgConnection, username: &str) -> Result<bool, AppError> {
        use diesel::dsl::exists;
        diesel::select(exists(users::table.filter(users::username.eq(username))))
            .get_result(conn)
            .map_err(db_err)
    }

    pub fn email_exists(conn: &mut PgConnection, email: &str) -> Result<bool, AppError> {
        use diesel::dsl::exists;
        diesel::select(exists(users::table.filter(users::email.eq(email))))
            .get_result(conn)
            .map_err(db_err)
    }

    /// Paginated list of all users, ordered by created_at descending.
    pub fn list_users(
        conn: &mut PgConnection,
        page: i64,
        per_page: i64,
    ) -> Result<(Vec<DbUser>, i64), AppError> {
        let total: i64 = users::table.count().get_result(conn).map_err(db_err)?;

        let offset = (page.saturating_sub(1)) * per_page;
        let rows = users::table
            .select(DbUser::as_select())
            .order(users::created_at.desc())
            .offset(offset)
            .limit(per_page)
            .load(conn)
            .map_err(db_err)?;

        Ok((rows, total))
    }

    /// Update display_name and email for a user, returning the updated row.
    pub fn update_user(
        conn: &mut PgConnection,
        id: Uuid,
        display_name: &str,
        email: &str,
    ) -> Result<DbUser, AppError> {
        diesel::update(users::table.filter(users::id.eq(id)))
            .set((
                users::display_name.eq(display_name),
                users::email.eq(email),
                users::updated_at.eq(chrono::Utc::now()),
            ))
            .returning(DbUser::as_returning())
            .get_result(conn)
            .map_err(|e| match e {
                diesel::result::Error::DatabaseError(
                    diesel::result::DatabaseErrorKind::UniqueViolation,
                    info,
                ) => AppError::Conflict(info.message().to_string()),
                other => db_err(other),
            })
    }
}

fn db_err(e: diesel::result::Error) -> AppError {
    AppError::Internal(format!("database error: {e}"))
}
