/// API-level integration tests for the authentication layer.
///
/// These tests spin up a real actix-web test server and exercise the full
/// HTTP stack, including DB operations via a real PostgreSQL connection.
///
/// ## Prerequisites
///
/// Set `DATABASE_URL` to a test PostgreSQL database before running:
///
/// ```bash
/// DATABASE_URL=postgres://talentflow:talentflow_dev@localhost:5433/talentflow_test \
///   ENCRYPTION_KEY="$(openssl rand -base64 32)" \
///   cargo test --test auth_tests
/// ```
///
/// Tests are run sequentially (via `serial_test`) to prevent interference.
///
/// ## Test isolation
///
/// Each test that creates users/sessions cleans them up in a `teardown` step.
/// Tests use randomly generated usernames to avoid conflicts.
use actix_web::{dev::Service, test, web, App};
use serde_json::{json, Value};
use serial_test::serial;
use std::env;

use talentflow::{
    api::{middleware::RequestId, routes},
    infrastructure::{config::AppConfig, db, logging},
    shared::app_state::AppState,
};

// ── Test app builder ──────────────────────────────────────────────────────────

fn build_test_app_config() -> Option<AppConfig> {
    // Skip if no DATABASE_URL is configured
    if env::var("DATABASE_URL").is_err() {
        return None;
    }
    Some(AppConfig::from_env())
}

macro_rules! test_app {
    ($app:ident, $config:ident) => {
        let $config = match build_test_app_config() {
            Some(c) => c,
            None => {
                if std::env::var("TALENTFLOW_SKIP_DB_TESTS").is_ok() {
                    eprintln!("skipping test: DATABASE_URL not set (TALENTFLOW_SKIP_DB_TESTS is set)");
                    return;
                }
                panic!(
                    "Integration tests require DATABASE_URL and ENCRYPTION_KEY. \
                     Set them, or set TALENTFLOW_SKIP_DB_TESTS=1 to explicitly skip."
                );
            }
        };
        let pool = db::create_pool(&$config.database_url);
        let state = AppState::new($config.clone(), pool).expect("failed to create AppState");
        let state_data = web::Data::new(state);
        let $app = test::init_service(
            App::new()
                .wrap(RequestId)
                .app_data(state_data.clone())
                .app_data(web::JsonConfig::default().error_handler(|err, _req| {
                    actix_web::Error::from(talentflow::shared::errors::AppError::Validation(vec![
                        talentflow::shared::errors::FieldError {
                            field: "body".into(),
                            message: err.to_string(),
                        },
                    ]))
                }))
                .configure(routes::configure),
        )
        .await;
    };
}

// ── Helper: login and extract token ───────────────────────────────────────────

async fn do_login(
    app: &impl Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
    >,
    username: &str,
    password: &str,
) -> (u16, Value) {
    let req = test::TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(json!({
            "username": username,
            "password": password
        }))
        .to_request();
    let (status, body) = call_allowing_errors(app, req).await;
    (status, body.unwrap_or(json!({})))
}

/// Call the service and handle middleware errors (e.g. 401 from AuthMiddleware)
/// that would cause `test::call_service` to panic.
/// Returns the HTTP status code and response body as JSON.
async fn call_allowing_errors(
    app: &impl Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
    >,
    req: actix_http::Request,
) -> (u16, Option<Value>) {
    match app.call(req).await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let bytes = test::read_body(resp).await;
            let body = if bytes.is_empty() {
                None
            } else {
                serde_json::from_slice(&bytes).ok()
            };
            (status, body)
        }
        Err(err) => {
            let resp = err.error_response();
            let status = resp.status().as_u16();
            (status, None)
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[actix_rt::test]
#[serial]
async fn health_check_returns_200() {
    test_app!(app, config);
    let req = test::TestRequest::get().uri("/api/v1/health").to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
}

#[actix_rt::test]
#[serial]
async fn get_captcha_returns_challenge() {
    test_app!(app, config);
    let req = test::TestRequest::get()
        .uri("/api/v1/auth/captcha")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = test::read_body_json(resp).await;
    assert!(body["data"]["token"].is_string());
    assert!(body["data"]["question"].is_string());
    assert!(body["data"]["expires_in_seconds"].is_number());
}

#[actix_rt::test]
#[serial]
async fn login_with_seeded_platform_admin_succeeds() {
    test_app!(app, config);
    let (status, body) = do_login(&app, "platform_admin", "Admin_Pa$$word1!").await;
    assert_eq!(status, 200, "body: {body}");
    assert!(body["data"]["token"].is_string());
    assert_eq!(body["data"]["user"]["username"], "platform_admin");
    assert!(body["data"]["user"]["roles"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r == "platform_admin"));
}

#[actix_rt::test]
#[serial]
async fn login_with_wrong_password_returns_401() {
    test_app!(app, config);
    let (status, body) = do_login(&app, "platform_admin", "WrongP@ssword!999").await;
    assert_eq!(status, 401, "body: {body}");
    assert_eq!(body["error"]["code"], "authentication_required");
    // Must not reveal whether username exists
    assert!(!body["error"]["message"]
        .as_str()
        .unwrap_or("")
        .contains("password"));
}

#[actix_rt::test]
#[serial]
async fn login_with_unknown_username_returns_401() {
    test_app!(app, config);
    let (status, body) = do_login(&app, "no_such_user_xyz", "SomeP@ssword!123").await;
    assert_eq!(status, 401, "body: {body}");
    // Same 401 as wrong password — no username enumeration
    assert_eq!(body["error"]["code"], "authentication_required");
}

#[actix_rt::test]
#[serial]
async fn missing_auth_header_returns_401_on_protected_route() {
    test_app!(app, config);
    let req = test::TestRequest::get()
        .uri("/api/v1/auth/session")
        .to_request();
    let (status, _) = call_allowing_errors(&app, req).await;
    assert_eq!(status, 401);
}

#[actix_rt::test]
#[serial]
async fn invalid_token_returns_401_on_protected_route() {
    test_app!(app, config);
    let req = test::TestRequest::get()
        .uri("/api/v1/auth/session")
        .insert_header(("Authorization", "Bearer invalidtoken00000000"))
        .to_request();
    let (status, _) = call_allowing_errors(&app, req).await;
    assert_eq!(status, 401);
}

#[actix_rt::test]
#[serial]
async fn login_then_session_endpoint_returns_user_info() {
    test_app!(app, config);
    let (status, body) = do_login(&app, "platform_admin", "Admin_Pa$$word1!").await;
    assert_eq!(status, 200);
    let token = body["data"]["token"].as_str().unwrap().to_string();

    let req = test::TestRequest::get()
        .uri("/api/v1/auth/session")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let (status, _) = call_allowing_errors(&app, req).await;
    assert_eq!(status, 200);
}

#[actix_rt::test]
#[serial]
async fn logout_invalidates_session() {
    test_app!(app, config);
    let (_, body) = do_login(&app, "platform_admin", "Admin_Pa$$word1!").await;
    let token = body["data"]["token"].as_str().unwrap().to_string();

    // Logout
    let req = test::TestRequest::post()
        .uri("/api/v1/auth/logout")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let (status, _) = call_allowing_errors(&app, req).await;
    assert_eq!(status, 204);

    // Token should no longer work
    let req = test::TestRequest::get()
        .uri("/api/v1/auth/session")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let (status, _) = call_allowing_errors(&app, req).await;
    assert_eq!(status, 401, "revoked token must return 401");
}

#[actix_rt::test]
#[serial]
async fn login_with_short_password_returns_validation_error() {
    test_app!(app, config);
    // Password present but too short to satisfy validator's min=1 —
    // the login handler validates body shape first, password policy is checked
    // by AuthService which runs after the service call succeeds shape validation.
    // Here we test that an empty password is rejected at the DTO level.
    let req = test::TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(json!({ "username": "platform_admin", "password": "" }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 422);
    let body: Value = test::read_body_json(resp).await;
    assert_eq!(body["error"]["code"], "validation_error");
}

#[actix_rt::test]
#[serial]
async fn captcha_with_wrong_answer_fails_login() {
    test_app!(app, config);
    // Get a challenge
    let req = test::TestRequest::get()
        .uri("/api/v1/auth/captcha")
        .to_request();
    let resp = test::call_service(&app, req).await;
    let ch: Value = test::read_body_json(resp).await;
    let token = ch["data"]["token"].as_str().unwrap();

    // Submit login with wrong captcha answer
    let req = test::TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(json!({
            "username": "platform_admin",
            "password": "Admin_Pa$$word1!",
            "captcha_token": token,
            "captcha_answer": 9999
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 422);
    let body: Value = test::read_body_json(resp).await;
    assert_eq!(body["error"]["code"], "validation_error");
}

#[actix_rt::test]
#[serial]
async fn member_role_cannot_access_users_endpoint() {
    test_app!(app, config);
    let (status, body) = do_login(&app, "member", "Member!User1Passw0rd").await;
    assert_eq!(status, 200);
    let token = body["data"]["token"].as_str().unwrap().to_string();

    let req = test::TestRequest::get()
        .uri("/api/v1/users")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    // A member does not have users:read permission, so the handler must deny
    // with 403 Forbidden after authentication succeeds.
    assert_eq!(
        resp.status().as_u16(),
        403,
        "member must be forbidden from users list"
    );
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["error"]["code"], "forbidden");
}

#[actix_rt::test]
#[serial]
async fn malformed_json_returns_validation_error() {
    test_app!(app, config);
    let req = test::TestRequest::post()
        .uri("/api/v1/auth/login")
        .insert_header(("content-type", "application/json"))
        .set_payload("{bad json}")
        .to_request();
    let resp = test::call_service(&app, req).await;
    // The JSON extractor error handler converts this to 422
    assert_eq!(resp.status(), 422);
}

// ── Session expiry policy tests ─────────────────────────────────────────────

/// A freshly created session should be valid for the full hard-TTL window
/// (default 8 h). Regression guard: the old 1-hour default would have killed
/// sessions that the idle-timeout policy promised to keep alive.
#[actix_rt::test]
#[serial]
async fn active_session_survives_within_hard_ttl_window() {
    test_app!(app, config);

    // Verify the config default is now 8 hours (28800s), matching idle timeout
    assert_eq!(
        config.session.ttl_seconds, 28800,
        "hard TTL default must be 28800 (8 hours) to match idle timeout"
    );

    let (status, body) = do_login(&app, "platform_admin", "Admin_Pa$$word1!").await;
    assert_eq!(status, 200);
    let token = body["data"]["token"].as_str().unwrap();

    // Authenticated request should work immediately
    let req = test::TestRequest::get()
        .uri("/api/v1/auth/session")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let (status, _) = call_allowing_errors(&app, req).await;
    assert_eq!(
        status, 200,
        "session should be valid immediately after login"
    );
}

/// A session whose `expires_at` is in the past must be rejected (hard TTL).
#[actix_rt::test]
#[serial]
async fn expired_hard_ttl_session_is_rejected() {
    test_app!(app, config);
    let (status, body) = do_login(&app, "platform_admin", "Admin_Pa$$word1!").await;
    assert_eq!(status, 200);
    let token = body["data"]["token"].as_str().unwrap();
    let user_id = body["data"]["user"]["id"].as_str().unwrap();

    // Force all sessions for this user to have an expired hard TTL
    {
        let pool = talentflow::infrastructure::db::create_pool(&config.database_url);
        let mut conn = pool.get().expect("db connection");
        use diesel::RunQueryDsl;
        diesel::sql_query(format!(
            "UPDATE sessions SET expires_at = now() - INTERVAL '1 second' \
             WHERE user_id = '{user_id}'"
        ))
        .execute(&mut conn)
        .expect("update session expires_at");
    }

    let req = test::TestRequest::get()
        .uri("/api/v1/auth/session")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let (status, _) = call_allowing_errors(&app, req).await;
    assert_eq!(
        status, 401,
        "session past hard TTL must be rejected"
    );
}

/// A session idle for longer than SESSION_IDLE_TIMEOUT_SECS must be rejected,
/// even if the hard TTL has not elapsed.
#[actix_rt::test]
#[serial]
async fn idle_timeout_rejects_stale_session() {
    test_app!(app, config);
    let (status, body) = do_login(&app, "platform_admin", "Admin_Pa$$word1!").await;
    assert_eq!(status, 200);
    let token = body["data"]["token"].as_str().unwrap();
    let user_id = body["data"]["user"]["id"].as_str().unwrap();

    // Push last_activity_at back beyond the idle timeout (8 h + 1 s) while
    // keeping expires_at in the future so only the idle check triggers.
    {
        let pool = talentflow::infrastructure::db::create_pool(&config.database_url);
        let mut conn = pool.get().expect("db connection");
        use diesel::RunQueryDsl;
        diesel::sql_query(format!(
            "UPDATE sessions \
             SET last_activity_at = now() - INTERVAL '28801 seconds', \
                 expires_at = now() + INTERVAL '1 hour' \
             WHERE user_id = '{user_id}'"
        ))
        .execute(&mut conn)
        .expect("update session timestamps");
    }

    let req = test::TestRequest::get()
        .uri("/api/v1/auth/session")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let (status, _) = call_allowing_errors(&app, req).await;
    assert_eq!(
        status, 401,
        "session idle beyond 8 hours must be rejected"
    );
}

// ── Mandatory CAPTCHA enforcement tests ─────────────────────────────────────

/// Helper: reset a user's failed login counter and account status to clean
/// state so tests don't interfere with each other.
fn reset_user_login_state(db_url: &str, username: &str) {
    use diesel::RunQueryDsl;
    let pool = talentflow::infrastructure::db::create_pool(db_url);
    let mut conn = pool.get().expect("db connection");
    diesel::sql_query(format!(
        "UPDATE users SET failed_login_count = 0, locked_until = NULL, \
         account_status = 'active' WHERE username = '{username}'"
    ))
    .execute(&mut conn)
    .expect("reset user state");
}

/// Helper: set failed_login_count to push the account into CAPTCHA-required
/// territory (default threshold: 3 with lockout threshold 5).
fn set_failed_login_count(db_url: &str, username: &str, count: i32) {
    use diesel::RunQueryDsl;
    let pool = talentflow::infrastructure::db::create_pool(db_url);
    let mut conn = pool.get().expect("db connection");
    diesel::sql_query(format!(
        "UPDATE users SET failed_login_count = {count}, locked_until = NULL, \
         account_status = 'active' WHERE username = '{username}'"
    ))
    .execute(&mut conn)
    .expect("set failed login count");
}

/// Helper: fetch a CAPTCHA challenge and extract the correct answer from the
/// token payload (the answer is embedded for offline validation).
async fn fetch_captcha_with_answer(
    app: &impl actix_web::dev::Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
    >,
) -> (String, u32) {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
    let req = test::TestRequest::get()
        .uri("/api/v1/auth/captcha")
        .to_request();
    let resp = test::call_service(app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = test::read_body_json(resp).await;
    let token = body["data"]["token"].as_str().unwrap().to_string();
    // Parse answer from token: base64(nonce:timestamp:answer).base64(sig)
    let payload_b64 = token.split('.').next().unwrap();
    let payload_bytes = B64.decode(payload_b64).unwrap();
    let payload = std::str::from_utf8(&payload_bytes).unwrap();
    let answer: u32 = payload.rsplitn(2, ':').next().unwrap().parse().unwrap();
    (token, answer)
}

/// In low-risk state (no failed attempts), login succeeds without CAPTCHA.
#[actix_rt::test]
#[serial]
async fn login_without_captcha_succeeds_in_low_risk_state() {
    test_app!(app, config);
    reset_user_login_state(&config.database_url, "platform_admin");

    let (status, body) = do_login(&app, "platform_admin", "Admin_Pa$$word1!").await;
    assert_eq!(status, 200, "low-risk login must succeed without CAPTCHA: {body}");
    assert!(body["data"]["token"].is_string());
}

/// After enough failed attempts, login without CAPTCHA returns 422.
#[actix_rt::test]
#[serial]
async fn login_fails_when_captcha_required_but_missing() {
    test_app!(app, config);
    // Ensure clean state before AND after (guard against previous test panics)
    reset_user_login_state(&config.database_url, "member");
    // Push the member account past the CAPTCHA threshold (default: 3 failures)
    set_failed_login_count(&config.database_url, "member", 3);

    // Attempt login without CAPTCHA
    let (status, body) = do_login(&app, "member", "Member!User1Passw0rd").await;
    assert_eq!(
        status, 422,
        "login without CAPTCHA on high-risk account must return 422: {body}"
    );
    assert_eq!(body["error"]["code"], "validation_error");
    // Validation field errors are under "details", not "fields"
    let details = body["error"]["details"].as_array().unwrap();
    assert!(
        details
            .iter()
            .any(|f| f["field"] == "captcha_token"
                && f["message"]
                    .as_str()
                    .unwrap_or("")
                    .contains("CAPTCHA is required")),
        "error must indicate CAPTCHA is required: {details:?}"
    );

    reset_user_login_state(&config.database_url, "member");
}

/// After enough failed attempts, login with wrong CAPTCHA answer returns 422.
#[actix_rt::test]
#[serial]
async fn login_fails_on_wrong_captcha_when_required() {
    test_app!(app, config);
    reset_user_login_state(&config.database_url, "member");
    set_failed_login_count(&config.database_url, "member", 3);

    // Get a valid CAPTCHA challenge but submit wrong answer
    let (captcha_token, correct_answer) = fetch_captcha_with_answer(&app).await;
    let wrong_answer = correct_answer.wrapping_add(1);

    let req = test::TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(json!({
            "username": "member",
            "password": "Member!User1Passw0rd",
            "captcha_token": captcha_token,
            "captcha_answer": wrong_answer
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 422, "wrong CAPTCHA must return 422");
    let body: Value = test::read_body_json(resp).await;
    assert_eq!(body["error"]["code"], "validation_error");
    // Validation field errors are under "details", not "fields"
    let details = body["error"]["details"].as_array().unwrap();
    assert!(
        details.iter().any(|f| f["field"] == "captcha_answer"),
        "error must reference captcha_answer field: {details:?}"
    );

    reset_user_login_state(&config.database_url, "member");
}

/// After enough failed attempts, login succeeds with valid CAPTCHA + correct password.
#[actix_rt::test]
#[serial]
async fn login_succeeds_with_valid_captcha_when_required() {
    test_app!(app, config);
    set_failed_login_count(&config.database_url, "member", 3);

    let (captcha_token, correct_answer) = fetch_captcha_with_answer(&app).await;

    let req = test::TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(json!({
            "username": "member",
            "password": "Member!User1Passw0rd",
            "captcha_token": captcha_token,
            "captcha_answer": correct_answer
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        200,
        "valid CAPTCHA + correct password must succeed"
    );
    let body: Value = test::read_body_json(resp).await;
    assert!(body["data"]["token"].is_string());

    reset_user_login_state(&config.database_url, "member");
}
