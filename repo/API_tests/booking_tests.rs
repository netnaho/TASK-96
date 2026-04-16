/// API-level integration tests for the booking workflow.
///
/// ## Prerequisites
///
/// ```bash
/// DATABASE_URL=postgres://talentflow:talentflow_dev@localhost:5433/talentflow_test \
///   ENCRYPTION_KEY="$(openssl rand -base64 32)" \
///   cargo test --test booking_tests
/// ```
///
/// Tests run sequentially. The seeded `platform_admin` user is used for most tests.
use actix_web::{test, web, App};
use diesel::prelude::*;
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

// ── Helpers ──────────────────────────────────────────────────────────────────

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
        .expect("login failed")
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
    authed_post_with_headers(app, uri, token, body, vec![]).await
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

/// Seed a booking_slot directly via SQL for testing.
async fn seed_slot(
    app: &impl actix_web::dev::Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
    >,
    config: &AppConfig,
) -> (String, String) {
    // We need to insert a slot using the DB pool directly
    let pool = db::create_pool(&config.database_url);
    let mut conn = pool.get().expect("failed to get conn");
    use diesel::prelude::*;
    use diesel::sql_query;

    // Get a site_id
    let site: Vec<(uuid::Uuid,)> = diesel::sql_query("SELECT id FROM office_sites LIMIT 1")
        .load::<SiteIdRow>(&mut conn)
        .unwrap_or_default()
        .into_iter()
        .map(|r| (r.id,))
        .collect();

    let site_id = if site.is_empty() {
        // Insert a site
        let sid = uuid::Uuid::new_v4();
        let code = format!("T{}", &sid.to_string()[..8]);
        diesel::sql_query(format!(
            "INSERT INTO office_sites (id, code, name, timezone) \
             VALUES ('{sid}', '{code}', 'Test Site', 'UTC') \
             ON CONFLICT (code) DO NOTHING"
        ))
        .execute(&mut conn)
        .unwrap();
        sid
    } else {
        site[0].0
    };

    // Insert a slot with a unique date to avoid conflicts across test runs
    let slot_id = uuid::Uuid::new_v4();
    let unique_date = format!(
        "2099-{:02}-{:02}",
        (slot_id.as_bytes()[0] % 12) + 1,
        (slot_id.as_bytes()[1] % 28) + 1
    );
    diesel::sql_query(format!(
        "INSERT INTO booking_slots (id, site_id, slot_date, start_time, end_time, capacity) \
         VALUES ('{slot_id}', '{site_id}', '{unique_date}', '09:00', '10:00', 2)"
    ))
    .execute(&mut conn)
    .unwrap();

    (site_id.to_string(), slot_id.to_string())
}

// Helper struct for QueryableByName
#[derive(diesel::QueryableByName)]
struct SiteIdRow {
    #[diesel(sql_type = diesel::sql_types::Uuid)]
    id: uuid::Uuid,
}

/// Create a candidate for booking tests
async fn seed_candidate(
    app: &impl actix_web::dev::Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
    >,
    token: &str,
) -> String {
    let uniq = uuid::Uuid::new_v4().to_string();
    let (status, body) = authed_post(
        app,
        "/api/v1/candidates",
        token,
        json!({
            "first_name": "Book",
            "last_name": "Cand",
            "email": format!("book.cand.{uniq}@example.com")
        }),
    )
    .await;
    assert_eq!(status, 201, "candidate create failed: {body}");
    body["data"]["id"].as_str().unwrap().to_string()
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[actix_rt::test]
#[serial]
async fn create_hold_returns_pending_confirmation() {
    test_app!(app, config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;
    let candidate_id = seed_candidate(&app, &token).await;
    let (site_id, slot_id) = seed_slot(&app, &config).await;

    let (status, body) = authed_post(
        &app,
        "/api/v1/bookings",
        &token,
        json!({
            "candidate_id": candidate_id,
            "site_id": site_id,
            "slot_id": slot_id,
        }),
    )
    .await;

    assert_eq!(status, 201, "body: {body}");
    assert_eq!(body["data"]["status"], "pending_confirmation");
    assert!(body["data"]["hold_expires_at"].is_string());
    assert_eq!(body["data"]["slot_id"], slot_id);
}

#[actix_rt::test]
#[serial]
async fn overbooking_prevented() {
    test_app!(app, config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    // Create a slot with capacity=1
    let pool = db::create_pool(&config.database_url);
    let mut conn = pool.get().unwrap();
    let site_id = uuid::Uuid::new_v4();
    let slot_id = uuid::Uuid::new_v4();
    let ovr_code = format!("O{}", &site_id.to_string()[..8]);
    diesel::sql_query(format!(
        "INSERT INTO office_sites (id, code, name, timezone) \
         VALUES ('{site_id}', '{ovr_code}', 'Overbook Test', 'UTC')"
    ))
    .execute(&mut conn)
    .unwrap();
    let ovr_date = format!(
        "2098-{:02}-{:02}",
        (slot_id.as_bytes()[0] % 12) + 1,
        (slot_id.as_bytes()[1] % 28) + 1
    );
    diesel::sql_query(format!(
        "INSERT INTO booking_slots (id, site_id, slot_date, start_time, end_time, capacity) \
         VALUES ('{slot_id}', '{site_id}', '{ovr_date}', '09:00', '10:00', 1)"
    ))
    .execute(&mut conn)
    .unwrap();

    let cand1 = seed_candidate(&app, &token).await;
    let cand2 = seed_candidate(&app, &token).await;

    // First booking succeeds
    let (s1, _) = authed_post(
        &app,
        "/api/v1/bookings",
        &token,
        json!({"candidate_id": cand1, "site_id": site_id.to_string(), "slot_id": slot_id.to_string()}),
    )
    .await;
    assert_eq!(s1, 201);

    // Second booking should fail — slot full
    let (s2, body2) = authed_post(
        &app,
        "/api/v1/bookings",
        &token,
        json!({"candidate_id": cand2, "site_id": site_id.to_string(), "slot_id": slot_id.to_string()}),
    )
    .await;
    assert_eq!(s2, 409, "expected conflict, got: {body2}");
}

#[actix_rt::test]
#[serial]
async fn confirm_blocked_without_agreement() {
    test_app!(app, config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;
    let candidate_id = seed_candidate(&app, &token).await;
    let (site_id, slot_id) = seed_slot(&app, &config).await;

    // Create hold
    let (_, hold_body) = authed_post(
        &app,
        "/api/v1/bookings",
        &token,
        json!({"candidate_id": candidate_id, "site_id": site_id, "slot_id": slot_id}),
    )
    .await;
    let booking_id = hold_body["data"]["id"].as_str().unwrap();

    // Try to confirm without submitting agreement — should fail eligibility
    let (status, body) = authed_post(
        &app,
        &format!("/api/v1/bookings/{booking_id}/confirm"),
        &token,
        json!({}),
    )
    .await;
    assert_eq!(status, 422, "body: {body}");
    assert_eq!(body["error"]["code"], "eligibility_failed");
}

#[actix_rt::test]
#[serial]
async fn invalid_state_transition_rejected() {
    test_app!(app, config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;
    let candidate_id = seed_candidate(&app, &token).await;
    let (site_id, slot_id) = seed_slot(&app, &config).await;

    let (_, hold_body) = authed_post(
        &app,
        "/api/v1/bookings",
        &token,
        json!({"candidate_id": candidate_id, "site_id": site_id, "slot_id": slot_id}),
    )
    .await;
    let booking_id = hold_body["data"]["id"].as_str().unwrap();

    // Try to start a pending_confirmation booking — invalid
    let (status, body) = authed_post(
        &app,
        &format!("/api/v1/bookings/{booking_id}/start"),
        &token,
        json!({}),
    )
    .await;
    assert_eq!(status, 409, "body: {body}");
    assert_eq!(body["error"]["code"], "invalid_state_transition");
}

#[actix_rt::test]
#[serial]
async fn idempotent_duplicate_hold() {
    test_app!(app, config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;
    let candidate_id = seed_candidate(&app, &token).await;
    let (site_id, slot_id) = seed_slot(&app, &config).await;

    let key = uuid::Uuid::new_v4().to_string();
    let payload = json!({
        "candidate_id": candidate_id,
        "site_id": site_id,
        "slot_id": slot_id,
    });

    // First request
    let (s1, b1) = authed_post_with_headers(
        &app,
        "/api/v1/bookings",
        &token,
        payload.clone(),
        vec![("Idempotency-Key", &key)],
    )
    .await;
    assert_eq!(s1, 201);

    // Second request with same key — should return same booking
    let (s2, b2) = authed_post_with_headers(
        &app,
        "/api/v1/bookings",
        &token,
        payload,
        vec![("Idempotency-Key", &key)],
    )
    .await;
    assert_eq!(s2, 201);
    assert_eq!(b1["data"]["id"], b2["data"]["id"]);
}

#[actix_rt::test]
#[serial]
async fn unauthenticated_booking_returns_401() {
    test_app!(app, _config);

    let req = test::TestRequest::post()
        .uri("/api/v1/bookings")
        .set_json(json!({"candidate_id": "x", "site_id": "y", "slot_id": "z"}))
        .to_request();
    // Middleware returns Err on missing auth, so use Service::call directly
    use actix_web::dev::Service;
    let status = match app.call(req).await {
        Ok(resp) => resp.status().as_u16(),
        Err(err) => err.error_response().status().as_u16(),
    };
    assert_eq!(status, 401);
}

#[actix_rt::test]
#[serial]
async fn member_cannot_see_other_bookings() {
    test_app!(app, config);
    // Admin creates a booking
    let admin_token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;
    let candidate_id = seed_candidate(&app, &admin_token).await;
    let (site_id, slot_id) = seed_slot(&app, &config).await;

    let (_, hold_body) = authed_post(
        &app,
        "/api/v1/bookings",
        &admin_token,
        json!({"candidate_id": candidate_id, "site_id": site_id, "slot_id": slot_id}),
    )
    .await;
    let booking_id = hold_body["data"]["id"].as_str().unwrap();

    // Member tries to access it
    let member_token = login_as(&app, "member", "Member!User1Passw0rd").await;
    let (status, body) = authed_get(
        &app,
        &format!("/api/v1/bookings/{booking_id}"),
        &member_token,
    )
    .await;
    assert_eq!(status, 403, "body: {body}");
}

#[actix_rt::test]
#[serial]
async fn cancel_non_breach_outside_24h() {
    test_app!(app, config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;
    let candidate_id = seed_candidate(&app, &token).await;
    let (site_id, slot_id) = seed_slot(&app, &config).await;

    // Slot is in 2030 — well outside 24h
    let (_, hold_body) = authed_post(
        &app,
        "/api/v1/bookings",
        &token,
        json!({"candidate_id": candidate_id, "site_id": site_id, "slot_id": slot_id}),
    )
    .await;
    let booking_id = hold_body["data"]["id"].as_str().unwrap();

    // Cancel without reason — should succeed (non-breach)
    let (status, body) = authed_post(
        &app,
        &format!("/api/v1/bookings/{booking_id}/cancel"),
        &token,
        json!({}),
    )
    .await;
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["data"]["status"], "cancelled");
    assert!(body["data"]["breach_reason"].is_null());
}

#[actix_rt::test]
#[serial]
async fn list_bookings_paginated() {
    test_app!(app, config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    let (status, body) = authed_get(&app, "/api/v1/bookings?page=1&per_page=10", &token).await;
    assert_eq!(status, 200, "body: {body}");
    assert!(body["data"].is_array());
    assert!(body["pagination"]["total"].is_number());
}

#[actix_rt::test]
#[serial]
async fn get_booking_detail() {
    test_app!(app, config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;
    let candidate_id = seed_candidate(&app, &token).await;
    let (site_id, slot_id) = seed_slot(&app, &config).await;

    let (_, hold_body) = authed_post(
        &app,
        "/api/v1/bookings",
        &token,
        json!({"candidate_id": candidate_id, "site_id": site_id, "slot_id": slot_id}),
    )
    .await;
    let booking_id = hold_body["data"]["id"].as_str().unwrap();

    let (status, body) = authed_get(&app, &format!("/api/v1/bookings/{booking_id}"), &token).await;
    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["data"]["id"], booking_id);
    assert_eq!(body["data"]["status"], "pending_confirmation");
}

// ── Canonical idempotency tests ───────────────────────────────────────────────

/// Replaying a create-hold request with the same key and same payload must return
/// 201 with the same booking ID (not a duplicate).
#[actix_rt::test]
#[serial]
async fn create_hold_idempotent_replay_same_payload() {
    test_app!(app, config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;
    let candidate_id = seed_candidate(&app, &token).await;
    let (site_id, slot_id) = seed_slot(&app, &config).await;

    let idem_key = format!("test-idem-replay-{}", uuid::Uuid::new_v4());
    let payload = json!({
        "candidate_id": candidate_id,
        "site_id": site_id,
        "slot_id": slot_id,
    });

    // First request
    let (status1, body1) = authed_post_with_headers(
        &app,
        "/api/v1/bookings",
        &token,
        payload.clone(),
        vec![("Idempotency-Key", &idem_key)],
    )
    .await;
    assert_eq!(status1, 201, "first create-hold must succeed: {body1}");
    let booking_id = body1["data"]["id"].as_str().unwrap().to_string();

    // Replay with same key + same payload
    let (status2, body2) = authed_post_with_headers(
        &app,
        "/api/v1/bookings",
        &token,
        payload,
        vec![("Idempotency-Key", &idem_key)],
    )
    .await;
    assert_eq!(status2, 201, "idempotent replay must return 201: {body2}");
    assert_eq!(
        body2["data"]["id"].as_str().unwrap(),
        booking_id,
        "replay must return the same booking ID"
    );
}

/// Replaying a create-hold request with the same key but a different slot must
/// return 409 with code `idempotency_conflict`.
#[actix_rt::test]
#[serial]
async fn create_hold_idempotent_conflict_different_payload() {
    test_app!(app, config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;
    let candidate_id = seed_candidate(&app, &token).await;
    let (site_id, slot_id_a) = seed_slot(&app, &config).await;
    let (_, slot_id_b) = seed_slot(&app, &config).await;

    let idem_key = format!("test-idem-conflict-{}", uuid::Uuid::new_v4());

    // First request with slot_id_a
    let (status1, body1) = authed_post_with_headers(
        &app,
        "/api/v1/bookings",
        &token,
        json!({
            "candidate_id": candidate_id,
            "site_id": site_id,
            "slot_id": slot_id_a,
        }),
        vec![("Idempotency-Key", &idem_key)],
    )
    .await;
    assert_eq!(status1, 201, "first request must succeed: {body1}");

    // Second request with same key but different slot_id → conflict
    let (status2, body2) = authed_post_with_headers(
        &app,
        "/api/v1/bookings",
        &token,
        json!({
            "candidate_id": candidate_id,
            "site_id": site_id,
            "slot_id": slot_id_b,
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

/// A request without an Idempotency-Key header must never be checked against
/// the canonical store — two identical requests create two bookings.
#[actix_rt::test]
#[serial]
async fn create_hold_no_idempotency_key_creates_duplicate() {
    test_app!(app, config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;
    let candidate_id = seed_candidate(&app, &token).await;
    let (site_id, slot_id) = seed_slot(&app, &config).await;

    let payload = json!({
        "candidate_id": candidate_id,
        "site_id": site_id,
        "slot_id": slot_id,
    });

    let (status1, body1) = authed_post(&app, "/api/v1/bookings", &token, payload.clone()).await;
    let (status2, body2) = authed_post(&app, "/api/v1/bookings", &token, payload).await;

    // Both may succeed (slot capacity ≥ 2) or the second may fail with 409 capacity
    // conflict — either way the two booking IDs must differ if both succeed.
    if status1 == 201 && status2 == 201 {
        assert_ne!(
            body1["data"]["id"], body2["data"]["id"],
            "without idempotency key, two requests must produce distinct bookings"
        );
    }
}

// ── Submit-agreement idempotency tests ────────────────────────────────────────

/// Replaying a submit_agreement request with the same Idempotency-Key and same
/// payload must return 200 with the same agreement evidence.
#[actix_rt::test]
#[serial]
async fn submit_agreement_idempotent_replay_same_payload() {
    test_app!(app, config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;
    let candidate_id = seed_candidate(&app, &token).await;
    let (site_id, slot_id) = seed_slot(&app, &config).await;

    // Create a hold first
    let (status, body) = authed_post(
        &app,
        "/api/v1/bookings",
        &token,
        json!({
            "candidate_id": candidate_id,
            "site_id": site_id,
            "slot_id": slot_id,
        }),
    )
    .await;
    assert_eq!(status, 201, "create hold must succeed: {body}");
    let booking_id = body["data"]["id"].as_str().unwrap().to_string();

    let idem_key = format!("test-agree-replay-{}", uuid::Uuid::new_v4());
    let payload = json!({ "typed_name": "John Q. Public" });

    // First submit_agreement
    let (status1, body1) = authed_post_with_headers(
        &app,
        &format!("/api/v1/bookings/{booking_id}/agreement"),
        &token,
        payload.clone(),
        vec![("Idempotency-Key", &idem_key)],
    )
    .await;
    assert_eq!(status1, 200, "first agreement submit must succeed: {body1}");
    let hash1 = body1["data"]["hash"].as_str().unwrap().to_string();

    // Replay with same key + same payload
    let (status2, body2) = authed_post_with_headers(
        &app,
        &format!("/api/v1/bookings/{booking_id}/agreement"),
        &token,
        payload,
        vec![("Idempotency-Key", &idem_key)],
    )
    .await;
    assert_eq!(status2, 200, "idempotent replay must return 200: {body2}");
    assert_eq!(
        body2["data"]["hash"].as_str().unwrap(),
        hash1,
        "replay must return the same agreement hash"
    );
}

/// Replaying a submit_agreement request with the same key but a different
/// typed_name must return 409 with code `idempotency_conflict`.
#[actix_rt::test]
#[serial]
async fn submit_agreement_idempotent_conflict_different_payload() {
    test_app!(app, config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;
    let candidate_id = seed_candidate(&app, &token).await;
    let (site_id, slot_id) = seed_slot(&app, &config).await;

    // Create a hold
    let (status, body) = authed_post(
        &app,
        "/api/v1/bookings",
        &token,
        json!({
            "candidate_id": candidate_id,
            "site_id": site_id,
            "slot_id": slot_id,
        }),
    )
    .await;
    assert_eq!(status, 201, "create hold must succeed: {body}");
    let booking_id = body["data"]["id"].as_str().unwrap().to_string();

    let idem_key = format!("test-agree-conflict-{}", uuid::Uuid::new_v4());

    // First submit with typed_name "Alice A."
    let (status1, body1) = authed_post_with_headers(
        &app,
        &format!("/api/v1/bookings/{booking_id}/agreement"),
        &token,
        json!({ "typed_name": "Alice A." }),
        vec![("Idempotency-Key", &idem_key)],
    )
    .await;
    assert_eq!(status1, 200, "first agreement submit must succeed: {body1}");

    // Second submit with same key but different typed_name → conflict
    let (status2, body2) = authed_post_with_headers(
        &app,
        &format!("/api/v1/bookings/{booking_id}/agreement"),
        &token,
        json!({ "typed_name": "Bob B." }),
        vec![("Idempotency-Key", &idem_key)],
    )
    .await;
    assert_eq!(status2, 409, "different payload must produce 409: {body2}");
    assert_eq!(
        body2["error"]["code"], "idempotency_conflict",
        "error code must be idempotency_conflict, got: {body2}"
    );
}

// ── Pagination count isolation tests ──────────────────────────────────────────

/// A member user listing bookings must see a total count that reflects only
/// their own bookings. Admin-created bookings must not be counted.
#[actix_rt::test]
#[serial]
async fn member_booking_list_total_excludes_other_users_records() {
    test_app!(app, config);
    let admin_token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;
    let member_token = login_as(&app, "member", "Member!User1Passw0rd").await;

    // Admin creates a booking
    let candidate_id = seed_candidate(&app, &admin_token).await;
    let (site_id, slot_id) = seed_slot(&app, &config).await;

    let (hold_status, hold_body) = authed_post(
        &app,
        "/api/v1/bookings",
        &admin_token,
        json!({
            "candidate_id": candidate_id,
            "site_id": site_id,
            "slot_id": slot_id,
        }),
    )
    .await;
    assert_eq!(hold_status, 201, "admin create-hold failed: {hold_body}");

    // Member lists bookings — total must not include admin's booking
    let (member_status, member_body) = authed_get(
        &app,
        "/api/v1/bookings?page=1&per_page=100",
        &member_token,
    )
    .await;
    assert_eq!(member_status, 200, "member list failed: {member_body}");

    let member_total = member_body["pagination"]["total"].as_i64().unwrap();
    let member_data = member_body["data"].as_array().unwrap();

    assert_eq!(
        member_total,
        member_data.len() as i64,
        "member booking total must equal returned items (no count leak); total={member_total}, items={}",
        member_data.len()
    );

    // Admin lists bookings — must see at least the one just created
    let (admin_status, admin_body) = authed_get(
        &app,
        "/api/v1/bookings?page=1&per_page=100",
        &admin_token,
    )
    .await;
    assert_eq!(admin_status, 200);
    let admin_total = admin_body["pagination"]["total"].as_i64().unwrap();
    assert!(
        admin_total >= 1,
        "admin must see at least the booking they created; total={admin_total}"
    );
}

// ── New endpoint coverage tests ───────────────────────────────────────────────

/// POST /api/v1/bookings/{id}/start — transitions confirmed → in_progress
#[actix_rt::test]
#[serial]
async fn start_confirmed_booking_transitions_to_in_progress() {
    test_app!(app, config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;
    let candidate_id = seed_candidate(&app, &token).await;
    let (site_id, slot_id) = seed_slot(&app, &config).await;

    // Create hold
    let (_, hold_body) = authed_post(
        &app,
        "/api/v1/bookings",
        &token,
        json!({"candidate_id": candidate_id, "site_id": site_id, "slot_id": slot_id}),
    )
    .await;
    let booking_id = hold_body["data"]["id"].as_str().unwrap().to_string();

    // Bypass eligibility gate: set status = 'confirmed' directly in DB
    let pool = db::create_pool(&config.database_url);
    let mut conn = pool.get().expect("failed to get conn");
    diesel::sql_query(format!(
        "UPDATE booking_orders SET status = 'confirmed' WHERE id = '{booking_id}'"
    ))
    .execute(&mut conn)
    .unwrap();

    // POST /start — no request body required
    let req = test::TestRequest::post()
        .uri(&format!("/api/v1/bookings/{booking_id}/start"))
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    let status = resp.status().as_u16();
    let body: Value = test::read_body_json(resp).await;

    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["data"]["status"], "in_progress");
}

/// POST /api/v1/bookings/{id}/complete — transitions in_progress → completed
#[actix_rt::test]
#[serial]
async fn complete_in_progress_booking_transitions_to_completed() {
    test_app!(app, config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;
    let candidate_id = seed_candidate(&app, &token).await;
    let (site_id, slot_id) = seed_slot(&app, &config).await;

    // Create hold
    let (_, hold_body) = authed_post(
        &app,
        "/api/v1/bookings",
        &token,
        json!({"candidate_id": candidate_id, "site_id": site_id, "slot_id": slot_id}),
    )
    .await;
    let booking_id = hold_body["data"]["id"].as_str().unwrap().to_string();

    // Set status = 'in_progress' directly in DB
    let pool = db::create_pool(&config.database_url);
    let mut conn = pool.get().expect("failed to get conn");
    diesel::sql_query(format!(
        "UPDATE booking_orders SET status = 'in_progress' WHERE id = '{booking_id}'"
    ))
    .execute(&mut conn)
    .unwrap();

    // POST /complete — no request body required
    let req = test::TestRequest::post()
        .uri(&format!("/api/v1/bookings/{booking_id}/complete"))
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    let status = resp.status().as_u16();
    let body: Value = test::read_body_json(resp).await;

    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["data"]["status"], "completed");
}

/// POST /api/v1/bookings/{id}/reschedule — moves booking to a new slot
#[actix_rt::test]
#[serial]
async fn reschedule_booking_to_new_slot() {
    test_app!(app, config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;
    let candidate_id = seed_candidate(&app, &token).await;
    let (site_id, slot_id) = seed_slot(&app, &config).await;

    // Create hold on the original slot (far-future date via seed_slot)
    let (_, hold_body) = authed_post(
        &app,
        "/api/v1/bookings",
        &token,
        json!({"candidate_id": candidate_id, "site_id": site_id, "slot_id": slot_id}),
    )
    .await;
    let booking_id = hold_body["data"]["id"].as_str().unwrap().to_string();

    // Seed a second slot to reschedule into
    let (_, new_slot_id) = seed_slot(&app, &config).await;

    let (status, body) = authed_post(
        &app,
        &format!("/api/v1/bookings/{booking_id}/reschedule"),
        &token,
        json!({"new_slot_id": new_slot_id}),
    )
    .await;

    assert_eq!(status, 200, "body: {body}");
    assert_eq!(
        body["data"]["slot_id"].as_str().unwrap(),
        new_slot_id,
        "slot_id must be updated to the new slot"
    );
}

/// POST /api/v1/bookings/{id}/exception — marks booking with exception status.
/// Exception is only allowed from Confirmed or InProgress; advance to in_progress via SQL.
#[actix_rt::test]
#[serial]
async fn mark_exception_on_booking() {
    test_app!(app, config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;
    let candidate_id = seed_candidate(&app, &token).await;
    let (site_id, slot_id) = seed_slot(&app, &config).await;

    // Create hold
    let (_, hold_body) = authed_post(
        &app,
        "/api/v1/bookings",
        &token,
        json!({"candidate_id": candidate_id, "site_id": site_id, "slot_id": slot_id}),
    )
    .await;
    let booking_id = hold_body["data"]["id"].as_str().unwrap().to_string();

    // Advance to in_progress via SQL — state machine: PendingConfirmation → Exception
    // is invalid; Exception is only reachable from Confirmed or InProgress.
    let pool = db::create_pool(&config.database_url);
    let mut conn = pool.get().expect("failed to get conn");
    diesel::sql_query(format!(
        "UPDATE booking_orders SET status = 'in_progress' WHERE id = '{booking_id}'"
    ))
    .execute(&mut conn)
    .unwrap();

    let (status, body) = authed_post(
        &app,
        &format!("/api/v1/bookings/{booking_id}/exception"),
        &token,
        json!({"detail": "Equipment failure during appointment"}),
    )
    .await;

    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["data"]["status"], "exception");
}

/// POST /api/v1/bookings/{id}/cancel within 24h without reason_code returns 422
#[actix_rt::test]
#[serial]
async fn cancel_within_24h_without_reason_code_returns_422() {
    test_app!(app, config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;
    let candidate_id = seed_candidate(&app, &token).await;
    let (site_id, slot_id) = seed_slot(&app, &config).await;

    // Create hold
    let (_, hold_body) = authed_post(
        &app,
        "/api/v1/bookings",
        &token,
        json!({"candidate_id": candidate_id, "site_id": site_id, "slot_id": slot_id}),
    )
    .await;
    let booking_id = hold_body["data"]["id"].as_str().unwrap().to_string();

    // Force the booking into breach territory:
    // Set scheduled_date = TODAY so start = today 09:00; cutoff = yesterday 09:00.
    // Since now > cutoff, the 24h breach window is active.
    // Also advance status to 'confirmed' — breach rules only apply to bookings that
    // have moved past the hold stage (PendingConfirmation cancels freely).
    let pool = db::create_pool(&config.database_url);
    let mut conn = pool.get().expect("failed to get conn");
    diesel::sql_query(format!(
        "UPDATE booking_orders \
         SET status = 'confirmed', \
             scheduled_date = CURRENT_DATE \
         WHERE id = '{booking_id}'"
    ))
    .execute(&mut conn)
    .unwrap();

    // Attempt cancel without reason_code — should be rejected (breach, reason_code required)
    let (status, body) = authed_post(
        &app,
        &format!("/api/v1/bookings/{booking_id}/cancel"),
        &token,
        json!({"reason": "emergency"}),
    )
    .await;

    assert_eq!(status, 422, "expected 422 validation_error, got: {body}");
}

/// POST /api/v1/bookings/{id}/cancel within 24h with valid reason_code succeeds
#[actix_rt::test]
#[serial]
async fn cancel_within_24h_with_valid_reason_code_succeeds() {
    test_app!(app, config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;
    let candidate_id = seed_candidate(&app, &token).await;
    let (site_id, slot_id) = seed_slot(&app, &config).await;

    // Create hold
    let (_, hold_body) = authed_post(
        &app,
        "/api/v1/bookings",
        &token,
        json!({"candidate_id": candidate_id, "site_id": site_id, "slot_id": slot_id}),
    )
    .await;
    let booking_id = hold_body["data"]["id"].as_str().unwrap().to_string();

    // Advance to confirmed + set scheduled_date = TODAY to ensure breach territory.
    let pool = db::create_pool(&config.database_url);
    let mut conn = pool.get().expect("failed to get conn");
    diesel::sql_query(format!(
        "UPDATE booking_orders \
         SET status = 'confirmed', \
             scheduled_date = CURRENT_DATE \
         WHERE id = '{booking_id}'"
    ))
    .execute(&mut conn)
    .unwrap();

    let (status, body) = authed_post(
        &app,
        &format!("/api/v1/bookings/{booking_id}/cancel"),
        &token,
        json!({"reason": "emergency", "reason_code": "late_cancellation"}),
    )
    .await;

    assert_eq!(status, 200, "body: {body}");
    assert_eq!(body["data"]["status"], "cancelled");
}

/// GET /api/v1/sites — returns 200 with a data array
#[actix_rt::test]
#[serial]
async fn list_sites_returns_200() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    let (status, body) = authed_get(&app, "/api/v1/sites", &token).await;

    assert_eq!(status, 200, "body: {body}");
    assert!(
        body["data"].is_array(),
        "response data must be an array, got: {body}"
    );
}

/// GET /api/v1/sites/{random_uuid} — returns 404 for unknown site
#[actix_rt::test]
#[serial]
async fn get_site_not_found_returns_404() {
    test_app!(app, _config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;
    let random_id = uuid::Uuid::new_v4();

    let (status, body) = authed_get(&app, &format!("/api/v1/sites/{random_id}"), &token).await;

    assert_eq!(status, 404, "body: {body}");
}

/// GET /api/v1/sites/{id} — returns 200 with matching site data
#[actix_rt::test]
#[serial]
async fn get_site_by_id_returns_200() {
    test_app!(app, config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    // seed_slot creates an office_site if none exists and returns its site_id
    let (site_id, _slot_id) = seed_slot(&app, &config).await;

    let (status, body) = authed_get(&app, &format!("/api/v1/sites/{site_id}"), &token).await;

    assert_eq!(status, 200, "body: {body}");
    assert_eq!(
        body["data"]["id"].as_str().unwrap(),
        site_id,
        "returned site id must match requested id"
    );
}
