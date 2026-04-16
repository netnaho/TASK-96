/// Authentication and session management use cases.
///
/// ## Lockout policy
/// After `config.lockout.threshold` failed consecutive attempts the account is locked
/// for `config.lockout.duration_seconds` seconds.  The error returned to the caller
/// is always the same generic message regardless of whether the username exists,
/// preventing username enumeration.
///
/// ## CAPTCHA escalation
/// After `config.lockout.captcha_required_after_failures` consecutive failed
/// attempts (default: `threshold - 2`), CAPTCHA is mandatory.  Missing or invalid
/// CAPTCHA returns a 422 validation error before password verification.
/// When CAPTCHA is not required, it is still validated if voluntarily provided.
///
/// ## Session model
/// - Tokens: 32 random bytes, hex-encoded (64 chars) returned to client.
/// - Storage: SHA-256 of the token is stored; the plaintext never touches the DB.
/// - Hard expiry: `config.session.ttl_seconds` from creation (default 8 hours).
///   The session is unconditionally invalid after this time regardless of activity.
/// - Idle timeout: `SESSION_IDLE_TIMEOUT_SECS` (8 hours) from `last_activity_at`.
///   An active session that is continuously used will hit the hard expiry first;
///   an inactive session will be killed by the idle timeout.  Both policies use
///   the same 8-hour window by default, but operators can shorten hard TTL via
///   `SESSION_TTL_SECONDS` to enforce a stricter absolute cap.
/// - Per-user cap: `config.session.max_per_user`; oldest session evicted on overflow.
///
/// ## Secrets in logs
/// Passwords, tokens, and hashes are NEVER logged.  Only user IDs and session IDs appear.
use chrono::{Duration, Utc};
use diesel::PgConnection;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    domain::auth::models::{AuthContext, ScopedRole, Session},
    infrastructure::{
        captcha::{self, CaptchaError},
        config::AppConfig,
        crypto,
        db::{
            models::NewDbSession,
            repositories::{
                audit_repo::PgAuditRepository, role_repo::PgRoleRepository,
                session_repo::PgSessionRepository, user_repo::PgUserRepository,
            },
        },
    },
    shared::errors::{AppError, FieldError},
};

/// Idle timeout in seconds (8 hours).
pub const SESSION_IDLE_TIMEOUT_SECS: i64 = 8 * 3600;

// ============================================================
// Inputs
// ============================================================

pub struct LoginInput {
    pub username: String,
    /// Plaintext password from the client — validated and discarded immediately.
    pub password: String,
    pub device_fingerprint: Option<String>,
    pub ip_address: Option<String>,
    pub captcha_token: Option<String>,
    pub captcha_answer: Option<u32>,
}

pub struct LoginOutput {
    /// Plaintext session token to return to the client.
    pub token: String,
    pub session: Session,
    pub user_id: Uuid,
    pub username: String,
}

// ============================================================
// Service methods — all take a `&mut PgConnection` (caller wraps in web::block)
// ============================================================

pub struct AuthService;

impl AuthService {
    /// Authenticate a user, enforce lockout/CAPTCHA rules, create a session.
    ///
    /// ## CAPTCHA enforcement
    ///
    /// CAPTCHA is **mandatory** when the target account has accumulated
    /// `>= config.lockout.captcha_required_after_failures` consecutive failed
    /// login attempts (default: `lockout_threshold - 2`, i.e. 3 with threshold 5).
    /// When mandatory and missing/invalid, a 422 validation error is returned
    /// *before* the password is checked.
    ///
    /// When CAPTCHA is not required, it is still validated if provided (so
    /// clients that always send CAPTCHA get consistent behavior).
    pub fn login(
        conn: &mut PgConnection,
        input: LoginInput,
        config: &AppConfig,
        captcha_key: &[u8; 32],
    ) -> Result<LoginOutput, AppError> {
        // 1. Load user — use the same opaque error for not-found and bad password
        //    to prevent username enumeration.
        let user = PgUserRepository::find_by_username(conn, &input.username)
            .map_err(|_| generic_auth_error())?
            .ok_or_else(|| {
                warn!(username = %input.username, "login attempt for unknown username");
                generic_auth_error()
            })?;

        // 2. CAPTCHA enforcement: required when the account has accumulated
        //    enough consecutive failures to trigger the threshold.
        let captcha_required = user.failed_login_count
            >= config.lockout.captcha_required_after_failures as i32;

        if captcha_required {
            // CAPTCHA is mandatory — reject if missing
            match (&input.captcha_token, input.captcha_answer) {
                (Some(token), Some(answer)) => {
                    captcha::validate(token, answer, captcha_key).map_err(|e| match e {
                        CaptchaError::WrongAnswer => AppError::Validation(vec![FieldError {
                            field: "captcha_answer".into(),
                            message: e.to_string(),
                        }]),
                        _ => AppError::Validation(vec![FieldError {
                            field: "captcha_token".into(),
                            message: e.to_string(),
                        }]),
                    })?;
                }
                _ => {
                    return Err(AppError::Validation(vec![FieldError {
                        field: "captcha_token".into(),
                        message: "CAPTCHA is required after multiple failed login attempts".into(),
                    }]));
                }
            }
        } else if let (Some(token), Some(answer)) = (&input.captcha_token, input.captcha_answer) {
            // CAPTCHA not required but provided — still validate it
            captcha::validate(token, answer, captcha_key).map_err(|e| match e {
                CaptchaError::WrongAnswer => AppError::Validation(vec![FieldError {
                    field: "captcha_answer".into(),
                    message: e.to_string(),
                }]),
                _ => AppError::Validation(vec![FieldError {
                    field: "captcha_token".into(),
                    message: e.to_string(),
                }]),
            })?;
        }

        // 3. Lockout check (account_status or locked_until in the future)
        if user.account_status == "locked" || user.account_status == "suspended" {
            if let Some(until) = user.locked_until {
                if Utc::now() < until {
                    warn!(user_id = %user.id, "login rejected: account locked");
                    return Err(generic_auth_error());
                }
                // Lockout has expired — reset it before proceeding
                PgUserRepository::reset_failed_logins(conn, user.id)?;
            } else {
                warn!(user_id = %user.id, "login rejected: account suspended");
                return Err(generic_auth_error());
            }
        }

        // 4. Verify password (Argon2id)
        let password_ok = crypto::verify_password(&input.password, &user.password_hash)
            .map_err(|_| AppError::Internal("password verification failed".into()))?;

        if !password_ok {
            warn!(user_id = %user.id, "login attempt: wrong password");
            PgUserRepository::increment_failed_logins(conn, user.id)?;

            // Apply lockout if threshold reached
            let new_count = user.failed_login_count + 1;
            if new_count >= config.lockout.threshold as i32 {
                let locked_until =
                    Utc::now() + Duration::seconds(config.lockout.duration_seconds as i64);
                PgUserRepository::apply_lockout(conn, user.id, locked_until)?;
                warn!(user_id = %user.id, "account locked after repeated failures");

                emit_audit(
                    conn,
                    Some(user.id),
                    input.ip_address.as_deref(),
                    "auth.account_locked",
                    "user",
                    Some(user.id),
                    None,
                );
            }

            return Err(generic_auth_error());
        }

        // 5. Successful authentication — reset failure counter
        PgUserRepository::reset_failed_logins(conn, user.id)?;
        PgUserRepository::update_last_login(conn, user.id)?;

        // 6. Enforce per-user session cap (evict oldest if at limit)
        let active = PgSessionRepository::count_active_for_user(conn, user.id)?;
        if active >= config.session.max_per_user as i64 {
            PgSessionRepository::evict_oldest_for_user(conn, user.id)?;
        }

        // 7. Create session
        let token = crypto::generate_session_token();
        let token_hash = crypto::hash_token(&token);
        let expires_at = Utc::now() + Duration::seconds(config.session.ttl_seconds as i64);

        let new_session = NewDbSession {
            id: Uuid::new_v4(),
            user_id: user.id,
            token_hash,
            device_fingerprint: input.device_fingerprint.clone(),
            ip_address: input.ip_address.clone(),
            expires_at,
            last_activity_at: Some(Utc::now()),
        };
        let db_session = PgSessionRepository::create(conn, new_session)?;

        info!(user_id = %user.id, session_id = %db_session.id, "login successful");

        emit_audit(
            conn,
            Some(user.id),
            input.ip_address.as_deref(),
            "auth.login",
            "session",
            Some(db_session.id),
            Some(serde_json::json!({"device_fingerprint": input.device_fingerprint})),
        );

        Ok(LoginOutput {
            token,
            session: Session {
                id: db_session.id,
                user_id: user.id,
                device_fingerprint: db_session.device_fingerprint,
                ip_address: db_session.ip_address,
                expires_at: db_session.expires_at,
                last_activity_at: db_session.last_activity_at,
                created_at: db_session.created_at,
            },
            user_id: user.id,
            username: user.username,
        })
    }

    /// Revoke a session (logout). The session must belong to `user_id`.
    pub fn logout(
        conn: &mut PgConnection,
        session_id: Uuid,
        user_id: Uuid,
        ip_address: Option<&str>,
    ) -> Result<(), AppError> {
        PgSessionRepository::delete(conn, session_id)?;

        info!(user_id = %user_id, session_id = %session_id, "logout");

        emit_audit(
            conn,
            Some(user_id),
            ip_address,
            "auth.logout",
            "session",
            Some(session_id),
            None,
        );
        Ok(())
    }

    /// Validate a bearer token, enforce idle timeout, touch last_activity_at.
    /// Returns the `AuthContext` on success.
    pub fn validate_session(conn: &mut PgConnection, token: &str) -> Result<AuthContext, AppError> {
        let token_hash = crypto::hash_token(token);

        let session = PgSessionRepository::find_valid_by_token_hash(conn, &token_hash)?
            .ok_or(AppError::AuthenticationRequired)?;

        // Enforce idle timeout
        let last_activity = session.last_activity_at.unwrap_or(session.created_at);
        if Utc::now() - last_activity > Duration::seconds(SESSION_IDLE_TIMEOUT_SECS) {
            PgSessionRepository::delete(conn, session.id)?;
            warn!(session_id = %session.id, "session expired due to idle timeout");
            return Err(AppError::AuthenticationRequired);
        }

        // Touch last_activity_at (best-effort; don't fail the request on error)
        let _ = PgSessionRepository::touch(conn, session.id);

        // Load user
        let user = PgUserRepository::find_by_id(conn, session.user_id)?
            .ok_or_else(|| AppError::Internal("session references deleted user".into()))?;

        // Re-check account status — user may have been suspended after session was created
        if user.account_status == "suspended" || user.account_status == "deactivated" {
            warn!(user_id = %user.id, "request rejected: account inactive");
            return Err(AppError::AuthenticationRequired);
        }

        // Load roles and permissions
        let db_user_roles = PgRoleRepository::find_user_roles(conn, user.id)?;
        let role_ids: Vec<uuid::Uuid> = db_user_roles.iter().map(|r| r.role_id).collect();
        let db_permissions = PgRoleRepository::find_permissions_for_roles(conn, &role_ids)?;

        let roles: Vec<ScopedRole> = db_user_roles
            .iter()
            .map(|ur| {
                // We need the role name — join via an extra query or look it up separately.
                // For now, we embed role_id in ScopedRole and enrich it below.
                ScopedRole {
                    role_name: ur.role_id.to_string(), // temporary; enriched below
                    scope_type: ur.scope_type.clone(),
                    scope_id: ur.scope_id,
                }
            })
            .collect();

        // Enrich role names
        let all_roles = PgRoleRepository::list_roles(conn)?;
        let roles: Vec<ScopedRole> = db_user_roles
            .iter()
            .map(|ur| {
                let name = all_roles
                    .iter()
                    .find(|r| r.id == ur.role_id)
                    .map(|r| r.name.clone())
                    .unwrap_or_else(|| ur.role_id.to_string());
                ScopedRole {
                    role_name: name,
                    scope_type: ur.scope_type.clone(),
                    scope_id: ur.scope_id,
                }
            })
            .collect();

        let permissions: Vec<(String, String)> = db_permissions
            .iter()
            .map(|p| (p.resource.clone(), p.action.clone()))
            .collect();

        Ok(AuthContext {
            user_id: user.id,
            username: user.username,
            session_id: session.id,
            roles,
            permissions,
        })
    }
}

// ============================================================
// Helpers
// ============================================================

/// Return a generic 401 that does not reveal whether the username exists.
fn generic_auth_error() -> AppError {
    AppError::AuthenticationRequired
}

/// Fire-and-forget audit event insertion.  Failures are logged but do not
/// propagate — an audit write must never block a user action.
fn emit_audit(
    conn: &mut PgConnection,
    actor_id: Option<Uuid>,
    actor_ip: Option<&str>,
    action: &str,
    resource_type: &str,
    resource_id: Option<Uuid>,
    new_value: Option<serde_json::Value>,
) {
    use crate::infrastructure::db::models::NewDbAuditEvent;
    let event = NewDbAuditEvent {
        id: Uuid::new_v4(),
        actor_id,
        actor_ip: actor_ip.map(str::to_string),
        action: action.to_string(),
        resource_type: resource_type.to_string(),
        resource_id,
        old_value: None,
        new_value,
        metadata: serde_json::json!({}),
        correlation_id: None,
    };
    if let Err(e) = PgAuditRepository::insert(conn, event) {
        tracing::error!(error = %e, "failed to write audit event");
    }
}
