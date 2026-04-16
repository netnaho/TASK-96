/// Connector sync executor abstraction.
///
/// Separates _how_ a sync is performed from the service orchestration logic.
/// All implementations are synchronous and blocking — they run inside a
/// `web::block` thread in the HTTP path and directly on a scheduler thread
/// in the background job path.
///
/// ## Execution model
///
/// ```text
/// trigger_sync (service)
///   └── DefaultConnectorExecutor::execute
///         ├── base_url set  → execute_http_sync (HTTP/1.0 POST, 2-second timeouts)
///         │     ├── 2xx     → SyncOutcome::success(record_count, cursor)
///         │     └── error   → SyncOutcome::failed(detail)
///         └── no base_url   → execute_file_fallback (local NDJSON staging)
///               ├── inbound/bidirectional → scan import_<entity>*.ndjson files
///               └── outbound              → create empty export staging file
/// ```
///
/// ## Adding a new executor
///
/// Implement `ConnectorExecutor` and pass an instance into `trigger_sync`.
/// No framework dependencies are required — the trait is pure Rust.
use chrono::{DateTime, Utc};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;
use tracing::{debug, warn};

// ============================================================
// Outcome
// ============================================================

/// Result of a single connector sync execution.
#[derive(Debug)]
pub struct SyncOutcome {
    /// `true` → store as `succeeded`, `false` → store as `failed`.
    pub succeeded: bool,
    /// Number of records actually processed (imported or exported).
    pub record_count: i32,
    /// Opaque pagination cursor to persist as the new watermark.
    pub cursor: Option<String>,
    /// Human-readable failure reason; `None` on success.
    pub error_message: Option<String>,
}

impl SyncOutcome {
    pub fn success(record_count: i32, cursor: Option<String>) -> Self {
        Self {
            succeeded: true,
            record_count,
            cursor,
            error_message: None,
        }
    }

    pub fn failed(error: impl Into<String>) -> Self {
        Self {
            succeeded: false,
            record_count: 0,
            cursor: None,
            error_message: Some(error.into()),
        }
    }
}

// ============================================================
// Trait
// ============================================================

/// Abstraction over how a connector sync is performed.
///
/// Implementations must be `Send + Sync` and must never panic — all failures
/// must be expressed as `SyncOutcome::failed(...)`.
pub trait ConnectorExecutor: Send + Sync {
    /// Execute the sync for the given connector and entity type.
    ///
    /// - `connector_type`: `"inbound"`, `"outbound"`, or `"bidirectional"`
    /// - `base_url`: the connector's configured remote endpoint, or `None`
    /// - `entity_type`: e.g. `"candidates"`, `"offers"`
    /// - `watermark`: `(last_sync_at, last_sync_cursor)` for incremental sync
    fn execute(
        &self,
        connector_type: &str,
        base_url: Option<&str>,
        entity_type: &str,
        watermark: Option<(Option<DateTime<Utc>>, Option<String>)>,
    ) -> SyncOutcome;
}

// ============================================================
// Default production executor
// ============================================================

/// Production executor: HTTP-backed with local file fallback.
///
/// When `base_url` is set, attempts an HTTP/1.0 POST to `{base_url}/sync`.
/// When `base_url` is absent or the HTTP call fails, falls back to local
/// NDJSON file staging at `storage_path`.
pub struct DefaultConnectorExecutor {
    pub storage_path: String,
}

impl ConnectorExecutor for DefaultConnectorExecutor {
    fn execute(
        &self,
        connector_type: &str,
        base_url: Option<&str>,
        entity_type: &str,
        watermark: Option<(Option<DateTime<Utc>>, Option<String>)>,
    ) -> SyncOutcome {
        match base_url {
            Some(url) if !url.is_empty() => {
                execute_http_sync(url, connector_type, entity_type, &watermark)
            }
            _ => execute_file_fallback(&self.storage_path, connector_type, entity_type),
        }
    }
}

// ============================================================
// HTTP sync (blocking, no external deps)
// ============================================================

/// Attempt an HTTP sync against the connector's remote endpoint.
///
/// Protocol: POST `{base_url}/sync` with a JSON body describing the request.
/// Expected response: JSON with `"record_count"` (integer) and optional
/// `"cursor"` (string).  A 2xx without parseable JSON is treated as success
/// with `record_count = 0`.
fn execute_http_sync(
    base_url: &str,
    connector_type: &str,
    entity_type: &str,
    watermark: &Option<(Option<DateTime<Utc>>, Option<String>)>,
) -> SyncOutcome {
    // Build the sync endpoint: strip trailing slash then append /sync
    let base = base_url.trim_end_matches('/');
    let sync_url = format!("{base}/sync");

    // Build request payload
    let since = watermark
        .as_ref()
        .and_then(|(ts, _)| ts.as_ref())
        .map(|t| t.to_rfc3339());
    let cursor_in = watermark.as_ref().and_then(|(_, c)| c.as_ref()).cloned();

    let body = serde_json::json!({
        "entity_type": entity_type,
        "connector_type": connector_type,
        "since": since,
        "cursor": cursor_in,
    })
    .to_string();

    debug!(
        url = %sync_url,
        entity_type = %entity_type,
        "executing HTTP connector sync"
    );

    match post_sync_http(&sync_url, &body) {
        Ok(response_body) => {
            // Parse record_count and cursor from response JSON (best-effort)
            let parsed = serde_json::from_str::<serde_json::Value>(&response_body).ok();
            let record_count = parsed
                .as_ref()
                .and_then(|v| v.get("record_count"))
                .and_then(|n| n.as_i64())
                .map(|n| n.clamp(0, i32::MAX as i64) as i32)
                .unwrap_or(0);
            let cursor_out = parsed
                .as_ref()
                .and_then(|v| v.get("cursor"))
                .and_then(|c| c.as_str())
                .map(String::from);

            debug!(record_count = record_count, "HTTP connector sync succeeded");
            SyncOutcome::success(record_count, cursor_out)
        }
        Err(e) => {
            warn!(error = %e, url = %sync_url, "HTTP connector sync failed");
            SyncOutcome::failed(e)
        }
    }
}

/// Minimal blocking HTTP/1.0 POST.  Returns the response body on 2xx,
/// or an error string for connection failures, timeouts, and non-2xx.
///
/// Only `http://` URLs targeting localhost or RFC 1918 private addresses are
/// allowed.  Public hosts are rejected before any connection is attempted.
/// Connect and read timeouts are both 2 seconds.
fn post_sync_http(url: &str, body: &str) -> Result<String, String> {
    // Enforce local-network-only host boundary
    crate::shared::network::validate_local_url(url)?;

    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("only http:// URLs are supported, got: {url}"))?;

    let (host_port, path_part) = rest.split_once('/').unwrap_or((rest, ""));
    let path = format!("/{path_part}");

    let (host, port) = if let Some(colon_pos) = host_port.rfind(':') {
        let host = &host_port[..colon_pos];
        let port: u16 = host_port[colon_pos + 1..]
            .parse()
            .map_err(|e| format!("invalid port in URL '{url}': {e}"))?;
        (host, port)
    } else {
        (host_port, 80u16)
    };

    let addr = format!("{host}:{port}");
    let stream = TcpStream::connect_timeout(
        &addr
            .parse()
            .map_err(|e| format!("invalid address '{addr}': {e}"))?,
        Duration::from_secs(2),
    )
    .map_err(|e| format!("connect to {addr}: {e}"))?;

    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;

    let mut stream = stream;

    let request = format!(
        "POST {path} HTTP/1.0\r\n\
         Host: {host}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {len}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        len = body.len()
    );

    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("write request: {e}"))?;

    // Read up to 8 KB of response (status + headers + body)
    let mut buf = vec![0u8; 8192];
    let n = stream.read(&mut buf).unwrap_or(0);
    let response = String::from_utf8_lossy(&buf[..n]);

    // Extract status code from first line ("HTTP/1.x DDD ...")
    let status_code = response
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);

    if !(200..300).contains(&status_code) {
        return Err(format!(
            "HTTP {status_code} from {url} ({})",
            response.lines().next().unwrap_or("").trim()
        ));
    }

    // Split off the body after the blank line separating headers from body
    let body_start = response.find("\r\n\r\n").map(|i| i + 4).unwrap_or(n);
    Ok(response[body_start..].to_string())
}

// ============================================================
// File-based fallback (offline / on-prem)
// ============================================================

/// Execute a sync using local NDJSON files when no remote endpoint is configured.
///
/// - **inbound / bidirectional**: scans `{storage_path}/import_{entity_type}*.ndjson`
///   and counts non-empty lines as the record count.
/// - **outbound**: creates an empty export staging file at
///   `{storage_path}/export_{entity_type}_{timestamp}.ndjson` so that an
///   external process can populate it.  `record_count` is `0` because no DB
///   records are queried here — export_data (the manual API) handles that path.
fn execute_file_fallback(
    storage_path: &str,
    connector_type: &str,
    entity_type: &str,
) -> SyncOutcome {
    match connector_type {
        "inbound" | "bidirectional" => {
            let count = count_staged_import_records(storage_path, entity_type);
            debug!(
                storage_path = %storage_path,
                entity_type = %entity_type,
                count = count,
                "file fallback: inbound scan complete"
            );
            SyncOutcome::success(count, None)
        }
        "outbound" => {
            let ts = Utc::now().format("%Y%m%d%H%M%S");
            let path = format!("{storage_path}/export_{entity_type}_{ts}.ndjson");
            if let Some(parent) = std::path::Path::new(&path).parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    return SyncOutcome::failed(format!(
                        "cannot create storage directory '{storage_path}': {e}"
                    ));
                }
            }
            match std::fs::File::create(&path) {
                Ok(_) => {
                    debug!(path = %path, "file fallback: outbound staging file created");
                    SyncOutcome::success(0, None)
                }
                Err(e) => {
                    SyncOutcome::failed(format!("cannot create export staging file '{path}': {e}"))
                }
            }
        }
        other => SyncOutcome::failed(format!("unknown connector_type: '{other}'")),
    }
}

/// Count non-empty lines across all `import_{entity_type}*.ndjson` files
/// in `storage_path`.  Unreadable files are silently skipped.
fn count_staged_import_records(storage_path: &str, entity_type: &str) -> i32 {
    let dir = std::path::Path::new(storage_path);
    if !dir.exists() {
        return 0;
    }
    let prefix = format!("import_{entity_type}");
    let mut count = 0i32;

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with(&prefix) && name_str.ends_with(".ndjson") {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    count += content.lines().filter(|l| !l.trim().is_empty()).count() as i32;
                }
            }
        }
    }
    count
}
