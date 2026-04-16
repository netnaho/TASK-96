use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use argon2::{
    password_hash::{rand_core::OsRng, SaltString},
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rand::RngCore;

// ============================================================
// Password policy
// ============================================================

/// Minimum accepted password length.
pub const MIN_PASSWORD_LEN: usize = 12;

/// Validate password complexity.
///
/// Rules:
/// - At least 12 characters
/// - At least one uppercase letter (A-Z)
/// - At least one lowercase letter (a-z)
/// - At least one ASCII digit (0-9)
/// - At least one special character (non-alphanumeric ASCII printable)
///
/// Returns `Ok(())` on pass, or `Err(Vec<String>)` listing all violations.
pub fn validate_password_complexity(password: &str) -> Result<(), Vec<String>> {
    let mut errors: Vec<String> = Vec::new();

    if password.len() < MIN_PASSWORD_LEN {
        errors.push(format!(
            "password must be at least {MIN_PASSWORD_LEN} characters"
        ));
    }
    if !password.chars().any(|c| c.is_ascii_uppercase()) {
        errors.push("password must contain at least one uppercase letter".into());
    }
    if !password.chars().any(|c| c.is_ascii_lowercase()) {
        errors.push("password must contain at least one lowercase letter".into());
    }
    if !password.chars().any(|c| c.is_ascii_digit()) {
        errors.push("password must contain at least one digit".into());
    }
    if !password
        .chars()
        .any(|c| c.is_ascii_punctuation() || c == ' ')
    {
        errors.push("password must contain at least one special character (!@#$%^&* etc.)".into());
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

// ============================================================
// Argon2id hashing
// ============================================================

/// Hash a plaintext password using Argon2id with a random salt.
/// Caller must have already validated complexity via `validate_password_complexity`.
pub fn hash_password(password: &str) -> Result<String, argon2::password_hash::Error> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2.hash_password(password.as_bytes(), &salt)?;
    Ok(hash.to_string())
}

/// Verify a plaintext password against a stored Argon2id hash.
/// Returns `Ok(true)` on match, `Ok(false)` on mismatch.
pub fn verify_password(password: &str, hash: &str) -> Result<bool, argon2::password_hash::Error> {
    let parsed = PasswordHash::new(hash)?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

// ============================================================
// Session token
// ============================================================

/// Generate a cryptographically random session token (64-character hex string).
/// The plaintext token is returned to the client; only its hash is stored.
pub fn generate_session_token() -> String {
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

/// Hash a session token for storage (SHA-256, hex-encoded).
/// The stored hash can never be reversed to the original token.
pub fn hash_token(token: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

// ============================================================
// Field-level encryption (AES-256-GCM)
// ============================================================

/// Encrypt arbitrary bytes using AES-256-GCM.
/// Returns raw bytes (12-byte nonce prepended to ciphertext).
pub fn encrypt(plaintext: &[u8], key_b64: &str) -> Result<Vec<u8>, String> {
    let key_bytes = BASE64
        .decode(key_b64)
        .map_err(|e| format!("invalid encryption key: {e}"))?;
    if key_bytes.len() != 32 {
        return Err("encryption key must be exactly 32 bytes".to_string());
    }
    let cipher =
        Aes256Gcm::new_from_slice(&key_bytes).map_err(|e| format!("cipher init error: {e}"))?;
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| format!("encryption error: {e}"))?;
    let mut result = nonce_bytes.to_vec();
    result.extend_from_slice(&ciphertext);
    Ok(result)
}

/// Decrypt bytes previously encrypted with [`encrypt`].
pub fn decrypt(ciphertext: &[u8], key_b64: &str) -> Result<Vec<u8>, String> {
    if ciphertext.len() < 12 {
        return Err("ciphertext too short".to_string());
    }
    let key_bytes = BASE64
        .decode(key_b64)
        .map_err(|e| format!("invalid encryption key: {e}"))?;
    if key_bytes.len() != 32 {
        return Err("encryption key must be exactly 32 bytes".to_string());
    }
    let cipher =
        Aes256Gcm::new_from_slice(&key_bytes).map_err(|e| format!("cipher init error: {e}"))?;
    let nonce = Nonce::from_slice(&ciphertext[..12]);
    cipher
        .decrypt(nonce, &ciphertext[12..])
        .map_err(|e| format!("decryption error: {e}"))
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_hash_and_verify_round_trip() {
        let pw = "CorrectH0rse!Battery";
        let hash = hash_password(pw).unwrap();
        assert!(verify_password(pw, &hash).unwrap());
        assert!(!verify_password("wrongpassword", &hash).unwrap());
    }

    #[test]
    fn password_too_short_rejected() {
        let errs = validate_password_complexity("Aa1!xyz").unwrap_err();
        assert!(errs.iter().any(|e| e.contains("12 characters")));
    }

    #[test]
    fn password_no_uppercase_rejected() {
        let errs = validate_password_complexity("nouppercase1!abc").unwrap_err();
        assert!(errs.iter().any(|e| e.contains("uppercase")));
    }

    #[test]
    fn password_no_digit_rejected() {
        let errs = validate_password_complexity("NoDigitHere!!!A").unwrap_err();
        assert!(errs.iter().any(|e| e.contains("digit")));
    }

    #[test]
    fn password_no_special_rejected() {
        let errs = validate_password_complexity("NoSpecialChar1A").unwrap_err();
        assert!(errs.iter().any(|e| e.contains("special")));
    }

    #[test]
    fn valid_password_accepted() {
        assert!(validate_password_complexity("CorrectH0rse!Battery").is_ok());
    }

    #[test]
    fn token_hash_is_deterministic() {
        let token = generate_session_token();
        let h1 = hash_token(&token);
        let h2 = hash_token(&token);
        assert_eq!(h1, h2);
    }

    #[test]
    fn different_tokens_produce_different_hashes() {
        let h1 = hash_token(&generate_session_token());
        let h2 = hash_token(&generate_session_token());
        assert_ne!(h1, h2);
    }

    #[test]
    fn encrypt_decrypt_round_trip() {
        // 32-byte key, base64-encoded
        let key = BASE64.encode([0x42u8; 32]);
        let plaintext = b"sensitive data";
        let ciphertext = encrypt(plaintext, &key).unwrap();
        let decrypted = decrypt(&ciphertext, &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn decrypt_with_wrong_key_fails() {
        let key1 = BASE64.encode([0x42u8; 32]);
        let key2 = BASE64.encode([0x99u8; 32]);
        let ciphertext = encrypt(b"secret", &key1).unwrap();
        assert!(decrypt(&ciphertext, &key2).is_err());
    }
}
