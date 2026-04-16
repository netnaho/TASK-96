use tracing_subscriber::{fmt, EnvFilter};

/// Initialise structured JSON logging with env-driven filter (RUST_LOG).
///
/// Log records include:
/// - timestamp
/// - level
/// - target module
/// - span fields (correlation_id, request_id injected by middleware)
/// - message
///
/// Sensitive values must never appear in log messages. Domain services
/// are responsible for redacting before logging.
pub fn init() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    fmt()
        .json()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .init();
}
