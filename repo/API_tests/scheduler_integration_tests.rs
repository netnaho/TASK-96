/// Integration tests for scheduler-triggered background behaviors.
///
/// ## Prerequisites
///
/// ```bash
/// DATABASE_URL=postgres://talentflow:talentflow_dev@localhost:5433/talentflow_test \
///   ENCRYPTION_KEY="$(openssl rand -base64 32)" \
///   cargo test --test scheduler_integration_tests
/// ```
///
/// Tests run sequentially. The seeded `platform_admin` user is used for all tests.
use actix_web::{test, web, App};
use diesel::prelude::*;
use serde_json::{json, Value};
use serial_test::serial;
use std::env;

use talentflow::{
    api::{middleware::RequestId, routes},
    application::booking_service::BookingService,
    infrastructure::{
        config::AppConfig,
        db,
        db::repositories::{
            idempotency_repo::PgIdempotencyRepository, session_repo::PgSessionRepository,
        },
    },
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
        .expect("login failed")
        .to_string()
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

/// Seed a booking_slot directly via SQL for testing.
async fn seed_slot(config: &AppConfig) -> (String, String) {
    let pool = db::create_pool(&config.database_url);
    let mut conn = pool.get().expect("failed to get conn");

    // Helper struct for QueryableByName
    #[derive(diesel::QueryableByName)]
    struct SiteIdRow {
        #[diesel(sql_type = diesel::sql_types::Uuid)]
        id: uuid::Uuid,
    }

    let site_rows: Vec<SiteIdRow> =
        diesel::sql_query("SELECT id FROM office_sites LIMIT 1")
            .load(&mut conn)
            .unwrap_or_default();

    let site_id = if site_rows.is_empty() {
        let sid = uuid::Uuid::new_v4();
        let code = format!("S{}", &sid.to_string()[..8]);
        diesel::sql_query(format!(
            "INSERT INTO office_sites (id, code, name, timezone) \
             VALUES ('{sid}', '{code}', 'Scheduler Test Site', 'UTC') \
             ON CONFLICT (code) DO NOTHING"
        ))
        .execute(&mut conn)
        .unwrap();
        sid
    } else {
        site_rows.into_iter().next().unwrap().id
    };

    let slot_id = uuid::Uuid::new_v4();
    let unique_date = format!(
        "2097-{:02}-{:02}",
        (slot_id.as_bytes()[0] % 12) + 1,
        (slot_id.as_bytes()[1] % 28) + 1
    );
    diesel::sql_query(format!(
        "INSERT INTO booking_slots (id, site_id, slot_date, start_time, end_time, capacity) \
         VALUES ('{slot_id}', '{site_id}', '{unique_date}', '10:00', '11:00', 5)"
    ))
    .execute(&mut conn)
    .unwrap();

    (site_id.to_string(), slot_id.to_string())
}

/// Create a candidate via the API and return its ID.
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
            "first_name": "Sched",
            "last_name":  "Cand",
            "email":      format!("sched.cand.{uniq}@example.com")
        }),
    )
    .await;
    assert_eq!(status, 201, "candidate create failed: {body}");
    body["data"]["id"].as_str().unwrap().to_string()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// `BookingService::release_expired_holds` should transition a booking whose
/// `hold_expires_at` is in the past from `pending_confirmation` to `cancelled`.
#[actix_rt::test]
#[serial]
async fn release_expired_holds_cancels_stale_booking() {
    test_app!(app, config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;
    let candidate_id = seed_candidate(&app, &token).await;
    let (site_id, slot_id) = seed_slot(&config).await;

    // 1. Create a hold via the API — it starts in pending_confirmation.
    let (status, body) = authed_post(
        &app,
        "/api/v1/bookings",
        &token,
        json!({
            "candidate_id": candidate_id,
            "site_id":       site_id,
            "slot_id":       slot_id,
        }),
    )
    .await;
    assert_eq!(status, 201, "create hold failed: {body}");
    let booking_id = body["data"]["id"].as_str().unwrap().to_string();

    // 2. Manually expire the hold by setting hold_expires_at to the past.
    let pool = db::create_pool(&config.database_url);
    let mut conn = pool.get().expect("failed to get conn");
    diesel::sql_query(format!(
        "UPDATE booking_orders \
         SET hold_expires_at = now() - INTERVAL '1 hour' \
         WHERE id = '{booking_id}'"
    ))
    .execute(&mut conn)
    .expect("failed to expire hold");

    // 3. Call the service function that the background scheduler uses.
    let released = BookingService::release_expired_holds(&mut conn)
        .expect("release_expired_holds failed");
    assert!(
        released >= 1,
        "expected at least one hold to be released, got {released}"
    );

    // 4. Fetch the booking and verify its status is now cancelled.
    let (get_status, get_body) =
        authed_get(&app, &format!("/api/v1/bookings/{booking_id}"), &token).await;
    assert_eq!(get_status, 200, "get booking failed: {get_body}");
    assert_eq!(
        get_body["data"]["status"], "cancelled",
        "booking should be cancelled after hold expiry; got: {get_body}"
    );
}

/// `PgSessionRepository::delete_expired` should remove sessions whose
/// `expires_at` is in the past, causing subsequent auth requests to fail with 401.
#[actix_rt::test]
#[serial]
async fn delete_expired_sessions_revokes_auth() {
    test_app!(app, config);

    // 1. Log in to obtain a valid session.
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;

    // 2. Verify the token works.
    let (pre_status, _) = authed_get(&app, "/api/v1/bookings?page=1&per_page=1", &token).await;
    assert_eq!(pre_status, 200, "token should be valid before expiry");

    // 3. Expire the session via direct SQL (set expires_at to the past).
    //    We match on a substring of the token hash — easier to target by username.
    let pool = db::create_pool(&config.database_url);
    let mut conn = pool.get().expect("failed to get conn");
    diesel::sql_query(
        "UPDATE sessions \
         SET expires_at = now() - INTERVAL '1 hour' \
         WHERE user_id = (SELECT id FROM users WHERE username = 'platform_admin') \
           AND expires_at > now()",
    )
    .execute(&mut conn)
    .expect("failed to expire sessions");

    // 4. Call the scheduler's cleanup function.
    let pruned = PgSessionRepository::delete_expired(&mut conn)
        .expect("delete_expired failed");
    assert!(pruned >= 1, "expected at least one session to be pruned, got {pruned}");

    // 5. The token should now be rejected.
    //    Auth middleware returns Err rather than Ok(4xx) on missing/invalid session.
    use actix_web::dev::Service;
    let req = test::TestRequest::get()
        .uri("/api/v1/bookings?page=1&per_page=1")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let status = match app.call(req).await {
        Ok(resp) => resp.status().as_u16(),
        Err(err) => err.error_response().status().as_u16(),
    };
    assert_eq!(status, 401, "expired session should yield 401");
}

/// `PgIdempotencyRepository::delete_expired` should prune expired keys so that
/// reusing the same key creates a fresh record rather than replaying the old one.
#[actix_rt::test]
#[serial]
async fn delete_expired_idempotency_keys_allows_fresh_request() {
    test_app!(app, config);
    let token = login_as(&app, "platform_admin", "Admin_Pa$$word1!").await;
    let candidate_id_a = seed_candidate(&app, &token).await;
    let (site_id, slot_id_a) = seed_slot(&config).await;

    let idem_key = format!("sched-idem-{}", uuid::Uuid::new_v4());

    // 1. Create a booking with the idempotency key.
    let (status1, body1) = authed_post_with_headers(
        &app,
        "/api/v1/bookings",
        &token,
        json!({
            "candidate_id": candidate_id_a,
            "site_id":       site_id,
            "slot_id":       slot_id_a,
        }),
        vec![("Idempotency-Key", &idem_key)],
    )
    .await;
    assert_eq!(status1, 201, "first booking failed: {body1}");
    let booking_id_first = body1["data"]["id"].as_str().unwrap().to_string();

    // 2. Expire the idempotency key via direct SQL.
    let pool = db::create_pool(&config.database_url);
    let mut conn = pool.get().expect("failed to get conn");
    diesel::sql_query(format!(
        "UPDATE idempotency_keys \
         SET expires_at = now() - INTERVAL '1 hour' \
         WHERE key = '{idem_key}'"
    ))
    .execute(&mut conn)
    .expect("failed to expire idempotency key");

    // 3. Run the scheduler pruning function.
    let pruned = PgIdempotencyRepository::delete_expired(&mut conn)
        .expect("delete_expired failed");
    assert!(pruned >= 1, "expected at least one key to be pruned, got {pruned}");

    // 4. Reuse the same key with a new slot — should create a *new* booking
    //    (not a replay of the old one) because the old key was pruned.
    let candidate_id_b = seed_candidate(&app, &token).await;
    let (_, slot_id_b) = seed_slot(&config).await;

    let (status2, body2) = authed_post_with_headers(
        &app,
        "/api/v1/bookings",
        &token,
        json!({
            "candidate_id": candidate_id_b,
            "site_id":       site_id,
            "slot_id":       slot_id_b,
        }),
        vec![("Idempotency-Key", &idem_key)],
    )
    .await;
    assert_eq!(status2, 201, "second booking (after key pruned) failed: {body2}");

    let booking_id_second = body2["data"]["id"].as_str().unwrap().to_string();
    assert_ne!(
        booking_id_first, booking_id_second,
        "after pruning the expired key a new record must be created, not a replay"
    );
}
