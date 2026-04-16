/// Unit tests for the offline CAPTCHA module.
/// No database required.

#[cfg(test)]
mod tests {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
    use base64::Engine;
    use talentflow::infrastructure::captcha::{
        derive_captcha_key, issue_challenge, validate, CaptchaError, CAPTCHA_TTL_SECS,
    };

    fn key() -> [u8; 32] {
        [0x42u8; 32]
    }

    fn extract_answer(token: &str) -> u32 {
        let parts: Vec<&str> = token.splitn(2, '.').collect();
        let payload = B64.decode(parts[0]).unwrap();
        let s = std::str::from_utf8(&payload).unwrap().to_string();
        let segs: Vec<&str> = s.splitn(3, ':').collect();
        segs[2].parse().unwrap()
    }

    #[test]
    fn valid_challenge_and_answer_accepted() {
        let ch = issue_challenge(&key());
        let answer = extract_answer(&ch.token);
        assert!(validate(&ch.token, answer, &key()).is_ok());
    }

    #[test]
    fn wrong_answer_returns_wrong_answer_error() {
        let ch = issue_challenge(&key());
        let answer = extract_answer(&ch.token);
        assert_eq!(
            validate(&ch.token, answer + 1, &key()),
            Err(CaptchaError::WrongAnswer)
        );
    }

    #[test]
    fn tampered_signature_returns_invalid_token() {
        let ch = issue_challenge(&key());
        let answer = extract_answer(&ch.token);
        // Flip a byte in the signature half
        let mut parts: Vec<String> = ch.token.splitn(2, '.').map(str::to_string).collect();
        let sig_part = parts[1].clone();
        let mut sig_bytes = B64.decode(&sig_part).unwrap();
        sig_bytes[0] ^= 0xff;
        parts[1] = B64.encode(&sig_bytes);
        let bad_token = parts.join(".");
        assert_eq!(
            validate(&bad_token, answer, &key()),
            Err(CaptchaError::InvalidToken)
        );
    }

    #[test]
    fn wrong_key_returns_invalid_token() {
        let ch = issue_challenge(&key());
        let answer = extract_answer(&ch.token);
        let other_key = [0x99u8; 32];
        assert_eq!(
            validate(&ch.token, answer, &other_key),
            Err(CaptchaError::InvalidToken)
        );
    }

    #[test]
    fn malformed_token_returns_invalid_token() {
        assert_eq!(
            validate("notavalidtoken", 42, &key()),
            Err(CaptchaError::InvalidToken)
        );
        assert_eq!(validate("", 0, &key()), Err(CaptchaError::InvalidToken));
    }

    #[test]
    fn derive_captcha_key_from_valid_b64_key() {
        let b64_key = base64::engine::general_purpose::STANDARD.encode([0x42u8; 32]);
        let result = derive_captcha_key(&b64_key);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 32);
    }

    #[test]
    fn derive_captcha_key_fails_on_invalid_b64() {
        assert!(derive_captcha_key("!!!not_base64!!!").is_err());
    }

    #[test]
    fn challenge_expires_in_matches_constant() {
        let ch = issue_challenge(&key());
        assert_eq!(ch.expires_in_seconds, CAPTCHA_TTL_SECS);
    }

    #[test]
    fn challenge_question_contains_plus() {
        let ch = issue_challenge(&key());
        assert!(
            ch.question.contains('+'),
            "question should be an addition problem"
        );
    }
}
