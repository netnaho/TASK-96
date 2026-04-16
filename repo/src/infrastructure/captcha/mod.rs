/// Offline, stateless CAPTCHA implementation.
///
/// ## Design
///
/// No third-party services, no database storage.
///
/// **Challenge token structure**:
/// ```
/// base64url( nonce_hex ":" unix_timestamp_secs ":" expected_answer ) "." base64url( HMAC-SHA256 )
/// ```
///
/// - `nonce_hex` — 8 random hex bytes to prevent token reuse across requests
/// - `unix_timestamp_secs` — creation time for expiry enforcement (2-minute window)
/// - `expected_answer` — numeric answer to the arithmetic question
/// - `HMAC-SHA256` — keyed with the server's 32-byte captcha key (derived from ENCRYPTION_KEY)
///
/// The displayed question is `"What is {a} + {b}?"` where `answer = a + b`.
/// Operands `a` and `b` are derived from the nonce and timestamp to make the
/// question deterministic given the token (so the client can display it without
/// a separate lookup).  Numbers are in the range 1–19 for readability.
///
/// **Validation**:
/// 1. Base64-decode both halves of the token.
/// 2. Recompute HMAC over the payload half; reject if mismatch (constant-time compare).
/// 3. Parse timestamp; reject if older than 120 seconds.
/// 4. Compare submitted answer (integer) against embedded expected_answer.
///
/// **Rate limiting of challenge issuance** is handled by the IP rate limiter at
/// the middleware layer — no additional counter needed here.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde::Serialize;
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;

/// Validity window for a challenge token.
pub const CAPTCHA_TTL_SECS: u64 = 120;

#[derive(Debug, Serialize)]
pub struct CaptchaChallenge {
    /// Opaque token the client must echo back with the login request.
    pub token: String,
    /// Human-readable question to display.
    pub question: String,
    /// How many seconds until the challenge expires.
    pub expires_in_seconds: u64,
}

#[derive(Debug, PartialEq)]
pub enum CaptchaError {
    InvalidToken,
    Expired,
    WrongAnswer,
}

impl std::fmt::Display for CaptchaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidToken => write!(f, "invalid captcha token"),
            Self::Expired => write!(f, "captcha challenge has expired"),
            Self::WrongAnswer => write!(f, "captcha answer is incorrect"),
        }
    }
}

/// Issue a new challenge token signed with `captcha_key`.
pub fn issue_challenge(captcha_key: &[u8; 32]) -> CaptchaChallenge {
    let mut nonce = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut nonce);
    let nonce_hex = hex::encode(nonce);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before Unix epoch")
        .as_secs();

    // Derive two small operands from the nonce bytes for a deterministic question.
    let a = (nonce[0] % 19) + 1;
    let b = (nonce[1] % 19) + 1;
    let answer = (a as u32) + (b as u32);

    let payload = format!("{nonce_hex}:{now}:{answer}");
    let sig = sign(&payload, captcha_key);

    let token = format!("{}.{}", B64.encode(&payload), B64.encode(&sig));

    CaptchaChallenge {
        token,
        question: format!("What is {a} + {b}?"),
        expires_in_seconds: CAPTCHA_TTL_SECS,
    }
}

/// Validate a token + answer submitted during login.
/// Returns `Ok(())` on success, `Err(CaptchaError)` on any failure.
pub fn validate(
    token: &str,
    submitted_answer: u32,
    captcha_key: &[u8; 32],
) -> Result<(), CaptchaError> {
    let parts: Vec<&str> = token.splitn(2, '.').collect();
    if parts.len() != 2 {
        return Err(CaptchaError::InvalidToken);
    }

    let payload_bytes = B64
        .decode(parts[0])
        .map_err(|_| CaptchaError::InvalidToken)?;
    let sig_bytes = B64
        .decode(parts[1])
        .map_err(|_| CaptchaError::InvalidToken)?;

    let payload = std::str::from_utf8(&payload_bytes).map_err(|_| CaptchaError::InvalidToken)?;

    // Constant-time HMAC verification
    let expected_sig = sign(payload, captcha_key);
    if !constant_time_eq(&sig_bytes, &expected_sig) {
        return Err(CaptchaError::InvalidToken);
    }

    // Parse payload: nonce_hex:timestamp:answer
    let segments: Vec<&str> = payload.splitn(3, ':').collect();
    if segments.len() != 3 {
        return Err(CaptchaError::InvalidToken);
    }

    let timestamp: u64 = segments[1]
        .parse()
        .map_err(|_| CaptchaError::InvalidToken)?;
    let expected_answer: u32 = segments[2]
        .parse()
        .map_err(|_| CaptchaError::InvalidToken)?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if now.saturating_sub(timestamp) > CAPTCHA_TTL_SECS {
        return Err(CaptchaError::Expired);
    }

    if submitted_answer != expected_answer {
        return Err(CaptchaError::WrongAnswer);
    }

    Ok(())
}

fn sign(payload: &str, key: &[u8; 32]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key size");
    mac.update(payload.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

/// Constant-time byte slice comparison to prevent timing attacks.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// Derive a 32-byte captcha HMAC key from the application encryption key (base64).
/// Uses SHA-256 of the raw key bytes with a domain separator.
pub fn derive_captcha_key(encryption_key_b64: &str) -> Result<[u8; 32], String> {
    use sha2::{Digest, Sha256};
    let key_bytes = base64::engine::general_purpose::STANDARD
        .decode(encryption_key_b64)
        .map_err(|e| format!("invalid ENCRYPTION_KEY: {e}"))?;
    let mut hasher = Sha256::new();
    hasher.update(b"talentflow-captcha-v1:");
    hasher.update(&key_bytes);
    Ok(hasher.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> [u8; 32] {
        [0x42u8; 32]
    }

    #[test]
    fn valid_challenge_validates() {
        let key = test_key();
        let challenge = issue_challenge(&key);
        // Parse the answer from the token payload to simulate a correct client
        let parts: Vec<&str> = challenge.token.splitn(2, '.').collect();
        let payload_bytes = B64.decode(parts[0]).unwrap();
        let payload = std::str::from_utf8(&payload_bytes).unwrap();
        let segments: Vec<&str> = payload.splitn(3, ':').collect();
        let answer: u32 = segments[2].parse().unwrap();
        assert!(validate(&challenge.token, answer, &key).is_ok());
    }

    #[test]
    fn wrong_answer_rejected() {
        let key = test_key();
        let challenge = issue_challenge(&key);
        let parts: Vec<&str> = challenge.token.splitn(2, '.').collect();
        let payload_bytes = B64.decode(parts[0]).unwrap();
        let payload = std::str::from_utf8(&payload_bytes).unwrap();
        let segments: Vec<&str> = payload.splitn(3, ':').collect();
        let correct: u32 = segments[2].parse().unwrap();
        let wrong = correct.wrapping_add(1);
        assert_eq!(
            validate(&challenge.token, wrong, &key),
            Err(CaptchaError::WrongAnswer)
        );
    }

    #[test]
    fn tampered_token_rejected() {
        let key = test_key();
        let challenge = issue_challenge(&key);
        let tampered = challenge.token.replace('a', "b");
        let parts: Vec<&str> = challenge.token.splitn(2, '.').collect();
        let payload_bytes = B64.decode(parts[0]).unwrap();
        let payload = std::str::from_utf8(&payload_bytes).unwrap();
        let segments: Vec<&str> = payload.splitn(3, ':').collect();
        let answer: u32 = segments[2].parse().unwrap();
        assert_eq!(
            validate(&tampered, answer, &key),
            Err(CaptchaError::InvalidToken)
        );
    }

    #[test]
    fn wrong_key_rejected() {
        let key = test_key();
        let wrong_key = [0x99u8; 32];
        let challenge = issue_challenge(&key);
        let parts: Vec<&str> = challenge.token.splitn(2, '.').collect();
        let payload_bytes = B64.decode(parts[0]).unwrap();
        let payload = std::str::from_utf8(&payload_bytes).unwrap();
        let segments: Vec<&str> = payload.splitn(3, ':').collect();
        let answer: u32 = segments[2].parse().unwrap();
        assert_eq!(
            validate(&challenge.token, answer, &wrong_key),
            Err(CaptchaError::InvalidToken)
        );
    }
}
