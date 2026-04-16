/// Unit tests for seed password resolution logic.
/// No database required.

#[cfg(test)]
mod tests {
    use talentflow::infrastructure::config::resolve_seed_password;

    #[test]
    fn explicit_password_is_used_as_is() {
        let result = resolve_seed_password(Some("MyExplicit!Pass123"));
        assert_eq!(result.value, "MyExplicit!Pass123");
        assert!(!result.was_generated);
    }

    #[test]
    fn none_generates_random_password() {
        let result = resolve_seed_password(None);
        assert!(result.was_generated);
        assert!(
            result.value.starts_with("Seed!"),
            "generated password must start with Seed! prefix, got: {}",
            result.value
        );
        // Seed! (5 chars) + 24 hex chars = 29 chars total
        assert_eq!(result.value.len(), 29, "generated password length");
    }

    #[test]
    fn empty_string_generates_random_password() {
        let result = resolve_seed_password(Some(""));
        assert!(result.was_generated);
        assert!(result.value.starts_with("Seed!"));
    }

    #[test]
    fn generated_passwords_are_unique() {
        let a = resolve_seed_password(None);
        let b = resolve_seed_password(None);
        assert_ne!(
            a.value, b.value,
            "two generated passwords must be different"
        );
    }

    #[test]
    fn generated_password_meets_complexity() {
        // Must have uppercase, lowercase, digit, and special char
        let result = resolve_seed_password(None);
        let pw = &result.value;
        assert!(pw.len() >= 12, "password too short: {}", pw.len());
        assert!(pw.chars().any(|c| c.is_uppercase()), "needs uppercase");
        assert!(pw.chars().any(|c| c.is_lowercase()), "needs lowercase");
        assert!(
            pw.chars().any(|c| !c.is_alphanumeric()),
            "needs special char"
        );
    }
}
