/// Idempotency helpers shared across application services.
use actix_web::HttpRequest;
use sha2::{Digest, Sha256};

/// Compute a hex-encoded SHA-256 hash of the request body bytes.
/// Used to detect same-key-different-payload conflicts.
pub fn body_hash(body: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(body);
    hex::encode(hasher.finalize())
}

/// Extract the `Idempotency-Key` header value from a request.
pub fn extract_idempotency_key(req: &HttpRequest) -> Option<String> {
    req.headers()
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

/// Compute a (key, hash) pair from a request.
///
/// - If no `Idempotency-Key` header is present, returns `(None, None)`.
/// - Otherwise returns the key plus the SHA-256 hash of `body_bytes`.
///   Pass `b""` for endpoints with no request body.
pub fn idempotency_info(req: &HttpRequest, body_bytes: &[u8]) -> (Option<String>, Option<String>) {
    let key = extract_idempotency_key(req);
    let hash = key.as_ref().map(|_| body_hash(body_bytes));
    (key, hash)
}
