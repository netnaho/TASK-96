/// Unit tests for the connector executor and sync state logic.
///
/// These tests run without a database — they verify:
/// - `SyncOutcome` constructor semantics
/// - `DefaultConnectorExecutor` behaviour for each connector type and URL scenario
/// - File-based staging record counting
/// - HTTP failure path (unreachable / unsupported URLs)
use talentflow::application::connector_executor::{
    ConnectorExecutor, DefaultConnectorExecutor, SyncOutcome,
};

// ── Minimal temp-dir helper (no extra crate deps) ─────────────────────────────

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(prefix: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let p = std::env::temp_dir().join(format!("{prefix}_{nanos}"));
        std::fs::create_dir_all(&p).expect("temp dir");
        TempDir(p)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }

    fn path_str(&self) -> &str {
        self.0.to_str().expect("utf-8 path")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

// ── SyncOutcome constructors ──────────────────────────────────────────────────

#[test]
fn test_outcome_success_fields() {
    let o = SyncOutcome::success(42, Some("tok_abc".into()));
    assert!(o.succeeded);
    assert_eq!(o.record_count, 42);
    assert_eq!(o.cursor.as_deref(), Some("tok_abc"));
    assert!(o.error_message.is_none());
}

#[test]
fn test_outcome_success_zero_records() {
    let o = SyncOutcome::success(0, None);
    assert!(o.succeeded);
    assert_eq!(o.record_count, 0);
    assert!(o.cursor.is_none());
    assert!(o.error_message.is_none());
}

#[test]
fn test_outcome_failed_fields() {
    let o = SyncOutcome::failed("connection refused on port 9999");
    assert!(!o.succeeded);
    assert_eq!(o.record_count, 0);
    assert!(o.cursor.is_none());
    assert_eq!(
        o.error_message.as_deref(),
        Some("connection refused on port 9999")
    );
}

// ── File fallback — inbound ───────────────────────────────────────────────────

#[test]
fn test_inbound_no_files_returns_success_with_zero() {
    let dir = TempDir::new("tf_sync_test");
    let exec = DefaultConnectorExecutor {
        storage_path: dir.path_str().to_owned(),
    };
    let outcome = exec.execute("inbound", None, "candidates", None);
    assert!(outcome.succeeded, "no staged files = zero-record success");
    assert_eq!(outcome.record_count, 0);
    assert!(outcome.error_message.is_none());
}

#[test]
fn test_inbound_counts_staged_import_records() {
    let dir = TempDir::new("tf_sync_test");
    // Write 3 NDJSON records into a staged import file
    std::fs::write(
        dir.path().join("import_candidates_20240101120000.ndjson"),
        "{\"id\":1}\n{\"id\":2}\n{\"id\":3}\n",
    )
    .unwrap();

    let exec = DefaultConnectorExecutor {
        storage_path: dir.path_str().to_owned(),
    };
    let outcome = exec.execute("inbound", None, "candidates", None);
    assert!(outcome.succeeded);
    assert_eq!(outcome.record_count, 3);
}

#[test]
fn test_inbound_sums_records_across_multiple_files() {
    let dir = TempDir::new("tf_sync_test");
    std::fs::write(dir.path().join("import_offers_a.ndjson"), "{}\n{}\n").unwrap();
    std::fs::write(dir.path().join("import_offers_b.ndjson"), "{}\n{}\n{}\n").unwrap();
    // This file should NOT be counted — different entity type
    std::fs::write(
        dir.path().join("import_candidates_a.ndjson"),
        "{}\n{}\n{}\n{}\n",
    )
    .unwrap();

    let exec = DefaultConnectorExecutor {
        storage_path: dir.path_str().to_owned(),
    };
    let outcome = exec.execute("inbound", None, "offers", None);
    assert!(outcome.succeeded);
    assert_eq!(
        outcome.record_count, 5,
        "only 'offers' files should be counted"
    );
}

#[test]
fn test_inbound_skips_empty_lines() {
    let dir = TempDir::new("tf_sync_test");
    std::fs::write(
        dir.path().join("import_users_20240101.ndjson"),
        "{\"id\":1}\n\n   \n{\"id\":2}\n",
    )
    .unwrap();

    let exec = DefaultConnectorExecutor {
        storage_path: dir.path_str().to_owned(),
    };
    let outcome = exec.execute("inbound", None, "users", None);
    assert!(outcome.succeeded);
    assert_eq!(
        outcome.record_count, 2,
        "blank/whitespace lines must not be counted"
    );
}

// ── File fallback — outbound ──────────────────────────────────────────────────

#[test]
fn test_outbound_no_base_url_creates_staging_file_and_succeeds() {
    let dir = TempDir::new("tf_sync_test");
    let exec = DefaultConnectorExecutor {
        storage_path: dir.path_str().to_owned(),
    };
    let outcome = exec.execute("outbound", None, "offers", None);
    assert!(outcome.succeeded, "outbound file fallback must succeed");
    assert_eq!(outcome.record_count, 0);
    assert!(outcome.error_message.is_none());

    // At least one export staging file must have been created
    let files: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("export_offers_")
        })
        .collect();
    assert!(
        !files.is_empty(),
        "an export staging file must exist after outbound fallback"
    );
}

// ── File fallback — bidirectional ─────────────────────────────────────────────

#[test]
fn test_bidirectional_scans_inbound_files() {
    let dir = TempDir::new("tf_sync_test");
    std::fs::write(
        dir.path().join("import_bookings_20240101.ndjson"),
        "{}\n{}\n",
    )
    .unwrap();

    let exec = DefaultConnectorExecutor {
        storage_path: dir.path_str().to_owned(),
    };
    let outcome = exec.execute("bidirectional", None, "bookings", None);
    assert!(outcome.succeeded);
    assert_eq!(outcome.record_count, 2);
}

// ── HTTP failure paths ────────────────────────────────────────────────────────

#[test]
fn test_unreachable_base_url_returns_failed() {
    // Port 1 is reserved and should always be unreachable.
    let exec = DefaultConnectorExecutor {
        storage_path: "/tmp".into(),
    };
    let outcome = exec.execute(
        "inbound",
        Some("http://127.0.0.1:1/api"),
        "candidates",
        None,
    );
    assert!(
        !outcome.succeeded,
        "unreachable URL must produce a failed outcome"
    );
    assert!(
        outcome.error_message.is_some(),
        "error_message must be set on failure"
    );
    assert_eq!(outcome.record_count, 0);
}

#[test]
fn test_non_http_url_returns_failed() {
    let exec = DefaultConnectorExecutor {
        storage_path: "/tmp".into(),
    };
    let outcome = exec.execute(
        "inbound",
        Some("ftp://fileserver.internal/sync"),
        "candidates",
        None,
    );
    assert!(!outcome.succeeded, "non-http URL must return failed");
    let msg = outcome.error_message.as_deref().unwrap_or("");
    assert!(
        msg.contains("http://"),
        "error message must mention the URL scheme requirement, got: {msg}"
    );
}

#[test]
fn test_empty_base_url_falls_back_to_file() {
    // An empty string base_url is treated as "not configured" → file fallback
    let dir = TempDir::new("tf_sync_test");
    let exec = DefaultConnectorExecutor {
        storage_path: dir.path_str().to_owned(),
    };
    let outcome = exec.execute("inbound", Some(""), "offers", None);
    // File fallback: no staged files → success(0)
    assert!(
        outcome.succeeded,
        "empty URL must fall through to file fallback"
    );
    assert_eq!(outcome.record_count, 0);
}

// ── Unknown connector type ────────────────────────────────────────────────────

#[test]
fn test_unknown_connector_type_without_url_returns_failed() {
    let dir = TempDir::new("tf_sync_test");
    let exec = DefaultConnectorExecutor {
        storage_path: dir.path_str().to_owned(),
    };
    let outcome = exec.execute("push_only", None, "candidates", None);
    assert!(
        !outcome.succeeded,
        "unknown connector_type must return failed"
    );
    let msg = outcome.error_message.as_deref().unwrap_or("");
    assert!(
        msg.contains("push_only"),
        "error message must name the unknown type, got: {msg}"
    );
}
