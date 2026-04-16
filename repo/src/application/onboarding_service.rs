/// Onboarding checklist and item management use cases.
///
/// ## Readiness calculation
/// `readiness_pct = floor(required_completed / total_required * 100)`
/// When `total_required = 0`, readiness is 100 (nothing blocking).
///
/// ## Health attestation
/// The `health_attestation` text field is AES-256-GCM encrypted at rest.
/// It is only decrypted when `reveal_sensitive = true` is passed by an authorized caller.
///
/// ## Authorization
/// Follows the same pattern as offers: platform_admin and club_admin have full access;
/// members can only view checklists for candidates they are assigned to.
use chrono::Utc;
use diesel::PgConnection;
use tracing::info;
use uuid::Uuid;

use crate::{
    application::idempotency_op::IdempotencyOp,
    domain::{
        auth::models::AuthContext,
        onboarding::models::{
            OnboardingChecklist, OnboardingItem, OnboardingItemStatus, ReadinessReport,
        },
    },
    infrastructure::{
        crypto,
        db::{
            models::{NewDbOnboardingChecklist, NewDbOnboardingItem},
            repositories::{
                audit_repo::PgAuditRepository, onboarding_repo::PgOnboardingRepository,
            },
        },
    },
    shared::errors::AppError,
};

// ============================================================
// Inputs
// ============================================================

pub struct CreateChecklistInput {
    pub offer_id: Uuid,
    pub candidate_id: Uuid,
    pub assigned_to: Option<Uuid>,
    pub due_date: Option<chrono::NaiveDate>,
    pub idempotency_key: Option<String>,
    pub request_hash: Option<String>,
}

pub struct AddItemInput {
    pub title: String,
    pub description: Option<String>,
    pub item_order: i32,
    pub requires_upload: bool,
    pub required: bool,
    pub item_due_date: Option<chrono::NaiveDate>,
    pub health_attestation: Option<String>,
    pub idempotency_key: Option<String>,
    pub request_hash: Option<String>,
}

pub struct UpdateItemInput {
    pub status: OnboardingItemStatus,
    pub upload_storage_key: Option<String>,
    /// Plaintext health attestation text — encrypted before storage.
    pub health_attestation: Option<String>,
    pub idempotency_key: Option<String>,
    pub request_hash: Option<String>,
}

pub struct ListChecklistsInput {
    pub candidate_id: Option<Uuid>,
    pub page: i64,
    pub per_page: i64,
}

// ============================================================
// Service
// ============================================================

pub struct OnboardingService;

impl OnboardingService {
    pub fn create_checklist(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        input: CreateChecklistInput,
    ) -> Result<OnboardingChecklist, AppError> {
        ctx.require_permission("onboarding", "create")?;

        let idem = IdempotencyOp::new(
            input.idempotency_key.as_deref(),
            input.request_hash.as_deref(),
            ctx.user_id,
            "/api/v1/onboarding/checklists",
        );
        if let Some(replay_id) = idem.check(conn)? {
            let db = PgOnboardingRepository::find_checklist(conn, replay_id)?
                .ok_or_else(|| AppError::NotFound("onboarding_checklist".into()))?;
            return Ok(checklist_db_to_domain(db));
        }

        let new = NewDbOnboardingChecklist {
            id: Uuid::new_v4(),
            offer_id: input.offer_id,
            candidate_id: input.candidate_id,
            assigned_to: input.assigned_to,
            due_date: input.due_date,
        };

        let db = PgOnboardingRepository::create_checklist(conn, new)?;

        info!(
            actor = %ctx.user_id,
            checklist_id = %db.id,
            "onboarding checklist created"
        );

        emit_audit(
            conn,
            ctx,
            "onboarding.checklist_created",
            "onboarding_checklist",
            Some(db.id),
            None,
            Some(serde_json::json!({
                "offer_id": db.offer_id,
                "candidate_id": db.candidate_id,
            })),
        );

        idem.record(conn, 201, Some(db.id));
        Ok(checklist_db_to_domain(db))
    }

    pub fn get_checklist(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        checklist_id: Uuid,
    ) -> Result<OnboardingChecklist, AppError> {
        ctx.require_permission("onboarding", "read")?;

        let db = PgOnboardingRepository::find_checklist(conn, checklist_id)?
            .ok_or_else(|| AppError::NotFound("onboarding_checklist".into()))?;

        enforce_checklist_access(ctx, &db)?;

        Ok(checklist_db_to_domain(db))
    }

    pub fn list_checklists(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        input: ListChecklistsInput,
    ) -> Result<(Vec<OnboardingChecklist>, i64), AppError> {
        ctx.require_permission("onboarding", "read")?;

        // Non-admin users may only see checklists they are assigned to.
        let assigned_to_filter =
            if ctx.has_role("platform_admin") || ctx.has_role("club_admin") {
                None
            } else {
                Some(ctx.user_id)
            };

        let offset = (input.page.saturating_sub(1)) * input.per_page;
        let (rows, total) = PgOnboardingRepository::list_checklists(
            conn,
            input.candidate_id,
            assigned_to_filter,
            offset,
            input.per_page,
        )?;

        let checklists = rows.into_iter().map(checklist_db_to_domain).collect();
        Ok((checklists, total))
    }

    pub fn add_item(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        checklist_id: Uuid,
        input: AddItemInput,
        encryption_key: &str,
    ) -> Result<OnboardingItem, AppError> {
        ctx.require_permission("onboarding", "create")?;

        let idem = IdempotencyOp::new(
            input.idempotency_key.as_deref(),
            input.request_hash.as_deref(),
            ctx.user_id,
            "/api/v1/onboarding/items",
        );
        if let Some(replay_id) = idem.check(conn)? {
            let db = PgOnboardingRepository::find_item(conn, replay_id)?
                .ok_or_else(|| AppError::NotFound("onboarding_item".into()))?;
            return Ok(item_db_to_domain(db));
        }

        // Verify checklist exists and caller has access
        let checklist = PgOnboardingRepository::find_checklist(conn, checklist_id)?
            .ok_or_else(|| AppError::NotFound("onboarding_checklist".into()))?;
        enforce_checklist_access(ctx, &checklist)?;

        let health_attestation_encrypted =
            encrypt_opt(input.health_attestation.as_deref(), encryption_key)?;

        let new = NewDbOnboardingItem {
            id: Uuid::new_v4(),
            checklist_id,
            title: input.title,
            description: input.description,
            item_order: input.item_order,
            status: OnboardingItemStatus::NotStarted.as_str().to_string(),
            requires_upload: input.requires_upload,
            health_attestation_encrypted,
            required: input.required,
            item_due_date: input.item_due_date,
        };

        let db = PgOnboardingRepository::create_item(conn, new)?;

        info!(
            actor = %ctx.user_id,
            checklist_id = %checklist_id,
            item_id = %db.id,
            "onboarding item added"
        );

        emit_audit(
            conn,
            ctx,
            "onboarding.item_added",
            "onboarding_item",
            Some(db.id),
            None,
            Some(serde_json::json!({
                "checklist_id": checklist_id,
                "title": db.title,
                "required": db.required,
            })),
        );

        idem.record(conn, 201, Some(db.id));
        Ok(item_db_to_domain(db))
    }

    pub fn update_item(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        checklist_id: Uuid,
        item_id: Uuid,
        input: UpdateItemInput,
        encryption_key: &str,
    ) -> Result<OnboardingItem, AppError> {
        ctx.require_permission("onboarding", "update")?;

        let idem = IdempotencyOp::new(
            input.idempotency_key.as_deref(),
            input.request_hash.as_deref(),
            ctx.user_id,
            "/api/v1/onboarding/items",
        );
        if let Some(replay_id) = idem.check(conn)? {
            let db = PgOnboardingRepository::find_item(conn, replay_id)?
                .ok_or_else(|| AppError::NotFound("onboarding_item".into()))?;
            return Ok(item_db_to_domain(db));
        }

        // Verify checklist access
        let checklist = PgOnboardingRepository::find_checklist(conn, checklist_id)?
            .ok_or_else(|| AppError::NotFound("onboarding_checklist".into()))?;
        enforce_checklist_access(ctx, &checklist)?;

        let existing = PgOnboardingRepository::find_item(conn, item_id)?
            .ok_or_else(|| AppError::NotFound("onboarding_item".into()))?;

        if existing.checklist_id != checklist_id {
            return Err(AppError::NotFound("onboarding_item".into()));
        }

        let health_attestation_encrypted =
            encrypt_opt(input.health_attestation.as_deref(), encryption_key)?;

        let (completed_at, completed_by) = if input.status == OnboardingItemStatus::Completed {
            (Some(Utc::now()), Some(ctx.user_id))
        } else {
            (None, None)
        };

        let updated = PgOnboardingRepository::update_item_status(
            conn,
            item_id,
            input.status.as_str(),
            input.upload_storage_key.as_deref(),
            health_attestation_encrypted.as_deref(),
            completed_at,
            completed_by,
        )?;

        info!(
            actor = %ctx.user_id,
            item_id = %item_id,
            status = %input.status.as_str(),
            "onboarding item updated"
        );

        emit_audit(
            conn,
            ctx,
            "onboarding.item_updated",
            "onboarding_item",
            Some(item_id),
            Some(serde_json::json!({"status": existing.status})),
            Some(serde_json::json!({"status": input.status.as_str()})),
        );

        idem.record(conn, 200, Some(updated.id));
        Ok(item_db_to_domain(updated))
    }

    pub fn list_items(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        checklist_id: Uuid,
    ) -> Result<Vec<OnboardingItem>, AppError> {
        ctx.require_permission("onboarding", "read")?;

        let checklist = PgOnboardingRepository::find_checklist(conn, checklist_id)?
            .ok_or_else(|| AppError::NotFound("onboarding_checklist".into()))?;
        enforce_checklist_access(ctx, &checklist)?;

        let rows = PgOnboardingRepository::find_items_for_checklist(conn, checklist_id)?;
        Ok(rows.into_iter().map(item_db_to_domain).collect())
    }

    /// Compute and return the readiness report for a checklist.
    pub fn readiness(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        checklist_id: Uuid,
    ) -> Result<ReadinessReport, AppError> {
        ctx.require_permission("onboarding", "read")?;

        let _ = PgOnboardingRepository::find_checklist(conn, checklist_id)?
            .ok_or_else(|| AppError::NotFound("onboarding_checklist".into()))?;

        let db_items = PgOnboardingRepository::find_items_for_checklist(conn, checklist_id)?;
        let items: Vec<OnboardingItem> = db_items.into_iter().map(item_db_to_domain).collect();
        Ok(ReadinessReport::compute(checklist_id, &items))
    }
}

// ============================================================
// Conversion helpers
// ============================================================

fn checklist_db_to_domain(
    db: crate::infrastructure::db::models::DbOnboardingChecklist,
) -> OnboardingChecklist {
    OnboardingChecklist {
        id: db.id,
        offer_id: db.offer_id,
        candidate_id: db.candidate_id,
        assigned_to: db.assigned_to,
        due_date: db.due_date,
        created_at: db.created_at,
        updated_at: db.updated_at,
    }
}

fn item_db_to_domain(db: crate::infrastructure::db::models::DbOnboardingItem) -> OnboardingItem {
    let status = parse_item_status(&db.status);
    OnboardingItem {
        id: db.id,
        checklist_id: db.checklist_id,
        title: db.title,
        description: db.description,
        item_order: db.item_order,
        status,
        requires_upload: db.requires_upload,
        upload_storage_key: db.upload_storage_key,
        health_attestation_encrypted: db.health_attestation_encrypted,
        required: db.required,
        item_due_date: db.item_due_date,
        completed_at: db.completed_at,
        completed_by: db.completed_by,
        created_at: db.created_at,
        updated_at: db.updated_at,
    }
}

fn parse_item_status(s: &str) -> OnboardingItemStatus {
    match s {
        "in_progress" => OnboardingItemStatus::InProgress,
        "completed" => OnboardingItemStatus::Completed,
        "blocked" => OnboardingItemStatus::Blocked,
        "skipped" => OnboardingItemStatus::Skipped,
        _ => OnboardingItemStatus::NotStarted,
    }
}

/// Object-level authorization for checklist access.
/// Platform admins and club admins have full access. Members must be the
/// assigned_to user of the checklist.
fn enforce_checklist_access(
    ctx: &AuthContext,
    checklist: &crate::infrastructure::db::models::DbOnboardingChecklist,
) -> Result<(), AppError> {
    if ctx.has_role("platform_admin") || ctx.has_role("club_admin") {
        return Ok(());
    }
    if checklist.assigned_to == Some(ctx.user_id) {
        return Ok(());
    }
    Err(AppError::Forbidden)
}

fn encrypt_opt(plaintext: Option<&str>, key: &str) -> Result<Option<Vec<u8>>, AppError> {
    match plaintext {
        Some(pt) if !pt.is_empty() => crypto::encrypt(pt.as_bytes(), key)
            .map(Some)
            .map_err(|e| AppError::Internal(format!("encryption failed: {e}"))),
        _ => Ok(None),
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
