/// Shared canonical-idempotency helper for application-layer services.
///
/// Every mutating service function that should be idempotency-aware follows
/// the same three-step protocol:
///
/// 1. Build an [`IdempotencyOp`] from the incoming key and hash.
/// 2. Call [`IdempotencyOp::check`] **before** performing any work.
///    - `Ok(None)` → no key or no existing record; proceed normally.
///    - `Ok(Some(id))` → replay match (same key + same hash); re-fetch the
///      resource by the stored `id` and return it.
///    - `Err(AppError::IdempotencyConflict)` → same key, different payload;
///      propagate the error to the caller (409).
/// 3. After a successful mutation, call [`IdempotencyOp::record`] to persist
///    the outcome in the canonical store with a 24-hour TTL.
///
/// Clients that omit the `Idempotency-Key` header are unaffected: all three
/// steps short-circuit when `key` is `None`.
use chrono::{Duration, Utc};
use diesel::PgConnection;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    infrastructure::db::{
        models::NewDbIdempotencyKey, repositories::idempotency_repo::PgIdempotencyRepository,
    },
    shared::errors::AppError,
};

/// Carries the idempotency context for a single service invocation.
pub struct IdempotencyOp<'a> {
    pub key: Option<&'a str>,
    pub request_hash: &'a str,
    pub user_id: Uuid,
    pub request_path: &'a str,
}

impl<'a> IdempotencyOp<'a> {
    /// Construct a new op. `request_hash` defaults to `""` when `None` (e.g.
    /// for no-body state-transition endpoints).
    pub fn new(
        key: Option<&'a str>,
        request_hash: Option<&'a str>,
        user_id: Uuid,
        request_path: &'a str,
    ) -> Self {
        Self {
            key: key.filter(|k| !k.is_empty()),
            request_hash: request_hash.unwrap_or(""),
            user_id,
            request_path,
        }
    }

    /// Check the canonical idempotency store.
    ///
    /// Returns:
    /// - `Ok(None)` — no key present or no active record; caller should proceed.
    /// - `Ok(Some(id))` — replay (same key + same hash); caller should re-fetch
    ///   the resource by `id` and return it directly.
    /// - `Err(AppError::IdempotencyConflict)` — same key, different hash.
    pub fn check(&self, conn: &mut PgConnection) -> Result<Option<Uuid>, AppError> {
        let key = match self.key {
            Some(k) => k,
            None => return Ok(None),
        };

        if let Some(record) = PgIdempotencyRepository::find_active(conn, key)? {
            if record.request_hash != self.request_hash {
                warn!(
                    idempotency_key = %key,
                    "idempotency conflict: same key, different request body"
                );
                return Err(AppError::IdempotencyConflict);
            }
            let stored_id = record
                .response_body
                .as_ref()
                .and_then(|b| b.get("id"))
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<Uuid>().ok());
            info!(idempotency_key = %key, "idempotent replay");
            return Ok(Some(stored_id.unwrap_or(Uuid::nil())));
        }

        Ok(None)
    }

    /// Record a completed mutation in the canonical store (non-fatal on error).
    ///
    /// `response_status` should be the HTTP status that was returned (e.g. 200 or 201).
    /// `resource_id` is stored in `response_body` so that future replays can
    /// re-fetch the live resource.
    pub fn record(&self, conn: &mut PgConnection, response_status: i32, resource_id: Option<Uuid>) {
        let key = match self.key {
            Some(k) => k,
            None => return,
        };

        let expires_at = Utc::now() + Duration::hours(24);
        let record = NewDbIdempotencyKey {
            key: key.to_owned(),
            user_id: self.user_id,
            request_path: self.request_path.to_owned(),
            request_hash: self.request_hash.to_owned(),
            response_status,
            response_body: resource_id.map(|id| serde_json::json!({ "id": id })),
            expires_at,
        };

        if let Err(e) = PgIdempotencyRepository::insert(conn, record) {
            warn!(error = %e, idempotency_key = %key, "failed to record idempotency key — non-fatal");
        }
    }
}
