/// Booking order lifecycle management.
///
/// ## Hold flow
/// 1. `create_hold` → reserves inventory slot (SELECT FOR UPDATE), creates order
///    in PendingConfirmation with hold_expires_at = now + 15 min.
/// 2. Client submits agreement evidence via `submit_agreement`.
/// 3. `confirm` → runs eligibility gate (all 5 checks). If all pass,
///    transitions to Confirmed and clears hold_expires_at. If any fail,
///    returns the structured EligibilityResult (no transition).
///
/// ## State transitions
/// - start: Confirmed → InProgress
/// - complete: InProgress → Completed
/// - cancel: with 24h breach rules
/// - reschedule: releases old slot, creates hold on new slot
/// - exception: moves to Exception with detail
///
/// ## Idempotency
/// All mutating operations (`create_hold`, `submit_agreement`, `confirm`, `start`,
/// `complete`, `cancel`, `reschedule`, `mark_exception`) accept an optional
/// idempotency key. If an operation already exists with the same key, the stored
/// result is replayed instead of re-executing.
///
/// ## Module layout
/// - **Inputs** — DTOs for hold creation, cancel, reschedule, exception
/// - **BookingService** — public API (create_hold, submit_agreement, confirm,
///   start, complete, cancel, reschedule, mark_exception, get, list, release_expired_holds)
/// - **transition()** — generic FSM transition helper with audit + logging
/// - **enforce_booking_access()** — object-level authorization (owner or admin)
/// - **db_to_domain() / parse_status()** — conversion helpers
/// - **emit_audit()** — audit event writer
use chrono::{Duration, Utc};
use diesel::Connection;
use diesel::PgConnection;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    application::{eligibility_service::EligibilityService, idempotency_op::IdempotencyOp},
    domain::{
        auth::models::AuthContext,
        bookings::models::{
            AgreementEvidence, BookingOrder, BookingStatus, BreachReasonCode, EligibilityResult,
            CANCEL_CUTOFF_HOURS, HOLD_DURATION_MINUTES,
        },
    },
    infrastructure::db::{
        models::{DbBookingOrder, NewDbBookingOrder, NewDbIdempotencyKey},
        repositories::{
            audit_repo::PgAuditRepository, booking_repo::PgBookingRepository,
            idempotency_repo::PgIdempotencyRepository, inventory_repo::PgInventoryRepository,
        },
    },
    shared::errors::{AppError, FieldError},
};

// ============================================================
// Inputs
// ============================================================

pub struct CreateHoldInput {
    pub candidate_id: Uuid,
    pub site_id: Uuid,
    pub slot_id: Uuid,
    pub notes: Option<String>,
    pub idempotency_key: Option<String>,
    /// SHA-256 hex hash of the serialised request body — used for conflict
    /// detection when the same idempotency key is reused with different payload.
    pub request_hash: Option<String>,
}

pub struct SubmitAgreementInput {
    pub typed_name: String,
    pub idempotency_key: Option<String>,
    pub request_hash: Option<String>,
}

pub struct CancelInput {
    /// Required when cancelling within 24 hours (breach).
    pub reason: Option<String>,
    pub reason_code: Option<String>,
}

pub struct RescheduleInput {
    pub new_slot_id: Uuid,
}

pub struct ExceptionInput {
    pub detail: String,
}

pub struct ListBookingsInput {
    pub candidate_id: Option<Uuid>,
    pub page: i64,
    pub per_page: i64,
}

// ============================================================
// Service
// ============================================================

pub struct BookingService;

impl BookingService {
    /// Create a hold on an inventory slot.
    ///
    /// - Checks canonical idempotency store (24h window) for duplicate requests.
    ///   Same key + same hash → replay. Same key + different hash → 409 IdempotencyConflict.
    /// - Falls back to legacy `booking_orders.idempotency_key` lookup for keys
    ///   created before the canonical store was introduced.
    /// - Reserves slot capacity inside a transaction (SELECT FOR UPDATE).
    /// - Creates booking in PendingConfirmation with 15-minute hold.
    /// - Records the new key in the canonical store after successful creation.
    pub fn create_hold(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        input: CreateHoldInput,
    ) -> Result<BookingOrder, AppError> {
        ctx.require_permission("bookings", "create")?;

        const REQUEST_PATH: &str = "/api/v1/bookings";

        if let Some(ref key) = input.idempotency_key {
            // ── Canonical store check ─────────────────────────────────────────
            if let Some(record) = PgIdempotencyRepository::find_active(conn, key)? {
                let incoming_hash = input.request_hash.as_deref().unwrap_or("");
                if record.request_hash != incoming_hash {
                    warn!(
                        idempotency_key = %key,
                        "idempotency conflict: same key, different request body"
                    );
                    return Err(AppError::IdempotencyConflict);
                }
                // Same hash — replay the stored booking
                info!(
                    idempotency_key = %key,
                    "idempotent replay via canonical store"
                );
                // The stored response_body contains the booking JSON; fetch the
                // live booking to return a strongly-typed value.
                if let Some(id_val) = record
                    .response_body
                    .as_ref()
                    .and_then(|b| b.get("id"))
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<Uuid>().ok())
                {
                    if let Some(existing) = PgBookingRepository::find_by_id(conn, id_val)? {
                        return Ok(db_to_domain(existing));
                    }
                }
            }

            // ── Legacy fallback: booking_orders.idempotency_key ──────────────
            if let Some(existing) = PgBookingRepository::find_by_idempotency_key(conn, key)? {
                info!(
                    idempotency_key = %key,
                    booking_id = %existing.id,
                    "idempotent replay via legacy key"
                );
                return Ok(db_to_domain(existing));
            }
        }

        let now = Utc::now();
        let hold_expires_at = now + Duration::minutes(HOLD_DURATION_MINUTES);
        let booking_id = Uuid::new_v4();

        // Transaction: reserve slot + insert booking atomically
        let db_order = conn.transaction(|conn| {
            let reserved = PgInventoryRepository::reserve_slot(conn, input.slot_id)?;
            if !reserved {
                return Err(AppError::Conflict(
                    "slot is fully booked — no capacity available".into(),
                ));
            }

            // Look up slot details for scheduled_date / times
            let slot = PgInventoryRepository::find_by_id(conn, input.slot_id)?
                .ok_or_else(|| AppError::NotFound("booking_slot".into()))?;

            let new = NewDbBookingOrder {
                id: booking_id,
                candidate_id: input.candidate_id,
                site_id: input.site_id,
                status: BookingStatus::PendingConfirmation.as_str().to_string(),
                scheduled_date: slot.slot_date,
                scheduled_time_start: Some(slot.start_time),
                scheduled_time_end: Some(slot.end_time),
                notes: input.notes,
                slot_id: Some(input.slot_id),
                hold_expires_at: Some(hold_expires_at),
                idempotency_key: input.idempotency_key.clone(),
                created_by: ctx.user_id,
            };

            PgBookingRepository::create(conn, new)
        })?;

        info!(
            actor = %ctx.user_id,
            booking_id = %db_order.id,
            slot_id = %input.slot_id,
            hold_expires_at = %hold_expires_at,
            "hold created"
        );

        emit_audit(
            conn,
            ctx,
            "booking.hold_created",
            "booking_order",
            Some(db_order.id),
            None,
            Some(serde_json::json!({
                "slot_id": input.slot_id,
                "hold_expires_at": hold_expires_at.to_rfc3339(),
                "candidate_id": input.candidate_id,
            })),
        );

        // ── Record in canonical idempotency store ─────────────────────────────
        if let Some(ref key) = input.idempotency_key {
            let expires_at = now + Duration::hours(24);
            let response_body = serde_json::json!({ "id": db_order.id });
            let record = NewDbIdempotencyKey {
                key: key.clone(),
                user_id: ctx.user_id,
                request_path: REQUEST_PATH.to_owned(),
                request_hash: input.request_hash.unwrap_or_default(),
                response_status: 201,
                response_body: Some(response_body),
                expires_at,
            };
            if let Err(e) = PgIdempotencyRepository::insert(conn, record) {
                warn!(error = %e, "failed to record idempotency key — non-fatal");
            }
        }

        Ok(db_to_domain(db_order))
    }

    /// Submit agreement evidence for a booking (must be in PendingConfirmation).
    pub fn submit_agreement(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        booking_id: Uuid,
        input: SubmitAgreementInput,
    ) -> Result<AgreementEvidence, AppError> {
        ctx.require_permission("bookings", "update")?;

        let request_path = format!("/api/v1/bookings/{}/agreement", booking_id);
        let idem = IdempotencyOp::new(
            input.idempotency_key.as_deref(),
            input.request_hash.as_deref(),
            ctx.user_id,
            &request_path,
        );
        if let Some(_replay_id) = idem.check(conn)? {
            // Replay: return the existing agreement evidence from the booking
            let booking = PgBookingRepository::find_by_id(conn, booking_id)?
                .ok_or_else(|| AppError::NotFound("booking_order".into()))?;
            return Ok(AgreementEvidence {
                typed_name: booking.agreement_signed_by.unwrap_or_default(),
                signed_at: booking.agreement_signed_at.unwrap_or_else(Utc::now),
                hash: booking.agreement_hash.unwrap_or_default(),
            });
        }

        let booking = PgBookingRepository::find_by_id(conn, booking_id)?
            .ok_or_else(|| AppError::NotFound("booking_order".into()))?;

        enforce_booking_access(ctx, &booking)?;

        let status = parse_status(&booking.status)?;
        if status != BookingStatus::PendingConfirmation {
            return Err(AppError::InvalidStateTransition(
                "agreement can only be submitted for pending_confirmation bookings".into(),
            ));
        }

        if input.typed_name.trim().is_empty() {
            return Err(AppError::Validation(vec![FieldError {
                field: "typed_name".into(),
                message: "typed_name is required for agreement confirmation".into(),
            }]));
        }

        let now = Utc::now();
        let evidence =
            AgreementEvidence::new(input.typed_name, now, booking.candidate_id, booking_id);

        PgBookingRepository::set_agreement(
            conn,
            booking_id,
            &evidence.typed_name,
            evidence.signed_at,
            &evidence.hash,
        )?;

        info!(
            actor = %ctx.user_id,
            booking_id = %booking_id,
            "agreement submitted"
        );

        emit_audit(
            conn,
            ctx,
            "booking.agreement_submitted",
            "booking_order",
            Some(booking_id),
            None,
            Some(serde_json::json!({
                "signed_by": evidence.typed_name,
                "signed_at": evidence.signed_at.to_rfc3339(),
                "hash": evidence.hash,
            })),
        );

        idem.record(conn, 200, Some(booking_id));
        Ok(evidence)
    }

    /// Confirm a booking — runs the full eligibility gate.
    ///
    /// - Checks canonical idempotency store (24h window). The hash for confirm
    ///   is the booking_id string (confirm has no request body).
    /// - If any eligibility check fails, returns `Ok(Err(EligibilityResult))` without
    ///   transitioning.
    /// - If all pass, transitions to Confirmed and records in canonical store.
    pub fn confirm(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        booking_id: Uuid,
        idempotency_key: Option<&str>,
    ) -> Result<Result<BookingOrder, EligibilityResult>, AppError> {
        ctx.require_permission("bookings", "update")?;

        // Confirm has no payload body — use the booking_id as a stable hash
        let confirm_hash = booking_id.to_string();
        let request_path = format!("/api/v1/bookings/{booking_id}/confirm");

        // ── Canonical idempotency check ───────────────────────────────────────
        if let Some(key) = idempotency_key {
            if let Some(record) = PgIdempotencyRepository::find_active(conn, key)? {
                if record.request_hash != confirm_hash {
                    warn!(
                        idempotency_key = %key,
                        "idempotency conflict on confirm: same key, different booking"
                    );
                    return Err(AppError::IdempotencyConflict);
                }
                info!(
                    idempotency_key = %key,
                    booking_id = %booking_id,
                    "idempotent replay of confirm via canonical store"
                );
                let existing = PgBookingRepository::find_by_id(conn, booking_id)?
                    .ok_or_else(|| AppError::NotFound("booking_order".into()))?;
                return Ok(Ok(db_to_domain(existing)));
            }
        }

        let booking = PgBookingRepository::find_by_id(conn, booking_id)?
            .ok_or_else(|| AppError::NotFound("booking_order".into()))?;

        enforce_booking_access(ctx, &booking)?;

        let status = parse_status(&booking.status)?;

        // Already confirmed — legacy idempotent replay
        if status == BookingStatus::Confirmed {
            if let Some(key) = idempotency_key {
                if booking.idempotency_key.as_deref() == Some(key) {
                    return Ok(Ok(db_to_domain(booking)));
                }
            }
            return Err(AppError::InvalidStateTransition(
                "booking is already confirmed".into(),
            ));
        }

        if status != BookingStatus::PendingConfirmation {
            return Err(AppError::InvalidStateTransition(format!(
                "{} → confirmed",
                status.as_str()
            )));
        }

        // Check hold expiry
        let now = Utc::now();
        if let Some(expires) = booking.hold_expires_at {
            if now >= expires {
                warn!(booking_id = %booking_id, "hold expired at confirmation attempt");
                return Err(AppError::InvalidStateTransition(
                    "hold has expired — create a new booking".into(),
                ));
            }
        }

        // Run eligibility gate — all 5 checks
        let result = EligibilityService::run_all(conn, &booking, now)?;

        if !result.eligible {
            let failed: Vec<&str> = result.failed_checks().iter().map(|c| c.name).collect();
            warn!(
                booking_id = %booking_id,
                failed = ?failed,
                "eligibility gate failed"
            );
            emit_audit(
                conn,
                ctx,
                "booking.eligibility_failed",
                "booking_order",
                Some(booking_id),
                None,
                Some(serde_json::json!({
                    "failed_checks": failed,
                })),
            );
            return Ok(Err(result));
        }

        // All checks passed — transition to Confirmed
        PgBookingRepository::set_status(conn, booking_id, BookingStatus::Confirmed.as_str())?;

        info!(
            actor = %ctx.user_id,
            booking_id = %booking_id,
            "booking confirmed"
        );

        emit_audit(
            conn,
            ctx,
            "booking.confirmed",
            "booking_order",
            Some(booking_id),
            Some(serde_json::json!({"status": "pending_confirmation"})),
            Some(serde_json::json!({"status": "confirmed"})),
        );

        let updated = PgBookingRepository::find_by_id(conn, booking_id)?
            .ok_or_else(|| AppError::Internal("booking_order not found after confirm".into()))?;

        // ── Record in canonical idempotency store ─────────────────────────────
        if let Some(key) = idempotency_key {
            let expires_at = now + Duration::hours(24);
            let record = NewDbIdempotencyKey {
                key: key.to_owned(),
                user_id: ctx.user_id,
                request_path,
                request_hash: confirm_hash,
                response_status: 200,
                response_body: Some(serde_json::json!({ "id": booking_id })),
                expires_at,
            };
            if let Err(e) = PgIdempotencyRepository::insert(conn, record) {
                warn!(error = %e, "failed to record idempotency key for confirm — non-fatal");
            }
        }

        Ok(Ok(db_to_domain(updated)))
    }

    /// Transition Confirmed → InProgress.
    pub fn start(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        booking_id: Uuid,
        idempotency_key: Option<String>,
        request_hash: Option<String>,
    ) -> Result<BookingOrder, AppError> {
        transition(conn, ctx, booking_id, BookingStatus::InProgress, idempotency_key, request_hash)
    }

    /// Transition InProgress → Completed.
    pub fn complete(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        booking_id: Uuid,
        idempotency_key: Option<String>,
        request_hash: Option<String>,
    ) -> Result<BookingOrder, AppError> {
        transition(conn, ctx, booking_id, BookingStatus::Completed, idempotency_key, request_hash)
    }

    /// Cancel a booking with 24-hour breach rules.
    ///
    /// - More than 24h before start → non-breach cancellation, no reason required.
    /// - Within 24h of start → breach: reason and reason_code are required.
    /// - Inventory is released in both cases.
    pub fn cancel(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        booking_id: Uuid,
        input: CancelInput,
        idempotency_key: Option<String>,
        request_hash: Option<String>,
    ) -> Result<BookingOrder, AppError> {
        ctx.require_permission("bookings", "update")?;

        let cancel_path = format!("/api/v1/bookings/{booking_id}/cancel");
        let idem = IdempotencyOp::new(
            idempotency_key.as_deref(),
            request_hash.as_deref(),
            ctx.user_id,
            &cancel_path,
        );
        if let Some(replay_id) = idem.check(conn)? {
            let existing = PgBookingRepository::find_by_id(conn, replay_id)?
                .ok_or_else(|| AppError::NotFound("booking_order".into()))?;
            return Ok(db_to_domain(existing));
        }

        let booking = PgBookingRepository::find_by_id(conn, booking_id)?
            .ok_or_else(|| AppError::NotFound("booking_order".into()))?;

        enforce_booking_access(ctx, &booking)?;

        let status = parse_status(&booking.status)?;
        if !status.can_transition_to(BookingStatus::Canceled) {
            return Err(AppError::InvalidStateTransition(format!(
                "{} → cancelled",
                status.as_str()
            )));
        }

        let now = Utc::now();
        let order = db_to_domain(booking.clone());
        let within_cutoff = order.is_within_cancel_cutoff(now);

        // Enforce breach rules
        if within_cutoff {
            // Within 24 hours — breach: reason_code is required
            let reason_code_str = input.reason_code.as_deref().unwrap_or("");
            let reason_code = BreachReasonCode::from_str(reason_code_str);
            if reason_code.is_none() || input.reason.is_none() {
                return Err(AppError::Validation(vec![FieldError {
                    field: "reason_code".into(),
                    message:
                        "cancellation within 24 hours requires reason and reason_code (late_cancellation, no_show, policy_violation, other)"
                            .into(),
                }]));
            }
            let reason_str = input.reason.as_deref().unwrap_or("");
            PgBookingRepository::set_breach(conn, booking_id, reason_str, reason_code_str)?;

            warn!(
                actor = %ctx.user_id,
                booking_id = %booking_id,
                reason_code = %reason_code_str,
                "breach cancellation within 24h"
            );

            emit_audit(
                conn,
                ctx,
                "booking.breach",
                "booking_order",
                Some(booking_id),
                None,
                Some(serde_json::json!({
                    "reason": input.reason,
                    "reason_code": reason_code_str,
                })),
            );
        }

        // Release inventory
        conn.transaction(|conn| {
            PgBookingRepository::set_status(conn, booking_id, BookingStatus::Canceled.as_str())?;
            if let Some(slot_id) = booking.slot_id {
                PgInventoryRepository::release_slot(conn, slot_id)?;
            }
            Ok::<_, AppError>(())
        })?;

        info!(
            actor = %ctx.user_id,
            booking_id = %booking_id,
            breach = within_cutoff,
            "booking cancelled"
        );

        emit_audit(
            conn,
            ctx,
            "booking.cancelled",
            "booking_order",
            Some(booking_id),
            Some(serde_json::json!({"status": booking.status})),
            Some(serde_json::json!({"status": "cancelled", "breach": within_cutoff})),
        );

        let updated = PgBookingRepository::find_by_id(conn, booking_id)?
            .ok_or_else(|| AppError::Internal("booking_order not found after cancel".into()))?;
        idem.record(conn, 200, Some(booking_id));
        Ok(db_to_domain(updated))
    }

    /// Reschedule a booking to a different slot.
    ///
    /// - Only allowed more than 24 hours before the original start time.
    /// - Releases the old slot and reserves the new one.
    /// - Resets to PendingConfirmation (agreement must be re-submitted).
    pub fn reschedule(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        booking_id: Uuid,
        input: RescheduleInput,
        idempotency_key: Option<String>,
        request_hash: Option<String>,
    ) -> Result<BookingOrder, AppError> {
        ctx.require_permission("bookings", "update")?;

        let reschedule_path = format!("/api/v1/bookings/{booking_id}/reschedule");
        let idem = IdempotencyOp::new(
            idempotency_key.as_deref(),
            request_hash.as_deref(),
            ctx.user_id,
            &reschedule_path,
        );
        if let Some(replay_id) = idem.check(conn)? {
            let existing = PgBookingRepository::find_by_id(conn, replay_id)?
                .ok_or_else(|| AppError::NotFound("booking_order".into()))?;
            return Ok(db_to_domain(existing));
        }

        let booking = PgBookingRepository::find_by_id(conn, booking_id)?
            .ok_or_else(|| AppError::NotFound("booking_order".into()))?;

        enforce_booking_access(ctx, &booking)?;

        let status = parse_status(&booking.status)?;
        if status != BookingStatus::PendingConfirmation && status != BookingStatus::Confirmed {
            return Err(AppError::InvalidStateTransition(
                "reschedule only allowed for pending_confirmation or confirmed bookings".into(),
            ));
        }

        // 24-hour cutoff enforcement
        let now = Utc::now();
        let order = db_to_domain(booking.clone());
        if order.is_within_cancel_cutoff(now) {
            return Err(AppError::InvalidStateTransition(
                "reschedule not allowed within 24 hours of start time".into(),
            ));
        }

        let new_hold_expires = now + Duration::minutes(HOLD_DURATION_MINUTES);

        conn.transaction(|conn| {
            // Release old slot
            if let Some(old_slot_id) = booking.slot_id {
                PgInventoryRepository::release_slot(conn, old_slot_id)?;
            }

            // Reserve new slot
            let reserved = PgInventoryRepository::reserve_slot(conn, input.new_slot_id)?;
            if !reserved {
                return Err(AppError::Conflict(
                    "new slot is fully booked — no capacity available".into(),
                ));
            }

            // Look up new slot for date/time
            let new_slot = PgInventoryRepository::find_by_id(conn, input.new_slot_id)?
                .ok_or_else(|| AppError::NotFound("booking_slot".into()))?;

            // Update booking to new slot, reset to PendingConfirmation
            PgBookingRepository::update_slot(
                conn,
                booking_id,
                input.new_slot_id,
                new_slot.slot_date,
                Some(new_slot.start_time),
                Some(new_slot.end_time),
                new_hold_expires,
            )?;

            Ok::<_, AppError>(())
        })?;

        info!(
            actor = %ctx.user_id,
            booking_id = %booking_id,
            new_slot_id = %input.new_slot_id,
            "booking rescheduled"
        );

        emit_audit(
            conn,
            ctx,
            "booking.rescheduled",
            "booking_order",
            Some(booking_id),
            Some(serde_json::json!({"slot_id": booking.slot_id})),
            Some(serde_json::json!({"slot_id": input.new_slot_id})),
        );

        let updated = PgBookingRepository::find_by_id(conn, booking_id)?
            .ok_or_else(|| AppError::Internal("booking_order not found after reschedule".into()))?;
        idem.record(conn, 200, Some(booking_id));
        Ok(db_to_domain(updated))
    }

    /// Mark a booking as Exception with a detail message.
    pub fn mark_exception(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        booking_id: Uuid,
        input: ExceptionInput,
        idempotency_key: Option<String>,
        request_hash: Option<String>,
    ) -> Result<BookingOrder, AppError> {
        ctx.require_permission("bookings", "update")?;

        let exception_path = format!("/api/v1/bookings/{booking_id}/exception");
        let idem = IdempotencyOp::new(
            idempotency_key.as_deref(),
            request_hash.as_deref(),
            ctx.user_id,
            &exception_path,
        );
        if let Some(replay_id) = idem.check(conn)? {
            let existing = PgBookingRepository::find_by_id(conn, replay_id)?
                .ok_or_else(|| AppError::NotFound("booking_order".into()))?;
            return Ok(db_to_domain(existing));
        }

        let booking = PgBookingRepository::find_by_id(conn, booking_id)?
            .ok_or_else(|| AppError::NotFound("booking_order".into()))?;

        enforce_booking_access(ctx, &booking)?;

        let status = parse_status(&booking.status)?;
        if !status.can_transition_to(BookingStatus::Exception) {
            return Err(AppError::InvalidStateTransition(format!(
                "{} → exception",
                status.as_str()
            )));
        }

        PgBookingRepository::set_exception(conn, booking_id, &input.detail)?;

        warn!(
            actor = %ctx.user_id,
            booking_id = %booking_id,
            "booking exception"
        );

        emit_audit(
            conn,
            ctx,
            "booking.exception",
            "booking_order",
            Some(booking_id),
            Some(serde_json::json!({"status": booking.status})),
            Some(serde_json::json!({"status": "exception", "detail": input.detail})),
        );

        let updated = PgBookingRepository::find_by_id(conn, booking_id)?
            .ok_or_else(|| AppError::Internal("booking_order not found after exception".into()))?;
        idem.record(conn, 200, Some(booking_id));
        Ok(db_to_domain(updated))
    }

    pub fn get(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        booking_id: Uuid,
    ) -> Result<BookingOrder, AppError> {
        ctx.require_permission("bookings", "read")?;

        let booking = PgBookingRepository::find_by_id(conn, booking_id)?
            .ok_or_else(|| AppError::NotFound("booking_order".into()))?;

        enforce_booking_access(ctx, &booking)?;

        Ok(db_to_domain(booking))
    }

    pub fn list(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        input: ListBookingsInput,
    ) -> Result<(Vec<BookingOrder>, i64), AppError> {
        ctx.require_permission("bookings", "read")?;

        let offset = (input.page.saturating_sub(1)) * input.per_page;
        let created_by_filter = ctx.ownership_filter();
        let (rows, total) =
            PgBookingRepository::list(conn, input.candidate_id, created_by_filter, offset, input.per_page)?;

        let orders = rows.into_iter().map(db_to_domain).collect();

        Ok((orders, total))
    }

    /// Release expired holds. Called by the background scheduler.
    /// This does NOT require an AuthContext — it is a system-level operation.
    pub fn release_expired_holds(conn: &mut PgConnection) -> Result<usize, AppError> {
        let now = Utc::now();
        let expired = PgBookingRepository::find_expired_holds(conn, now)?;
        let count = expired.len();

        for order in &expired {
            if let Err(e) = conn.transaction(|conn| {
                PgBookingRepository::set_status(conn, order.id, BookingStatus::Canceled.as_str())?;
                if let Some(slot_id) = order.slot_id {
                    PgInventoryRepository::release_slot(conn, slot_id)?;
                }
                Ok::<_, AppError>(())
            }) {
                tracing::error!(
                    booking_id = %order.id,
                    error = %e,
                    "failed to release expired hold"
                );
                continue;
            }

            info!(
                booking_id = %order.id,
                slot_id = ?order.slot_id,
                "expired hold released"
            );

            // Emit audit event (system actor)
            let event = crate::infrastructure::db::models::NewDbAuditEvent {
                id: Uuid::new_v4(),
                actor_id: None,
                actor_ip: None,
                action: "booking.hold_expired".to_string(),
                resource_type: "booking_order".to_string(),
                resource_id: Some(order.id),
                old_value: Some(serde_json::json!({"status": "pending_confirmation"})),
                new_value: Some(serde_json::json!({"status": "cancelled"})),
                metadata: serde_json::json!({"slot_id": order.slot_id}),
                correlation_id: None,
            };
            let _ = PgAuditRepository::insert(conn, event);
        }

        if count > 0 {
            info!(released = count, "expired holds sweep complete");
        }

        Ok(count)
    }
}

// ============================================================
// Generic transition helper
// ============================================================

fn transition(
    conn: &mut PgConnection,
    ctx: &AuthContext,
    booking_id: Uuid,
    target: BookingStatus,
    idempotency_key: Option<String>,
    request_hash: Option<String>,
) -> Result<BookingOrder, AppError> {
    ctx.require_permission("bookings", "update")?;

    let transition_path = format!("/api/v1/bookings/{booking_id}/{}", target.as_str());
    let idem = IdempotencyOp::new(
        idempotency_key.as_deref(),
        request_hash.as_deref(),
        ctx.user_id,
        &transition_path,
    );
    if let Some(replay_id) = idem.check(conn)? {
        let existing = PgBookingRepository::find_by_id(conn, replay_id)?
            .ok_or_else(|| AppError::NotFound("booking_order".into()))?;
        return Ok(db_to_domain(existing));
    }

    let booking = PgBookingRepository::find_by_id(conn, booking_id)?
        .ok_or_else(|| AppError::NotFound("booking_order".into()))?;

    enforce_booking_access(ctx, &booking)?;

    let current = parse_status(&booking.status)?;
    if !current.can_transition_to(target) {
        return Err(AppError::InvalidStateTransition(format!(
            "{} → {}",
            current.as_str(),
            target.as_str()
        )));
    }

    PgBookingRepository::set_status(conn, booking_id, target.as_str())?;

    info!(
        actor = %ctx.user_id,
        booking_id = %booking_id,
        from = %current.as_str(),
        to = %target.as_str(),
        "booking status transition"
    );

    emit_audit(
        conn,
        ctx,
        "booking.status_changed",
        "booking_order",
        Some(booking_id),
        Some(serde_json::json!({"status": current.as_str()})),
        Some(serde_json::json!({"status": target.as_str()})),
    );

    let updated = PgBookingRepository::find_by_id(conn, booking_id)?
        .ok_or_else(|| AppError::Internal("booking_order not found after transition".into()))?;
    idem.record(conn, 200, Some(booking_id));
    Ok(db_to_domain(updated))
}

// ============================================================
// Authorization
// ============================================================

fn enforce_booking_access(ctx: &AuthContext, db: &DbBookingOrder) -> Result<(), AppError> {
    if ctx.has_role("platform_admin") || ctx.has_role("club_admin") {
        return Ok(());
    }
    if db.created_by == ctx.user_id {
        return Ok(());
    }
    Err(AppError::Forbidden)
}

// ============================================================
// Conversion
// ============================================================

fn db_to_domain(db: DbBookingOrder) -> BookingOrder {
    let status = parse_status(&db.status).unwrap_or(BookingStatus::PendingConfirmation);
    let agreement_evidence = match (
        &db.agreement_signed_by,
        db.agreement_signed_at,
        &db.agreement_hash,
    ) {
        (Some(name), Some(at), Some(hash)) if !name.is_empty() && !hash.is_empty() => {
            Some(AgreementEvidence {
                typed_name: name.clone(),
                signed_at: at,
                hash: hash.clone(),
            })
        }
        _ => None,
    };

    BookingOrder {
        id: db.id,
        candidate_id: db.candidate_id,
        site_id: db.site_id,
        status,
        scheduled_date: db.scheduled_date,
        scheduled_time_start: db.scheduled_time_start,
        scheduled_time_end: db.scheduled_time_end,
        notes: db.notes,
        slot_id: db.slot_id,
        hold_expires_at: db.hold_expires_at,
        agreement_evidence,
        breach_reason: db.breach_reason,
        breach_reason_code: db.breach_reason_code,
        exception_detail: db.exception_detail,
        idempotency_key: db.idempotency_key,
        created_by: db.created_by,
        created_at: db.created_at,
        updated_at: db.updated_at,
    }
}

fn parse_status(s: &str) -> Result<BookingStatus, AppError> {
    BookingStatus::from_str(s)
        .ok_or_else(|| AppError::Internal(format!("unknown booking status: {s}")))
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
