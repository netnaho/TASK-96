/// Unit tests for AppError HTTP response mapping.
/// No database required.
#[cfg(test)]
mod tests {
    use actix_web::ResponseError;
    use talentflow::shared::errors::{AppError, FieldError};

    // ── Status code mapping ───────────────────────────────────────────────────

    #[test]
    fn validation_error_produces_422() {
        let err = AppError::Validation(vec![]);
        let resp = err.error_response();
        assert_eq!(resp.status().as_u16(), 422);
    }

    #[test]
    fn authentication_required_produces_401() {
        let err = AppError::AuthenticationRequired;
        let resp = err.error_response();
        assert_eq!(resp.status().as_u16(), 401);
    }

    #[test]
    fn forbidden_produces_403() {
        let err = AppError::Forbidden;
        let resp = err.error_response();
        assert_eq!(resp.status().as_u16(), 403);
    }

    #[test]
    fn not_found_produces_404() {
        let err = AppError::NotFound("widget".into());
        let resp = err.error_response();
        assert_eq!(resp.status().as_u16(), 404);
    }

    #[test]
    fn conflict_produces_409() {
        let err = AppError::Conflict("duplicate email".into());
        let resp = err.error_response();
        assert_eq!(resp.status().as_u16(), 409);
    }

    #[test]
    fn idempotency_conflict_produces_409() {
        let err = AppError::IdempotencyConflict;
        let resp = err.error_response();
        assert_eq!(resp.status().as_u16(), 409);
    }

    #[test]
    fn invalid_state_transition_produces_409() {
        let err = AppError::InvalidStateTransition("cannot confirm from cancelled".into());
        let resp = err.error_response();
        assert_eq!(resp.status().as_u16(), 409);
    }

    #[test]
    fn internal_error_produces_500() {
        let err = AppError::Internal("database unavailable".into());
        let resp = err.error_response();
        assert_eq!(resp.status().as_u16(), 500);
    }

    #[test]
    fn rate_limited_produces_429() {
        let err = AppError::RateLimited;
        let resp = err.error_response();
        assert_eq!(resp.status().as_u16(), 429);
    }

    // ── Error code strings ────────────────────────────────────────────────────

    fn body_json(err: &AppError) -> serde_json::Value {
        let resp = err.error_response();
        // `actix_web::body::to_bytes` is async; drive it with a one-shot tokio runtime.
        // tokio is a regular (non-dev) dependency with full features.
        let bytes = tokio::runtime::Runtime::new()
            .expect("tokio runtime")
            .block_on(actix_web::body::to_bytes(resp.into_body()))
            .expect("failed to read body");
        serde_json::from_slice(&bytes).expect("body is not valid JSON")
    }

    #[test]
    fn validation_error_code_string() {
        let json = body_json(&AppError::Validation(vec![]));
        assert_eq!(json["error"]["code"], "validation_error");
    }

    #[test]
    fn authentication_required_code_string() {
        let json = body_json(&AppError::AuthenticationRequired);
        assert_eq!(json["error"]["code"], "authentication_required");
    }

    #[test]
    fn forbidden_code_string() {
        let json = body_json(&AppError::Forbidden);
        assert_eq!(json["error"]["code"], "forbidden");
    }

    #[test]
    fn not_found_code_string() {
        let json = body_json(&AppError::NotFound("x".into()));
        assert_eq!(json["error"]["code"], "not_found");
    }

    #[test]
    fn conflict_code_string() {
        let json = body_json(&AppError::Conflict("x".into()));
        assert_eq!(json["error"]["code"], "conflict");
    }

    #[test]
    fn idempotency_conflict_code_string() {
        let json = body_json(&AppError::IdempotencyConflict);
        assert_eq!(json["error"]["code"], "idempotency_conflict");
    }

    #[test]
    fn invalid_state_transition_code_string() {
        let json = body_json(&AppError::InvalidStateTransition("x".into()));
        assert_eq!(json["error"]["code"], "invalid_state_transition");
    }

    #[test]
    fn internal_error_code_string() {
        let json = body_json(&AppError::Internal("x".into()));
        assert_eq!(json["error"]["code"], "internal_error");
    }

    #[test]
    fn rate_limited_code_string() {
        let json = body_json(&AppError::RateLimited);
        assert_eq!(json["error"]["code"], "rate_limited");
    }

    // ── Validation details payload ────────────────────────────────────────────

    #[test]
    fn validation_error_details_contains_field_errors() {
        let fields = vec![
            FieldError {
                field: "email".into(),
                message: "must be a valid email".into(),
            },
            FieldError {
                field: "first_name".into(),
                message: "must not be blank".into(),
            },
        ];
        let err = AppError::Validation(fields);
        let json = body_json(&err);

        let details = &json["error"]["details"];
        assert!(details.is_array(), "details should be an array");
        let arr = details.as_array().unwrap();
        assert_eq!(arr.len(), 2, "should have two field errors");
        assert_eq!(arr[0]["field"], "email");
        assert_eq!(arr[0]["message"], "must be a valid email");
        assert_eq!(arr[1]["field"], "first_name");
        assert_eq!(arr[1]["message"], "must not be blank");
    }

    #[test]
    fn non_validation_errors_omit_details_key() {
        // details should be absent (skipped via skip_serializing_if)
        let json = body_json(&AppError::Forbidden);
        assert!(
            json["error"].get("details").is_none(),
            "non-validation errors must not include a details field"
        );
    }

    // ── From<diesel::result::Error> ───────────────────────────────────────────

    #[test]
    fn diesel_not_found_converts_to_internal_error() {
        let diesel_err = diesel::result::Error::NotFound;
        let app_err: AppError = diesel_err.into();
        assert!(
            matches!(app_err, AppError::Internal(_)),
            "diesel errors should become AppError::Internal"
        );
    }

    #[test]
    fn diesel_error_message_is_preserved() {
        let diesel_err =
            diesel::result::Error::RollbackTransaction;
        let app_err: AppError = diesel_err.into();
        let msg = app_err.to_string();
        assert!(
            msg.contains("internal error"),
            "Internal error message should start with 'internal error'; got: {msg}"
        );
    }
}
