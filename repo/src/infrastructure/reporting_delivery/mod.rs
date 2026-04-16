/// Local delivery gateway abstraction for reporting alerts.
///
/// ## Design principles
///
/// - **Local-network-only**: gateways are reached via `http://host:port/path`.
///   No TLS, no external services, no third-party dependencies.
/// - **Best-effort**: delivery errors are logged and recorded in the alert's
///   `delivery_meta` field.  They never block snapshot processing or bubble up
///   as application errors.
/// - **Opt-in**: all gateways default to no-op (Skipped) when not configured.
///   `REPORTING_DELIVERY_ENABLED=false` (the default) short-circuits every call.
///
/// ## Delivery protocol
///
/// Each adapter performs a single blocking HTTP/1.0 POST to its configured URL.
/// The request body is a JSON object with the alert fields.  A 2xx response is
/// treated as success; anything else (connection failure, timeout, non-2xx) is
/// recorded as an error outcome.  Timeouts are 2 seconds (connect + read).
///
/// ## Adding a new adapter
///
/// Implement `DeliveryGateway` for a new struct:
///
/// ```rust
/// struct MyAdapter { ... }
/// impl DeliveryGateway for MyAdapter {
///     fn deliver(&self, payload: &AlertPayload) -> DeliveryOutcome { ... }
/// }
/// ```
///
/// Then include it in the `CompositeDeliveryGateway` built by `build_gateway`.
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use tracing::{debug, warn};
use uuid::Uuid;

use crate::infrastructure::config::ReportingDeliveryConfig;

// ============================================================
// Core types
// ============================================================

/// Outcome of a single delivery attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryOutcome {
    /// Message was accepted by the gateway (HTTP 2xx).
    Delivered,
    /// No gateway configured or delivery is disabled — intentional no-op.
    Skipped,
    /// Attempt was made but failed (connection error, timeout, non-2xx, etc.).
    Error(String),
}

impl DeliveryOutcome {
    pub fn as_str(&self) -> &str {
        match self {
            DeliveryOutcome::Delivered => "delivered",
            DeliveryOutcome::Skipped => "skipped",
            DeliveryOutcome::Error(_) => "error",
        }
    }

    /// Serialise to a JSON `Value` suitable for storing in `delivery_meta`.
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            DeliveryOutcome::Error(msg) => {
                serde_json::json!({ "status": "error", "detail": msg })
            }
            other => serde_json::json!({ "status": other.as_str() }),
        }
    }
}

/// Payload passed to every delivery adapter.
#[derive(Debug, Clone)]
pub struct AlertPayload {
    pub alert_id: Uuid,
    pub subscription_id: Uuid,
    pub severity: String,
    pub message: String,
}

/// Trait for a local delivery channel.
///
/// Implementations must be synchronous (the snapshot job runs on a blocking
/// thread) and must never panic — return `DeliveryOutcome::Error(_)` on failure.
pub trait DeliveryGateway: Send + Sync {
    fn deliver(&self, payload: &AlertPayload) -> DeliveryOutcome;
    /// Human-readable name used in log messages.
    fn name(&self) -> &str;
}

// ============================================================
// Local email gateway adapter
// ============================================================

/// Delivers alerts to a local email gateway via HTTP POST.
///
/// The gateway is expected to be a simple local MTA shim (e.g. a sidecar that
/// converts the JSON payload to an SMTP message).  No SMTP client is bundled.
///
/// Configure via `REPORTING_EMAIL_GATEWAY_URL=http://localhost:8025/send`.
pub struct LocalEmailGatewayAdapter {
    gateway_url: Option<String>,
}

impl LocalEmailGatewayAdapter {
    pub fn new(gateway_url: Option<String>) -> Self {
        Self { gateway_url }
    }
}

impl DeliveryGateway for LocalEmailGatewayAdapter {
    fn name(&self) -> &str {
        "local-email"
    }

    fn deliver(&self, payload: &AlertPayload) -> DeliveryOutcome {
        let url = match &self.gateway_url {
            Some(u) => u.clone(),
            None => return DeliveryOutcome::Skipped,
        };

        let body = serde_json::json!({
            "alert_id": payload.alert_id,
            "subscription_id": payload.subscription_id,
            "severity": payload.severity,
            "message": payload.message,
            "channel": "email",
        })
        .to_string();

        match post_local_http(&url, &body) {
            Ok(()) => {
                debug!(alert_id = %payload.alert_id, "email delivery succeeded");
                DeliveryOutcome::Delivered
            }
            Err(e) => {
                warn!(alert_id = %payload.alert_id, error = %e, "email delivery failed");
                DeliveryOutcome::Error(e)
            }
        }
    }
}

// ============================================================
// Local IM gateway adapter
// ============================================================

/// Delivers alerts to a local instant-messaging gateway via HTTP POST.
///
/// The gateway is expected to be a local webhook bridge (e.g. a Mattermost or
/// Slack-compatible incoming webhook running on the private network).
///
/// Configure via `REPORTING_IM_GATEWAY_URL=http://localhost:9090/hooks/alerts`.
pub struct LocalImGatewayAdapter {
    gateway_url: Option<String>,
}

impl LocalImGatewayAdapter {
    pub fn new(gateway_url: Option<String>) -> Self {
        Self { gateway_url }
    }
}

impl DeliveryGateway for LocalImGatewayAdapter {
    fn name(&self) -> &str {
        "local-im"
    }

    fn deliver(&self, payload: &AlertPayload) -> DeliveryOutcome {
        let url = match &self.gateway_url {
            Some(u) => u.clone(),
            None => return DeliveryOutcome::Skipped,
        };

        let body = serde_json::json!({
            "alert_id": payload.alert_id,
            "subscription_id": payload.subscription_id,
            "severity": payload.severity,
            "text": format!("[{}] {}", payload.severity.to_uppercase(), payload.message),
            "channel": "im",
        })
        .to_string();

        match post_local_http(&url, &body) {
            Ok(()) => {
                debug!(alert_id = %payload.alert_id, "IM delivery succeeded");
                DeliveryOutcome::Delivered
            }
            Err(e) => {
                warn!(alert_id = %payload.alert_id, error = %e, "IM delivery failed");
                DeliveryOutcome::Error(e)
            }
        }
    }
}

// ============================================================
// Composite gateway
// ============================================================

/// Runs all configured adapters for a single alert and aggregates outcomes.
///
/// Built by `build_gateway` from the application config.  Each adapter is
/// attempted independently — a failure in one does not prevent others from
/// running.
pub struct CompositeDeliveryGateway {
    adapters: Vec<Box<dyn DeliveryGateway>>,
}

impl CompositeDeliveryGateway {
    pub fn new(adapters: Vec<Box<dyn DeliveryGateway>>) -> Self {
        Self { adapters }
    }

    /// Run all adapters and return per-adapter outcomes as a JSON object.
    pub fn deliver_all(&self, payload: &AlertPayload) -> serde_json::Value {
        let mut outcomes = serde_json::Map::new();
        for adapter in &self.adapters {
            let outcome = adapter.deliver(payload);
            outcomes.insert(adapter.name().to_string(), outcome.to_json());
        }
        serde_json::Value::Object(outcomes)
    }
}

/// Build a `CompositeDeliveryGateway` from application config.
///
/// When `config.enabled = false`, both adapters are constructed with `None`
/// URLs, ensuring they always return `Skipped`.
pub fn build_gateway(config: &ReportingDeliveryConfig) -> CompositeDeliveryGateway {
    let (email_url, im_url) = if config.enabled {
        (
            config.email_gateway_url.clone(),
            config.im_gateway_url.clone(),
        )
    } else {
        (None, None)
    };

    CompositeDeliveryGateway::new(vec![
        Box::new(LocalEmailGatewayAdapter::new(email_url)),
        Box::new(LocalImGatewayAdapter::new(im_url)),
    ])
}

// ============================================================
// Minimal stdlib HTTP/1.0 POST (no external dependencies)
// ============================================================

/// Send an HTTP/1.0 POST request to a local gateway URL with a JSON body.
///
/// Only `http://` URLs targeting localhost or RFC 1918 private addresses are
/// allowed.  Public hosts are rejected before any connection is attempted.
/// Timeouts are 2 seconds for connect and 2 seconds for read.
/// Returns `Ok(())` on a 2xx response and `Err(description)` otherwise.
pub(crate) fn post_local_http(url: &str, body: &str) -> Result<(), String> {
    // Enforce local-network-only host boundary
    crate::shared::network::validate_local_url(url)?;

    // Parse URL: only http:// is supported
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("only http:// URLs are supported, got: {url}"))?;

    // Split into host:port and path
    let (host_port, path) = rest.split_once('/').unwrap_or((rest, ""));
    let path = format!("/{path}");

    // Resolve host and port
    let (host, port) = if let Some(colon_pos) = host_port.rfind(':') {
        let host = &host_port[..colon_pos];
        let port: u16 = host_port[colon_pos + 1..]
            .parse()
            .map_err(|e| format!("invalid port in URL: {e}"))?;
        (host, port)
    } else {
        (host_port, 80u16)
    };

    // Connect (2-second timeout)
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

    // Write HTTP/1.0 POST request
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

    // Read status line only (e.g. "HTTP/1.0 200 OK")
    let mut status_buf = [0u8; 16];
    let n = stream.read(&mut status_buf).unwrap_or(0);
    let status_line = std::str::from_utf8(&status_buf[..n]).unwrap_or("");

    // Extract status code: "HTTP/1.x DDD ..."
    let code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);

    if (200..300).contains(&code) {
        Ok(())
    } else {
        Err(format!(
            "gateway returned HTTP {code} ({})",
            status_line.trim()
        ))
    }
}
