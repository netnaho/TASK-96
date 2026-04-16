/// Candidate profile management use cases.
///
/// ## Authorization model
/// - `platform_admin`: full access, all candidates.
/// - `club_admin` (unscoped): full access, all candidates.
/// - `club_admin` (scoped to organization): only candidates where `organization_id` matches.
/// - `member`: read-only, only candidates they created (`created_by = user_id`).
///
/// ## PII handling
/// `phone` and `ssn_last4` are AES-256-GCM encrypted at rest.  The encrypted bytes are
/// returned as-is to callers that hold the raw `Candidate`.  The service decrypts them
/// only when `reveal_sensitive = true` is requested and the caller has `candidates:read`.
use diesel::PgConnection;
use tracing::info;
use uuid::Uuid;

use crate::{
    application::idempotency_op::IdempotencyOp,
    domain::{
        auth::models::AuthContext,
        candidates::models::{Candidate, CandidateDetail, CandidateSummary},
    },
    infrastructure::{
        crypto,
        db::{
            models::NewDbCandidate,
            repositories::{audit_repo::PgAuditRepository, candidate_repo::PgCandidateRepository},
        },
    },
    shared::errors::AppError,
};

// ============================================================
// Inputs
// ============================================================

pub struct CreateCandidateInput {
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    /// Plaintext phone — encrypted before storage.
    pub phone: Option<String>,
    /// Plaintext SSN last-4 — encrypted before storage.
    pub ssn_last4: Option<String>,
    pub resume_storage_key: Option<String>,
    pub source: Option<String>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
    pub organization_id: Option<Uuid>,
    pub idempotency_key: Option<String>,
    pub request_hash: Option<String>,
}

pub struct UpdateCandidateInput {
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub phone: Option<String>,
    pub ssn_last4: Option<String>,
    pub resume_storage_key: Option<String>,
    pub source: Option<String>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
    pub organization_id: Option<Uuid>,
    pub idempotency_key: Option<String>,
    pub request_hash: Option<String>,
}

pub struct ListCandidatesInput {
    pub page: i64,
    pub per_page: i64,
}

// ============================================================
// Service
// ============================================================

pub struct CandidateService;

impl CandidateService {
    /// Create a new candidate profile, encrypting PII before persistence.
    pub fn create(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        input: CreateCandidateInput,
        encryption_key: &str,
    ) -> Result<Candidate, AppError> {
        ctx.require_permission("candidates", "create")?;

        // Canonical idempotency check
        let idem = IdempotencyOp::new(
            input.idempotency_key.as_deref(),
            input.request_hash.as_deref(),
            ctx.user_id,
            "/api/v1/candidates",
        );
        if let Some(id) = idem.check(conn)? {
            let db = PgCandidateRepository::find_by_id(conn, id)?
                .ok_or_else(|| AppError::NotFound("candidate".into()))?;
            return Ok(db_to_domain(db));
        }

        // A scoped club_admin can only create candidates in their organization
        if let Some(org_id) = input.organization_id {
            ctx.require_scope_or_admin("organization", org_id)?;
        }

        let phone_encrypted = encrypt_opt(input.phone.as_deref(), encryption_key)?;
        let ssn_last4_encrypted = encrypt_opt(input.ssn_last4.as_deref(), encryption_key)?;

        let new = NewDbCandidate {
            id: Uuid::new_v4(),
            first_name: input.first_name,
            last_name: input.last_name,
            email: input.email,
            phone_encrypted,
            ssn_last4_encrypted,
            resume_storage_key: input.resume_storage_key,
            source: input.source,
            tags: input.tags,
            notes: input.notes,
            organization_id: input.organization_id,
            created_by: Some(ctx.user_id),
        };

        let db = PgCandidateRepository::create(conn, new)?;

        info!(
            actor = %ctx.user_id,
            candidate_id = %db.id,
            "candidate created"
        );

        emit_audit(
            conn,
            ctx,
            "candidate.created",
            "candidate",
            Some(db.id),
            None,
            Some(serde_json::json!({
                "email": db.email,
                "organization_id": db.organization_id,
            })),
        );

        idem.record(conn, 201, Some(db.id));
        Ok(db_to_domain(db))
    }

    /// Fetch a candidate by ID, enforcing object-level authorization.
    pub fn get(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        id: Uuid,
        reveal_sensitive: bool,
        encryption_key: &str,
    ) -> Result<CandidateDetail, AppError> {
        ctx.require_permission("candidates", "read")?;

        let db = PgCandidateRepository::find_by_id(conn, id)?
            .ok_or_else(|| AppError::NotFound("candidate".into()))?;

        enforce_read_access(ctx, &db_to_domain(db.clone()))?;

        let (phone, ssn_last4) = if reveal_sensitive {
            let phone = decrypt_opt(db.phone_encrypted.as_deref(), encryption_key)?;
            let ssn = decrypt_opt(db.ssn_last4_encrypted.as_deref(), encryption_key)?;
            (phone, ssn)
        } else {
            (None, None)
        };

        Ok(CandidateDetail {
            id: db.id,
            first_name: db.first_name,
            last_name: db.last_name,
            email: db.email,
            phone,
            ssn_last4,
            resume_storage_key: db.resume_storage_key,
            source: db.source,
            tags: db.tags,
            notes: db.notes,
            organization_id: db.organization_id,
            created_by: db.created_by,
            created_at: db.created_at,
            updated_at: db.updated_at,
        })
    }

    /// Update a candidate, re-encrypting PII if changed.
    pub fn update(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        id: Uuid,
        input: UpdateCandidateInput,
        encryption_key: &str,
    ) -> Result<Candidate, AppError> {
        ctx.require_permission("candidates", "update")?;

        let idem = IdempotencyOp::new(
            input.idempotency_key.as_deref(),
            input.request_hash.as_deref(),
            ctx.user_id,
            "/api/v1/candidates",
        );
        if let Some(replay_id) = idem.check(conn)? {
            let db = PgCandidateRepository::find_by_id(conn, replay_id)?
                .ok_or_else(|| AppError::NotFound("candidate".into()))?;
            return Ok(db_to_domain(db));
        }

        let existing_db = PgCandidateRepository::find_by_id(conn, id)?
            .ok_or_else(|| AppError::NotFound("candidate".into()))?;

        enforce_write_access(ctx, &db_to_domain(existing_db.clone()))?;

        // A scoped club_admin cannot move a candidate out of their organization
        if let Some(org_id) = input.organization_id {
            ctx.require_scope_or_admin("organization", org_id)?;
        }

        let phone_encrypted = encrypt_opt(input.phone.as_deref(), encryption_key)?;
        let ssn_last4_encrypted = encrypt_opt(input.ssn_last4.as_deref(), encryption_key)?;

        let updated = PgCandidateRepository::update(
            conn,
            id,
            &input.first_name,
            &input.last_name,
            &input.email,
            phone_encrypted.as_deref(),
            ssn_last4_encrypted.as_deref(),
            input.resume_storage_key.as_deref(),
            input.source.as_deref(),
            &input.tags,
            input.notes.as_deref(),
            input.organization_id,
        )?;

        info!(actor = %ctx.user_id, candidate_id = %id, "candidate updated");

        emit_audit(
            conn,
            ctx,
            "candidate.updated",
            "candidate",
            Some(id),
            Some(serde_json::json!({"email": existing_db.email})),
            Some(serde_json::json!({"email": updated.email})),
        );

        idem.record(conn, 200, Some(updated.id));
        Ok(db_to_domain(updated))
    }

    /// Paginated list, scoped by the caller's role.
    ///
    /// Both the result set and the total count are filtered at the DB level
    /// so that member users cannot infer the existence of other users' records
    /// via pagination metadata.
    pub fn list(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        input: ListCandidatesInput,
    ) -> Result<(Vec<CandidateSummary>, i64), AppError> {
        ctx.require_permission("candidates", "read")?;

        let offset = (input.page.saturating_sub(1)) * input.per_page;
        let org_filter = scoped_org_filter(ctx);
        let created_by_filter = ctx.ownership_filter();

        let (rows, total) =
            PgCandidateRepository::list(conn, org_filter, created_by_filter, offset, input.per_page)?;

        let summaries = rows
            .into_iter()
            .map(|r| CandidateSummary {
                id: r.id,
                first_name: r.first_name,
                last_name: r.last_name,
                email: r.email,
                source: r.source,
                tags: r.tags,
                organization_id: r.organization_id,
                created_at: r.created_at,
            })
            .collect();

        Ok((summaries, total))
    }
}

// ============================================================
// Authorization helpers
// ============================================================

/// Enforce read access at the object level.
fn enforce_read_access(ctx: &AuthContext, candidate: &Candidate) -> Result<(), AppError> {
    if ctx.has_role("platform_admin") {
        return Ok(());
    }
    if ctx.has_role("club_admin") {
        // Unscoped club_admin: access all
        if !ctx
            .roles
            .iter()
            .any(|r| r.role_name == "club_admin" && r.scope_type.is_some())
        {
            return Ok(());
        }
        // Scoped club_admin: candidate must be in their org
        if let Some(org_id) = candidate.organization_id {
            if ctx.has_scoped_role("club_admin", "organization", org_id) {
                return Ok(());
            }
        }
        return Err(AppError::Forbidden);
    }
    // member: only own candidates
    if candidate.created_by == Some(ctx.user_id) {
        return Ok(());
    }
    Err(AppError::Forbidden)
}

/// Enforce write access at the object level (same rules as read).
fn enforce_write_access(ctx: &AuthContext, candidate: &Candidate) -> Result<(), AppError> {
    enforce_read_access(ctx, candidate)
}

/// Return the organization_id filter for list queries based on the caller's scope.
fn scoped_org_filter(ctx: &AuthContext) -> Option<Uuid> {
    if ctx.has_role("platform_admin") {
        return None;
    }
    // Find a scoped club_admin role
    for role in &ctx.roles {
        if role.role_name == "club_admin" {
            if let (Some(scope_type), Some(scope_id)) = (&role.scope_type, role.scope_id) {
                if scope_type == "organization" {
                    return Some(scope_id);
                }
            }
        }
    }
    None
}

// ============================================================
// Conversion helpers
// ============================================================

fn db_to_domain(db: crate::infrastructure::db::models::DbCandidate) -> Candidate {
    Candidate {
        id: db.id,
        first_name: db.first_name,
        last_name: db.last_name,
        email: db.email,
        phone_encrypted: db.phone_encrypted,
        ssn_last4_encrypted: db.ssn_last4_encrypted,
        resume_storage_key: db.resume_storage_key,
        source: db.source,
        tags: db.tags,
        notes: db.notes,
        organization_id: db.organization_id,
        created_by: db.created_by,
        created_at: db.created_at,
        updated_at: db.updated_at,
    }
}

fn encrypt_opt(plaintext: Option<&str>, key: &str) -> Result<Option<Vec<u8>>, AppError> {
    match plaintext {
        Some(pt) if !pt.is_empty() => crypto::encrypt(pt.as_bytes(), key)
            .map(Some)
            .map_err(|e| AppError::Internal(format!("encryption failed: {e}"))),
        _ => Ok(None),
    }
}

fn decrypt_opt(ciphertext: Option<&[u8]>, key: &str) -> Result<Option<String>, AppError> {
    match ciphertext {
        Some(ct) => {
            let bytes = crypto::decrypt(ct, key)
                .map_err(|e| AppError::Internal(format!("decryption failed: {e}")))?;
            Ok(Some(String::from_utf8(bytes).map_err(|_| {
                AppError::Internal("decrypted value is not UTF-8".into())
            })?))
        }
        None => Ok(None),
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
    use crate::infrastructure::db::models::NewDbAuditEvent;
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
