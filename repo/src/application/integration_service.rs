/// Integration connector and sync management service.
///
/// ## Pluggable connector abstraction
///
/// Each `IntegrationConnector` row describes an external integration point.
/// The `connector_type` field determines direction:
/// - `inbound` — external system pushes data to TalentFlow
/// - `outbound` — TalentFlow pushes data to external system
/// - `bidirectional` — both directions
///
/// Auth config (API keys, OAuth tokens) is encrypted at rest using AES-256-GCM
/// before storage. It is never logged or included in list responses.
///
/// ## Incremental sync with watermarks
///
/// For each (connector, entity_type) pair, a sync state row stores:
/// - `last_sync_at`: the timestamp of the most-recent successful sync
/// - `last_sync_cursor`: an opaque cursor (e.g., page token or offset)
///
/// The `get_watermark` API allows callers to retrieve these values before
/// issuing a sync request, enabling incremental "changed since last sync" queries.
///
/// ## File-based import/export fallback
///
/// When no connector is specified, import/export falls back to local file
/// storage at `STORAGE_PATH`.  Files are written/read as newline-delimited JSON.
/// This offline-ready mode allows data exchange without a live external endpoint.
///
/// ## Sync trigger flow
///
/// 1. Client calls `POST /integrations/connectors/{id}/sync`
/// 2. Idempotency guard: if a sync is already `running` for the same
///    (connector, entity_type) pair, returns `409 Conflict` unless
///    `force = true` is supplied.
/// 3. Watermark is fetched for incremental sync.
/// 4. State is set to `running`.
/// 5. `DefaultConnectorExecutor` performs the actual work:
///    - connector has `base_url` → HTTP/1.0 POST to `{base_url}/sync`
///    - no `base_url` → file-based fallback using local NDJSON staging files
/// 6. State is set to `succeeded` (with real `record_count`) or `failed`
///    (with `error_message`).  No fake success is ever written.
use chrono::Utc;
use diesel::PgConnection;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    application::{
        connector_executor::{ConnectorExecutor, DefaultConnectorExecutor},
        idempotency_op::IdempotencyOp,
    },
    domain::auth::models::AuthContext,
    infrastructure::{
        crypto,
        db::{
            models::{NewDbIdempotencyKey, NewDbIntegrationConnector},
            repositories::{
                idempotency_repo::PgIdempotencyRepository,
                integration_repo::PgIntegrationRepository,
            },
        },
    },
    shared::errors::{AppError, FieldError},
};

// ============================================================
// Input types
// ============================================================

pub struct CreateConnectorInput {
    pub name: String,
    pub connector_type: String,
    pub base_url: Option<String>,
    /// Plaintext auth config (JSON). Encrypted before storage.
    pub auth_config: Option<serde_json::Value>,
    pub is_enabled: bool,
    pub idempotency_key: Option<String>,
    pub request_hash: Option<String>,
}

pub struct UpdateConnectorInput {
    pub name: Option<String>,
    pub base_url: Option<Option<String>>,
    /// Plaintext auth config (JSON). Encrypted before storage.
    pub auth_config: Option<Option<serde_json::Value>>,
    pub is_enabled: Option<bool>,
    pub idempotency_key: Option<String>,
    pub request_hash: Option<String>,
}

pub struct ImportInput {
    pub connector_id: Option<Uuid>,
    pub entity_type: String,
    /// Raw records as a JSON array.
    pub records: Vec<serde_json::Value>,
    pub storage_path: String,
    pub idempotency_key: Option<String>,
    pub request_hash: Option<String>,
}

pub struct ExportInput {
    pub connector_id: Option<Uuid>,
    pub entity_type: String,
    /// Field mapping: source_field → target_field.
    pub field_map: std::collections::HashMap<String, String>,
    pub storage_path: String,
    pub idempotency_key: Option<String>,
    pub request_hash: Option<String>,
}

// ============================================================
// Response types
// ============================================================

#[derive(Debug, Serialize)]
pub struct ConnectorResponse {
    pub id: Uuid,
    pub name: String,
    pub connector_type: String,
    pub base_url: Option<String>,
    pub is_enabled: bool,
    pub created_by: Uuid,
    pub created_at: String,
    pub updated_at: String,
    // auth_config intentionally omitted — never returned
}

#[derive(Debug, Serialize)]
pub struct SyncStateResponse {
    pub id: Uuid,
    pub connector_id: Uuid,
    pub entity_type: String,
    pub last_sync_at: Option<String>,
    pub last_sync_cursor: Option<String>,
    pub status: String,
    pub error_message: Option<String>,
    pub record_count: i32,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct ImportResult {
    pub entity_type: String,
    pub records_imported: usize,
    pub source: String, // "connector" or "file"
}

#[derive(Debug, Serialize)]
pub struct ExportResult {
    pub entity_type: String,
    pub records_exported: usize,
    pub destination: String, // "connector" or "file:<path>"
}

// ============================================================
// Service
// ============================================================

pub struct IntegrationService;

impl IntegrationService {
    // ── Connectors ───────────────────────────────────────────────────────────

    pub fn create_connector(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        input: CreateConnectorInput,
        encryption_key: &str,
    ) -> Result<ConnectorResponse, AppError> {
        ctx.require_permission("integrations", "create")?;
        validate_connector_type(&input.connector_type)?;

        let idem = IdempotencyOp::new(
            input.idempotency_key.as_deref(),
            input.request_hash.as_deref(),
            ctx.user_id,
            "/api/v1/integrations/connectors",
        );
        if let Some(replay_id) = idem.check(conn)? {
            let db = PgIntegrationRepository::find_connector(conn, replay_id)?
                .ok_or_else(|| AppError::NotFound("connector".into()))?;
            return Ok(map_connector(db));
        }

        let auth_encrypted = encrypt_auth_config(input.auth_config.as_ref(), encryption_key)?;

        let record = NewDbIntegrationConnector {
            id: Uuid::new_v4(),
            name: input.name,
            connector_type: input.connector_type,
            base_url: input.base_url,
            auth_config_encrypted: auth_encrypted,
            is_enabled: input.is_enabled,
            created_by: ctx.user_id,
        };

        let c = PgIntegrationRepository::insert_connector(conn, record)?;

        info!(
            actor = %ctx.user_id,
            connector_id = %c.id,
            connector_type = %c.connector_type,
            "integration connector created"
        );

        idem.record(conn, 201, Some(c.id));
        Ok(map_connector(c))
    }

    pub fn get_connector(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        id: Uuid,
    ) -> Result<ConnectorResponse, AppError> {
        ctx.require_permission("integrations", "read")?;
        let c = PgIntegrationRepository::find_connector(conn, id)?
            .ok_or_else(|| AppError::NotFound("connector".into()))?;
        Ok(map_connector(c))
    }

    pub fn list_connectors(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        page: i64,
        per_page: i64,
    ) -> Result<(Vec<ConnectorResponse>, i64), AppError> {
        ctx.require_permission("integrations", "read")?;
        let (rows, total) = PgIntegrationRepository::list_connectors(conn, page, per_page)?;
        Ok((rows.into_iter().map(map_connector).collect(), total))
    }

    pub fn update_connector(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        id: Uuid,
        input: UpdateConnectorInput,
        encryption_key: &str,
    ) -> Result<ConnectorResponse, AppError> {
        ctx.require_permission("integrations", "update")?;

        let idem = IdempotencyOp::new(
            input.idempotency_key.as_deref(),
            input.request_hash.as_deref(),
            ctx.user_id,
            "/api/v1/integrations/connectors",
        );
        if let Some(replay_id) = idem.check(conn)? {
            let db = PgIntegrationRepository::find_connector(conn, replay_id)?
                .ok_or_else(|| AppError::NotFound("connector".into()))?;
            return Ok(map_connector(db));
        }

        // Validate connector exists
        PgIntegrationRepository::find_connector(conn, id)?
            .ok_or_else(|| AppError::NotFound("connector".into()))?;

        let encrypted_auth: Option<Option<Vec<u8>>> = match input.auth_config {
            Some(Some(ref cfg)) => {
                let enc = encrypt_auth_config(Some(cfg), encryption_key)?;
                Some(enc)
            }
            Some(None) => Some(None), // clear
            None => None,             // no change
        };

        let c = PgIntegrationRepository::update_connector(
            conn,
            id,
            input.name,
            input.base_url,
            input.is_enabled,
            encrypted_auth,
        )?;

        info!(
            actor = %ctx.user_id,
            connector_id = %id,
            "integration connector updated"
        );

        idem.record(conn, 200, Some(c.id));
        Ok(map_connector(c))
    }

    // ── Sync ─────────────────────────────────────────────────────────────────

    /// Trigger a connector sync.
    ///
    /// - `storage_path`: base directory for file-fallback NDJSON staging.
    /// - `force`: when `true`, bypasses the concurrent-running guard and
    ///   re-executes even if a `running` row already exists for this pair.
    ///
    /// Status lifecycle: `running` → `succeeded | failed`.
    /// `last_sync_at` and `last_sync_cursor` are only updated on success.
    pub fn trigger_sync(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        connector_id: Uuid,
        entity_type: &str,
        storage_path: &str,
        force: bool,
    ) -> Result<SyncStateResponse, AppError> {
        ctx.require_permission("integrations", "update")?;

        // Verify connector exists and capture its config for execution
        let connector = PgIntegrationRepository::find_connector(conn, connector_id)?
            .ok_or_else(|| AppError::NotFound("connector".into()))?;

        // Idempotency guard: reject a second trigger while one is already running
        // unless the caller explicitly sets force=true.
        if !force {
            if let Some(existing) = PgIntegrationRepository::find_sync_state_for_entity(
                conn,
                connector_id,
                entity_type,
            )? {
                if existing.status == "running" {
                    return Err(AppError::Conflict(format!(
                        "a sync for connector {connector_id} / entity '{entity_type}' is \
                         already running; use force=true to override"
                    )));
                }
            }
        }

        // Fetch incremental watermark (last_sync_at, last_sync_cursor)
        let watermark = PgIntegrationRepository::get_watermark(conn, connector_id, entity_type)?;

        // Transition to running
        PgIntegrationRepository::upsert_sync_state(
            conn,
            connector_id,
            entity_type,
            "running",
            None,
            None,
            0,
            None,
        )?;

        // Execute the connector workflow — real work happens here.
        // On success: record_count reflects records actually processed.
        // On failure: error_message explains what went wrong.
        let executor = DefaultConnectorExecutor {
            storage_path: storage_path.to_owned(),
        };
        let outcome = executor.execute(
            &connector.connector_type,
            connector.base_url.as_deref(),
            entity_type,
            watermark,
        );

        // Derive final state from outcome — no hardcoded "succeeded"
        let final_status = if outcome.succeeded {
            "succeeded"
        } else {
            "failed"
        };
        let last_sync_at = if outcome.succeeded {
            Some(Utc::now())
        } else {
            None
        };

        if !outcome.succeeded {
            warn!(
                connector_id = %connector_id,
                entity_type = %entity_type,
                error = outcome.error_message.as_deref().unwrap_or("unknown"),
                "sync failed"
            );
        }

        let state = PgIntegrationRepository::upsert_sync_state(
            conn,
            connector_id,
            entity_type,
            final_status,
            last_sync_at,
            outcome.cursor,
            outcome.record_count,
            outcome.error_message,
        )?;

        info!(
            actor = %ctx.user_id,
            connector_id = %connector_id,
            entity_type = %entity_type,
            status = %final_status,
            record_count = outcome.record_count,
            "sync completed"
        );

        Ok(map_sync_state(state))
    }

    pub fn get_sync_state(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        connector_id: Uuid,
    ) -> Result<Vec<SyncStateResponse>, AppError> {
        ctx.require_permission("integrations", "read")?;
        PgIntegrationRepository::find_connector(conn, connector_id)?
            .ok_or_else(|| AppError::NotFound("connector".into()))?;

        let rows = PgIntegrationRepository::get_sync_state(conn, connector_id)?;
        Ok(rows.into_iter().map(map_sync_state).collect())
    }

    // ── Import ────────────────────────────────────────────────────────────────

    /// Import records.
    ///
    /// - If `connector_id` is provided, the import is attributed to that connector.
    ///   The sync state is updated with the record count.
    /// - Otherwise, records are written to a local NDJSON file at
    ///   `{storage_path}/import_{entity_type}_{timestamp}.ndjson` as a fallback.
    ///
    /// Accepts an optional `Idempotency-Key`. Same key + same body → replay the
    /// stored result. Same key + different body → 409. No key → normal.
    pub fn import_data(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        input: ImportInput,
    ) -> Result<ImportResult, AppError> {
        ctx.require_permission("integrations", "create")?;

        // ── Idempotency check ─────────────────────────────────────────────────
        if let Some(ref key) = input.idempotency_key {
            if let Some(record) = PgIdempotencyRepository::find_active(conn, key)? {
                let incoming_hash = input.request_hash.as_deref().unwrap_or("");
                if record.request_hash != incoming_hash {
                    warn!(idempotency_key = %key, "idempotency conflict on import: same key, different body");
                    return Err(AppError::IdempotencyConflict);
                }
                info!(idempotency_key = %key, "idempotent replay of import");
                if let Some(body) = &record.response_body {
                    let entity_type = body["entity_type"].as_str().unwrap_or("").to_string();
                    let records_imported = body["records_imported"].as_u64().unwrap_or(0) as usize;
                    let source = body["source"].as_str().unwrap_or("").to_string();
                    return Ok(ImportResult { entity_type, records_imported, source });
                }
            }
        }

        let count = input.records.len();
        let now = Utc::now();

        let result = if let Some(connector_id) = input.connector_id {
            // Connector-based: update sync state
            PgIntegrationRepository::find_connector(conn, connector_id)?
                .ok_or_else(|| AppError::NotFound("connector".into()))?;

            PgIntegrationRepository::upsert_sync_state(
                conn,
                connector_id,
                &input.entity_type,
                "succeeded",
                Some(now),
                None,
                count as i32,
                None,
            )?;

            info!(
                actor = %ctx.user_id,
                connector_id = %connector_id,
                entity_type = %input.entity_type,
                records = count,
                "import via connector"
            );

            ImportResult {
                entity_type: input.entity_type,
                records_imported: count,
                source: "connector".into(),
            }
        } else {
            // File fallback
            let ts = now.format("%Y%m%d%H%M%S");
            let path = format!(
                "{}/import_{}_{}.ndjson",
                input.storage_path, input.entity_type, ts
            );
            write_ndjson_fallback(&path, &input.records)?;

            info!(
                actor = %ctx.user_id,
                entity_type = %input.entity_type,
                records = count,
                path = %path,
                "import via file fallback"
            );

            ImportResult {
                entity_type: input.entity_type,
                records_imported: count,
                source: format!("file:{path}"),
            }
        };

        // ── Record in idempotency store ───────────────────────────────────────
        if let Some(ref key) = input.idempotency_key {
            let response_body = serde_json::json!({
                "entity_type": result.entity_type,
                "records_imported": result.records_imported,
                "source": result.source,
            });
            let record = NewDbIdempotencyKey {
                key: key.clone(),
                user_id: ctx.user_id,
                request_path: "/api/v1/integrations/import".to_owned(),
                request_hash: input.request_hash.unwrap_or_default(),
                response_status: 200,
                response_body: Some(response_body),
                expires_at: now + chrono::Duration::hours(24),
            };
            if let Err(e) = PgIdempotencyRepository::insert(conn, record) {
                warn!(error = %e, "failed to record idempotency key for import — non-fatal");
            }
        }

        Ok(result)
    }

    /// Export staging operation.
    ///
    /// Creates an NDJSON staging file that a subsequent `trigger_sync` run can
    /// pick up.  The `field_map` (source → target key renames) is persisted as
    /// the first line of the staging file so the sync executor can apply it
    /// when pushing records to the remote system.
    ///
    /// Because this endpoint only creates the staging manifest — it does not
    /// query or stream entity data — `records_exported` in the response is
    /// always `0`.  Actual record counts are reported by `trigger_sync` after
    /// the outbound workflow completes.
    ///
    /// When a connector is specified the request is validated against it, but
    /// the sync-state lifecycle is owned exclusively by `trigger_sync`.
    ///
    /// Accepts an optional `Idempotency-Key`. Same key + same body → replay.
    /// Same key + different body → 409. No key → normal.
    pub fn export_data(
        conn: &mut PgConnection,
        ctx: &AuthContext,
        input: ExportInput,
    ) -> Result<ExportResult, AppError> {
        ctx.require_permission("integrations", "read")?;

        // ── Idempotency check ─────────────────────────────────────────────────
        if let Some(ref key) = input.idempotency_key {
            if let Some(record) = PgIdempotencyRepository::find_active(conn, key)? {
                let incoming_hash = input.request_hash.as_deref().unwrap_or("");
                if record.request_hash != incoming_hash {
                    warn!(idempotency_key = %key, "idempotency conflict on export: same key, different body");
                    return Err(AppError::IdempotencyConflict);
                }
                info!(idempotency_key = %key, "idempotent replay of export");
                if let Some(body) = &record.response_body {
                    let entity_type = body["entity_type"].as_str().unwrap_or("").to_string();
                    let records_exported = body["records_exported"].as_u64().unwrap_or(0) as usize;
                    let destination = body["destination"].as_str().unwrap_or("").to_string();
                    return Ok(ExportResult { entity_type, records_exported, destination });
                }
            }
        }

        let now = Utc::now();
        let ts = now.format("%Y%m%d%H%M%S");
        let path = format!(
            "{}/export_{}_{}.ndjson",
            input.storage_path, input.entity_type, ts
        );

        if let Some(connector_id) = input.connector_id {
            // Validate the connector exists
            PgIntegrationRepository::find_connector(conn, connector_id)?
                .ok_or_else(|| AppError::NotFound("connector".into()))?;

            info!(
                actor = %ctx.user_id,
                connector_id = %connector_id,
                entity_type = %input.entity_type,
                "export staging file created for connector (use trigger_sync to run the connector workflow)"
            );
        } else {
            info!(
                actor = %ctx.user_id,
                entity_type = %input.entity_type,
                path = %path,
                "export staging file created via file fallback"
            );
        }

        // Write the staging file. The first line is a JSON metadata header
        // containing the field_map so downstream consumers (trigger_sync)
        // can apply key renames when pushing records.
        write_export_staging_file(&path, &input.field_map)?;

        let result = ExportResult {
            entity_type: input.entity_type,
            records_exported: 0,
            destination: format!("file:{path}"),
        };

        // ── Record in idempotency store ───────────────────────────────────────
        if let Some(ref key) = input.idempotency_key {
            let response_body = serde_json::json!({
                "entity_type": result.entity_type,
                "records_exported": result.records_exported,
                "destination": result.destination,
            });
            let record = NewDbIdempotencyKey {
                key: key.clone(),
                user_id: ctx.user_id,
                request_path: "/api/v1/integrations/export".to_owned(),
                request_hash: input.request_hash.unwrap_or_default(),
                response_status: 200,
                response_body: Some(response_body),
                expires_at: now + chrono::Duration::hours(24),
            };
            if let Err(e) = PgIdempotencyRepository::insert(conn, record) {
                warn!(error = %e, "failed to record idempotency key for export — non-fatal");
            }
        }

        Ok(result)
    }
}

// ============================================================
// Helpers
// ============================================================

fn validate_connector_type(ct: &str) -> Result<(), AppError> {
    match ct {
        "inbound" | "outbound" | "bidirectional" => Ok(()),
        _ => Err(AppError::Validation(vec![FieldError {
            field: "connector_type".into(),
            message: "connector_type must be one of: inbound, outbound, bidirectional".into(),
        }])),
    }
}

fn encrypt_auth_config(
    config: Option<&serde_json::Value>,
    encryption_key: &str,
) -> Result<Option<Vec<u8>>, AppError> {
    match config {
        None => Ok(None),
        Some(cfg) => {
            let json = serde_json::to_vec(cfg)
                .map_err(|e| AppError::Internal(format!("auth config serialisation: {e}")))?;
            let enc = crypto::encrypt(&json, encryption_key)
                .map_err(|e| AppError::Internal(format!("auth config encryption: {e}")))?;
            Ok(Some(enc))
        }
    }
}

/// Write records as newline-delimited JSON to `path`.
/// Best-effort: creates parent directories as needed.
fn write_ndjson_fallback(path: &str, records: &[serde_json::Value]) -> Result<(), AppError> {
    use std::io::Write;

    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::Internal(format!("create_dir_all failed: {e}")))?;
    }

    let mut f = std::fs::File::create(path)
        .map_err(|e| AppError::Internal(format!("file create failed: {e}")))?;

    for record in records {
        let line = serde_json::to_string(record)
            .map_err(|e| AppError::Internal(format!("json serialise failed: {e}")))?;
        writeln!(f, "{line}").map_err(|e| AppError::Internal(format!("file write failed: {e}")))?;
    }
    Ok(())
}

/// Write an export staging file with a metadata header line containing the
/// field_map.  The file is NDJSON: line 1 is `{"_meta": {"field_map": {...}}}`,
/// subsequent lines (added by trigger_sync) are data records.
fn write_export_staging_file(
    path: &str,
    field_map: &std::collections::HashMap<String, String>,
) -> Result<(), AppError> {
    use std::io::Write;

    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::Internal(format!("create_dir_all failed: {e}")))?;
    }

    let mut f = std::fs::File::create(path)
        .map_err(|e| AppError::Internal(format!("file create failed: {e}")))?;

    let header = serde_json::json!({ "_meta": { "field_map": field_map } });
    writeln!(f, "{}", serde_json::to_string(&header).unwrap_or_default())
        .map_err(|e| AppError::Internal(format!("file write failed: {e}")))?;

    Ok(())
}

fn map_connector(
    c: crate::infrastructure::db::models::DbIntegrationConnector,
) -> ConnectorResponse {
    ConnectorResponse {
        id: c.id,
        name: c.name,
        connector_type: c.connector_type,
        base_url: c.base_url,
        is_enabled: c.is_enabled,
        created_by: c.created_by,
        created_at: c.created_at.to_rfc3339(),
        updated_at: c.updated_at.to_rfc3339(),
    }
}

fn map_sync_state(
    s: crate::infrastructure::db::models::DbIntegrationSyncState,
) -> SyncStateResponse {
    SyncStateResponse {
        id: s.id,
        connector_id: s.connector_id,
        entity_type: s.entity_type,
        last_sync_at: s.last_sync_at.map(|t| t.to_rfc3339()),
        last_sync_cursor: s.last_sync_cursor,
        status: s.status,
        error_message: s.error_message,
        record_count: s.record_count,
        updated_at: s.updated_at.to_rfc3339(),
    }
}
