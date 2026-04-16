/// Offer lifecycle and approval workflow use cases.
///
/// ## State machine (OfferStatus transitions)
/// ```
/// Draft → PendingApproval | Withdrawn
/// PendingApproval → Approved | Withdrawn
/// Approved → Sent | Withdrawn
/// Sent → Accepted | Declined | Withdrawn | Expired
/// Accepted / Declined / Withdrawn / Expired → (terminal)
/// ```
///
/// Transitions are validated by `OfferStatus::can_transition_to` in the domain model.
///
/// ## Compensation
/// Stored as AES-256-GCM encrypted JSON (`CompensationData` struct).
/// Validated against range constraints before encryption.
///
/// ## Approval chain
/// Steps are ordered by `step_order` (ascending).  The lowest-order pending step is the
/// "active" step.  When a step is approved, the next step becomes active.  When the last
/// step is approved the offer transitions to `Approved`.  Any rejection transitions to
/// `Draft` (offer author can revise and re-submit).
use chrono::Utc;
use diesel::PgConnection;
use tracing::info;
use uuid::Uuid;

use crate::{
    application::idempotency_op::IdempotencyOp,
    domain::{
        auth::models::AuthContext,
        offers::models::{ApprovalDecision, ApprovalStep, CompensationData, Offer, OfferStatus},
    },
    infrastructure::{
        crypto,
        db::{
            models::{NewDbApprovalStep, NewDbOffer},
            repositories::{
                audit_repo::PgAuditRepository,
                offer_repo::{PgApprovalRepository, PgOfferRepository},
            },
        },
    },
    shared::errors::AppError,
};

// ============================================================
// Inputs
// ============================================================

pub struct CreateOfferInput {
    pub candidate_id: Uuid,
    pub title: String,
    pub department: Option<String>,
    pub compensation: Option<CompensationData>,
    pub start_date: Option<chrono::NaiveDate>,
    pub expires_at: Option<chrono::DateTime<Utc>>,
    pub template_id: Option<Uuid>,
    pub clause_version: Option<String>,
    pub idempotency_key: Option<String>,
    pub request_hash: Option<String>,
}

pub struct UpdateOfferInput {
    pub title: String,
    pub department: Option<String>,
    pub compensation: Option<CompensationData>,
    pub start_date: Option<chrono::NaiveDate>,
    pub expires_at: Option<chrono::DateTime<Utc>>,
    pub template_id: Option<Uuid>,
    pub clause_version: Option<String>,
    pub idempotency_key: Option<String>,
    pub request_hash: Option<String>,
}

pub struct ListOffersInput {
    pub candidate_id: Option<Uuid>,
    pub page: i64,
    pub per_page: i64,
}

pub struct AddApprovalStepInput {
    pub approver_id: Uuid,
    pub step_order: i32,
    pub idempotency_key: Option<String>,
    pub request_hash: Option<String>,
}

pub struct RecordApprovalInput {
    pub step_id: Uuid,
    pub decision: ApprovalDecision,
    pub comments: Option<String>,
    pub idempotency_key: Option<String>,
    pub request_hash: Option<String>,
}

// ============================================================
// OfferService
// ============================================================

pub struct OfferService;

impl OfferService {
    pub fn create(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        input: CreateOfferInput,
        encryption_key: &str,
    ) -> Result<Offer, AppError> {
        ctx.require_permission("offers", "create")?;

        let idem = IdempotencyOp::new(
            input.idempotency_key.as_deref(),
            input.request_hash.as_deref(),
            ctx.user_id,
            "/api/v1/offers",
        );
        if let Some(replay_id) = idem.check(conn)? {
            let db = PgOfferRepository::find_by_id(conn, replay_id)?
                .ok_or_else(|| AppError::NotFound("offer".into()))?;
            return Ok(db_to_domain(db));
        }

        let compensation_encrypted =
            encrypt_compensation(input.compensation.as_ref(), encryption_key)?;
        let (salary_cents, bonus_target_pct, equity_units, pto_days, k401_match_pct) =
            compensation_to_columns(input.compensation.as_ref());

        let new = NewDbOffer {
            id: Uuid::new_v4(),
            candidate_id: input.candidate_id,
            title: input.title,
            department: input.department,
            compensation_encrypted,
            start_date: input.start_date,
            status: OfferStatus::Draft.as_str().to_string(),
            expires_at: input.expires_at,
            template_id: input.template_id,
            clause_version: input.clause_version,
            created_by: ctx.user_id,
            salary_cents,
            bonus_target_pct,
            equity_units,
            pto_days,
            k401_match_pct,
            currency: "USD".to_string(),
        };

        let db = PgOfferRepository::create(conn, new)?;

        info!(actor = %ctx.user_id, offer_id = %db.id, "offer created");

        emit_audit(
            conn,
            ctx,
            "offer.created",
            "offer",
            Some(db.id),
            None,
            Some(serde_json::json!({"status": db.status, "candidate_id": db.candidate_id})),
        );

        idem.record(conn, 201, Some(db.id));
        Ok(db_to_domain(db))
    }

    pub fn get(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        id: Uuid,
        reveal_compensation: bool,
        encryption_key: &str,
    ) -> Result<(Offer, Option<CompensationData>), AppError> {
        ctx.require_permission("offers", "read")?;

        let db = PgOfferRepository::find_by_id(conn, id)?
            .ok_or_else(|| AppError::NotFound("offer".into()))?;

        enforce_offer_access(ctx, &db)?;

        let compensation = if reveal_compensation {
            decrypt_compensation(db.compensation_encrypted.as_deref(), encryption_key)?
        } else {
            None
        };

        Ok((db_to_domain(db), compensation))
    }

    pub fn update(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        id: Uuid,
        input: UpdateOfferInput,
        encryption_key: &str,
    ) -> Result<Offer, AppError> {
        ctx.require_permission("offers", "update")?;

        let idem = IdempotencyOp::new(
            input.idempotency_key.as_deref(),
            input.request_hash.as_deref(),
            ctx.user_id,
            "/api/v1/offers",
        );
        if let Some(replay_id) = idem.check(conn)? {
            let db = PgOfferRepository::find_by_id(conn, replay_id)?
                .ok_or_else(|| AppError::NotFound("offer".into()))?;
            return Ok(db_to_domain(db));
        }

        let existing = PgOfferRepository::find_by_id(conn, id)?
            .ok_or_else(|| AppError::NotFound("offer".into()))?;

        enforce_offer_access(ctx, &existing)?;

        // Only draft offers may be updated
        let status = parse_status(&existing.status)?;
        if status != OfferStatus::Draft {
            return Err(AppError::InvalidStateTransition(
                "only draft offers can be updated".into(),
            ));
        }

        let compensation_encrypted =
            encrypt_compensation(input.compensation.as_ref(), encryption_key)?;
        let (salary_cents, bonus_target_pct, equity_units, pto_days, k401_match_pct) =
            compensation_to_columns(input.compensation.as_ref());

        let updated = PgOfferRepository::update(
            conn,
            id,
            &input.title,
            input.department.as_deref(),
            compensation_encrypted.as_deref(),
            input.start_date,
            existing.status.as_str(),
            input.expires_at,
            input.template_id,
            input.clause_version.as_deref(),
            salary_cents,
            bonus_target_pct,
            equity_units,
            pto_days,
            k401_match_pct,
            &existing.currency,
        )?;

        info!(actor = %ctx.user_id, offer_id = %id, "offer updated");

        emit_audit(
            conn,
            ctx,
            "offer.updated",
            "offer",
            Some(id),
            Some(serde_json::json!({"title": existing.title})),
            Some(serde_json::json!({"title": updated.title})),
        );

        idem.record(conn, 200, Some(updated.id));
        Ok(db_to_domain(updated))
    }

    /// Transition an offer to a new status, enforcing state machine rules.
    pub fn transition(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        id: Uuid,
        target: OfferStatus,
        idempotency_key: Option<String>,
        request_hash: Option<String>,
    ) -> Result<Offer, AppError> {
        ctx.require_permission("offers", "update")?;

        let idem = IdempotencyOp::new(
            idempotency_key.as_deref(),
            request_hash.as_deref(),
            ctx.user_id,
            "/api/v1/offers/transition",
        );
        if let Some(replay_id) = idem.check(conn)? {
            let db = PgOfferRepository::find_by_id(conn, replay_id)?
                .ok_or_else(|| AppError::NotFound("offer".into()))?;
            return Ok(db_to_domain(db));
        }

        let existing = PgOfferRepository::find_by_id(conn, id)?
            .ok_or_else(|| AppError::NotFound("offer".into()))?;

        enforce_offer_access(ctx, &existing)?;

        let current = parse_status(&existing.status)?;
        if !current.can_transition_to(target) {
            return Err(AppError::InvalidStateTransition(format!(
                "{} → {}",
                current.as_str(),
                target.as_str()
            )));
        }

        PgOfferRepository::set_status(conn, id, target.as_str())?;

        info!(
            actor = %ctx.user_id,
            offer_id = %id,
            from = %current.as_str(),
            to = %target.as_str(),
            "offer status transition"
        );

        emit_audit(
            conn,
            ctx,
            "offer.status_changed",
            "offer",
            Some(id),
            Some(serde_json::json!({"status": current.as_str()})),
            Some(serde_json::json!({"status": target.as_str()})),
        );

        let updated = PgOfferRepository::find_by_id(conn, id)?
            .ok_or_else(|| AppError::Internal("offer not found after transition".into()))?;
        idem.record(conn, 200, Some(updated.id));
        Ok(db_to_domain(updated))
    }

    pub fn list(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        input: ListOffersInput,
    ) -> Result<(Vec<Offer>, i64), AppError> {
        ctx.require_permission("offers", "read")?;

        let offset = (input.page.saturating_sub(1)) * input.per_page;
        let created_by_filter = ctx.ownership_filter();
        let (rows, total) =
            PgOfferRepository::list(conn, input.candidate_id, created_by_filter, offset, input.per_page)?;

        let offers = rows.into_iter().map(db_to_domain).collect();

        Ok((offers, total))
    }
}

// ============================================================
// ApprovalService
// ============================================================

pub struct ApprovalService;

impl ApprovalService {
    pub fn list_steps(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        offer_id: Uuid,
    ) -> Result<Vec<ApprovalStep>, AppError> {
        ctx.require_permission("approvals", "read")?;

        let _ = PgOfferRepository::find_by_id(conn, offer_id)?
            .ok_or_else(|| AppError::NotFound("offer".into()))?;

        let steps = PgApprovalRepository::find_steps_for_offer(conn, offer_id)?;
        Ok(steps.into_iter().map(step_db_to_domain).collect())
    }

    /// Add an approval step to an offer (only while status is PendingApproval or Draft).
    pub fn add_step(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        offer_id: Uuid,
        input: AddApprovalStepInput,
    ) -> Result<ApprovalStep, AppError> {
        ctx.require_permission("approvals", "create")?;

        let idem = IdempotencyOp::new(
            input.idempotency_key.as_deref(),
            input.request_hash.as_deref(),
            ctx.user_id,
            "/api/v1/offers/approvals",
        );
        if let Some(replay_id) = idem.check(conn)? {
            let db = PgApprovalRepository::find_step_by_id(conn, replay_id)?
                .ok_or_else(|| AppError::NotFound("approval_step".into()))?;
            return Ok(step_db_to_domain(db));
        }

        let offer_db = PgOfferRepository::find_by_id(conn, offer_id)?
            .ok_or_else(|| AppError::NotFound("offer".into()))?;

        enforce_offer_access(ctx, &offer_db)?;

        let status = parse_status(&offer_db.status)?;
        if status != OfferStatus::Draft && status != OfferStatus::PendingApproval {
            return Err(AppError::InvalidStateTransition(
                "approval steps can only be added to draft or pending_approval offers".into(),
            ));
        }

        let new = NewDbApprovalStep {
            id: Uuid::new_v4(),
            offer_id,
            step_order: input.step_order,
            approver_id: input.approver_id,
            decision: ApprovalDecision::Pending.as_str().to_string(),
        };

        let db = PgApprovalRepository::create_step(conn, new)?;

        info!(
            actor = %ctx.user_id,
            offer_id = %offer_id,
            step_id = %db.id,
            approver_id = %input.approver_id,
            "approval step added"
        );

        emit_audit(
            conn,
            ctx,
            "approval.step_added",
            "approval_step",
            Some(db.id),
            None,
            Some(serde_json::json!({
                "offer_id": offer_id,
                "step_order": input.step_order,
                "approver_id": input.approver_id,
            })),
        );

        idem.record(conn, 201, Some(db.id));
        Ok(step_db_to_domain(db))
    }

    /// Record an approval decision on a step.
    ///
    /// - The caller must be the step's assigned approver (or platform_admin).
    /// - Only `pending` steps may receive a decision.
    /// - On `approved`: if no more pending steps exist the offer transitions to `Approved`.
    /// - On `rejected`: offer transitions back to `Draft` so the author can revise.
    pub fn record_decision(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        offer_id: Uuid,
        input: RecordApprovalInput,
    ) -> Result<ApprovalStep, AppError> {
        ctx.require_permission("approvals", "update")?;

        let idem = IdempotencyOp::new(
            input.idempotency_key.as_deref(),
            input.request_hash.as_deref(),
            ctx.user_id,
            "/api/v1/offers/approvals/decision",
        );
        if let Some(replay_id) = idem.check(conn)? {
            let db = PgApprovalRepository::find_step_by_id(conn, replay_id)?
                .ok_or_else(|| AppError::NotFound("approval_step".into()))?;
            return Ok(step_db_to_domain(db));
        }

        let step_db = PgApprovalRepository::find_step_by_id(conn, input.step_id)?
            .ok_or_else(|| AppError::NotFound("approval_step".into()))?;

        if step_db.offer_id != offer_id {
            return Err(AppError::NotFound("approval_step".into()));
        }

        // Only the assigned approver (or platform_admin) may decide
        if step_db.approver_id != ctx.user_id && !ctx.has_role("platform_admin") {
            return Err(AppError::Forbidden);
        }

        // Step must be pending
        let current_decision = ApprovalDecision::from_str(&step_db.decision)
            .ok_or_else(|| AppError::Internal("unknown decision value".into()))?;
        if current_decision != ApprovalDecision::Pending {
            return Err(AppError::InvalidStateTransition(
                "step already decided".into(),
            ));
        }

        let decided_at = Utc::now();
        let updated_step = PgApprovalRepository::record_decision(
            conn,
            input.step_id,
            input.decision.as_str(),
            decided_at,
            input.comments.as_deref(),
        )?;

        info!(
            actor = %ctx.user_id,
            offer_id = %offer_id,
            step_id = %input.step_id,
            decision = %input.decision.as_str(),
            "approval decision recorded"
        );

        emit_audit(
            conn,
            ctx,
            "approval.decision_recorded",
            "approval_step",
            Some(input.step_id),
            Some(serde_json::json!({"decision": "pending"})),
            Some(serde_json::json!({"decision": input.decision.as_str()})),
        );

        // Auto-advance offer status
        match input.decision {
            ApprovalDecision::Approved => {
                let next_pending = PgApprovalRepository::find_next_pending(conn, offer_id)?;
                if next_pending.is_none() {
                    // All steps approved — move offer to Approved
                    PgOfferRepository::set_status(conn, offer_id, OfferStatus::Approved.as_str())?;
                    info!(offer_id = %offer_id, "offer fully approved");
                    emit_audit(
                        conn,
                        ctx,
                        "offer.status_changed",
                        "offer",
                        Some(offer_id),
                        Some(serde_json::json!({"status": "pending_approval"})),
                        Some(serde_json::json!({"status": "approved"})),
                    );
                }
            }
            ApprovalDecision::Rejected => {
                // Roll back to Draft for revision
                PgOfferRepository::set_status(conn, offer_id, OfferStatus::Draft.as_str())?;
                info!(offer_id = %offer_id, "offer rolled back to draft after rejection");
                emit_audit(
                    conn,
                    ctx,
                    "offer.status_changed",
                    "offer",
                    Some(offer_id),
                    Some(serde_json::json!({"status": "pending_approval"})),
                    Some(serde_json::json!({"status": "draft"})),
                );
            }
            _ => {}
        }

        idem.record(conn, 200, Some(updated_step.id));
        Ok(step_db_to_domain(updated_step))
    }
}

// ============================================================
// Authorization helpers
// ============================================================

fn enforce_offer_access(
    ctx: &AuthContext,
    db: &crate::infrastructure::db::models::DbOffer,
) -> Result<(), AppError> {
    if ctx.has_role("platform_admin") || ctx.has_role("club_admin") {
        return Ok(());
    }
    if db.created_by == ctx.user_id {
        return Ok(());
    }
    Err(AppError::Forbidden)
}

// ============================================================
// Conversion and encryption helpers
// ============================================================

fn db_to_domain(db: crate::infrastructure::db::models::DbOffer) -> Offer {
    let status = parse_status(&db.status).unwrap_or(OfferStatus::Draft);
    Offer {
        id: db.id,
        candidate_id: db.candidate_id,
        title: db.title,
        department: db.department,
        compensation_encrypted: db.compensation_encrypted,
        start_date: db.start_date,
        status,
        expires_at: db.expires_at,
        template_id: db.template_id,
        clause_version: db.clause_version,
        created_by: db.created_by,
        created_at: db.created_at,
        updated_at: db.updated_at,
        salary_cents: db.salary_cents,
        bonus_target_pct: db.bonus_target_pct,
        equity_units: db.equity_units,
        pto_days: db.pto_days,
        k401_match_pct: db.k401_match_pct,
        currency: db.currency,
    }
}

/// Extract structured column values from a `CompensationData`.
/// Converts base_salary_usd (whole dollars) to salary_cents for storage.
fn compensation_to_columns(comp: Option<&CompensationData>) -> (Option<i64>, Option<f64>, Option<i32>, Option<i16>, Option<f64>) {
    match comp {
        None => (None, None, None, None, None),
        Some(c) => (
            Some(c.base_salary_usd as i64 * 100),
            Some(c.bonus_target_pct),
            Some(c.equity_units as i32),
            Some(c.pto_days as i16),
            Some(c.k401_match_pct),
        ),
    }
}

fn step_db_to_domain(db: crate::infrastructure::db::models::DbApprovalStep) -> ApprovalStep {
    let decision = ApprovalDecision::from_str(&db.decision).unwrap_or(ApprovalDecision::Pending);
    ApprovalStep {
        id: db.id,
        offer_id: db.offer_id,
        step_order: db.step_order,
        approver_id: db.approver_id,
        decision,
        decided_at: db.decided_at,
        comments: db.comments,
        created_at: db.created_at,
    }
}

fn parse_status(s: &str) -> Result<OfferStatus, AppError> {
    match s {
        "draft" => Ok(OfferStatus::Draft),
        "pending_approval" => Ok(OfferStatus::PendingApproval),
        "approved" => Ok(OfferStatus::Approved),
        "sent" => Ok(OfferStatus::Sent),
        "accepted" => Ok(OfferStatus::Accepted),
        "declined" => Ok(OfferStatus::Declined),
        "withdrawn" => Ok(OfferStatus::Withdrawn),
        "expired" => Ok(OfferStatus::Expired),
        other => Err(AppError::Internal(format!("unknown offer status: {other}"))),
    }
}

fn encrypt_compensation(
    compensation: Option<&CompensationData>,
    key: &str,
) -> Result<Option<Vec<u8>>, AppError> {
    match compensation {
        None => Ok(None),
        Some(c) => {
            let errors = c.validate();
            if !errors.is_empty() {
                let fields: Vec<crate::shared::errors::FieldError> = errors
                    .into_iter()
                    .map(|msg| crate::shared::errors::FieldError {
                        field: "compensation".into(),
                        message: msg,
                    })
                    .collect();
                return Err(AppError::Validation(fields));
            }
            let json = serde_json::to_vec(c)
                .map_err(|e| AppError::Internal(format!("compensation serialize: {e}")))?;
            crypto::encrypt(&json, key)
                .map(Some)
                .map_err(|e| AppError::Internal(format!("compensation encrypt: {e}")))
        }
    }
}

fn decrypt_compensation(
    ciphertext: Option<&[u8]>,
    key: &str,
) -> Result<Option<CompensationData>, AppError> {
    match ciphertext {
        None => Ok(None),
        Some(ct) => {
            let bytes = crypto::decrypt(ct, key)
                .map_err(|e| AppError::Internal(format!("compensation decrypt: {e}")))?;
            serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|e| AppError::Internal(format!("compensation deserialize: {e}")))
        }
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
