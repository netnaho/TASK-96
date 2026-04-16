/// Unit tests for password complexity validation and hashing.
/// These tests do not require a database connection.

#[cfg(test)]
mod tests {
    use talentflow::infrastructure::crypto::{
        hash_password, validate_password_complexity, verify_password, MIN_PASSWORD_LEN,
    };

    // ── Complexity validation ──────────────────────────────────────────────────

    #[test]
    fn accepts_valid_password() {
        assert!(validate_password_complexity("CorrectH0rse!Battery").is_ok());
        assert!(validate_password_complexity("Tr0ub4dor&3!").is_ok());
        assert!(validate_password_complexity("Passw0rd!Secret$42").is_ok());
    }

    #[test]
    fn rejects_too_short() {
        let errs = validate_password_complexity("Sh0rt!A").unwrap_err();
        assert!(errs
            .iter()
            .any(|e| e.contains(&MIN_PASSWORD_LEN.to_string())));
    }

    #[test]
    fn rejects_missing_uppercase() {
        let errs = validate_password_complexity("nouppercase1!abcd").unwrap_err();
        assert!(errs.iter().any(|e| e.contains("uppercase")));
    }

    #[test]
    fn rejects_missing_lowercase() {
        let errs = validate_password_complexity("NOLOWERCASE1!ABCD").unwrap_err();
        assert!(errs.iter().any(|e| e.contains("lowercase")));
    }

    #[test]
    fn rejects_missing_digit() {
        let errs = validate_password_complexity("NoDigitHere!!!Abc").unwrap_err();
        assert!(errs.iter().any(|e| e.contains("digit")));
    }

    #[test]
    fn rejects_missing_special_char() {
        let errs = validate_password_complexity("NoSpecialChar1Abc").unwrap_err();
        assert!(errs.iter().any(|e| e.contains("special")));
    }

    #[test]
    fn collects_multiple_violations() {
        // Short, no uppercase, no digit, no special
        let errs = validate_password_complexity("abc").unwrap_err();
        assert!(errs.len() >= 3, "expected multiple errors, got: {:?}", errs);
    }

    // ── Hashing ───────────────────────────────────────────────────────────────

    #[test]
    fn hash_and_verify_correct_password() {
        let pw = "CorrectH0rse!Battery";
        let hash = hash_password(pw).expect("hashing should succeed");
        assert!(
            verify_password(pw, &hash).expect("verify should not error"),
            "correct password should verify"
        );
    }

    #[test]
    fn wrong_password_does_not_verify() {
        let hash = hash_password("CorrectH0rse!Battery").unwrap();
        assert!(
            !verify_password("WrongP@ssw0rd123!", &hash).unwrap(),
            "wrong password must not verify"
        );
    }

    #[test]
    fn two_hashes_of_same_password_differ() {
        let pw = "CorrectH0rse!Battery";
        let h1 = hash_password(pw).unwrap();
        let h2 = hash_password(pw).unwrap();
        assert_ne!(h1, h2, "salted hashes must be unique per invocation");
    }
}
