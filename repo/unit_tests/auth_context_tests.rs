/// Unit tests for AuthContext permission and scope logic.
/// No database required.

#[cfg(test)]
mod tests {
    use talentflow::domain::auth::models::{AuthContext, ScopedRole};
    use uuid::Uuid;

    fn ctx_with_roles(roles: Vec<(&str, Option<&str>, Option<Uuid>)>) -> AuthContext {
        AuthContext {
            user_id: Uuid::new_v4(),
            username: "testuser".into(),
            session_id: Uuid::new_v4(),
            roles: roles
                .into_iter()
                .map(|(name, st, sid)| ScopedRole {
                    role_name: name.to_string(),
                    scope_type: st.map(str::to_string),
                    scope_id: sid,
                })
                .collect(),
            permissions: vec![
                ("offers".into(), "read".into()),
                ("candidates".into(), "read".into()),
            ],
        }
    }

    #[test]
    fn platform_admin_bypasses_all_permission_checks() {
        let ctx = ctx_with_roles(vec![("platform_admin", None, None)]);
        assert!(ctx.require_permission("anything", "delete").is_ok());
    }

    #[test]
    fn member_with_permission_passes_check() {
        let ctx = ctx_with_roles(vec![("member", None, None)]);
        assert!(ctx.require_permission("offers", "read").is_ok());
    }

    #[test]
    fn member_without_permission_gets_forbidden() {
        let ctx = ctx_with_roles(vec![("member", None, None)]);
        let err = ctx.require_permission("users", "delete").unwrap_err();
        assert!(matches!(
            err,
            talentflow::shared::errors::AppError::Forbidden
        ));
    }

    #[test]
    fn require_self_or_admin_allows_own_resource() {
        let ctx = ctx_with_roles(vec![("member", None, None)]);
        assert!(ctx.require_self_or_admin(ctx.user_id).is_ok());
    }

    #[test]
    fn require_self_or_admin_denies_other_resource() {
        let ctx = ctx_with_roles(vec![("member", None, None)]);
        let other = Uuid::new_v4();
        assert!(ctx.require_self_or_admin(other).is_err());
    }

    #[test]
    fn club_admin_passes_self_or_admin_check() {
        let ctx = ctx_with_roles(vec![("club_admin", None, None)]);
        let other = Uuid::new_v4();
        assert!(ctx.require_self_or_admin(other).is_ok());
    }

    #[test]
    fn scoped_club_admin_matches_correct_scope() {
        let scope_id = Uuid::new_v4();
        let ctx = ctx_with_roles(vec![("club_admin", Some("organization"), Some(scope_id))]);
        assert!(ctx.require_scope_or_admin("organization", scope_id).is_ok());
    }

    #[test]
    fn scoped_club_admin_rejects_wrong_scope() {
        let scope_id = Uuid::new_v4();
        let other_scope = Uuid::new_v4();
        let ctx = ctx_with_roles(vec![("club_admin", Some("organization"), Some(scope_id))]);
        assert!(ctx
            .require_scope_or_admin("organization", other_scope)
            .is_err());
    }

    #[test]
    fn has_role_returns_false_for_missing_role() {
        let ctx = ctx_with_roles(vec![("member", None, None)]);
        assert!(!ctx.has_role("platform_admin"));
    }

    // ── require_self_or_platform_admin (strict self-service check) ──

    #[test]
    fn self_or_platform_admin_allows_own_resource() {
        let ctx = ctx_with_roles(vec![("member", None, None)]);
        assert!(ctx.require_self_or_platform_admin(ctx.user_id).is_ok());
    }

    #[test]
    fn self_or_platform_admin_allows_platform_admin() {
        let ctx = ctx_with_roles(vec![("platform_admin", None, None)]);
        let other = Uuid::new_v4();
        assert!(ctx.require_self_or_platform_admin(other).is_ok());
    }

    #[test]
    fn self_or_platform_admin_denies_club_admin_on_other_user() {
        let ctx = ctx_with_roles(vec![("club_admin", None, None)]);
        let other = Uuid::new_v4();
        assert!(ctx.require_self_or_platform_admin(other).is_err());
    }

    #[test]
    fn self_or_platform_admin_denies_member_on_other_user() {
        let ctx = ctx_with_roles(vec![("member", None, None)]);
        let other = Uuid::new_v4();
        assert!(ctx.require_self_or_platform_admin(other).is_err());
    }

    #[test]
    fn self_or_platform_admin_allows_club_admin_on_own_resource() {
        let ctx = ctx_with_roles(vec![("club_admin", None, None)]);
        assert!(ctx.require_self_or_platform_admin(ctx.user_id).is_ok());
    }

    #[test]
    fn self_or_platform_admin_denies_scoped_club_admin_on_other_user() {
        let scope_id = Uuid::new_v4();
        let ctx = ctx_with_roles(vec![("club_admin", Some("organization"), Some(scope_id))]);
        let other = Uuid::new_v4();
        assert!(ctx.require_self_or_platform_admin(other).is_err());
    }
}
