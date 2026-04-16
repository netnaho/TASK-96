/// User and role management use cases.
///
/// Responsibilities:
/// - User CRUD (create, read, update, deactivate)
/// - Role assignment and revocation (including scoped roles)
/// - Permission resolution for authorization checks
/// - Account status transitions
///
/// This service depends on:
/// - `UserRepository` for user persistence
/// - `crypto` module for password hashing on create/update
use diesel::PgConnection;
use tracing::info;
use uuid::Uuid;

use crate::{
    application::idempotency_op::IdempotencyOp,
    domain::auth::models::AuthContext,
    infrastructure::{
        crypto,
        db::{
            models::{
                DbPermission, DbRole, DbUser, DbUserRole, NewDbAuditEvent, NewDbUser, NewDbUserRole,
            },
            repositories::{
                audit_repo::PgAuditRepository, role_repo::PgRoleRepository,
                user_repo::PgUserRepository,
            },
        },
    },
    shared::errors::{AppError, FieldError},
};

// ============================================================
// Inputs
// ============================================================

pub struct CreateUserInput {
    pub username: String,
    pub email: String,
    pub password: String,
    pub display_name: String,
    pub idempotency_key: Option<String>,
    pub request_hash: Option<String>,
}

pub struct UpdateUserInput {
    pub display_name: String,
    pub email: String,
    pub idempotency_key: Option<String>,
    pub request_hash: Option<String>,
}

// ============================================================
// Service
// ============================================================

pub struct UserService;

impl UserService {
    /// Paginated list of all users. Platform admin only.
    pub fn list_users(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        page: i64,
        per_page: i64,
    ) -> Result<(Vec<DbUser>, i64), AppError> {
        require_platform_admin(ctx)?;
        PgUserRepository::list_users(conn, page, per_page)
    }

    /// Get a single user by ID. Platform admin or the user themselves.
    pub fn get_user(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        id: Uuid,
    ) -> Result<DbUser, AppError> {
        ctx.require_self_or_platform_admin(id)?;
        PgUserRepository::find_by_id(conn, id)?.ok_or_else(|| AppError::NotFound("user".into()))
    }

    /// Create a new user. Platform admin only.
    pub fn create_user(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        input: CreateUserInput,
    ) -> Result<DbUser, AppError> {
        require_platform_admin(ctx)?;

        let idem = IdempotencyOp::new(
            input.idempotency_key.as_deref(),
            input.request_hash.as_deref(),
            ctx.user_id,
            "/api/v1/users",
        );
        if let Some(replay_id) = idem.check(conn)? {
            let db = PgUserRepository::find_by_id(conn, replay_id)?
                .ok_or_else(|| AppError::NotFound("user".into()))?;
            return Ok(db);
        }

        // Validate password complexity
        crypto::validate_password_complexity(&input.password).map_err(|errs| {
            AppError::Validation(
                errs.into_iter()
                    .map(|msg| FieldError {
                        field: "password".into(),
                        message: msg,
                    })
                    .collect(),
            )
        })?;

        // Check uniqueness
        if PgUserRepository::username_exists(conn, &input.username)? {
            return Err(AppError::Conflict("username already exists".into()));
        }
        if PgUserRepository::email_exists(conn, &input.email)? {
            return Err(AppError::Conflict("email already exists".into()));
        }

        let password_hash = crypto::hash_password(&input.password)
            .map_err(|e| AppError::Internal(format!("password hashing failed: {e}")))?;

        let new_user = NewDbUser {
            id: Uuid::new_v4(),
            username: input.username,
            email: input.email,
            password_hash,
            display_name: input.display_name,
            account_status: "active".to_string(),
        };

        let user = PgUserRepository::create(conn, new_user)?;

        info!(actor = %ctx.user_id, user_id = %user.id, "user created");

        emit_audit(
            conn,
            ctx,
            "user.created",
            "user",
            Some(user.id),
            None,
            Some(serde_json::json!({
                "username": user.username,
                "email": user.email,
            })),
        );

        idem.record(conn, 201, Some(user.id));
        Ok(user)
    }

    /// Update display_name and email for a user. Platform admin or self.
    pub fn update_user(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        id: Uuid,
        input: UpdateUserInput,
    ) -> Result<DbUser, AppError> {
        ctx.require_self_or_platform_admin(id)?;

        let idem = IdempotencyOp::new(
            input.idempotency_key.as_deref(),
            input.request_hash.as_deref(),
            ctx.user_id,
            "/api/v1/users",
        );
        if let Some(replay_id) = idem.check(conn)? {
            let db = PgUserRepository::find_by_id(conn, replay_id)?
                .ok_or_else(|| AppError::NotFound("user".into()))?;
            return Ok(db);
        }

        let existing = PgUserRepository::find_by_id(conn, id)?
            .ok_or_else(|| AppError::NotFound("user".into()))?;

        let updated = PgUserRepository::update_user(conn, id, &input.display_name, &input.email)?;

        info!(actor = %ctx.user_id, user_id = %id, "user updated");

        emit_audit(
            conn,
            ctx,
            "user.updated",
            "user",
            Some(id),
            Some(serde_json::json!({
                "display_name": existing.display_name,
                "email": existing.email,
            })),
            Some(serde_json::json!({
                "display_name": updated.display_name,
                "email": updated.email,
            })),
        );

        idem.record(conn, 200, Some(updated.id));
        Ok(updated)
    }

    /// List roles assigned to a user. Platform admin or self.
    pub fn list_user_roles(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        user_id: Uuid,
    ) -> Result<Vec<DbUserRole>, AppError> {
        ctx.require_self_or_platform_admin(user_id)?;
        PgRoleRepository::find_user_roles(conn, user_id)
    }

    /// Assign a role to a user by role name. Platform admin only.
    pub fn assign_role(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        user_id: Uuid,
        role_name: &str,
        scope_type: Option<String>,
        scope_id: Option<Uuid>,
        idempotency_key: Option<String>,
        request_hash: Option<String>,
    ) -> Result<(), AppError> {
        require_platform_admin(ctx)?;

        let request_path = format!("/api/v1/users/{}/roles", user_id);
        let idem = IdempotencyOp::new(
            idempotency_key.as_deref(),
            request_hash.as_deref(),
            ctx.user_id,
            &request_path,
        );
        if idem.check(conn)?.is_some() {
            return Ok(());
        }

        // Verify user exists
        PgUserRepository::find_by_id(conn, user_id)?
            .ok_or_else(|| AppError::NotFound("user".into()))?;

        // Resolve role by name
        let role = PgRoleRepository::find_role_by_name(conn, role_name)?
            .ok_or_else(|| AppError::NotFound("role".into()))?;

        let new = NewDbUserRole {
            user_id,
            role_id: role.id,
            scope_type: scope_type.clone(),
            scope_id,
            granted_by: Some(ctx.user_id),
        };

        PgRoleRepository::assign_role(conn, new)?;

        info!(
            actor = %ctx.user_id,
            user_id = %user_id,
            role = role_name,
            "role assigned"
        );

        emit_audit(
            conn,
            ctx,
            "user.role_assigned",
            "user",
            Some(user_id),
            None,
            Some(serde_json::json!({
                "role_id": role.id,
                "role_name": role_name,
                "scope_type": scope_type,
                "scope_id": scope_id,
            })),
        );

        idem.record(conn, 204, None);
        Ok(())
    }

    /// Revoke a role from a user by role ID. Platform admin only.
    pub fn revoke_role(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        user_id: Uuid,
        role_id: Uuid,
        idempotency_key: Option<String>,
        request_hash: Option<String>,
    ) -> Result<(), AppError> {
        require_platform_admin(ctx)?;

        let request_path = format!("/api/v1/users/{}/roles/{}", user_id, role_id);
        let idem = IdempotencyOp::new(
            idempotency_key.as_deref(),
            request_hash.as_deref(),
            ctx.user_id,
            &request_path,
        );
        if idem.check(conn)?.is_some() {
            return Ok(());
        }

        PgRoleRepository::revoke_role(conn, user_id, role_id)?;

        info!(
            actor = %ctx.user_id,
            user_id = %user_id,
            role_id = %role_id,
            "role revoked"
        );

        emit_audit(
            conn,
            ctx,
            "user.role_revoked",
            "user",
            Some(user_id),
            Some(serde_json::json!({
                "role_id": role_id,
            })),
            None,
        );

        idem.record(conn, 204, None);
        Ok(())
    }

    /// List all available roles. Restricted to platform_admin.
    pub fn list_roles(conn: &mut PgConnection, ctx: &AuthContext) -> Result<Vec<DbRole>, AppError> {
        require_platform_admin(ctx)?;
        PgRoleRepository::list_roles(conn)
    }

    /// List all available permissions. Restricted to platform_admin.
    pub fn list_permissions(
        conn: &mut PgConnection,
        ctx: &AuthContext,
    ) -> Result<Vec<DbPermission>, AppError> {
        require_platform_admin(ctx)?;
        PgRoleRepository::list_permissions(conn)
    }
}

// ============================================================
// Helpers
// ============================================================

fn require_platform_admin(ctx: &AuthContext) -> Result<(), AppError> {
    if ctx.has_role("platform_admin") {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

fn emit_audit(
    conn: &mut PgConnection,
    ctx: &AuthContext,
    action: &str,
    resource_type: &str,
    resource_id: Option<Uuid>,
    old_value: Option<serde_json::Value>,
    new_value: Option<serde_json::Value>,
) {
    let event = NewDbAuditEvent {
        id: Uuid::new_v4(),
        actor_id: Some(ctx.user_id),
        actor_ip: None,
        action: action.to_string(),
        resource_type: resource_type.to_string(),
        resource_id,
        old_value,
        new_value,
        metadata: serde_json::json!({}),
        correlation_id: None,
    };
    if let Err(e) = PgAuditRepository::insert(conn, event) {
        tracing::error!(error = %e, "failed to write audit event");
    }
}
