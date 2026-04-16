/// E2E HTTP tests for the TalentFlow API.
///
/// These tests exercise the real HTTP boundary using `reqwest` against a live
/// running server. The server base URL is provided via the `E2E_BASE_URL`
/// environment variable (e.g. `http://localhost:8080`).
///
/// # Skip behaviour
///
/// - If `E2E_BASE_URL` is set → tests run against that server.
/// - If `E2E_BASE_URL` is **not** set AND `TALENTFLOW_SKIP_E2E_TESTS=1` → each
///   test silently returns (vacuous pass), suitable for CI that does not spin up
///   a server.
/// - If `E2E_BASE_URL` is **not** set AND `TALENTFLOW_SKIP_E2E_TESTS` is also
///   not set → `panic!` with an actionable error message. This prevents the
///   suite from silently passing when someone forgets to configure the
///   environment.
///
/// ```bash
/// # Run against a local server:
/// E2E_BASE_URL=http://localhost:8080 cargo test --test e2e_tests
///
/// # Explicitly skip (vacuous pass):
/// TALENTFLOW_SKIP_E2E_TESTS=1 cargo test --test e2e_tests
/// ```
///
/// Tests are run sequentially (via `serial_test`) to prevent interference.
use reqwest::Client;
use serde_json::{json, Value};
use serial_test::serial;
use std::env;

// ── Skip / base-URL helper ────────────────────────────────────────────────────

/// Return the base URL for the live server.
///
/// - `Some(url)` → test should run.
/// - `None`      → test should skip (vacuous pass); only reachable when
///   `TALENTFLOW_SKIP_E2E_TESTS` is set.
/// - panics      → when neither `E2E_BASE_URL` nor `TALENTFLOW_SKIP_E2E_TESTS`
///   is set, so the suite cannot silently pass due to misconfiguration.
fn base_url() -> Option<String> {
    match env::var("E2E_BASE_URL") {
        Ok(url) => Some(url),
        Err(_) => {
            if env::var("TALENTFLOW_SKIP_E2E_TESTS").is_ok() {
                None
            } else {
                panic!(
                    "E2E_BASE_URL is not set. Either:\n  \
                     1. Set E2E_BASE_URL=http://localhost:8080 to run against a live server\n  \
                     2. Set TALENTFLOW_SKIP_E2E_TESTS=1 to explicitly skip (test will pass vacuously)"
                );
            }
        }
    }
}

// ── Login helpers ─────────────────────────────────────────────────────────────

/// Log in as the seeded platform admin and return the bearer token.
async fn admin_login(client: &Client, base: &str) -> String {
    let resp = client
        .post(format!("{base}/api/v1/auth/login"))
        .json(&json!({
            "username": "platform_admin",
            "password": "Admin_Pa$$word1!"
        }))
        .send()
        .await
        .expect("admin login request failed");

    assert_eq!(
        resp.status().as_u16(),
        200,
        "admin login must succeed for E2E setup"
    );

    let body: Value = resp.json().await.expect("admin login response is not JSON");
    body["data"]["token"]
        .as_str()
        .expect("admin login response missing data.token")
        .to_string()
}

/// Log in as the seeded club_admin and return the bearer token.
async fn club_admin_login(client: &Client, base: &str) -> String {
    let resp = client
        .post(format!("{base}/api/v1/auth/login"))
        .json(&json!({
            "username": "club_admin",
            "password": "ClubAdm1n!Passw0rd"
        }))
        .send()
        .await
        .expect("club_admin login request failed");

    assert_eq!(
        resp.status().as_u16(),
        200,
        "club_admin login must succeed for E2E setup"
    );

    let body: Value = resp.json().await.expect("club_admin login response is not JSON");
    body["data"]["token"]
        .as_str()
        .expect("club_admin login response missing data.token")
        .to_string()
}

/// Log in as the seeded member and return the bearer token.
async fn member_login(client: &Client, base: &str) -> String {
    let resp = client
        .post(format!("{base}/api/v1/auth/login"))
        .json(&json!({
            "username": "member",
            "password": "Member!User1Passw0rd"
        }))
        .send()
        .await
        .expect("member login request failed");

    assert_eq!(
        resp.status().as_u16(),
        200,
        "member login must succeed for E2E setup"
    );

    let body: Value = resp.json().await.expect("member login response is not JSON");
    body["data"]["token"]
        .as_str()
        .expect("member login response missing data.token")
        .to_string()
}

// ── Shared setup helpers ──────────────────────────────────────────────────────

/// Create a candidate via the API and return its UUID string.
async fn create_candidate(client: &Client, base: &str, token: &str, email: &str) -> String {
    let resp = client
        .post(format!("{base}/api/v1/candidates"))
        .bearer_auth(token)
        .json(&json!({
            "first_name": "E2E",
            "last_name":  "Candidate",
            "email":      email
        }))
        .send()
        .await
        .expect("create candidate request failed");

    assert_eq!(
        resp.status().as_u16(),
        201,
        "candidate creation must return 201 for E2E setup"
    );

    let body: Value = resp.json().await.expect("create candidate response is not JSON");
    body["data"]["id"]
        .as_str()
        .expect("create candidate response missing data.id")
        .to_string()
}

/// Create an offer for a candidate and return its UUID string.
async fn create_offer(client: &Client, base: &str, token: &str, candidate_id: &str) -> String {
    let resp = client
        .post(format!("{base}/api/v1/offers"))
        .bearer_auth(token)
        .json(&json!({
            "candidate_id": candidate_id,
            "title":        "E2E Test Offer",
            "department":   "Engineering"
        }))
        .send()
        .await
        .expect("create offer request failed");

    assert_eq!(
        resp.status().as_u16(),
        201,
        "offer creation must return 201 for E2E setup"
    );

    let body: Value = resp.json().await.expect("create offer response is not JSON");
    body["data"]["id"]
        .as_str()
        .expect("create offer response missing data.id")
        .to_string()
}

/// Generate a unique email address for test isolation.
fn unique_email(prefix: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    format!("{prefix}.{ts}@e2e.example.com")
}

// ═══════════════════════════════════════════════════════════════════════════════
// ── Auth tests ────────────────────────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════════════

#[actix_rt::test]
#[serial]
async fn health_check_returns_200() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let resp = client
        .get(format!("{base}/api/v1/health"))
        .send()
        .await
        .expect("health check request failed");

    assert_eq!(resp.status().as_u16(), 200, "health check must return 200");
}

#[actix_rt::test]
#[serial]
async fn health_check_response_shape() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let body: Value = client
        .get(format!("{base}/api/v1/health"))
        .send()
        .await
        .expect("health check request failed")
        .json()
        .await
        .expect("health check response is not JSON");

    assert!(
        body["status"].is_string(),
        "response must have a top-level 'status' field; got: {body}"
    );
    assert_eq!(
        body["status"].as_str().unwrap(),
        "ok",
        "health check status must be 'ok'"
    );
}

#[actix_rt::test]
#[serial]
async fn login_with_valid_credentials_returns_token() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let resp = client
        .post(format!("{base}/api/v1/auth/login"))
        .json(&json!({
            "username": "platform_admin",
            "password": "Admin_Pa$$word1!"
        }))
        .send()
        .await
        .expect("login request failed");

    assert_eq!(resp.status().as_u16(), 200, "valid credentials must return 200");

    let body: Value = resp.json().await.expect("login response is not JSON");
    assert!(
        body["data"]["token"].is_string(),
        "response must contain data.token; got: {body}"
    );
}

#[actix_rt::test]
#[serial]
async fn login_with_invalid_credentials_returns_401() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let resp = client
        .post(format!("{base}/api/v1/auth/login"))
        .json(&json!({
            "username": "platform_admin",
            "password": "WrongP@ssword!999"
        }))
        .send()
        .await
        .expect("login request failed");

    assert_eq!(
        resp.status().as_u16(),
        401,
        "invalid credentials must return 401"
    );
}

#[actix_rt::test]
#[serial]
async fn session_endpoint_requires_auth() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let resp = client
        .get(format!("{base}/api/v1/auth/session"))
        .send()
        .await
        .expect("session request failed");

    assert_eq!(
        resp.status().as_u16(),
        401,
        "session endpoint without token must return 401"
    );
}

#[actix_rt::test]
#[serial]
async fn session_endpoint_returns_user_info_when_authed() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;

    let body: Value = client
        .get(format!("{base}/api/v1/auth/session"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("session request failed")
        .json()
        .await
        .expect("session response is not JSON");

    assert!(
        body["data"]["user_id"].is_string(),
        "session response must contain data.user_id; got: {body}"
    );
    assert!(
        body["data"]["username"].is_string(),
        "session response must contain data.username; got: {body}"
    );
}

#[actix_rt::test]
#[serial]
async fn logout_invalidates_session() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;

    // Step 1: logout
    let resp = client
        .post(format!("{base}/api/v1/auth/logout"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("logout request failed");

    assert_eq!(resp.status().as_u16(), 204, "logout must return 204 No Content");

    // Step 2: the same token must now be rejected
    let resp = client
        .get(format!("{base}/api/v1/auth/session"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("session-after-logout request failed");

    assert_eq!(
        resp.status().as_u16(),
        401,
        "revoked token must return 401 after logout"
    );
}

#[actix_rt::test]
#[serial]
async fn captcha_endpoint_returns_challenge() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let body: Value = client
        .get(format!("{base}/api/v1/auth/captcha"))
        .send()
        .await
        .expect("captcha request failed")
        .json()
        .await
        .expect("captcha response is not JSON");

    assert!(
        body["data"]["token"].is_string(),
        "captcha response must contain data.token; got: {body}"
    );
    assert!(
        body["data"]["question"].is_string(),
        "captcha response must contain data.question; got: {body}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// ── Users tests ───────────────────────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════════════

#[actix_rt::test]
#[serial]
async fn list_users_returns_paginated_envelope() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;

    let body: Value = client
        .get(format!("{base}/api/v1/users?page=1&per_page=10"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("list users request failed")
        .json()
        .await
        .expect("list users response is not JSON");

    assert!(
        body["data"].is_array(),
        "list users must return a data array; got: {body}"
    );
    assert!(
        body["pagination"]["page"].is_number(),
        "list users must include pagination.page; got: {body}"
    );
    assert!(
        body["pagination"]["per_page"].is_number(),
        "list users must include pagination.per_page; got: {body}"
    );
    assert!(
        body["pagination"]["total"].is_number(),
        "list users must include pagination.total; got: {body}"
    );
}

#[actix_rt::test]
#[serial]
async fn list_users_requires_auth() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let resp = client
        .get(format!("{base}/api/v1/users"))
        .send()
        .await
        .expect("list users (unauthed) request failed");

    assert_eq!(
        resp.status().as_u16(),
        401,
        "list users without token must return 401"
    );
}

#[actix_rt::test]
#[serial]
async fn member_cannot_list_users() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = member_login(&client, &base).await;

    let resp = client
        .get(format!("{base}/api/v1/users"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("member list users request failed");

    assert_eq!(
        resp.status().as_u16(),
        403,
        "member must receive 403 when listing users"
    );
}

#[actix_rt::test]
#[serial]
async fn get_user_not_found_returns_404() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;

    let body: Value = client
        .get(format!("{base}/api/v1/users/00000000-0000-0000-0000-000000000099"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("get user (404) request failed")
        .json()
        .await
        .expect("get user (404) response is not JSON");

    assert_eq!(
        body["error"]["code"].as_str().unwrap_or(""),
        "not_found",
        "nonexistent user must return not_found error; got: {body}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// ── Candidates tests ──────────────────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════════════

#[actix_rt::test]
#[serial]
async fn create_candidate_returns_201_with_correct_fields() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;
    let email = unique_email("create-cand");

    let resp = client
        .post(format!("{base}/api/v1/candidates"))
        .bearer_auth(&token)
        .json(&json!({
            "first_name": "Alice",
            "last_name":  "Smith",
            "email":      email,
            "source":     "referral",
            "tags":       ["rust", "senior"]
        }))
        .send()
        .await
        .expect("create candidate request failed");

    assert_eq!(resp.status().as_u16(), 201, "create candidate must return 201");

    let body: Value = resp.json().await.expect("create candidate response is not JSON");
    assert_eq!(body["data"]["first_name"], "Alice", "first_name mismatch; got: {body}");
    assert_eq!(body["data"]["last_name"], "Smith", "last_name mismatch; got: {body}");
    assert!(body["data"]["id"].is_string(), "response must include data.id; got: {body}");
    // Phone is never echoed back in plaintext
    assert!(
        body["data"]["phone"].is_null(),
        "phone must be null in create response (not echoed); got: {body}"
    );
}

#[actix_rt::test]
#[serial]
async fn create_candidate_with_invalid_email_returns_422() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;

    let body: Value = client
        .post(format!("{base}/api/v1/candidates"))
        .bearer_auth(&token)
        .json(&json!({
            "first_name": "Bad",
            "last_name":  "Email",
            "email":      "not-an-email"
        }))
        .send()
        .await
        .expect("create candidate (bad email) request failed")
        .json()
        .await
        .expect("create candidate (bad email) response is not JSON");

    assert_eq!(
        body["error"]["code"].as_str().unwrap_or(""),
        "validation_error",
        "invalid email must return validation_error; got: {body}"
    );
}

#[actix_rt::test]
#[serial]
async fn create_candidate_unauthenticated_returns_401() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let resp = client
        .post(format!("{base}/api/v1/candidates"))
        .json(&json!({
            "first_name": "No",
            "last_name":  "Auth",
            "email":      "noauth@e2e.example.com"
        }))
        .send()
        .await
        .expect("create candidate (unauthed) request failed");

    assert_eq!(
        resp.status().as_u16(),
        401,
        "unauthenticated create candidate must return 401"
    );
}

#[actix_rt::test]
#[serial]
async fn member_cannot_create_candidate() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = member_login(&client, &base).await;

    let body: Value = client
        .post(format!("{base}/api/v1/candidates"))
        .bearer_auth(&token)
        .json(&json!({
            "first_name": "X",
            "last_name":  "Y",
            "email":      unique_email("member-cand")
        }))
        .send()
        .await
        .expect("member create candidate request failed")
        .json()
        .await
        .expect("member create candidate response is not JSON");

    assert_eq!(
        body["error"]["code"].as_str().unwrap_or(""),
        "forbidden",
        "member must receive forbidden when creating candidates; got: {body}"
    );
}

#[actix_rt::test]
#[serial]
async fn list_candidates_returns_paginated_envelope() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;

    let body: Value = client
        .get(format!("{base}/api/v1/candidates?page=1&per_page=10"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("list candidates request failed")
        .json()
        .await
        .expect("list candidates response is not JSON");

    assert!(body["data"].is_array(), "data must be an array; got: {body}");
    assert!(
        body["pagination"]["page"].is_number(),
        "must include pagination.page; got: {body}"
    );
    assert!(
        body["pagination"]["per_page"].is_number(),
        "must include pagination.per_page; got: {body}"
    );
    assert!(
        body["pagination"]["total"].is_number(),
        "must include pagination.total; got: {body}"
    );
}

#[actix_rt::test]
#[serial]
async fn get_candidate_by_id_returns_correct_data() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;
    let email = unique_email("get-cand");
    let candidate_id = create_candidate(&client, &base, &token, &email).await;

    let body: Value = client
        .get(format!("{base}/api/v1/candidates/{candidate_id}"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("get candidate request failed")
        .json()
        .await
        .expect("get candidate response is not JSON");

    assert_eq!(
        body["data"]["id"].as_str().unwrap_or(""),
        candidate_id,
        "get candidate must return the correct id; got: {body}"
    );
    assert_eq!(
        body["data"]["email"].as_str().unwrap_or(""),
        email,
        "get candidate must return the correct email; got: {body}"
    );
}

#[actix_rt::test]
#[serial]
async fn get_candidate_not_found_returns_404() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;

    let resp = client
        .get(format!(
            "{base}/api/v1/candidates/00000000-0000-0000-0000-000000000001"
        ))
        .bearer_auth(&token)
        .send()
        .await
        .expect("get candidate (404) request failed");

    assert_eq!(
        resp.status().as_u16(),
        404,
        "nonexistent candidate must return 404"
    );
}

#[actix_rt::test]
#[serial]
async fn update_candidate_returns_updated_fields() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;
    let email = unique_email("upd-cand");
    let candidate_id = create_candidate(&client, &base, &token, &email).await;

    let updated_email = unique_email("upd-cand-new");
    let body: Value = client
        .put(format!("{base}/api/v1/candidates/{candidate_id}"))
        .bearer_auth(&token)
        .json(&json!({
            "first_name": "Updated",
            "last_name":  "Name",
            "email":      updated_email
        }))
        .send()
        .await
        .expect("update candidate request failed")
        .json()
        .await
        .expect("update candidate response is not JSON");

    assert_eq!(
        body["data"]["first_name"].as_str().unwrap_or(""),
        "Updated",
        "update must return updated first_name; got: {body}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// ── Offers tests ──────────────────────────────────────════════════════════════
// ═══════════════════════════════════════════════════════════════════════════════

#[actix_rt::test]
#[serial]
async fn create_offer_returns_201_with_draft_status() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;
    let candidate_id = create_candidate(&client, &base, &token, &unique_email("offer-cand")).await;

    let body: Value = client
        .post(format!("{base}/api/v1/offers"))
        .bearer_auth(&token)
        .json(&json!({
            "candidate_id": candidate_id,
            "title":        "Senior Rust Engineer",
            "department":   "Engineering"
        }))
        .send()
        .await
        .expect("create offer request failed")
        .json()
        .await
        .expect("create offer response is not JSON");

    assert!(body["data"]["id"].is_string(), "response must include data.id; got: {body}");
    assert_eq!(
        body["data"]["status"].as_str().unwrap_or(""),
        "draft",
        "newly created offer must have status 'draft'; got: {body}"
    );
    assert_eq!(
        body["data"]["title"].as_str().unwrap_or(""),
        "Senior Rust Engineer",
        "title must match; got: {body}"
    );
}

#[actix_rt::test]
#[serial]
async fn create_offer_with_compensation_does_not_reveal_by_default() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;
    let candidate_id = create_candidate(&client, &base, &token, &unique_email("comp-cand")).await;

    let create_body: Value = client
        .post(format!("{base}/api/v1/offers"))
        .bearer_auth(&token)
        .json(&json!({
            "candidate_id": candidate_id,
            "title":        "Offer With Comp",
            "compensation": {
                "base_salary_usd":   120000,
                "bonus_target_pct":  10.0,
                "equity_units":      500,
                "pto_days":          20,
                "k401_match_pct":    5.0
            }
        }))
        .send()
        .await
        .expect("create offer (comp) request failed")
        .json()
        .await
        .expect("create offer (comp) response is not JSON");

    let offer_id = create_body["data"]["id"].as_str().unwrap().to_string();

    // GET without reveal_compensation — compensation must be null
    let get_body: Value = client
        .get(format!("{base}/api/v1/offers/{offer_id}"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("get offer (no reveal) request failed")
        .json()
        .await
        .expect("get offer (no reveal) response is not JSON");

    assert!(
        get_body["data"]["compensation"].is_null(),
        "compensation must be null without reveal_compensation=true; got: {get_body}"
    );
}

#[actix_rt::test]
#[serial]
async fn list_offers_returns_paginated_envelope() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;

    let body: Value = client
        .get(format!("{base}/api/v1/offers?page=1&per_page=10"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("list offers request failed")
        .json()
        .await
        .expect("list offers response is not JSON");

    assert!(body["data"].is_array(), "data must be an array; got: {body}");
    assert!(
        body["pagination"]["total"].is_number(),
        "must include pagination.total; got: {body}"
    );
}

#[actix_rt::test]
#[serial]
async fn get_offer_not_found_returns_404() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;

    let resp = client
        .get(format!(
            "{base}/api/v1/offers/00000000-0000-0000-0000-000000000002"
        ))
        .bearer_auth(&token)
        .send()
        .await
        .expect("get offer (404) request failed");

    assert_eq!(resp.status().as_u16(), 404, "nonexistent offer must return 404");
}

#[actix_rt::test]
#[serial]
async fn update_offer_returns_updated_title() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;
    let candidate_id = create_candidate(&client, &base, &token, &unique_email("upd-offer-c")).await;
    let offer_id = create_offer(&client, &base, &token, &candidate_id).await;

    let body: Value = client
        .put(format!("{base}/api/v1/offers/{offer_id}"))
        .bearer_auth(&token)
        .json(&json!({ "title": "Updated Title" }))
        .send()
        .await
        .expect("update offer request failed")
        .json()
        .await
        .expect("update offer response is not JSON");

    assert_eq!(
        body["data"]["title"].as_str().unwrap_or(""),
        "Updated Title",
        "title must be updated; got: {body}"
    );
}

#[actix_rt::test]
#[serial]
async fn submit_offer_transitions_to_pending_approval() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;
    let candidate_id = create_candidate(&client, &base, &token, &unique_email("submit-offer-c")).await;
    let offer_id = create_offer(&client, &base, &token, &candidate_id).await;

    let body: Value = client
        .post(format!("{base}/api/v1/offers/{offer_id}/submit"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("submit offer request failed")
        .json()
        .await
        .expect("submit offer response is not JSON");

    assert_eq!(
        body["data"]["status"].as_str().unwrap_or(""),
        "pending_approval",
        "submitted offer must have status 'pending_approval'; got: {body}"
    );
}

#[actix_rt::test]
#[serial]
async fn withdraw_offer_transitions_to_withdrawn() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;
    let candidate_id = create_candidate(&client, &base, &token, &unique_email("wdraw-offer-c")).await;
    let offer_id = create_offer(&client, &base, &token, &candidate_id).await;

    let body: Value = client
        .post(format!("{base}/api/v1/offers/{offer_id}/withdraw"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("withdraw offer request failed")
        .json()
        .await
        .expect("withdraw offer response is not JSON");

    assert_eq!(
        body["data"]["status"].as_str().unwrap_or(""),
        "withdrawn",
        "withdrawn offer must have status 'withdrawn'; got: {body}"
    );
}

#[actix_rt::test]
#[serial]
async fn double_withdraw_returns_409_invalid_state_transition() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;
    let candidate_id = create_candidate(&client, &base, &token, &unique_email("dbl-wdraw-c")).await;
    let offer_id = create_offer(&client, &base, &token, &candidate_id).await;

    // First withdraw — must succeed
    client
        .post(format!("{base}/api/v1/offers/{offer_id}/withdraw"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("first withdraw request failed");

    // Second withdraw — must fail with 409
    let resp = client
        .post(format!("{base}/api/v1/offers/{offer_id}/withdraw"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("second withdraw request failed");

    assert_eq!(
        resp.status().as_u16(),
        409,
        "second withdraw must return 409"
    );

    let body: Value = resp.json().await.expect("second withdraw response is not JSON");
    assert_eq!(
        body["error"]["code"].as_str().unwrap_or(""),
        "invalid_state_transition",
        "error code must be invalid_state_transition; got: {body}"
    );
}

#[actix_rt::test]
#[serial]
async fn member_cannot_create_offer() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    // Create candidate as admin first (member cannot create candidates either)
    let admin_token = admin_login(&client, &base).await;
    let candidate_id =
        create_candidate(&client, &base, &admin_token, &unique_email("member-offer-c")).await;

    let member_token = member_login(&client, &base).await;
    let body: Value = client
        .post(format!("{base}/api/v1/offers"))
        .bearer_auth(&member_token)
        .json(&json!({
            "candidate_id": candidate_id,
            "title":        "Offer by Member"
        }))
        .send()
        .await
        .expect("member create offer request failed")
        .json()
        .await
        .expect("member create offer response is not JSON");

    assert_eq!(
        body["error"]["code"].as_str().unwrap_or(""),
        "forbidden",
        "member must receive forbidden when creating offers; got: {body}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// ── Onboarding tests ──────────────────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════════════

#[actix_rt::test]
#[serial]
async fn create_checklist_returns_201() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;
    let candidate_id =
        create_candidate(&client, &base, &token, &unique_email("onb-cand")).await;
    let offer_id = create_offer(&client, &base, &token, &candidate_id).await;

    let body: Value = client
        .post(format!("{base}/api/v1/onboarding/checklists"))
        .bearer_auth(&token)
        .json(&json!({
            "offer_id":     offer_id,
            "candidate_id": candidate_id
        }))
        .send()
        .await
        .expect("create checklist request failed")
        .json()
        .await
        .expect("create checklist response is not JSON");

    assert!(
        body["data"]["id"].is_string(),
        "create checklist must return data.id; got: {body}"
    );
    assert_eq!(
        body["data"]["offer_id"].as_str().unwrap_or(""),
        offer_id,
        "checklist offer_id must match; got: {body}"
    );
}

#[actix_rt::test]
#[serial]
async fn list_checklists_returns_paginated_envelope() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;

    let body: Value = client
        .get(format!("{base}/api/v1/onboarding/checklists?page=1&per_page=10"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("list checklists request failed")
        .json()
        .await
        .expect("list checklists response is not JSON");

    assert!(body["data"].is_array(), "data must be an array; got: {body}");
    assert!(
        body["pagination"]["total"].is_number(),
        "must include pagination.total; got: {body}"
    );
}

#[actix_rt::test]
#[serial]
async fn add_item_to_checklist_returns_201() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;
    let candidate_id =
        create_candidate(&client, &base, &token, &unique_email("item-cand")).await;
    let offer_id = create_offer(&client, &base, &token, &candidate_id).await;

    let cl_body: Value = client
        .post(format!("{base}/api/v1/onboarding/checklists"))
        .bearer_auth(&token)
        .json(&json!({ "offer_id": offer_id, "candidate_id": candidate_id }))
        .send()
        .await
        .expect("create checklist request failed")
        .json()
        .await
        .expect("create checklist response is not JSON");

    let checklist_id = cl_body["data"]["id"].as_str().unwrap().to_string();

    let item_body: Value = client
        .post(format!("{base}/api/v1/onboarding/checklists/{checklist_id}/items"))
        .bearer_auth(&token)
        .json(&json!({
            "title":            "Background Check",
            "item_order":       1,
            "required":         true,
            "requires_upload":  false
        }))
        .send()
        .await
        .expect("add item request failed")
        .json()
        .await
        .expect("add item response is not JSON");

    assert!(
        item_body["data"]["id"].is_string(),
        "add item must return data.id; got: {item_body}"
    );
    assert_eq!(
        item_body["data"]["title"].as_str().unwrap_or(""),
        "Background Check",
        "item title must match; got: {item_body}"
    );
    assert_eq!(
        item_body["data"]["required"].as_bool().unwrap_or(false),
        true,
        "item required flag must be true; got: {item_body}"
    );
}

#[actix_rt::test]
#[serial]
async fn list_checklist_items_returns_array() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;
    let candidate_id =
        create_candidate(&client, &base, &token, &unique_email("list-items-cand")).await;
    let offer_id = create_offer(&client, &base, &token, &candidate_id).await;

    let cl_body: Value = client
        .post(format!("{base}/api/v1/onboarding/checklists"))
        .bearer_auth(&token)
        .json(&json!({ "offer_id": offer_id, "candidate_id": candidate_id }))
        .send()
        .await
        .expect("create checklist request failed")
        .json()
        .await
        .expect("create checklist response is not JSON");

    let checklist_id = cl_body["data"]["id"].as_str().unwrap().to_string();

    let body: Value = client
        .get(format!("{base}/api/v1/onboarding/checklists/{checklist_id}/items"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("list checklist items request failed")
        .json()
        .await
        .expect("list checklist items response is not JSON");

    assert!(
        body["data"].is_array(),
        "list items must return a data array; got: {body}"
    );
}

#[actix_rt::test]
#[serial]
async fn member_cannot_create_checklist() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let admin_token = admin_login(&client, &base).await;
    let candidate_id =
        create_candidate(&client, &base, &admin_token, &unique_email("member-cl-c")).await;
    let offer_id = create_offer(&client, &base, &admin_token, &candidate_id).await;

    let member_token = member_login(&client, &base).await;
    let body: Value = client
        .post(format!("{base}/api/v1/onboarding/checklists"))
        .bearer_auth(&member_token)
        .json(&json!({ "offer_id": offer_id, "candidate_id": candidate_id }))
        .send()
        .await
        .expect("member create checklist request failed")
        .json()
        .await
        .expect("member create checklist response is not JSON");

    assert_eq!(
        body["error"]["code"].as_str().unwrap_or(""),
        "forbidden",
        "member must receive forbidden when creating checklists; got: {body}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// ── Bookings tests ────────────────────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════════════

#[actix_rt::test]
#[serial]
async fn list_bookings_returns_paginated_envelope() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;

    let body: Value = client
        .get(format!("{base}/api/v1/bookings?page=1&per_page=10"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("list bookings request failed")
        .json()
        .await
        .expect("list bookings response is not JSON");

    assert!(body["data"].is_array(), "data must be an array; got: {body}");
    assert!(
        body["pagination"]["total"].is_number(),
        "must include pagination.total; got: {body}"
    );
}

#[actix_rt::test]
#[serial]
async fn list_bookings_requires_auth() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let resp = client
        .get(format!("{base}/api/v1/bookings"))
        .send()
        .await
        .expect("list bookings (unauthed) request failed");

    assert_eq!(
        resp.status().as_u16(),
        401,
        "list bookings without token must return 401"
    );
}

#[actix_rt::test]
#[serial]
async fn get_booking_not_found_returns_404() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;

    let resp = client
        .get(format!(
            "{base}/api/v1/bookings/00000000-0000-0000-0000-000000000003"
        ))
        .bearer_auth(&token)
        .send()
        .await
        .expect("get booking (404) request failed");

    assert_eq!(
        resp.status().as_u16(),
        404,
        "nonexistent booking must return 404"
    );
}

#[actix_rt::test]
#[serial]
async fn create_booking_with_invalid_slot_returns_error() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;
    let candidate_id =
        create_candidate(&client, &base, &token, &unique_email("booking-cand")).await;

    // Use a nonexistent site/slot — should get a 4xx (404 or 422 depending on validation order)
    let resp = client
        .post(format!("{base}/api/v1/bookings"))
        .bearer_auth(&token)
        .json(&json!({
            "candidate_id": candidate_id,
            "site_id":      "00000000-0000-0000-0000-000000000004",
            "slot_id":      "00000000-0000-0000-0000-000000000005"
        }))
        .send()
        .await
        .expect("create booking (invalid slot) request failed");

    let status = resp.status().as_u16();
    assert!(
        status == 404 || status == 422 || status == 409,
        "booking with nonexistent slot must return 4xx; got {status}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// ── Search tests ──────────────────────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════════════

#[actix_rt::test]
#[serial]
async fn search_endpoint_returns_paginated_envelope() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;

    let body: Value = client
        .get(format!("{base}/api/v1/search?q=engineer&page=1&per_page=10"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("search request failed")
        .json()
        .await
        .expect("search response is not JSON");

    assert!(
        body["data"].is_array(),
        "search must return a data array; got: {body}"
    );
    assert!(
        body["pagination"]["total"].is_number(),
        "search must include pagination.total; got: {body}"
    );
}

#[actix_rt::test]
#[serial]
async fn search_endpoint_requires_auth() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let resp = client
        .get(format!("{base}/api/v1/search?q=test"))
        .send()
        .await
        .expect("search (unauthed) request failed");

    assert_eq!(
        resp.status().as_u16(),
        401,
        "search without token must return 401"
    );
}

#[actix_rt::test]
#[serial]
async fn search_with_empty_query_returns_results() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;

    let resp = client
        .get(format!("{base}/api/v1/search"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("search (empty q) request failed");

    // Empty query is valid — must return 200 with a data array
    assert_eq!(
        resp.status().as_u16(),
        200,
        "search with no query param must return 200"
    );
}

#[actix_rt::test]
#[serial]
async fn search_with_invalid_sort_returns_422() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;

    let body: Value = client
        .get(format!("{base}/api/v1/search?q=test&sort_by=invalid_sort"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("search (bad sort) request failed")
        .json()
        .await
        .expect("search (bad sort) response is not JSON");

    assert_eq!(
        body["error"]["code"].as_str().unwrap_or(""),
        "validation_error",
        "invalid sort_by must return validation_error; got: {body}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// ── Reporting tests ───────────────────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════════════

#[actix_rt::test]
#[serial]
async fn create_reporting_subscription_returns_201() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;

    let body: Value = client
        .post(format!("{base}/api/v1/reporting/subscriptions"))
        .bearer_auth(&token)
        .json(&json!({
            "report_type": "snapshot",
            "parameters":  {}
        }))
        .send()
        .await
        .expect("create subscription request failed")
        .json()
        .await
        .expect("create subscription response is not JSON");

    assert!(
        body["data"]["id"].is_string(),
        "create subscription must return data.id; got: {body}"
    );
    assert_eq!(
        body["data"]["report_type"].as_str().unwrap_or(""),
        "snapshot",
        "report_type must match; got: {body}"
    );
}

#[actix_rt::test]
#[serial]
async fn list_reporting_subscriptions_returns_paginated_envelope() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;

    let body: Value = client
        .get(format!("{base}/api/v1/reporting/subscriptions?page=1&per_page=10"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("list subscriptions request failed")
        .json()
        .await
        .expect("list subscriptions response is not JSON");

    assert!(body["data"].is_array(), "data must be an array; got: {body}");
    assert!(
        body["pagination"]["total"].is_number(),
        "must include pagination.total; got: {body}"
    );
}

#[actix_rt::test]
#[serial]
async fn list_reporting_alerts_returns_paginated_envelope() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;

    let body: Value = client
        .get(format!("{base}/api/v1/reporting/alerts?page=1&per_page=10"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("list alerts request failed")
        .json()
        .await
        .expect("list alerts response is not JSON");

    assert!(body["data"].is_array(), "data must be an array; got: {body}");
    assert!(
        body["pagination"]["total"].is_number(),
        "must include pagination.total; got: {body}"
    );
}

#[actix_rt::test]
#[serial]
async fn reporting_subscription_requires_auth() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let resp = client
        .get(format!("{base}/api/v1/reporting/subscriptions"))
        .send()
        .await
        .expect("list subscriptions (unauthed) request failed");

    assert_eq!(
        resp.status().as_u16(),
        401,
        "reporting subscriptions without token must return 401"
    );
}

#[actix_rt::test]
#[serial]
async fn create_subscription_with_empty_report_type_returns_422() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;

    let body: Value = client
        .post(format!("{base}/api/v1/reporting/subscriptions"))
        .bearer_auth(&token)
        .json(&json!({
            "report_type": "",
            "parameters":  {}
        }))
        .send()
        .await
        .expect("create subscription (empty type) request failed")
        .json()
        .await
        .expect("create subscription (empty type) response is not JSON");

    assert_eq!(
        body["error"]["code"].as_str().unwrap_or(""),
        "validation_error",
        "empty report_type must return validation_error; got: {body}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// ── Audit tests ───────────────────────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════════════

#[actix_rt::test]
#[serial]
async fn list_audit_events_returns_paginated_envelope() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;

    let body: Value = client
        .get(format!("{base}/api/v1/audit?page=1&per_page=10"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("list audit events request failed")
        .json()
        .await
        .expect("list audit events response is not JSON");

    assert!(body["data"].is_array(), "data must be an array; got: {body}");
    assert!(
        body["pagination"]["total"].is_number(),
        "must include pagination.total; got: {body}"
    );
}

#[actix_rt::test]
#[serial]
async fn list_audit_events_requires_auth() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let resp = client
        .get(format!("{base}/api/v1/audit"))
        .send()
        .await
        .expect("list audit (unauthed) request failed");

    assert_eq!(
        resp.status().as_u16(),
        401,
        "audit endpoint without token must return 401"
    );
}

#[actix_rt::test]
#[serial]
async fn member_cannot_list_audit_events() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = member_login(&client, &base).await;

    let body: Value = client
        .get(format!("{base}/api/v1/audit"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("member audit request failed")
        .json()
        .await
        .expect("member audit response is not JSON");

    assert_eq!(
        body["error"]["code"].as_str().unwrap_or(""),
        "forbidden",
        "member must receive forbidden when listing audit events; got: {body}"
    );
}

#[actix_rt::test]
#[serial]
async fn audit_events_are_emitted_after_mutation() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;

    // Perform a mutation that must generate an audit event
    let _ = create_candidate(&client, &base, &token, &unique_email("audit-cand")).await;

    // Verify audit log is non-empty
    let body: Value = client
        .get(format!("{base}/api/v1/audit?page=1&per_page=1"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("list audit events (post-mutation) request failed")
        .json()
        .await
        .expect("list audit events (post-mutation) response is not JSON");

    let total = body["pagination"]["total"].as_i64().unwrap_or(0);
    assert!(
        total >= 1,
        "audit log must contain at least one event after a mutation; got total={total}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// ── RBAC cross-cutting tests ──────────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════════════

#[actix_rt::test]
#[serial]
async fn member_cannot_list_roles() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = member_login(&client, &base).await;

    let body: Value = client
        .get(format!("{base}/api/v1/roles"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("member list roles request failed")
        .json()
        .await
        .expect("member list roles response is not JSON");

    assert_eq!(
        body["error"]["code"].as_str().unwrap_or(""),
        "forbidden",
        "member must receive forbidden when listing roles; got: {body}"
    );
}

#[actix_rt::test]
#[serial]
async fn admin_can_list_roles() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;

    let body: Value = client
        .get(format!("{base}/api/v1/roles"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("admin list roles request failed")
        .json()
        .await
        .expect("admin list roles response is not JSON");

    assert!(
        body["data"].is_array(),
        "admin must see roles array; got: {body}"
    );
}

#[actix_rt::test]
#[serial]
async fn member_cannot_list_permissions() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = member_login(&client, &base).await;

    let body: Value = client
        .get(format!("{base}/api/v1/permissions"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("member list permissions request failed")
        .json()
        .await
        .expect("member list permissions response is not JSON");

    assert_eq!(
        body["error"]["code"].as_str().unwrap_or(""),
        "forbidden",
        "member must receive forbidden when listing permissions; got: {body}"
    );
}

#[actix_rt::test]
#[serial]
async fn admin_can_list_permissions() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;

    let body: Value = client
        .get(format!("{base}/api/v1/permissions"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("admin list permissions request failed")
        .json()
        .await
        .expect("admin list permissions response is not JSON");

    assert!(
        body["data"].is_array(),
        "admin must see permissions array; got: {body}"
    );
}

#[actix_rt::test]
#[serial]
async fn club_admin_can_list_candidates() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = club_admin_login(&client, &base).await;

    let resp = client
        .get(format!("{base}/api/v1/candidates?page=1&per_page=5"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("club_admin list candidates request failed");

    // club_admin should have at least read access (200), not be forbidden
    assert_eq!(
        resp.status().as_u16(),
        200,
        "club_admin must be able to list candidates"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// ── Idempotency smoke tests ───────────────────────────────────────────────────
// ═══════════════════════════════════════════════════════════════════════════════

#[actix_rt::test]
#[serial]
async fn create_candidate_idempotent_replay_returns_same_id() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;

    let email = unique_email("idem-cand");
    let idem_key = format!("e2e-idem-cand-{email}");
    let payload = json!({
        "first_name": "Idem",
        "last_name":  "Replay",
        "email":      email
    });

    let body1: Value = client
        .post(format!("{base}/api/v1/candidates"))
        .bearer_auth(&token)
        .header("Idempotency-Key", &idem_key)
        .json(&payload)
        .send()
        .await
        .expect("first create candidate (idem) request failed")
        .json()
        .await
        .expect("first create candidate (idem) response is not JSON");

    assert_eq!(
        body1["data"]["id"].is_string(),
        true,
        "first create must return an id; got: {body1}"
    );
    let id1 = body1["data"]["id"].as_str().unwrap().to_string();

    let body2: Value = client
        .post(format!("{base}/api/v1/candidates"))
        .bearer_auth(&token)
        .header("Idempotency-Key", &idem_key)
        .json(&payload)
        .send()
        .await
        .expect("second create candidate (idem) request failed")
        .json()
        .await
        .expect("second create candidate (idem) response is not JSON");

    let id2 = body2["data"]["id"].as_str().unwrap_or("").to_string();

    assert_eq!(
        id1, id2,
        "idempotent replay must return the same candidate id"
    );
}

#[actix_rt::test]
#[serial]
async fn create_candidate_different_idempotency_key_payload_conflict_returns_409() {
    let Some(base) = base_url() else {
        return;
    };

    let client = Client::new();
    let token = admin_login(&client, &base).await;
    let idem_key = unique_email("conflict-key"); // use unique string as key

    // First request
    client
        .post(format!("{base}/api/v1/candidates"))
        .bearer_auth(&token)
        .header("Idempotency-Key", &idem_key)
        .json(&json!({
            "first_name": "Conflict",
            "last_name":  "A",
            "email":      unique_email("conflict-a")
        }))
        .send()
        .await
        .expect("first request failed");

    // Second request: same key, different payload
    let resp = client
        .post(format!("{base}/api/v1/candidates"))
        .bearer_auth(&token)
        .header("Idempotency-Key", &idem_key)
        .json(&json!({
            "first_name": "Conflict",
            "last_name":  "B",
            "email":      unique_email("conflict-b")
        }))
        .send()
        .await
        .expect("second request (conflict) failed");

    assert_eq!(
        resp.status().as_u16(),
        409,
        "different payload with same idempotency key must return 409"
    );

    let body: Value = resp
        .json()
        .await
        .expect("conflict response is not JSON");
    assert_eq!(
        body["error"]["code"].as_str().unwrap_or(""),
        "idempotency_conflict",
        "error code must be idempotency_conflict; got: {body}"
    );
}
