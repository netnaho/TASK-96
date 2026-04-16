use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::shared::errors::AppError;

/// Represents an active, validated session record.
#[derive(Debug, Clone, Serialize)]
pub struct Session {
    pub id: Uuid,
    pub user_id: Uuid,
    pub device_fingerprint: Option<String>,
    pub ip_address: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub last_activity_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Scoped role assignment: a user may hold a role limited to a specific entity.
#[derive(Debug, Clone)]
pub struct ScopedRole {
    pub role_name: String,
    pub scope_type: Option<String>,
    pub scope_id: Option<Uuid>,
}

/// Authenticated identity, built from a valid session and loaded from the DB.
/// Stored in actix-web request extensions by `AuthMiddleware`.
#[derive(Debug, Clone)]
pub struct AuthContext {
    pub user_id: Uuid,
    pub username: String,
    pub session_id: Uuid,
    /// All scoped role assignments for this user.
    pub roles: Vec<ScopedRole>,
    /// Resolved (resource, action) permission pairs.
    pub permissions: Vec<(String, String)>,
}

impl AuthContext {
    /// Return `true` if the caller is a plain member with no elevated role.
    /// Used by list endpoints to scope queries to the caller's own records.
    pub fn is_member_only(&self) -> bool {
        self.has_role("member")
            && !self.has_role("club_admin")
            && !self.has_role("platform_admin")
    }

    /// If the caller is a plain member, returns `Some(user_id)` to filter
    /// list queries to records they own. Admins get `None` (no filter).
    pub fn ownership_filter(&self) -> Option<Uuid> {
        if self.is_member_only() {
            Some(self.user_id)
        } else {
            None
        }
    }

    /// Return `true` if the user holds the named role in any scope.
    pub fn has_role(&self, role_name: &str) -> bool {
        self.roles.iter().any(|r| r.role_name == role_name)
    }

    /// Return `true` if the user holds the named role scoped to the given entity.
    pub fn has_scoped_role(&self, role_name: &str, scope_type: &str, scope_id: Uuid) -> bool {
        self.roles.iter().any(|r| {
            r.role_name == role_name
                && r.scope_type.as_deref() == Some(scope_type)
                && r.scope_id == Some(scope_id)
        })
    }

    /// Check if the user has the given permission (platform_admin bypasses all checks).
    pub fn has_permission(&self, resource: &str, action: &str) -> bool {
        if self.has_role("platform_admin") {
            return true;
        }
        self.permissions
            .iter()
            .any(|(r, a)| r == resource && a == action)
    }

    /// Enforce a permission; returns `Err(Forbidden)` if the user lacks it.
    pub fn require_permission(&self, resource: &str, action: &str) -> Result<(), AppError> {
        if self.has_permission(resource, action) {
            Ok(())
        } else {
            tracing::warn!(
                user_id = %self.user_id,
                resource,
                action,
                "forbidden: permission check failed"
            );
            Err(AppError::Forbidden)
        }
    }

    /// For club_admin: enforce that the caller has an unscoped club_admin role
    /// OR a club_admin role scoped to the given entity.  platform_admin bypasses.
    pub fn require_scope_or_admin(&self, scope_type: &str, scope_id: Uuid) -> Result<(), AppError> {
        if self.has_role("platform_admin") {
            return Ok(());
        }
        // Unscoped club_admin can access all club data
        if self
            .roles
            .iter()
            .any(|r| r.role_name == "club_admin" && r.scope_type.is_none())
        {
            return Ok(());
        }
        if self.has_scoped_role("club_admin", scope_type, scope_id) {
            return Ok(());
        }
        tracing::warn!(
            user_id = %self.user_id,
            scope_type,
            %scope_id,
            "forbidden: scope check failed"
        );
        Err(AppError::Forbidden)
    }

    /// Ensure the caller is either accessing their own resource OR has elevated role.
    pub fn require_self_or_admin(&self, resource_owner_id: Uuid) -> Result<(), AppError> {
        if self.user_id == resource_owner_id
            || self.has_role("platform_admin")
            || self.has_role("club_admin")
        {
            Ok(())
        } else {
            tracing::warn!(
                user_id = %self.user_id,
                resource_owner_id = %resource_owner_id,
                "forbidden: object-level access denied"
            );
            Err(AppError::Forbidden)
        }
    }

    /// Strict self-service check: only the resource owner or a platform_admin may proceed.
    /// Unlike `require_self_or_admin`, this does NOT grant access to club_admin,
    /// preventing horizontal privilege escalation on user self-service endpoints.
    pub fn require_self_or_platform_admin(&self, resource_owner_id: Uuid) -> Result<(), AppError> {
        if self.user_id == resource_owner_id || self.has_role("platform_admin") {
            Ok(())
        } else {
            tracing::warn!(
                user_id = %self.user_id,
                resource_owner_id = %resource_owner_id,
                "forbidden: self-service access denied"
            );
            Err(AppError::Forbidden)
        }
    }
}
