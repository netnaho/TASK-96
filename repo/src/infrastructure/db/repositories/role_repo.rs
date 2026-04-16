use diesel::prelude::*;
use uuid::Uuid;

use crate::infrastructure::db::models::{DbPermission, DbRole, DbUserRole, NewDbUserRole};
use crate::infrastructure::db::schema::{permissions, role_permissions, roles, user_roles};
use crate::shared::errors::AppError;

pub struct PgRoleRepository;

impl PgRoleRepository {
    /// Return all roles assigned to a user along with their scope.
    pub fn find_user_roles(
        conn: &mut PgConnection,
        user_id: Uuid,
    ) -> Result<Vec<DbUserRole>, AppError> {
        user_roles::table
            .filter(user_roles::user_id.eq(user_id))
            .select(DbUserRole::as_select())
            .load(conn)
            .map_err(db_err)
    }

    /// Return the (resource, action) permission pairs for the given role IDs.
    pub fn find_permissions_for_roles(
        conn: &mut PgConnection,
        role_ids: &[Uuid],
    ) -> Result<Vec<DbPermission>, AppError> {
        role_permissions::table
            .inner_join(permissions::table.on(permissions::id.eq(role_permissions::permission_id)))
            .filter(role_permissions::role_id.eq_any(role_ids))
            .select(DbPermission::as_select())
            .load(conn)
            .map_err(db_err)
    }

    pub fn find_role_by_name(
        conn: &mut PgConnection,
        name: &str,
    ) -> Result<Option<DbRole>, AppError> {
        roles::table
            .filter(roles::name.eq(name))
            .select(DbRole::as_select())
            .first(conn)
            .optional()
            .map_err(db_err)
    }

    pub fn list_roles(conn: &mut PgConnection) -> Result<Vec<DbRole>, AppError> {
        roles::table
            .select(DbRole::as_select())
            .order(roles::name.asc())
            .load(conn)
            .map_err(db_err)
    }

    pub fn list_permissions(conn: &mut PgConnection) -> Result<Vec<DbPermission>, AppError> {
        permissions::table
            .select(DbPermission::as_select())
            .order((permissions::resource.asc(), permissions::action.asc()))
            .load(conn)
            .map_err(db_err)
    }

    /// Assign a role to a user. Returns Conflict if the assignment already exists.
    pub fn assign_role(conn: &mut PgConnection, new: NewDbUserRole) -> Result<(), AppError> {
        diesel::insert_into(user_roles::table)
            .values(&new)
            .execute(conn)
            .map(|_| ())
            .map_err(|e| match e {
                diesel::result::Error::DatabaseError(
                    diesel::result::DatabaseErrorKind::UniqueViolation,
                    info,
                ) => AppError::Conflict(info.message().to_string()),
                other => db_err(other),
            })
    }

    /// Revoke a role from a user. Returns NotFound if the assignment does not exist.
    pub fn revoke_role(
        conn: &mut PgConnection,
        user_id: Uuid,
        role_id: Uuid,
    ) -> Result<(), AppError> {
        let deleted = diesel::delete(
            user_roles::table
                .filter(user_roles::user_id.eq(user_id))
                .filter(user_roles::role_id.eq(role_id)),
        )
        .execute(conn)
        .map_err(db_err)?;

        if deleted == 0 {
            return Err(AppError::NotFound("user_role".into()));
        }
        Ok(())
    }
}

fn db_err(e: diesel::result::Error) -> AppError {
    AppError::Internal(format!("database error: {e}"))
}
