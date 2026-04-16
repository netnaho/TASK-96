/// API-level integration tests for candidates, offers, and onboarding.
///
/// ## Prerequisites
///
/// ```bash
/// DATABASE_URL=postgres://talentflow:talentflow_dev@localhost:5433/talentflow_test \
///   ENCRYPTION_KEY="$(openssl rand -base64 32)" \
///   cargo test --test business_tests
/// ```
///
/// Tests run sequentially via `serial_test`.  The seeded `platform_admin` user is used
/// for most tests; `club_admin` and `member` are used for authorization-boundary tests.
use actix_web::{dev::Service, test, web, App};
use diesel::RunQueryDsl;
use serde_json::{json, Value};
use serial_test::serial;
use std::env;

use talentflow::{
    api::{middleware::RequestId, routes},
    infrastructure::{config::AppConfig, db},
    shared::app_state::AppState,
};

// ── Test app builder ──────────────────────────────────────────────────────────

fn build_test_app_config() -> Option<AppConfig> {
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

// ── Helpers ───────────────────────────────────────────────────────────────────

async fn login_as(
    app: &impl actix_web::dev::Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
    >,
    username: &str,
    password: &str,
) -> String {
    let req = test::TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(json!({"username": username, "password": password}))
        .to_request();
    let resp = test::call_service(app, req).await;
    let body: Value = test::read_body_json(resp).await;
    body["data"]["token"]
        .as_str()
        .expect("login failed — no token in response")
        .to_string()
}

async fn authed_get(
    app: &impl actix_web::dev::Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
    >,
    uri: &str,
    token: &str,
) -> (u16, Value) {
    let req = test::TestRequest::get()
        .uri(uri)
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(app, req).await;
    let status = resp.status().as_u16();
    let body: Value = test::read_body_json(resp).await;
    (status, body)
}

async fn authed_post(
    app: &impl actix_web::dev::Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
    >,
    uri: &str,
    token: &str,
    body: Value,
) -> (u16, Value) {
    let req = test::TestRequest::post()
        .uri(uri)
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(body)
        .to_request();
    let resp = test::call_service(app, req).await;
    let status = resp.status().as_u16();
    let resp_body: Value = test::read_body_json(resp).await;
    (status, resp_body)
}

async fn authed_put(
    app: &impl actix_web::dev::Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
    >,
    uri: &str,
    token: &str,
    body: Value,
) -> (u16, Value) {
    authed_put_with_headers(app, uri, token, body, vec![]).await
}

async fn authed_post_with_headers(
    app: &impl actix_web::dev::Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
    >,
    uri: &str,
    token: &str,
    body: Value,
    extra_headers: Vec<(&str, &str)>,
) -> (u16, Value) {
    let mut req = test::TestRequest::post()
        .uri(uri)
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(body);
    for (k, v) in extra_headers {
        req = req.insert_header((k, v));
    }
    let resp = test::call_service(app, req.to_request()).await;
    let status = resp.status().as_u16();
    let resp_body: Value = test::read_body_json(resp).await;
    (status, resp_body)
}

async fn authed_put_with_headers(
    app: &impl actix_web::dev::Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
    >,
    uri: &str,
    token: &str,
    body: Value,
    extra_headers: Vec<(&str, &str)>,
) -> (u16, Value) {
    let mut req = test::TestRequest::put()
        .uri(uri)
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(body);
    for (k, v) in extra_headers {
        req = req.insert_header((k, v));
    }
    let resp = test::call_service(app, req.to_request()).await;
    let status = resp.status().as_u16();
    let resp_body: Value = test::read_body_json(resp).await;
    (status, resp_body)
}

async fn authed_delete_with_headers(
    app: &impl actix_web::dev::Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
    >,
    uri: &str,
    token: &str,
    extra_headers: Vec<(&str, &str)>,
) -> (u16, Value) {
    let mut req = test::TestRequest::delete()
        .uri(uri)
        .insert_header(("Authorization", format!("Bearer {token}")));
    for (k, v) in extra_headers {
        req = req.insert_header((k, v));
    }
    let resp = test::call_service(app, req.to_request()).await;
    let status = resp.status().as_u16();
    // DELETE 204 returns empty body — handle gracefully
    let bytes = test::read_body(resp).await;
    let resp_body: Value = if bytes.is_empty() {
        json!({})
    } else {
        serde_json::from_slice(&bytes).unwrap_or(json!({}))
    };
    (status, resp_body)
}

async fn authed_post_with_headers_raw(
    app: &impl actix_web::dev::Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
    >,
    uri: &str,
    token: &str,
    body: Value,
    extra_headers: Vec<(&str, &str)>,
) -> (u16, Value) {
    let mut req = test::TestRequest::post()
        .uri(uri)
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(body);
    for (k, v) in extra_headers {
        req = req.insert_header((k, v));
    }
    let resp = test::call_service(app, req.to_request()).await;
    let status = resp.status().as_u16();
    let bytes = test::read_body(resp).await;
    let resp_body: Value = if bytes.is_empty() {
        json!({})
    } else {
        serde_json::from_slice(&bytes).unwrap_or(json!({}))
    };
    (status, resp_body)
}

// ── Candidate tests ───────────────────────────────────────────────────────────

#[actix_rt::test]
#[serial]
async fn create_candidate_returns_201() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    let (status, body) = authed_post(
        &app,
        "/api/v1/candidates",
        &token,
        json!({
            "first_name": "Alice",
            "last_name": "Smith",
            "email": "alice.smith.test@example.com",
            "phone": "555-0100",
            "source": "referral",
            "tags": ["senior", "rust"]
        }),
    )
    .await;

    assert_eq!(status, 201, "body: {body}");
    assert_eq!(body["data"]["first_name"], "Alice");
    assert_eq!(body["data"]["last_name"], "Smith");
    assert_eq!(body["data"]["email"], "alice.smith.test@example.com");
    // Phone is NOT echoed back in create response
    assert!(body["data"]["phone"].is_null());
}

#[actix_rt::test]
#[serial]
async fn create_candidate_unauthenticated_returns_401() {
    test_app!(app, _config);

    let req = test::TestRequest::post()
        .uri("/api/v1/candidates")
        .set_json(json!({
            "first_name": "Bob",
            "last_name": "Jones",
            "email": "bob@example.com"
        }))
        .to_request();
    // Middleware returns Err, not HttpResponse, so use Service::call directly
    let status = match app.call(req).await {
        Ok(resp) => resp.status().as_u16(),
        Err(err) => err.error_response().status().as_u16(),
    };
    assert_eq!(status, 401);
}

#[actix_rt::test]
#[serial]
async fn create_candidate_invalid_email_returns_422() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    let (status, body) = authed_post(
        &app,
        "/api/v1/candidates",
        &token,
        json!({
            "first_name": "Test",
            "last_name": "User",
            "email": "not-an-email"
        }),
    )
    .await;

    assert_eq!(status, 422, "body: {body}");
    assert_eq!(body["error"]["code"], "validation_error");
}

#[actix_rt::test]
#[serial]
async fn list_candidates_returns_paginated_result() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    let (status, body) = authed_get(&app, "/api/v1/candidates?page=1&per_page=10", &token).await;
    assert_eq!(status, 200, "body: {body}");
    assert!(body["data"].is_array());
    assert!(body["pagination"]["page"].is_number());
    assert!(body["pagination"]["per_page"].is_number());
    assert!(body["pagination"]["total"].is_number());
}

#[actix_rt::test]
#[serial]
async fn get_candidate_not_found_returns_404() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    let (status, body) = authed_get(
        &app,
        "/api/v1/candidates/00000000-0000-0000-0000-000000000001",
        &token,
    )
    .await;
    assert_eq!(status, 404, "body: {body}");
    assert_eq!(body["error"]["code"], "not_found");
}

#[actix_rt::test]
#[serial]
async fn member_cannot_create_candidate() {
    test_app!(app, _config);
    let token = login_as(&app, "member", "Member!User1Passw0rd").await;

    let (status, body) = authed_post(
        &app,
        "/api/v1/candidates",
        &token,
        json!({
            "first_name": "X",
            "last_name": "Y",
            "email": "xy@example.com"
        }),
    )
    .await;

    assert_eq!(status, 403, "body: {body}");
    assert_eq!(body["error"]["code"], "forbidden");
}

// ── Offer tests ───────────────────────────────────────────────────────────────

#[actix_rt::test]
#[serial]
async fn create_offer_then_get_returns_draft_status() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    // First create a candidate
    let (c_status, c_body) = authed_post(
        &app,
        "/api/v1/candidates",
        &token,
        json!({
            "first_name": "Offer",
            "last_name": "Candidate",
            "email": "offer.candidate.test@example.com"
        }),
    )
    .await;
    assert_eq!(c_status, 201, "candidate create failed: {c_body}");
    let candidate_id = c_body["data"]["id"].as_str().unwrap().to_string();

    // Create an offer
    let (o_status, o_body) = authed_post(
        &app,
        "/api/v1/offers",
        &token,
        json!({
            "candidate_id": candidate_id,
            "title": "Senior Engineer",
            "department": "Engineering",
            "compensation": {
                "base_salary_usd": 150000,
                "bonus_target_pct": 15.0,
                "equity_units": 1000,
                "pto_days": 20,
                "k401_match_pct": 6.0
            }
        }),
    )
    .await;
    assert_eq!(o_status, 201, "offer create failed: {o_body}");
    assert_eq!(o_body["data"]["status"], "draft");
    let offer_id = o_body["data"]["id"].as_str().unwrap().to_string();

    // GET the offer — compensation should not be revealed by default
    let (get_status, get_body) =
        authed_get(&app, &format!("/api/v1/offers/{offer_id}"), &token).await;
    assert_eq!(get_status, 200, "body: {get_body}");
    assert_eq!(get_body["data"]["status"], "draft");
    assert!(get_body["data"]["compensation"].is_null());
}

#[actix_rt::test]
#[serial]
async fn submit_offer_transitions_to_pending_approval() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    // Create a candidate + offer
    let (_, c_body) = authed_post(
        &app,
        "/api/v1/candidates",
        &token,
        json!({"first_name": "Sub", "last_name": "Cand", "email": "sub.cand.test@example.com"}),
    )
    .await;
    let candidate_id = c_body["data"]["id"].as_str().unwrap();

    let (_, o_body) = authed_post(
        &app,
        "/api/v1/offers",
        &token,
        json!({"candidate_id": candidate_id, "title": "Engineer"}),
    )
    .await;
    let offer_id = o_body["data"]["id"].as_str().unwrap().to_string();

    // Submit
    let (sub_status, sub_body) = authed_post(
        &app,
        &format!("/api/v1/offers/{offer_id}/submit"),
        &token,
        json!({}),
    )
    .await;
    assert_eq!(sub_status, 200, "body: {sub_body}");
    assert_eq!(sub_body["data"]["status"], "pending_approval");
}

#[actix_rt::test]
#[serial]
async fn invalid_compensation_returns_422() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    let (_, c_body) = authed_post(
        &app,
        "/api/v1/candidates",
        &token,
        json!({"first_name": "V", "last_name": "W", "email": "vw.test@example.com"}),
    )
    .await;
    let candidate_id = c_body["data"]["id"].as_str().unwrap();

    // base_salary_usd = 0 is invalid
    let (status, body) = authed_post(
        &app,
        "/api/v1/offers",
        &token,
        json!({
            "candidate_id": candidate_id,
            "title": "Engineer",
            "compensation": {
                "base_salary_usd": 0,
                "bonus_target_pct": 10.0,
                "equity_units": 0,
                "pto_days": 20,
                "k401_match_pct": 5.0
            }
        }),
    )
    .await;
    assert_eq!(status, 422, "body: {body}");
    assert_eq!(body["error"]["code"], "validation_error");
}

#[actix_rt::test]
#[serial]
async fn invalid_state_transition_returns_409() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    let (_, c_body) = authed_post(
        &app,
        "/api/v1/candidates",
        &token,
        json!({"first_name": "Trans", "last_name": "Test", "email": "trans.test@example.com"}),
    )
    .await;
    let candidate_id = c_body["data"]["id"].as_str().unwrap();

    let (_, o_body) = authed_post(
        &app,
        "/api/v1/offers",
        &token,
        json!({"candidate_id": candidate_id, "title": "Eng"}),
    )
    .await;
    let offer_id = o_body["data"]["id"].as_str().unwrap().to_string();

    // Try to withdraw from draft (valid) then try to withdraw again from withdrawn (invalid)
    authed_post(
        &app,
        &format!("/api/v1/offers/{offer_id}/withdraw"),
        &token,
        json!({}),
    )
    .await;

    let (status, body) = authed_post(
        &app,
        &format!("/api/v1/offers/{offer_id}/withdraw"),
        &token,
        json!({}),
    )
    .await;
    assert_eq!(status, 409, "body: {body}");
    assert_eq!(body["error"]["code"], "invalid_state_transition");
}

// ── Onboarding tests ──────────────────────────────────────────────────────────

#[actix_rt::test]
#[serial]
async fn create_checklist_and_item_readiness_is_0() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    // Create candidate + offer
    let (_, c_body) = authed_post(
        &app,
        "/api/v1/candidates",
        &token,
        json!({"first_name": "On", "last_name": "Board", "email": "on.board.test@example.com"}),
    )
    .await;
    let candidate_id = c_body["data"]["id"].as_str().unwrap();

    let (_, o_body) = authed_post(
        &app,
        "/api/v1/offers",
        &token,
        json!({"candidate_id": candidate_id, "title": "Onboard Offer"}),
    )
    .await;
    let offer_id = o_body["data"]["id"].as_str().unwrap();

    // Create a checklist
    let (cl_status, cl_body) = authed_post(
        &app,
        "/api/v1/onboarding/checklists",
        &token,
        json!({"offer_id": offer_id, "candidate_id": candidate_id}),
    )
    .await;
    assert_eq!(cl_status, 201, "body: {cl_body}");
    let checklist_id = cl_body["data"]["id"].as_str().unwrap().to_string();

    // Add a required item
    let (item_status, item_body) = authed_post(
        &app,
        &format!("/api/v1/onboarding/checklists/{checklist_id}/items"),
        &token,
        json!({
            "title": "Background check",
            "item_order": 1,
            "required": true,
            "requires_upload": false
        }),
    )
    .await;
    assert_eq!(item_status, 201, "body: {item_body}");
    assert_eq!(item_body["data"]["required"], true);
    let item_id = item_body["data"]["id"].as_str().unwrap().to_string();

    // Mark item as completed
    let (upd_status, upd_body) = authed_put(
        &app,
        &format!("/api/v1/onboarding/checklists/{checklist_id}/items/{item_id}"),
        &token,
        json!({"status": "completed"}),
    )
    .await;
    assert_eq!(upd_status, 200, "body: {upd_body}");
    assert_eq!(upd_body["data"]["status"], "completed");
}

#[actix_rt::test]
#[serial]
async fn update_item_with_invalid_status_returns_422() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    let (_, c_body) = authed_post(
        &app,
        "/api/v1/candidates",
        &token,
        json!({"first_name": "Inv", "last_name": "St", "email": "inv.st.test@example.com"}),
    )
    .await;
    let candidate_id = c_body["data"]["id"].as_str().unwrap();

    let (_, o_body) = authed_post(
        &app,
        "/api/v1/offers",
        &token,
        json!({"candidate_id": candidate_id, "title": "Offer"}),
    )
    .await;
    let offer_id = o_body["data"]["id"].as_str().unwrap();

    let (_, cl_body) = authed_post(
        &app,
        "/api/v1/onboarding/checklists",
        &token,
        json!({"offer_id": offer_id, "candidate_id": candidate_id}),
    )
    .await;
    let checklist_id = cl_body["data"]["id"].as_str().unwrap().to_string();

    let (_, item_body) = authed_post(
        &app,
        &format!("/api/v1/onboarding/checklists/{checklist_id}/items"),
        &token,
        json!({"title": "Task", "item_order": 1, "required": false, "requires_upload": false}),
    )
    .await;
    let item_id = item_body["data"]["id"].as_str().unwrap().to_string();

    let (status, body) = authed_put(
        &app,
        &format!("/api/v1/onboarding/checklists/{checklist_id}/items/{item_id}"),
        &token,
        json!({"status": "flying"}),
    )
    .await;
    assert_eq!(status, 422, "body: {body}");
    assert_eq!(body["error"]["code"], "validation_error");
}

#[actix_rt::test]
#[serial]
async fn list_checklist_items_returns_array() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    let (_, c_body) = authed_post(
        &app,
        "/api/v1/candidates",
        &token,
        json!({"first_name": "Li", "last_name": "Items", "email": "li.items.test@example.com"}),
    )
    .await;
    let candidate_id = c_body["data"]["id"].as_str().unwrap();

    let (_, o_body) = authed_post(
        &app,
        "/api/v1/offers",
        &token,
        json!({"candidate_id": candidate_id, "title": "List Offer"}),
    )
    .await;
    let offer_id = o_body["data"]["id"].as_str().unwrap();

    let (_, cl_body) = authed_post(
        &app,
        "/api/v1/onboarding/checklists",
        &token,
        json!({"offer_id": offer_id, "candidate_id": candidate_id}),
    )
    .await;
    let checklist_id = cl_body["data"]["id"].as_str().unwrap().to_string();

    let (status, body) = authed_get(
        &app,
        &format!("/api/v1/onboarding/checklists/{checklist_id}/items"),
        &token,
    )
    .await;
    assert_eq!(status, 200, "body: {body}");
    assert!(body["data"].is_array());
}

// ── Canonical idempotency tests ───────────────────────────────────────────────

/// Replaying a create-candidate request with the same key and same payload must
/// return 201 with the same candidate ID (not a duplicate).
#[actix_rt::test]
#[serial]
async fn create_candidate_idempotent_replay_same_payload() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    let idem_key = format!("test-cand-replay-{}", uuid::Uuid::new_v4());
    let payload = json!({
        "first_name": "Idem",
        "last_name": "Replay",
        "email": format!("idem.replay.{}@example.com", uuid::Uuid::new_v4()),
    });

    let (status1, body1) = authed_post_with_headers(
        &app,
        "/api/v1/candidates",
        &token,
        payload.clone(),
        vec![("Idempotency-Key", &idem_key)],
    )
    .await;
    assert_eq!(status1, 201, "first create must succeed: {body1}");
    let candidate_id = body1["data"]["id"].as_str().unwrap().to_string();

    let (status2, body2) = authed_post_with_headers(
        &app,
        "/api/v1/candidates",
        &token,
        payload,
        vec![("Idempotency-Key", &idem_key)],
    )
    .await;
    assert_eq!(status2, 201, "idempotent replay must return 201: {body2}");
    assert_eq!(
        body2["data"]["id"].as_str().unwrap(),
        candidate_id,
        "replay must return the same candidate ID"
    );
}

/// Replaying a create-candidate request with the same key but a different email
/// must return 409 with code `idempotency_conflict`.
#[actix_rt::test]
#[serial]
async fn create_candidate_idempotent_conflict_different_payload() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    let idem_key = format!("test-cand-conflict-{}", uuid::Uuid::new_v4());

    let (status1, body1) = authed_post_with_headers(
        &app,
        "/api/v1/candidates",
        &token,
        json!({
            "first_name": "Idem",
            "last_name": "Conflict",
            "email": format!("idem.conflict.a.{}@example.com", uuid::Uuid::new_v4()),
        }),
        vec![("Idempotency-Key", &idem_key)],
    )
    .await;
    assert_eq!(status1, 201, "first create must succeed: {body1}");

    let (status2, body2) = authed_post_with_headers(
        &app,
        "/api/v1/candidates",
        &token,
        json!({
            "first_name": "Idem",
            "last_name": "Conflict",
            "email": format!("idem.conflict.b.{}@example.com", uuid::Uuid::new_v4()),
        }),
        vec![("Idempotency-Key", &idem_key)],
    )
    .await;
    assert_eq!(status2, 409, "different payload must produce 409: {body2}");
    assert_eq!(
        body2["error"]["code"], "idempotency_conflict",
        "error code must be idempotency_conflict, got: {body2}"
    );
}

/// A create-candidate request without an Idempotency-Key must not be deduplicated;
/// two requests with the same body create two distinct candidates.
#[actix_rt::test]
#[serial]
async fn create_candidate_no_idempotency_key_creates_duplicate() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    let email = format!("idem.nokey.{}@example.com", uuid::Uuid::new_v4());
    let payload = json!({
        "first_name": "Idem",
        "last_name": "NoKey",
        "email": email,
    });

    let (status1, body1) = authed_post(&app, "/api/v1/candidates", &token, payload.clone()).await;
    // First must succeed; second will conflict on the unique email — that's fine as long
    // as neither is deduplicated via idempotency.
    assert_eq!(status1, 201, "first create must succeed: {body1}");
    let id1 = body1["data"]["id"].as_str().unwrap().to_string();

    let (status2, body2) = authed_post(&app, "/api/v1/candidates", &token, payload).await;
    // The email is already taken so the second will return 409 conflict (not 201),
    // which confirms the second request was NOT short-circuited by idempotency replay.
    assert_ne!(
        status2, 201,
        "second identical request without idem key must not replay the first: {body2}"
    );
    // If it somehow succeeded it must have a different ID (belt-and-suspenders).
    if status2 == 201 {
        assert_ne!(
            body2["data"]["id"].as_str().unwrap(),
            id1,
            "must produce distinct IDs without idempotency key"
        );
    }
}

/// Replaying a create-offer request with the same key and same payload must
/// return 201 with the same offer ID.
#[actix_rt::test]
#[serial]
async fn create_offer_idempotent_replay_same_payload() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    // Seed a candidate to attach the offer to.
    let (_, c_body) = authed_post(
        &app,
        "/api/v1/candidates",
        &token,
        json!({
            "first_name": "Offer",
            "last_name": "IdemReplay",
            "email": format!("offer.idem.replay.{}@example.com", uuid::Uuid::new_v4()),
        }),
    )
    .await;
    let candidate_id = c_body["data"]["id"].as_str().unwrap().to_string();

    let idem_key = format!("test-offer-replay-{}", uuid::Uuid::new_v4());
    let payload = json!({
        "candidate_id": candidate_id,
        "title": "Idempotent Offer Replay",
    });

    let (status1, body1) = authed_post_with_headers(
        &app,
        "/api/v1/offers",
        &token,
        payload.clone(),
        vec![("Idempotency-Key", &idem_key)],
    )
    .await;
    assert_eq!(status1, 201, "first create must succeed: {body1}");
    let offer_id = body1["data"]["id"].as_str().unwrap().to_string();

    let (status2, body2) = authed_post_with_headers(
        &app,
        "/api/v1/offers",
        &token,
        payload,
        vec![("Idempotency-Key", &idem_key)],
    )
    .await;
    assert_eq!(status2, 201, "idempotent replay must return 201: {body2}");
    assert_eq!(
        body2["data"]["id"].as_str().unwrap(),
        offer_id,
        "replay must return the same offer ID"
    );
}

/// Replaying a create-offer request with the same key but a different title must
/// return 409 with code `idempotency_conflict`.
#[actix_rt::test]
#[serial]
async fn create_offer_idempotent_conflict_different_payload() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    let (_, c_body) = authed_post(
        &app,
        "/api/v1/candidates",
        &token,
        json!({
            "first_name": "Offer",
            "last_name": "IdemConflict",
            "email": format!("offer.idem.conflict.{}@example.com", uuid::Uuid::new_v4()),
        }),
    )
    .await;
    let candidate_id = c_body["data"]["id"].as_str().unwrap().to_string();

    let idem_key = format!("test-offer-conflict-{}", uuid::Uuid::new_v4());

    let (status1, body1) = authed_post_with_headers(
        &app,
        "/api/v1/offers",
        &token,
        json!({"candidate_id": candidate_id, "title": "Offer Title A"}),
        vec![("Idempotency-Key", &idem_key)],
    )
    .await;
    assert_eq!(status1, 201, "first create must succeed: {body1}");

    let (status2, body2) = authed_post_with_headers(
        &app,
        "/api/v1/offers",
        &token,
        json!({"candidate_id": candidate_id, "title": "Offer Title B — different"}),
        vec![("Idempotency-Key", &idem_key)],
    )
    .await;
    assert_eq!(status2, 409, "different payload must produce 409: {body2}");
    assert_eq!(
        body2["error"]["code"], "idempotency_conflict",
        "error code must be idempotency_conflict, got: {body2}"
    );
}

/// Replaying an update-offer (PUT) with the same key and same payload must return
/// 200 with the same offer ID rather than applying the update twice.
#[actix_rt::test]
#[serial]
async fn update_offer_idempotent_replay_same_payload() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    let (_, c_body) = authed_post(
        &app,
        "/api/v1/candidates",
        &token,
        json!({
            "first_name": "Upd",
            "last_name": "IdemReplay",
            "email": format!("upd.idem.replay.{}@example.com", uuid::Uuid::new_v4()),
        }),
    )
    .await;
    let candidate_id = c_body["data"]["id"].as_str().unwrap().to_string();

    let (_, o_body) = authed_post(
        &app,
        "/api/v1/offers",
        &token,
        json!({"candidate_id": candidate_id, "title": "Before Update"}),
    )
    .await;
    let offer_id = o_body["data"]["id"].as_str().unwrap().to_string();

    let idem_key = format!("test-offer-upd-replay-{}", uuid::Uuid::new_v4());
    let update_payload = json!({"title": "After Update"});

    let (status1, body1) = authed_put_with_headers(
        &app,
        &format!("/api/v1/offers/{offer_id}"),
        &token,
        update_payload.clone(),
        vec![("Idempotency-Key", &idem_key)],
    )
    .await;
    assert_eq!(status1, 200, "first update must succeed: {body1}");
    assert_eq!(body1["data"]["id"].as_str().unwrap(), offer_id);

    let (status2, body2) = authed_put_with_headers(
        &app,
        &format!("/api/v1/offers/{offer_id}"),
        &token,
        update_payload,
        vec![("Idempotency-Key", &idem_key)],
    )
    .await;
    assert_eq!(status2, 200, "idempotent replay must return 200: {body2}");
    assert_eq!(
        body2["data"]["id"].as_str().unwrap(),
        offer_id,
        "replay must return the same offer ID"
    );
}

/// Replaying a create-onboarding-checklist request with the same key and payload
/// must return 201 with the same checklist ID.
#[actix_rt::test]
#[serial]
async fn create_checklist_idempotent_replay_same_payload() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    let (_, c_body) = authed_post(
        &app,
        "/api/v1/candidates",
        &token,
        json!({
            "first_name": "Onb",
            "last_name": "IdemReplay",
            "email": format!("onb.idem.replay.{}@example.com", uuid::Uuid::new_v4()),
        }),
    )
    .await;
    let candidate_id = c_body["data"]["id"].as_str().unwrap().to_string();

    let (_, o_body) = authed_post(
        &app,
        "/api/v1/offers",
        &token,
        json!({"candidate_id": candidate_id, "title": "Onboarding Offer"}),
    )
    .await;
    let offer_id = o_body["data"]["id"].as_str().unwrap().to_string();

    let idem_key = format!("test-checklist-replay-{}", uuid::Uuid::new_v4());
    let payload = json!({
        "offer_id": offer_id,
        "candidate_id": candidate_id,
    });

    let (status1, body1) = authed_post_with_headers(
        &app,
        "/api/v1/onboarding/checklists",
        &token,
        payload.clone(),
        vec![("Idempotency-Key", &idem_key)],
    )
    .await;
    assert_eq!(status1, 201, "first create must succeed: {body1}");
    let checklist_id = body1["data"]["id"].as_str().unwrap().to_string();

    let (status2, body2) = authed_post_with_headers(
        &app,
        "/api/v1/onboarding/checklists",
        &token,
        payload,
        vec![("Idempotency-Key", &idem_key)],
    )
    .await;
    assert_eq!(status2, 201, "idempotent replay must return 201: {body2}");
    assert_eq!(
        body2["data"]["id"].as_str().unwrap(),
        checklist_id,
        "replay must return the same checklist ID"
    );
}

// ── Onboarding object-level authorization tests ───────────────────────────────

/// A member user should NOT see checklists they are not assigned to.
/// When no checklists are assigned to the member, the list must be empty.
#[actix_rt::test]
#[serial]
async fn member_cannot_list_unassigned_checklists() {
    test_app!(app, _config);
    let admin_token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;
    let member_token = login_as(&app, "member", "Member!User1Passw0rd").await;

    // Admin creates a candidate + offer + checklist with NO assigned_to
    let (_, c_body) = authed_post(
        &app,
        "/api/v1/candidates",
        &admin_token,
        json!({
            "first_name": "ObjAuth",
            "last_name": "Unassigned",
            "email": format!("objauth.unassigned.{}@example.com", uuid::Uuid::new_v4()),
        }),
    )
    .await;
    let candidate_id = c_body["data"]["id"].as_str().unwrap();

    let (_, o_body) = authed_post(
        &app,
        "/api/v1/offers",
        &admin_token,
        json!({"candidate_id": candidate_id, "title": "ObjAuth Offer"}),
    )
    .await;
    let offer_id = o_body["data"]["id"].as_str().unwrap();

    let (cl_status, cl_body) = authed_post(
        &app,
        "/api/v1/onboarding/checklists",
        &admin_token,
        json!({"offer_id": offer_id, "candidate_id": candidate_id}),
    )
    .await;
    assert_eq!(cl_status, 201, "admin must be able to create checklist: {cl_body}");

    // Member lists checklists — must not see the unassigned checklist
    let (status, body) = authed_get(
        &app,
        "/api/v1/onboarding/checklists",
        &member_token,
    )
    .await;
    assert_eq!(status, 200, "member GET /onboarding/checklists must return 200: {body}");
    let items = body["data"].as_array().expect("data must be an array");
    let checklist_id = cl_body["data"]["id"].as_str().unwrap();
    let found = items.iter().any(|c| c["id"].as_str() == Some(checklist_id));
    assert!(
        !found,
        "member must not see a checklist they are not assigned to"
    );
}

/// A member should see checklists where assigned_to equals their user ID.
#[actix_rt::test]
#[serial]
async fn member_can_list_assigned_checklists() {
    test_app!(app, _config);
    let admin_token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;
    let member_token = login_as(&app, "member", "Member!User1Passw0rd").await;

    // Fetch member's own user ID via GET /auth/session
    let (sess_status, sess_body) = authed_get(&app, "/api/v1/auth/session", &member_token).await;
    assert_eq!(sess_status, 200, "session fetch failed: {sess_body}");
    let member_id = sess_body["data"]["user_id"]
        .as_str()
        .expect("user_id missing from session response")
        .to_string();

    // Admin creates a candidate + offer + checklist assigned to the member
    let (_, c_body) = authed_post(
        &app,
        "/api/v1/candidates",
        &admin_token,
        json!({
            "first_name": "Assigned",
            "last_name": "Member",
            "email": format!("assigned.member.{}@example.com", uuid::Uuid::new_v4()),
        }),
    )
    .await;
    let candidate_id = c_body["data"]["id"].as_str().unwrap();

    let (_, o_body) = authed_post(
        &app,
        "/api/v1/offers",
        &admin_token,
        json!({"candidate_id": candidate_id, "title": "Assigned Offer"}),
    )
    .await;
    let offer_id = o_body["data"]["id"].as_str().unwrap();

    let (cl_status, cl_body) = authed_post(
        &app,
        "/api/v1/onboarding/checklists",
        &admin_token,
        json!({
            "offer_id": offer_id,
            "candidate_id": candidate_id,
            "assigned_to": member_id,
        }),
    )
    .await;
    assert_eq!(cl_status, 201, "admin must be able to create assigned checklist: {cl_body}");
    let checklist_id = cl_body["data"]["id"].as_str().unwrap().to_string();

    // Member lists checklists — must see the assigned checklist
    let (status, body) = authed_get(
        &app,
        "/api/v1/onboarding/checklists",
        &member_token,
    )
    .await;
    assert_eq!(status, 200, "member GET /onboarding/checklists must return 200: {body}");
    let items = body["data"].as_array().expect("data must be an array");
    let found = items.iter().any(|c| c["id"].as_str() == Some(&checklist_id));
    assert!(
        found,
        "member must see the checklist assigned to them; got: {body}"
    );
}

/// A platform_admin must retain full visibility of all checklists regardless of assigned_to.
#[actix_rt::test]
#[serial]
async fn admin_sees_all_checklists_regardless_of_assignment() {
    test_app!(app, _config);
    let admin_token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    // Create a checklist with no assigned_to
    let (_, c_body) = authed_post(
        &app,
        "/api/v1/candidates",
        &admin_token,
        json!({
            "first_name": "AdminVis",
            "last_name": "Full",
            "email": format!("adminvis.full.{}@example.com", uuid::Uuid::new_v4()),
        }),
    )
    .await;
    let candidate_id = c_body["data"]["id"].as_str().unwrap();

    let (_, o_body) = authed_post(
        &app,
        "/api/v1/offers",
        &admin_token,
        json!({"candidate_id": candidate_id, "title": "Admin Vis Offer"}),
    )
    .await;
    let offer_id = o_body["data"]["id"].as_str().unwrap();

    let (cl_status, cl_body) = authed_post(
        &app,
        "/api/v1/onboarding/checklists",
        &admin_token,
        json!({"offer_id": offer_id, "candidate_id": candidate_id}),
    )
    .await;
    assert_eq!(cl_status, 201, "admin checklist create failed: {cl_body}");
    let checklist_id = cl_body["data"]["id"].as_str().unwrap().to_string();

    // Admin lists checklists — must see the newly created checklist
    let (status, body) = authed_get(
        &app,
        "/api/v1/onboarding/checklists",
        &admin_token,
    )
    .await;
    assert_eq!(status, 200, "admin GET /onboarding/checklists failed: {body}");
    let items = body["data"].as_array().expect("data must be an array");
    let found = items.iter().any(|c| c["id"].as_str() == Some(&checklist_id));
    assert!(
        found,
        "platform_admin must see all checklists including unassigned ones; got: {body}"
    );
}

// ── Assign-role idempotency tests ─────────────────────────────────────────────

/// Create a fresh user for role-assignment tests (avoids unique-constraint
/// conflicts when the same role is assigned multiple times across test runs).
async fn create_test_user(
    app: &impl actix_web::dev::Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
    >,
    token: &str,
) -> String {
    let uniq = uuid::Uuid::new_v4().to_string();
    let (status, body) = authed_post_with_headers(
        app,
        "/api/v1/users",
        token,
        json!({
            "username": format!("testuser_{}", &uniq[..8]),
            "email": format!("testuser.{}@example.com", uniq),
            "password": "Test!Password12345",
            "display_name": "Test User",
        }),
        vec![],
    )
    .await;
    assert_eq!(status, 201, "create test user failed: {body}");
    body["data"]["id"].as_str().unwrap().to_string()
}

/// Replaying an assign_role request with the same Idempotency-Key and same payload
/// must return 204 without creating a duplicate role assignment.
#[actix_rt::test]
#[serial]
async fn assign_role_idempotent_replay_same_payload() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    let target_user_id = create_test_user(&app, &token).await;
    let idem_key = format!("test-assign-replay-{}", uuid::Uuid::new_v4());
    let payload = json!({ "role_name": "club_admin" });

    // First request
    let (status1, body1) = authed_post_with_headers_raw(
        &app,
        &format!("/api/v1/users/{target_user_id}/roles"),
        &token,
        payload.clone(),
        vec![("Idempotency-Key", &idem_key)],
    )
    .await;
    assert_eq!(status1, 204, "first assign_role must succeed: {body1}");

    // Replay with same key + same payload → must still return 204
    let (status2, body2) = authed_post_with_headers_raw(
        &app,
        &format!("/api/v1/users/{target_user_id}/roles"),
        &token,
        payload,
        vec![("Idempotency-Key", &idem_key)],
    )
    .await;
    assert_eq!(status2, 204, "idempotent replay must return 204: {body2}");
}

/// Replaying an assign_role request with the same key but a different role_name
/// must return 409 with code `idempotency_conflict`.
#[actix_rt::test]
#[serial]
async fn assign_role_idempotent_conflict_different_payload() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    let target_user_id = create_test_user(&app, &token).await;
    let idem_key = format!("test-assign-conflict-{}", uuid::Uuid::new_v4());

    // First request with role_name "guest"
    let (status1, body1) = authed_post_with_headers_raw(
        &app,
        &format!("/api/v1/users/{target_user_id}/roles"),
        &token,
        json!({ "role_name": "guest" }),
        vec![("Idempotency-Key", &idem_key)],
    )
    .await;
    assert_eq!(status1, 204, "first assign_role must succeed: {body1}");

    // Second request with same key but different role → conflict
    let (status2, body2) = authed_post_with_headers_raw(
        &app,
        &format!("/api/v1/users/{target_user_id}/roles"),
        &token,
        json!({ "role_name": "member" }),
        vec![("Idempotency-Key", &idem_key)],
    )
    .await;
    assert_eq!(status2, 409, "different payload must produce 409: {body2}");
    assert_eq!(
        body2["error"]["code"], "idempotency_conflict",
        "error code must be idempotency_conflict, got: {body2}"
    );
}

// ── Revoke-role idempotency tests ─────────────────────────────────────────────

/// Replaying a revoke_role request with the same Idempotency-Key must return 204.
#[actix_rt::test]
#[serial]
async fn revoke_role_idempotent_replay() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    // Create a fresh user and assign a role so we can revoke it
    let target_user_id = create_test_user(&app, &token).await;
    let role_id = "a0000000-0000-0000-0000-000000000001"; // guest role

    let assign_key = format!("test-revoke-setup-{}", uuid::Uuid::new_v4());
    let (assign_status, assign_body) = authed_post_with_headers_raw(
        &app,
        &format!("/api/v1/users/{target_user_id}/roles"),
        &token,
        json!({ "role_name": "guest" }),
        vec![("Idempotency-Key", &assign_key)],
    )
    .await;
    assert_eq!(assign_status, 204, "setup assign must succeed: {assign_body}");

    // Revoke with idempotency key
    let idem_key = format!("test-revoke-replay-{}", uuid::Uuid::new_v4());

    let (status1, body1) = authed_delete_with_headers(
        &app,
        &format!("/api/v1/users/{target_user_id}/roles/{role_id}"),
        &token,
        vec![("Idempotency-Key", &idem_key)],
    )
    .await;
    assert_eq!(status1, 204, "first revoke must succeed: {body1}");

    // Replay with same key → must still return 204
    let (status2, body2) = authed_delete_with_headers(
        &app,
        &format!("/api/v1/users/{target_user_id}/roles/{role_id}"),
        &token,
        vec![("Idempotency-Key", &idem_key)],
    )
    .await;
    assert_eq!(status2, 204, "idempotent replay must return 204: {body2}");
}

// ── Pagination count isolation tests ──────────────────────────────────────────

/// A member user listing candidates must see a total count that reflects only
/// their own records, not the full table. Admin-created candidates must not
/// inflate the member's pagination total.
#[actix_rt::test]
#[serial]
async fn member_candidate_list_total_excludes_other_users_records() {
    test_app!(app, _config);
    let admin_token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;
    let member_token = login_as(&app, "member", "Member!User1Passw0rd").await;

    // Admin creates a candidate — member did not create it
    let uniq = uuid::Uuid::new_v4();
    let (admin_status, admin_body) = authed_post(
        &app,
        "/api/v1/candidates",
        &admin_token,
        json!({
            "first_name": "Admin",
            "last_name": "Created",
            "email": format!("admin.count.{}@example.com", uniq),
        }),
    )
    .await;
    assert_eq!(admin_status, 201, "admin create must succeed: {admin_body}");

    // Member lists candidates — total must NOT include the admin's candidate
    let (member_status, member_body) = authed_get(
        &app,
        "/api/v1/candidates?page=1&per_page=100",
        &member_token,
    )
    .await;
    assert_eq!(member_status, 200, "member list failed: {member_body}");

    let member_total = member_body["pagination"]["total"].as_i64().unwrap();
    let member_data = member_body["data"].as_array().unwrap();

    // The total must equal the number of items returned (no leak)
    assert_eq!(
        member_total,
        member_data.len() as i64,
        "member total must equal returned item count (no count leak); total={member_total}, items={}",
        member_data.len()
    );

    // Admin lists candidates — must see at least the one just created
    let (admin_list_status, admin_list_body) = authed_get(
        &app,
        "/api/v1/candidates?page=1&per_page=100",
        &admin_token,
    )
    .await;
    assert_eq!(admin_list_status, 200);
    let admin_total = admin_list_body["pagination"]["total"].as_i64().unwrap();
    assert!(
        admin_total >= 1,
        "admin must see at least the candidate they created; total={admin_total}"
    );
}

/// A member user listing offers must see a total that reflects only offers
/// they created. Admin-created offers must not be counted.
#[actix_rt::test]
#[serial]
async fn member_offer_list_total_excludes_other_users_records() {
    test_app!(app, _config);
    let admin_token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;
    let member_token = login_as(&app, "member", "Member!User1Passw0rd").await;

    // Admin creates a candidate and an offer
    let uniq = uuid::Uuid::new_v4();
    let (_, cand_body) = authed_post(
        &app,
        "/api/v1/candidates",
        &admin_token,
        json!({
            "first_name": "OfferTest",
            "last_name": "Cand",
            "email": format!("offer.count.{}@example.com", uniq),
        }),
    )
    .await;
    let candidate_id = cand_body["data"]["id"].as_str().unwrap();

    let (offer_status, offer_body) = authed_post(
        &app,
        "/api/v1/offers",
        &admin_token,
        json!({
            "candidate_id": candidate_id,
            "title": "Count Test Offer",
        }),
    )
    .await;
    assert_eq!(offer_status, 201, "admin offer create failed: {offer_body}");

    // Member lists offers — total must not include admin's offer
    let (member_status, member_body) = authed_get(
        &app,
        "/api/v1/offers?page=1&per_page=100",
        &member_token,
    )
    .await;
    assert_eq!(member_status, 200, "member list failed: {member_body}");

    let member_total = member_body["pagination"]["total"].as_i64().unwrap();
    let member_data = member_body["data"].as_array().unwrap();

    assert_eq!(
        member_total,
        member_data.len() as i64,
        "member offer total must equal returned items (no count leak); total={member_total}, items={}",
        member_data.len()
    );
}

// ── Role/permission catalog access control ──────────────────────────────────

#[actix_rt::test]
#[serial]
async fn platform_admin_can_list_roles() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;
    let (status, body) = authed_get(&app, "/api/v1/roles", &token).await;
    assert_eq!(status, 200, "platform_admin must access roles catalog: {body}");
    assert!(body["data"].is_array());
}

#[actix_rt::test]
#[serial]
async fn platform_admin_can_list_permissions() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;
    let (status, body) = authed_get(&app, "/api/v1/permissions", &token).await;
    assert_eq!(
        status, 200,
        "platform_admin must access permissions catalog: {body}"
    );
    assert!(body["data"].is_array());
}

#[actix_rt::test]
#[serial]
async fn member_cannot_list_roles() {
    test_app!(app, _config);
    let token = login_as(&app, "member", "Member!User1Passw0rd").await;
    let (status, body) = authed_get(&app, "/api/v1/roles", &token).await;
    assert_eq!(
        status, 403,
        "member must be forbidden from roles catalog: {body}"
    );
    assert_eq!(body["error"]["code"], "forbidden");
}

#[actix_rt::test]
#[serial]
async fn member_cannot_list_permissions() {
    test_app!(app, _config);
    let token = login_as(&app, "member", "Member!User1Passw0rd").await;
    let (status, body) = authed_get(&app, "/api/v1/permissions", &token).await;
    assert_eq!(
        status, 403,
        "member must be forbidden from permissions catalog: {body}"
    );
    assert_eq!(body["error"]["code"], "forbidden");
}

#[actix_rt::test]
#[serial]
async fn unauthenticated_cannot_list_roles() {
    test_app!(app, _config);
    let req = test::TestRequest::get()
        .uri("/api/v1/roles")
        .to_request();
    let status = match app.call(req).await {
        Ok(resp) => resp.status().as_u16(),
        Err(err) => err.error_response().status().as_u16(),
    };
    assert_eq!(status, 401, "unauthenticated roles access must return 401");
}

#[actix_rt::test]
#[serial]
async fn unauthenticated_cannot_list_permissions() {
    test_app!(app, _config);
    let req = test::TestRequest::get()
        .uri("/api/v1/permissions")
        .to_request();
    let status = match app.call(req).await {
        Ok(resp) => resp.status().as_u16(),
        Err(err) => err.error_response().status().as_u16(),
    };
    assert_eq!(
        status, 401,
        "unauthenticated permissions access must return 401"
    );
}

// ── Audit immutability tests ────────────────────────────────────────────────

/// DB triggers must prevent UPDATE on audit_events rows.
#[actix_rt::test]
#[serial]
async fn test_audit_events_reject_update() {
    test_app!(_app, config);

    let db_pool = talentflow::infrastructure::db::create_pool(&config.database_url);
    let mut conn = db_pool.get().expect("db connection");

    // Insert a test audit event
    let event_id = uuid::Uuid::new_v4();
    diesel::sql_query(format!(
        "INSERT INTO audit_events (id, action, resource_type, metadata) \
         VALUES ('{event_id}', 'test.action', 'test', '{{}}')"
    ))
    .execute(&mut conn)
    .expect("insert audit event");

    // Attempt UPDATE — must be rejected by the trg_audit_no_update trigger
    let update_result = diesel::sql_query(format!(
        "UPDATE audit_events SET action = 'tampered' WHERE id = '{event_id}'"
    ))
    .execute(&mut conn);

    assert!(
        update_result.is_err(),
        "UPDATE on audit_events must be rejected by DB trigger"
    );
    let err_msg = format!("{}", update_result.unwrap_err());
    assert!(
        err_msg.contains("append-only"),
        "error must mention append-only constraint, got: {err_msg}"
    );
}

/// DB triggers must prevent DELETE on audit_events rows.
#[actix_rt::test]
#[serial]
async fn test_audit_events_reject_delete() {
    test_app!(_app, config);

    let db_pool = talentflow::infrastructure::db::create_pool(&config.database_url);
    let mut conn = db_pool.get().expect("db connection");

    // Insert a test audit event
    let event_id = uuid::Uuid::new_v4();
    diesel::sql_query(format!(
        "INSERT INTO audit_events (id, action, resource_type, metadata) \
         VALUES ('{event_id}', 'test.delete', 'test', '{{}}')"
    ))
    .execute(&mut conn)
    .expect("insert audit event");

    // Attempt DELETE — must be rejected by the trg_audit_no_delete trigger
    let delete_result = diesel::sql_query(format!(
        "DELETE FROM audit_events WHERE id = '{event_id}'"
    ))
    .execute(&mut conn);

    assert!(
        delete_result.is_err(),
        "DELETE on audit_events must be rejected by DB trigger"
    );
    let err_msg = format!("{}", delete_result.unwrap_err());
    assert!(
        err_msg.contains("append-only"),
        "error must mention append-only constraint, got: {err_msg}"
    );
}

// ── Offer Approval tests ──────────────────────────────────────────────────────

/// POST /api/v1/offers/{id}/approvals must return 201 with the created step.
#[actix_rt::test]
#[serial]
async fn create_approval_step_returns_201() {
    test_app!(app, _config);

    // Login and get the admin user id from the login response
    let req = test::TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(json!({"username": "platform_admin", "password": "Admin_Pa$$word1!"}))
        .to_request();
    let resp = test::call_service(&app, req).await;
    let login_body: Value = test::read_body_json(resp).await;
    let token = login_body["data"]["token"]
        .as_str()
        .expect("token in login response")
        .to_string();
    let admin_user_id = login_body["data"]["user"]["id"]
        .as_str()
        .expect("user.id in login response")
        .to_string();

    // Create a candidate then an offer
    let uniq = uuid::Uuid::new_v4();
    let (_, cand_body) = authed_post(
        &app,
        "/api/v1/candidates",
        &token,
        json!({
            "first_name": "ApprovalTest",
            "last_name": "Candidate",
            "email": format!("approval.create.{}@example.com", uniq),
        }),
    )
    .await;
    let candidate_id = cand_body["data"]["id"].as_str().unwrap();

    let (_, offer_body) = authed_post(
        &app,
        "/api/v1/offers",
        &token,
        json!({"candidate_id": candidate_id, "title": "Approval Step Offer"}),
    )
    .await;
    let offer_id = offer_body["data"]["id"].as_str().unwrap();

    // Submit the offer
    let (submit_status, submit_body) = authed_post(
        &app,
        &format!("/api/v1/offers/{offer_id}/submit"),
        &token,
        json!({}),
    )
    .await;
    assert_eq!(submit_status, 200, "offer submit failed: {submit_body}");

    // Create an approval step
    let (status, body) = authed_post(
        &app,
        &format!("/api/v1/offers/{offer_id}/approvals"),
        &token,
        json!({"approver_id": admin_user_id, "step_order": 1}),
    )
    .await;
    assert_eq!(status, 201, "create approval step failed: {body}");
    assert_eq!(
        body["data"]["approver_id"].as_str().unwrap(),
        admin_user_id,
        "approver_id mismatch"
    );
    assert_eq!(
        body["data"]["step_order"].as_i64().unwrap(),
        1,
        "step_order must be 1"
    );
}

/// GET /api/v1/offers/{id}/approvals must return a list with at least one item
/// after a step has been added.
#[actix_rt::test]
#[serial]
async fn list_approvals_for_submitted_offer() {
    test_app!(app, _config);

    let req = test::TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(json!({"username": "platform_admin", "password": "Admin_Pa$$word1!"}))
        .to_request();
    let resp = test::call_service(&app, req).await;
    let login_body: Value = test::read_body_json(resp).await;
    let token = login_body["data"]["token"].as_str().unwrap().to_string();
    let admin_user_id = login_body["data"]["user"]["id"].as_str().unwrap().to_string();

    let uniq = uuid::Uuid::new_v4();
    let (_, cand_body) = authed_post(
        &app,
        "/api/v1/candidates",
        &token,
        json!({
            "first_name": "ListApproval",
            "last_name": "Candidate",
            "email": format!("list.approval.{}@example.com", uniq),
        }),
    )
    .await;
    let candidate_id = cand_body["data"]["id"].as_str().unwrap();

    let (_, offer_body) = authed_post(
        &app,
        "/api/v1/offers",
        &token,
        json!({"candidate_id": candidate_id, "title": "List Approvals Offer"}),
    )
    .await;
    let offer_id = offer_body["data"]["id"].as_str().unwrap();

    let (submit_status, submit_body) = authed_post(
        &app,
        &format!("/api/v1/offers/{offer_id}/submit"),
        &token,
        json!({}),
    )
    .await;
    assert_eq!(submit_status, 200, "submit failed: {submit_body}");

    let (add_status, add_body) = authed_post(
        &app,
        &format!("/api/v1/offers/{offer_id}/approvals"),
        &token,
        json!({"approver_id": admin_user_id, "step_order": 1}),
    )
    .await;
    assert_eq!(add_status, 201, "add approval step failed: {add_body}");

    let (status, body) = authed_get(
        &app,
        &format!("/api/v1/offers/{offer_id}/approvals"),
        &token,
    )
    .await;
    assert_eq!(status, 200, "list approvals failed: {body}");
    let items = body["data"].as_array().expect("data must be array");
    assert!(
        !items.is_empty(),
        "approvals list must contain at least one item"
    );
}

/// PUT /api/v1/offers/{id}/approvals/{step_id} must record an approved decision.
#[actix_rt::test]
#[serial]
async fn decide_approval_approved() {
    test_app!(app, _config);

    let req = test::TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(json!({"username": "platform_admin", "password": "Admin_Pa$$word1!"}))
        .to_request();
    let resp = test::call_service(&app, req).await;
    let login_body: Value = test::read_body_json(resp).await;
    let token = login_body["data"]["token"].as_str().unwrap().to_string();
    let admin_user_id = login_body["data"]["user"]["id"].as_str().unwrap().to_string();

    let uniq = uuid::Uuid::new_v4();
    let (_, cand_body) = authed_post(
        &app,
        "/api/v1/candidates",
        &token,
        json!({
            "first_name": "DecideApproval",
            "last_name": "Candidate",
            "email": format!("decide.approval.{}@example.com", uniq),
        }),
    )
    .await;
    let candidate_id = cand_body["data"]["id"].as_str().unwrap();

    let (_, offer_body) = authed_post(
        &app,
        "/api/v1/offers",
        &token,
        json!({"candidate_id": candidate_id, "title": "Decide Approval Offer"}),
    )
    .await;
    let offer_id = offer_body["data"]["id"].as_str().unwrap();

    let (submit_status, submit_body) = authed_post(
        &app,
        &format!("/api/v1/offers/{offer_id}/submit"),
        &token,
        json!({}),
    )
    .await;
    assert_eq!(submit_status, 200, "submit failed: {submit_body}");

    let (add_status, add_body) = authed_post(
        &app,
        &format!("/api/v1/offers/{offer_id}/approvals"),
        &token,
        json!({"approver_id": admin_user_id, "step_order": 1}),
    )
    .await;
    assert_eq!(add_status, 201, "add approval step failed: {add_body}");
    let step_id = add_body["data"]["id"].as_str().unwrap();

    let (status, body) = authed_put(
        &app,
        &format!("/api/v1/offers/{offer_id}/approvals/{step_id}"),
        &token,
        json!({"decision": "approved"}),
    )
    .await;
    assert_eq!(status, 200, "decide approval failed: {body}");
    assert_eq!(
        body["data"]["decision"].as_str().unwrap(),
        "approved",
        "decision must be 'approved'"
    );
}

/// GET /api/v1/offers/{id}?reveal_compensation=true must return the compensation object.
#[actix_rt::test]
#[serial]
async fn get_offer_with_reveal_compensation() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    let uniq = uuid::Uuid::new_v4();
    let (_, cand_body) = authed_post(
        &app,
        "/api/v1/candidates",
        &token,
        json!({
            "first_name": "CompReveal",
            "last_name": "Candidate",
            "email": format!("comp.reveal.{}@example.com", uniq),
        }),
    )
    .await;
    let candidate_id = cand_body["data"]["id"].as_str().unwrap();

    let (offer_status, offer_body) = authed_post(
        &app,
        "/api/v1/offers",
        &token,
        json!({
            "candidate_id": candidate_id,
            "title": "Reveal Compensation Offer",
            "compensation": {
                "base_salary_usd": 90000,
                "bonus_target_pct": 10.0,
                "equity_units": 100,
                "pto_days": 20,
                "k401_match_pct": 4.0
            }
        }),
    )
    .await;
    assert_eq!(offer_status, 201, "offer create failed: {offer_body}");
    let offer_id = offer_body["data"]["id"].as_str().unwrap();

    let (status, body) = authed_get(
        &app,
        &format!("/api/v1/offers/{offer_id}?reveal_compensation=true"),
        &token,
    )
    .await;
    assert_eq!(status, 200, "get offer with reveal_compensation failed: {body}");
    assert!(
        !body["data"]["compensation"].is_null(),
        "compensation must be present when reveal_compensation=true; body={body}"
    );
}

// ── User Management tests ─────────────────────────────────────────────────────

/// GET /api/v1/users/{id} must return the user's details.
#[actix_rt::test]
#[serial]
async fn get_user_detail_as_admin() {
    test_app!(app, _config);

    let req = test::TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(json!({"username": "platform_admin", "password": "Admin_Pa$$word1!"}))
        .to_request();
    let resp = test::call_service(&app, req).await;
    let login_body: Value = test::read_body_json(resp).await;
    let token = login_body["data"]["token"].as_str().unwrap().to_string();
    let user_id = login_body["data"]["user"]["id"].as_str().unwrap().to_string();

    let (status, body) = authed_get(&app, &format!("/api/v1/users/{user_id}"), &token).await;
    assert_eq!(status, 200, "get user detail failed: {body}");
    assert_eq!(
        body["data"]["username"].as_str().unwrap(),
        "platform_admin",
        "username mismatch"
    );
}

/// POST /api/v1/users must create a new user and return 201.
#[actix_rt::test]
#[serial]
async fn create_user_returns_201() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    let uniq = uuid::Uuid::new_v4();
    let username = format!("testuser_{}", &uniq.to_string()[..8]);
    let email = format!("test.{}@example.com", uniq);

    let (status, body) = authed_post(
        &app,
        "/api/v1/users",
        &token,
        json!({
            "username": username,
            "email": email,
            "password": "TestP@ss1!Word99",
            "display_name": "Test User",
        }),
    )
    .await;
    assert_eq!(status, 201, "create user failed: {body}");
    assert!(
        body["data"]["username"]
            .as_str()
            .unwrap_or("")
            .starts_with("testuser_"),
        "username must start with 'testuser_'; got: {}",
        body["data"]["username"]
    );
}

/// PUT /api/v1/users/{id} must update display_name and return 200.
#[actix_rt::test]
#[serial]
async fn update_user_display_name() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    // Create a fresh user to update
    let uniq = uuid::Uuid::new_v4();
    let email = format!("update.user.{}@example.com", uniq);
    let (create_status, create_body) = authed_post(
        &app,
        "/api/v1/users",
        &token,
        json!({
            "username": format!("upduser_{}", &uniq.to_string()[..8]),
            "email": email,
            "password": "TestP@ss1!Word99",
            "display_name": "Original Name",
        }),
    )
    .await;
    assert_eq!(create_status, 201, "create user failed: {create_body}");
    let new_user_id = create_body["data"]["id"].as_str().unwrap();

    let (status, body) = authed_put(
        &app,
        &format!("/api/v1/users/{new_user_id}"),
        &token,
        json!({
            "display_name": "Updated Name",
            "email": email,
        }),
    )
    .await;
    assert_eq!(status, 200, "update user failed: {body}");
    assert_eq!(
        body["data"]["display_name"].as_str().unwrap(),
        "Updated Name",
        "display_name must be updated"
    );
}

/// GET /api/v1/users/{id}/roles must return an array for a user with roles.
#[actix_rt::test]
#[serial]
async fn list_user_roles_returns_array() {
    test_app!(app, _config);

    let req = test::TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(json!({"username": "platform_admin", "password": "Admin_Pa$$word1!"}))
        .to_request();
    let resp = test::call_service(&app, req).await;
    let login_body: Value = test::read_body_json(resp).await;
    let token = login_body["data"]["token"].as_str().unwrap().to_string();
    let user_id = login_body["data"]["user"]["id"].as_str().unwrap().to_string();

    let (status, body) = authed_get(
        &app,
        &format!("/api/v1/users/{user_id}/roles"),
        &token,
    )
    .await;
    assert_eq!(status, 200, "list user roles failed: {body}");
    assert!(
        body["data"].is_array(),
        "roles must be returned as an array; got: {body}"
    );
}

/// GET /api/v1/users?page=1&per_page=10 must return paginated user list.
#[actix_rt::test]
#[serial]
async fn list_users_paginated() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    let (status, body) = authed_get(&app, "/api/v1/users?page=1&per_page=10", &token).await;
    assert_eq!(status, 200, "list users failed: {body}");
    assert!(
        body["data"].is_array(),
        "response must have 'data' array; got: {body}"
    );
    assert!(
        body["pagination"].is_object(),
        "response must have 'pagination' object; got: {body}"
    );
}

// ── Reporting subscription tests ──────────────────────────────────────────────

/// PUT /api/v1/reporting/subscriptions/{id} must update is_active and return 200.
#[actix_rt::test]
#[serial]
async fn update_reporting_subscription_returns_200() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    // Create a subscription first
    let (create_status, create_body) = authed_post(
        &app,
        "/api/v1/reporting/subscriptions",
        &token,
        json!({"report_type": "snapshot", "parameters": {}}),
    )
    .await;
    assert_eq!(
        create_status, 201,
        "create subscription failed: {create_body}"
    );
    let sub_id = create_body["data"]["id"].as_str().unwrap();

    let (status, body) = authed_put(
        &app,
        &format!("/api/v1/reporting/subscriptions/{sub_id}"),
        &token,
        json!({"is_active": false}),
    )
    .await;
    assert_eq!(status, 200, "update subscription failed: {body}");
    assert_eq!(
        body["data"]["is_active"].as_bool().unwrap(),
        false,
        "is_active must be false after update"
    );
}

// ── Connector update tests ────────────────────────────────────────────────────

/// PUT /api/v1/integrations/connectors/{id} must update the connector name.
#[actix_rt::test]
#[serial]
async fn update_connector_name_returns_200() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    let uniq = uuid::Uuid::new_v4();
    let (create_status, create_body) = authed_post(
        &app,
        "/api/v1/integrations/connectors",
        &token,
        json!({
            "name": format!("Test Connector {}", uniq),
            "connector_type": "inbound",
            "is_enabled": true,
        }),
    )
    .await;
    assert_eq!(
        create_status, 201,
        "create connector failed: {create_body}"
    );
    let connector_id = create_body["data"]["id"].as_str().unwrap();

    let (status, body) = authed_put(
        &app,
        &format!("/api/v1/integrations/connectors/{connector_id}"),
        &token,
        json!({"name": "Updated Connector"}),
    )
    .await;
    assert_eq!(status, 200, "update connector failed: {body}");
    assert_eq!(
        body["data"]["name"].as_str().unwrap(),
        "Updated Connector",
        "connector name must be updated"
    );
}
