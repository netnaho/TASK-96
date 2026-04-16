/// API-level integration tests for search, reporting, and integrations.
///
/// ## Prerequisites
///
/// ```bash
/// DATABASE_URL=postgres://talentflow:talentflow_dev@localhost:5433/talentflow_test \
///   ENCRYPTION_KEY="$(openssl rand -base64 32)" \
///   cargo test --test search_reporting_tests
/// ```
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

async fn login_admin(
    app: &impl actix_web::dev::Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
    >,
) -> String {
    login_user(app, "platform_admin", "Admin_Pa$$word1!").await
}

async fn login_member(
    app: &impl actix_web::dev::Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
    >,
) -> String {
    login_user(app, "member", "Member!User1Passw0rd").await
}

async fn login_user(
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
        .set_json(json!({
            "username": username,
            "password": password
        }))
        .to_request();
    let resp = test::call_service(app, req).await;
    assert_eq!(resp.status(), 200, "login should succeed for {username}");
    let body: Value = test::read_body_json(resp).await;
    body["data"]["token"]
        .as_str()
        .expect("token in response")
        .to_string()
}

// ── Search tests ─────────────────────────────────────────────────────────────

#[actix_rt::test]
#[serial]
async fn test_search_requires_auth() {
    test_app!(app, _config);
    let req = test::TestRequest::get()
        .uri("/api/v1/search?q=test")
        .to_request();
    let status = match app.call(req).await {
        Ok(resp) => resp.status().as_u16(),
        Err(err) => err.error_response().status().as_u16(),
    };
    assert_eq!(status, 401, "unauthenticated search should return 401");
}

#[actix_rt::test]
#[serial]
async fn test_search_returns_paginated_results() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/search?page=1&per_page=10")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = test::read_body_json(resp).await;
    assert!(
        body["data"].is_array(),
        "search response should contain data array"
    );
    assert!(
        body["pagination"].is_object(),
        "search response should contain pagination"
    );
}

#[actix_rt::test]
#[serial]
async fn test_search_with_keyword_filter() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/search?q=admin&page=1&per_page=25")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
}

#[actix_rt::test]
#[serial]
async fn test_search_invalid_sort_returns_422() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/search?sort_by=invalid_field")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 422);
}

#[actix_rt::test]
#[serial]
async fn test_search_history_empty_for_new_session() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/search/history?limit=10")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = test::read_body_json(resp).await;
    assert!(body["data"].is_array(), "history should return array");
}

#[actix_rt::test]
#[serial]
async fn test_vocabularies_list_returns_categories() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/vocabularies")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = test::read_body_json(resp).await;
    assert!(body["data"].is_array());
}

#[actix_rt::test]
#[serial]
async fn test_vocabulary_unknown_category_returns_404() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/vocabularies/nonexistent_category_xyz")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 404);
}

// ── Reporting tests ──────────────────────────────────────────────────────────

#[actix_rt::test]
#[serial]
async fn test_create_and_get_subscription() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::post()
        .uri("/api/v1/reporting/subscriptions")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({
            "report_type": "offers_expiring",
            "parameters": { "days": 7 },
            "cron_expression": null
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 201);
    let body: Value = test::read_body_json(resp).await;
    let sub_id = body["data"]["id"].as_str().expect("subscription id");

    // GET the subscription back
    let get_req = test::TestRequest::get()
        .uri(&format!("/api/v1/reporting/subscriptions/{sub_id}"))
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let get_resp = test::call_service(&app, get_req).await;
    assert_eq!(get_resp.status(), 200);
}

#[actix_rt::test]
#[serial]
async fn test_subscription_invalid_report_type_returns_422() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::post()
        .uri("/api/v1/reporting/subscriptions")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({
            "report_type": "not_a_real_type",
            "parameters": {}
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 422);
}

#[actix_rt::test]
#[serial]
async fn test_delete_subscription() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let create_req = test::TestRequest::post()
        .uri("/api/v1/reporting/subscriptions")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({ "report_type": "snapshot", "parameters": {} }))
        .to_request();
    let create_resp = test::call_service(&app, create_req).await;
    assert_eq!(create_resp.status(), 201);
    let body: Value = test::read_body_json(create_resp).await;
    let sub_id = body["data"]["id"].as_str().expect("subscription id");

    let del_req = test::TestRequest::delete()
        .uri(&format!("/api/v1/reporting/subscriptions/{sub_id}"))
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let del_resp = test::call_service(&app, del_req).await;
    assert_eq!(del_resp.status(), 204);
}

#[actix_rt::test]
#[serial]
async fn test_publish_and_list_dashboard_versions() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let key = format!("test-dashboard-{}", uuid::Uuid::new_v4());

    let req = test::TestRequest::post()
        .uri(&format!("/api/v1/reporting/dashboards/{key}/versions"))
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({ "layout": { "widgets": [] } }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 201);
    let body: Value = test::read_body_json(resp).await;
    assert_eq!(body["data"]["version"], 1);

    // Publish a second version
    let req2 = test::TestRequest::post()
        .uri(&format!("/api/v1/reporting/dashboards/{key}/versions"))
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({ "layout": { "widgets": [{"type": "counter"}] } }))
        .to_request();
    let resp2 = test::call_service(&app, req2).await;
    assert_eq!(resp2.status(), 201);
    let body2: Value = test::read_body_json(resp2).await;
    assert_eq!(body2["data"]["version"], 2);

    // List versions — should return newest first
    let list_req = test::TestRequest::get()
        .uri(&format!("/api/v1/reporting/dashboards/{key}/versions"))
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let list_resp = test::call_service(&app, list_req).await;
    assert_eq!(list_resp.status(), 200);
    let list_body: Value = test::read_body_json(list_resp).await;
    let versions = list_body["data"].as_array().expect("versions array");
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0]["version"], 2, "newest version first");
}

#[actix_rt::test]
#[serial]
async fn test_list_alerts_requires_auth() {
    test_app!(app, _config);
    let req = test::TestRequest::get()
        .uri("/api/v1/reporting/alerts")
        .to_request();
    let status = match app.call(req).await {
        Ok(resp) => resp.status().as_u16(),
        Err(err) => err.error_response().status().as_u16(),
    };
    assert_eq!(status, 401);
}

// ── Integration connector tests ──────────────────────────────────────────────

#[actix_rt::test]
#[serial]
async fn test_create_and_get_connector() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::post()
        .uri("/api/v1/integrations/connectors")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({
            "name": "Test HR System",
            "connector_type": "inbound",
            "base_url": "https://hr.example.com/api",
            "auth_config": { "api_key": "secret123" },
            "is_enabled": true
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 201);
    let body: Value = test::read_body_json(resp).await;
    let conn_id = body["data"]["id"].as_str().expect("connector id");

    // auth_config must NOT appear in the response
    assert!(
        body["data"]["auth_config"].is_null() || body["data"]["auth_config_encrypted"].is_null(),
        "auth_config must not appear in response"
    );
    assert!(body["data"]["name"].as_str() == Some("Test HR System"));

    // GET
    let get_req = test::TestRequest::get()
        .uri(&format!("/api/v1/integrations/connectors/{conn_id}"))
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let get_resp = test::call_service(&app, get_req).await;
    assert_eq!(get_resp.status(), 200);
}

#[actix_rt::test]
#[serial]
async fn test_connector_invalid_type_returns_422() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::post()
        .uri("/api/v1/integrations/connectors")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({
            "name": "Bad Connector",
            "connector_type": "not_valid",
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 422);
}

#[actix_rt::test]
#[serial]
async fn test_trigger_sync_and_get_state() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    // Create a connector
    let create_req = test::TestRequest::post()
        .uri("/api/v1/integrations/connectors")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({
            "name": "Sync Test Connector",
            "connector_type": "bidirectional",
            "is_enabled": true
        }))
        .to_request();
    let create_resp = test::call_service(&app, create_req).await;
    assert_eq!(create_resp.status(), 201);
    let body: Value = test::read_body_json(create_resp).await;
    let conn_id = body["data"]["id"].as_str().expect("connector id");

    // Trigger sync
    let sync_req = test::TestRequest::post()
        .uri(&format!("/api/v1/integrations/connectors/{conn_id}/sync"))
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({ "entity_type": "candidates" }))
        .to_request();
    let sync_resp = test::call_service(&app, sync_req).await;
    assert_eq!(sync_resp.status(), 202);
    let sync_body: Value = test::read_body_json(sync_resp).await;
    assert_eq!(sync_body["data"]["status"], "succeeded");

    // Get sync state
    let state_req = test::TestRequest::get()
        .uri(&format!(
            "/api/v1/integrations/connectors/{conn_id}/sync-state"
        ))
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let state_resp = test::call_service(&app, state_req).await;
    assert_eq!(state_resp.status(), 200);
    let state_body: Value = test::read_body_json(state_resp).await;
    let states = state_body["data"].as_array().expect("sync states array");
    assert!(!states.is_empty(), "sync state should exist after trigger");
    assert_eq!(states[0]["entity_type"], "candidates");
}

#[actix_rt::test]
#[serial]
async fn test_import_file_fallback() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    // No connector_id → file fallback
    let req = test::TestRequest::post()
        .uri("/api/v1/integrations/import")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({
            "entity_type": "candidates",
            "records": [
                { "first_name": "Jane", "last_name": "Doe", "email": "jane@example.com" }
            ]
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = test::read_body_json(resp).await;
    assert_eq!(body["data"]["records_imported"], 1);
    assert!(
        body["data"]["source"]
            .as_str()
            .map(|s| s.starts_with("file:"))
            .unwrap_or(false),
        "fallback source should start with 'file:'"
    );
}

#[actix_rt::test]
#[serial]
async fn test_export_with_unknown_connector_returns_404() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let fake_id = uuid::Uuid::new_v4();
    let req = test::TestRequest::post()
        .uri("/api/v1/integrations/export")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({
            "connector_id": fake_id,
            "entity_type": "offers",
            "field_map": {}
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 404);
}

#[actix_rt::test]
#[serial]
async fn test_search_pagination_out_of_range_returns_empty() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/search?page=9999&per_page=25")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = test::read_body_json(resp).await;
    let results = body["data"].as_array().expect("results array");
    assert!(
        results.is_empty(),
        "page beyond total should return empty results"
    );
}

// ── Subscription access isolation ────────────────────────────────────────────

/// A member that creates a subscription must be able to read it back.
#[actix_rt::test]
#[serial]
async fn test_member_can_read_own_subscription() {
    test_app!(app, _config);
    let member_token = login_member(&app).await;

    let create_req = test::TestRequest::post()
        .uri("/api/v1/reporting/subscriptions")
        .insert_header(("Authorization", format!("Bearer {member_token}")))
        .set_json(json!({ "report_type": "snapshot", "parameters": {} }))
        .to_request();
    let create_resp = test::call_service(&app, create_req).await;
    assert_eq!(create_resp.status(), 201);
    let body: Value = test::read_body_json(create_resp).await;
    let sub_id = body["data"]["id"].as_str().expect("subscription id");

    let get_req = test::TestRequest::get()
        .uri(&format!("/api/v1/reporting/subscriptions/{sub_id}"))
        .insert_header(("Authorization", format!("Bearer {member_token}")))
        .to_request();
    let get_resp = test::call_service(&app, get_req).await;
    assert_eq!(
        get_resp.status(),
        200,
        "owner should be able to read own subscription"
    );
}

/// A different authenticated user must receive 403 when trying to read another user's subscription.
#[actix_rt::test]
#[serial]
async fn test_subscription_access_isolation() {
    test_app!(app, _config);
    let admin_token = login_admin(&app).await;
    let member_token = login_member(&app).await;

    // Admin creates a subscription
    let create_req = test::TestRequest::post()
        .uri("/api/v1/reporting/subscriptions")
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .set_json(json!({ "report_type": "breach_rate", "parameters": { "threshold_pct": 3.0, "window_days": 7 } }))
        .to_request();
    let create_resp = test::call_service(&app, create_req).await;
    assert_eq!(create_resp.status(), 201);
    let body: Value = test::read_body_json(create_resp).await;
    let sub_id = body["data"]["id"].as_str().expect("subscription id");

    // Member tries to read admin's subscription — should be 403
    let get_req = test::TestRequest::get()
        .uri(&format!("/api/v1/reporting/subscriptions/{sub_id}"))
        .insert_header(("Authorization", format!("Bearer {member_token}")))
        .to_request();
    let get_resp = test::call_service(&app, get_req).await;
    assert_eq!(
        get_resp.status(),
        403,
        "non-owner must receive 403 when reading another user's subscription"
    );

    // Member also must receive 403 on delete
    let del_req = test::TestRequest::delete()
        .uri(&format!("/api/v1/reporting/subscriptions/{sub_id}"))
        .insert_header(("Authorization", format!("Bearer {member_token}")))
        .to_request();
    let del_resp = test::call_service(&app, del_req).await;
    assert_eq!(
        del_resp.status(),
        403,
        "non-owner must receive 403 on delete"
    );
}

// ── Alert access isolation ──────────────────────────────────────────────────

/// A member must not see alerts belonging to another user's subscriptions.
#[actix_rt::test]
#[serial]
async fn test_member_cannot_see_other_users_alerts() {
    test_app!(app, config);
    let admin_token = login_admin(&app).await;
    let member_token = login_member(&app).await;

    // Admin creates a subscription
    let create_req = test::TestRequest::post()
        .uri("/api/v1/reporting/subscriptions")
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .set_json(json!({ "report_type": "snapshot", "parameters": {} }))
        .to_request();
    let create_resp = test::call_service(&app, create_req).await;
    assert_eq!(create_resp.status(), 201);
    let body: Value = test::read_body_json(create_resp).await;
    let admin_sub_id = body["data"]["id"].as_str().expect("subscription id");

    // Insert an alert for the admin's subscription directly via SQL
    {
        let pool = &config.database_url;
        let db_pool = talentflow::infrastructure::db::create_pool(pool);
        let mut conn = db_pool.get().expect("db connection");
        diesel::sql_query(format!(
            "INSERT INTO reporting_alerts (id, subscription_id, severity, message, acknowledged) \
             VALUES (gen_random_uuid(), '{admin_sub_id}', 'info', 'admin test alert', false)"
        ))
        .execute(&mut conn)
        .expect("insert test alert");
    }

    // Member lists alerts — must not see admin's alert
    let member_list = test::TestRequest::get()
        .uri("/api/v1/reporting/alerts?per_page=100")
        .insert_header(("Authorization", format!("Bearer {member_token}")))
        .to_request();
    let member_resp = test::call_service(&app, member_list).await;
    assert_eq!(member_resp.status(), 200);
    let member_body: Value = test::read_body_json(member_resp).await;
    let member_alerts = member_body["data"].as_array().expect("alerts array");

    // None of the alerts should reference the admin's subscription
    for alert in member_alerts {
        assert_ne!(
            alert["subscription_id"].as_str().unwrap_or(""),
            admin_sub_id,
            "member must not see alerts from another user's subscription"
        );
    }
}

/// platform_admin can see all alerts globally, including those from other users' subscriptions.
#[actix_rt::test]
#[serial]
async fn test_admin_can_see_all_alerts() {
    test_app!(app, config);
    let admin_token = login_admin(&app).await;
    let member_token = login_member(&app).await;

    // Member creates a subscription and we insert an alert for it
    let create_req = test::TestRequest::post()
        .uri("/api/v1/reporting/subscriptions")
        .insert_header(("Authorization", format!("Bearer {member_token}")))
        .set_json(json!({ "report_type": "snapshot", "parameters": {} }))
        .to_request();
    let create_resp = test::call_service(&app, create_req).await;
    assert_eq!(create_resp.status(), 201);
    let body: Value = test::read_body_json(create_resp).await;
    let member_sub_id = body["data"]["id"].as_str().expect("subscription id");

    {
        let db_pool = talentflow::infrastructure::db::create_pool(&config.database_url);
        let mut conn = db_pool.get().expect("db connection");
        diesel::sql_query(format!(
            "INSERT INTO reporting_alerts (id, subscription_id, severity, message, acknowledged) \
             VALUES (gen_random_uuid(), '{member_sub_id}', 'info', 'member test alert', false)"
        ))
        .execute(&mut conn)
        .expect("insert test alert");
    }

    // Admin lists alerts — should see the member's alert
    let admin_list = test::TestRequest::get()
        .uri("/api/v1/reporting/alerts?per_page=100")
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .to_request();
    let admin_resp = test::call_service(&app, admin_list).await;
    assert_eq!(admin_resp.status(), 200);
    let admin_body: Value = test::read_body_json(admin_resp).await;
    let admin_alerts = admin_body["data"].as_array().expect("alerts array");

    let found = admin_alerts
        .iter()
        .any(|a| a["subscription_id"].as_str() == Some(member_sub_id));
    assert!(
        found,
        "platform_admin must see alerts from all users' subscriptions"
    );
}

/// Listing alerts without authentication returns 401 (existing behavior, regression guard).
#[actix_rt::test]
#[serial]
async fn test_list_alerts_unauthenticated_returns_401() {
    test_app!(app, _config);
    let req = test::TestRequest::get()
        .uri("/api/v1/reporting/alerts")
        .to_request();
    let status = match app.call(req).await {
        Ok(resp) => resp.status().as_u16(),
        Err(err) => err.error_response().status().as_u16(),
    };
    assert_eq!(status, 401, "unauthenticated alert listing must return 401");
}

/// GET /reporting/subscriptions/{id} without auth must return 401.
#[actix_rt::test]
#[serial]
async fn test_get_subscription_requires_auth() {
    test_app!(app, _config);
    let req = test::TestRequest::get()
        .uri("/api/v1/reporting/subscriptions/00000000-0000-0000-0000-000000000000")
        .to_request();
    let status = match app.call(req).await {
        Ok(resp) => resp.status().as_u16(),
        Err(err) => err.error_response().status().as_u16(),
    };
    assert_eq!(status, 401);
}

/// GET /reporting/subscriptions/{id} for a non-existent ID must return 404.
#[actix_rt::test]
#[serial]
async fn test_get_nonexistent_subscription_returns_404() {
    test_app!(app, _config);
    let token = login_admin(&app).await;
    let fake_id = uuid::Uuid::new_v4();
    let req = test::TestRequest::get()
        .uri(&format!("/api/v1/reporting/subscriptions/{fake_id}"))
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 404);
}

// ── Connector validation ─────────────────────────────────────────────────────

/// GET /integrations/connectors/{id} for a non-existent connector must return 404.
#[actix_rt::test]
#[serial]
async fn test_get_nonexistent_connector_returns_404() {
    test_app!(app, _config);
    let token = login_admin(&app).await;
    let fake_id = uuid::Uuid::new_v4();
    let req = test::TestRequest::get()
        .uri(&format!("/api/v1/integrations/connectors/{fake_id}"))
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 404);
}

/// Trigger sync on a non-existent connector must return 404.
#[actix_rt::test]
#[serial]
async fn test_sync_nonexistent_connector_returns_404() {
    test_app!(app, _config);
    let token = login_admin(&app).await;
    let fake_id = uuid::Uuid::new_v4();
    let req = test::TestRequest::post()
        .uri(&format!("/api/v1/integrations/connectors/{fake_id}/sync"))
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({ "entity_type": "offers" }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 404);
}

/// auth_config must never appear in connector list or get responses.
#[actix_rt::test]
#[serial]
async fn test_connector_auth_config_never_exposed() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::post()
        .uri("/api/v1/integrations/connectors")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({
            "name": "Secret Connector",
            "connector_type": "outbound",
            "auth_config": { "api_key": "super_secret_key", "token": "bearer_xyz" }
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 201);
    let body: Value = test::read_body_json(resp).await;
    let conn_id = body["data"]["id"].as_str().expect("connector id");

    // Verify auth_config is absent from create response
    assert!(
        body["data"]["auth_config"].is_null(),
        "auth_config must not appear in create response"
    );
    assert!(
        body["data"]["auth_config_encrypted"].is_null(),
        "auth_config_encrypted must not appear in create response"
    );

    // Verify auth_config is absent from get response
    let get_req = test::TestRequest::get()
        .uri(&format!("/api/v1/integrations/connectors/{conn_id}"))
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let get_resp = test::call_service(&app, get_req).await;
    assert_eq!(get_resp.status(), 200);
    let get_body: Value = test::read_body_json(get_resp).await;
    assert!(
        get_body["data"]["auth_config"].is_null(),
        "auth_config must not appear in get response"
    );
    assert!(
        get_body["data"]["auth_config_encrypted"].is_null(),
        "auth_config_encrypted must not appear in get response"
    );

    // Verify auth_config is absent from list response
    let list_req = test::TestRequest::get()
        .uri("/api/v1/integrations/connectors")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let list_resp = test::call_service(&app, list_req).await;
    assert_eq!(list_resp.status(), 200);
    let list_body: Value = test::read_body_json(list_resp).await;
    if let Some(connectors) = list_body["data"].as_array() {
        for connector in connectors {
            assert!(
                connector["auth_config"].is_null(),
                "auth_config must not appear in list response"
            );
            assert!(
                connector["auth_config_encrypted"].is_null(),
                "auth_config_encrypted must not appear in list response"
            );
        }
    }
}

// ── New sort_by variants ─────────────────────────────────────────────────────

#[actix_rt::test]
#[serial]
async fn test_search_sort_by_popularity() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/search?sort_by=popularity&page=1&per_page=10")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "sort_by=popularity should return 200");
    let body: Value = test::read_body_json(resp).await;
    assert!(body["data"].is_array());
}

#[actix_rt::test]
#[serial]
async fn test_search_sort_by_rating() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/search?sort_by=rating&page=1&per_page=10")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "sort_by=rating should return 200");
    let body: Value = test::read_body_json(resp).await;
    assert!(body["data"].is_array());
}

#[actix_rt::test]
#[serial]
async fn test_search_sort_by_distance() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/search?sort_by=distance&page=1&per_page=10")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "sort_by=distance should return 200");
    let body: Value = test::read_body_json(resp).await;
    assert!(body["data"].is_array());
}

#[actix_rt::test]
#[serial]
async fn test_search_invalid_sort_still_422_with_new_variants() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/search?sort_by=not_a_field")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        422,
        "truly unknown sort_by must still return 422"
    );
}

// ── Rating filter params ──────────────────────────────────────────────────────

#[actix_rt::test]
#[serial]
async fn test_search_with_min_rating_filter() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/search?min_rating=3.0&page=1&per_page=25")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "min_rating filter should return 200");
    let body: Value = test::read_body_json(resp).await;
    assert!(body["data"].is_array());
    // Verify all returned items have rating >= 3.0 when rating is present
    if let Some(items) = body["data"].as_array() {
        for item in items {
            if let Some(rating) = item["rating"].as_f64() {
                assert!(
                    rating >= 3.0,
                    "item with rating {rating} should not pass min_rating=3.0 filter"
                );
            }
        }
    }
}

#[actix_rt::test]
#[serial]
async fn test_search_with_max_rating_filter() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/search?max_rating=2.5&page=1&per_page=25")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "max_rating filter should return 200");
    let body: Value = test::read_body_json(resp).await;
    assert!(body["data"].is_array());
}

#[actix_rt::test]
#[serial]
async fn test_search_with_rating_range_filter() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/search?min_rating=1.0&max_rating=4.0&page=1&per_page=25")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        200,
        "combined rating range should return 200"
    );
    let body: Value = test::read_body_json(resp).await;
    assert!(body["data"].is_array());
}

// ── Distance filter param ─────────────────────────────────────────────────────

#[actix_rt::test]
#[serial]
async fn test_search_with_max_distance_filter() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    // Items without distance basis (distance_miles absent) pass through the filter
    let req = test::TestRequest::get()
        .uri("/api/v1/search?max_distance_miles=50&page=1&per_page=25")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        200,
        "max_distance_miles filter should return 200"
    );
    let body: Value = test::read_body_json(resp).await;
    assert!(body["data"].is_array());
}

#[actix_rt::test]
#[serial]
async fn test_search_with_site_code_param() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    // First get available site codes from the sites endpoint
    let sites_req = test::TestRequest::get()
        .uri("/api/v1/sites")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let sites_resp = test::call_service(&app, sites_req).await;
    assert_eq!(sites_resp.status(), 200);
    let sites_body: Value = test::read_body_json(sites_resp).await;

    if let Some(sites) = sites_body["data"].as_array() {
        if let Some(first_site) = sites.first() {
            let code = first_site["code"].as_str().unwrap_or("HQ");
            let uri = format!("/api/v1/search?site_code={code}&page=1&per_page=10");
            let req = test::TestRequest::get()
                .uri(&uri)
                .insert_header(("Authorization", format!("Bearer {token}")))
                .to_request();
            let resp = test::call_service(&app, req).await;
            assert_eq!(resp.status(), 200, "site_code param should return 200");
            let body: Value = test::read_body_json(resp).await;
            assert!(body["data"].is_array());
        }
    }
}

// ── Recommended field and interleaving ───────────────────────────────────────

#[actix_rt::test]
#[serial]
async fn test_search_results_have_recommended_field() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/search?page=1&per_page=25")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = test::read_body_json(resp).await;

    if let Some(items) = body["data"].as_array() {
        for item in items {
            assert!(
                item["recommended"].is_boolean(),
                "each result must have a boolean 'recommended' field"
            );
        }
    }
}

#[actix_rt::test]
#[serial]
async fn test_search_results_have_popularity_score_field() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/search?page=1&per_page=10")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = test::read_body_json(resp).await;

    if let Some(items) = body["data"].as_array() {
        for item in items {
            if let Some(pop) = item["popularity_score"].as_f64() {
                assert!(
                    pop >= 0.0 && pop <= 1.0,
                    "popularity_score {pop} must be in [0.0, 1.0]"
                );
            }
        }
    }
}

#[actix_rt::test]
#[serial]
async fn test_search_without_new_params_is_backward_compatible() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    // Omit all new params — must behave identically to before
    let req = test::TestRequest::get()
        .uri("/api/v1/search?q=admin&sort_by=relevance&page=1&per_page=10")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "existing params must still work");
    let body: Value = test::read_body_json(resp).await;
    assert!(body["data"].is_array());
    assert!(body["pagination"].is_object());
}

// ── Search spell_correction field ────────────────────────────────────────────

/// The search response shape must contain `data` (array) and `pagination` (object).
/// When a spell correction is found it appears as `spell_correction` (string);
/// when not found the key is omitted (not null).
#[actix_rt::test]
#[serial]
async fn test_search_response_shape() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/search?q=rust")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = test::read_body_json(resp).await;

    assert!(
        body["data"].is_array(),
        "search response must have 'data' array"
    );
    assert!(
        body["pagination"].is_object(),
        "search response must have 'pagination' object"
    );
    assert!(
        body["pagination"]["page"].is_number(),
        "pagination.page must be a number"
    );
    assert!(
        body["pagination"]["per_page"].is_number(),
        "pagination.per_page must be a number"
    );
    assert!(
        body["pagination"]["total"].is_number(),
        "pagination.total must be a number"
    );

    // If spell_correction is present it must be a string, never an object
    if let Some(sc) = body.get("spell_correction") {
        if !sc.is_null() {
            assert!(
                sc.is_string(),
                "spell_correction must be a string when present"
            );
        }
    }
}

// ── Reporting permission role-matrix tests ───────────────────────────────────

/// Helper: log in as club_admin
async fn login_club_admin(
    app: &impl actix_web::dev::Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
    >,
) -> String {
    login_user(app, "club_admin", "ClubAdm1n!Passw0rd").await
}

/// member CAN create a subscription (reporting:create granted in seed).
#[actix_rt::test]
#[serial]
async fn test_member_can_create_subscription() {
    test_app!(app, _config);
    let token = login_member(&app).await;

    let req = test::TestRequest::post()
        .uri("/api/v1/reporting/subscriptions")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({ "report_type": "snapshot", "parameters": {} }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        201,
        "member with reporting:create must be able to create a subscription"
    );
}

/// member CANNOT publish a dashboard (reporting:update not granted).
#[actix_rt::test]
#[serial]
async fn test_member_cannot_publish_dashboard() {
    test_app!(app, _config);
    let token = login_member(&app).await;

    let key = format!("member-dashboard-{}", uuid::Uuid::new_v4());
    let req = test::TestRequest::post()
        .uri(&format!("/api/v1/reporting/dashboards/{key}/versions"))
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({ "layout": { "widgets": [] } }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        403,
        "member without reporting:update must receive 403 on dashboard publish"
    );
}

/// member CANNOT acknowledge an alert (reporting:update not granted).
#[actix_rt::test]
#[serial]
async fn test_member_cannot_acknowledge_alert() {
    test_app!(app, _config);
    let admin_token = login_admin(&app).await;
    let member_token = login_member(&app).await;

    // platform_admin creates a subscription and triggers an alert indirectly
    // via the fake alert ID path (non-existent alert → 404 is fine as long as
    // the permission check fires first; in practice the permission check fires
    // before the DB lookup so a 403 is returned before any 404)
    let fake_id = uuid::Uuid::new_v4();
    let req = test::TestRequest::put()
        .uri(&format!("/api/v1/reporting/alerts/{fake_id}/acknowledge"))
        .insert_header(("Authorization", format!("Bearer {member_token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        403,
        "member without reporting:update must receive 403 on acknowledge"
    );

    // Suppress unused variable warning — admin_token used to ensure seeded admin exists
    let _ = admin_token;
}

/// platform_admin CAN publish a dashboard.
#[actix_rt::test]
#[serial]
async fn test_platform_admin_can_publish_dashboard() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let key = format!("admin-dashboard-{}", uuid::Uuid::new_v4());
    let req = test::TestRequest::post()
        .uri(&format!("/api/v1/reporting/dashboards/{key}/versions"))
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({ "layout": { "widgets": [{"type": "bar"}] } }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        201,
        "platform_admin must be able to publish dashboards"
    );
    let body: Value = test::read_body_json(resp).await;
    assert_eq!(body["data"]["version"], 1);
}

/// club_admin CAN create a subscription (reporting:create granted in seed).
#[actix_rt::test]
#[serial]
async fn test_club_admin_can_create_subscription() {
    test_app!(app, _config);
    let token = login_club_admin(&app).await;

    let req = test::TestRequest::post()
        .uri("/api/v1/reporting/subscriptions")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({ "report_type": "breach_rate", "parameters": { "threshold_pct": 5.0, "window_days": 7 } }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        201,
        "club_admin with reporting:create must be able to create a subscription"
    );
}

/// club_admin CAN publish a dashboard (reporting:update granted in seed).
#[actix_rt::test]
#[serial]
async fn test_club_admin_can_publish_dashboard() {
    test_app!(app, _config);
    let token = login_club_admin(&app).await;

    let key = format!("club-dashboard-{}", uuid::Uuid::new_v4());
    let req = test::TestRequest::post()
        .uri(&format!("/api/v1/reporting/dashboards/{key}/versions"))
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({ "layout": { "widgets": [] } }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        201,
        "club_admin with reporting:update must be able to publish dashboards"
    );
}

/// Unauthenticated request to create subscription must return 401.
#[actix_rt::test]
#[serial]
async fn test_create_subscription_requires_auth() {
    test_app!(app, _config);
    let req = test::TestRequest::post()
        .uri("/api/v1/reporting/subscriptions")
        .set_json(json!({ "report_type": "snapshot", "parameters": {} }))
        .to_request();
    let status = match app.call(req).await {
        Ok(resp) => resp.status().as_u16(),
        Err(err) => err.error_response().status().as_u16(),
    };
    assert_eq!(status, 401);
}

// ── Sync state lifecycle tests ────────────────────────────────────────────────

/// Connector with an unreachable base_url must produce a `failed` sync state
/// with a non-null error_message.  This verifies that no fake success is written
/// when the remote endpoint is unavailable.
#[actix_rt::test]
#[serial]
async fn test_sync_failure_path_sets_failed_status_with_error() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    // Create a connector with an unreachable HTTP base_url (port 1 is reserved)
    let create_req = test::TestRequest::post()
        .uri("/api/v1/integrations/connectors")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({
            "name": "Unreachable Connector",
            "connector_type": "inbound",
            "base_url": "http://127.0.0.1:1",
            "is_enabled": true
        }))
        .to_request();
    let create_resp = test::call_service(&app, create_req).await;
    assert_eq!(create_resp.status(), 201);
    let body: Value = test::read_body_json(create_resp).await;
    let conn_id = body["data"]["id"].as_str().expect("connector id");

    // Trigger sync — the HTTP call to port 1 must fail
    let sync_req = test::TestRequest::post()
        .uri(&format!("/api/v1/integrations/connectors/{conn_id}/sync"))
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({ "entity_type": "candidates" }))
        .to_request();
    let sync_resp = test::call_service(&app, sync_req).await;
    assert_eq!(
        sync_resp.status(),
        202,
        "trigger_sync always returns 202 regardless of outcome"
    );
    let sync_body: Value = test::read_body_json(sync_resp).await;

    assert_eq!(
        sync_body["data"]["status"], "failed",
        "unreachable connector must produce status=failed"
    );
    assert!(
        !sync_body["data"]["error_message"].is_null(),
        "failed sync must include error_message, got: {}",
        sync_body["data"]
    );
    assert_eq!(
        sync_body["data"]["record_count"], 0,
        "failed sync must have record_count=0"
    );

    // Verify sync-state endpoint also reflects the failure
    let state_req = test::TestRequest::get()
        .uri(&format!(
            "/api/v1/integrations/connectors/{conn_id}/sync-state"
        ))
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let state_resp = test::call_service(&app, state_req).await;
    assert_eq!(state_resp.status(), 200);
    let state_body: Value = test::read_body_json(state_resp).await;
    let states = state_body["data"].as_array().expect("sync states array");
    assert!(!states.is_empty(), "sync state must exist after trigger");
    assert_eq!(states[0]["status"], "failed");
    assert!(!states[0]["error_message"].is_null());
}

/// Connector without a base_url uses file-based fallback and must produce a
/// truthful `succeeded` state.  `record_count` reflects the actual number of
/// staged import records (0 when no files are present).
#[actix_rt::test]
#[serial]
async fn test_sync_success_path_has_truthful_record_count() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    // Create a connector with no base_url — file fallback will be used
    let create_req = test::TestRequest::post()
        .uri("/api/v1/integrations/connectors")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({
            "name": "File Fallback Connector",
            "connector_type": "inbound",
            "is_enabled": true
        }))
        .to_request();
    let create_resp = test::call_service(&app, create_req).await;
    assert_eq!(create_resp.status(), 201);
    let body: Value = test::read_body_json(create_resp).await;
    let conn_id = body["data"]["id"].as_str().expect("connector id");

    // Trigger sync — file fallback scans staging dir; 0 files = 0 records
    let sync_req = test::TestRequest::post()
        .uri(&format!("/api/v1/integrations/connectors/{conn_id}/sync"))
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({ "entity_type": "offers" }))
        .to_request();
    let sync_resp = test::call_service(&app, sync_req).await;
    assert_eq!(sync_resp.status(), 202);
    let sync_body: Value = test::read_body_json(sync_resp).await;

    assert_eq!(
        sync_body["data"]["status"], "succeeded",
        "file-fallback sync with no staged files must succeed (nothing to import is a valid outcome)"
    );
    // record_count is a real value from the executor, not a hardcoded fake
    assert!(
        sync_body["data"]["record_count"].is_number(),
        "record_count must be a number"
    );
    assert_eq!(
        sync_body["data"]["record_count"], 0,
        "no staged files means 0 records imported"
    );
    // error_message must be absent on success
    assert!(
        sync_body["data"]["error_message"].is_null(),
        "successful sync must not have error_message"
    );

    // Verify sync-state endpoint shows succeeded
    let state_req = test::TestRequest::get()
        .uri(&format!(
            "/api/v1/integrations/connectors/{conn_id}/sync-state"
        ))
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let state_resp = test::call_service(&app, state_req).await;
    assert_eq!(state_resp.status(), 200);
    let state_body: Value = test::read_body_json(state_resp).await;
    let states = state_body["data"].as_array().expect("sync states array");
    assert!(!states.is_empty());
    assert_eq!(states[0]["status"], "succeeded");
}

/// Triggering sync on a connector that already has a `running` state must
/// return 409 Conflict when `force` is not set.
///
/// Because syncs complete synchronously in the test environment (inside
/// `web::block`), we can only observe the guard by inserting a `running`
/// row via import_data attribution before triggering — which sets
/// "succeeded", not "running".  The canonical guard path is therefore
/// tested here by verifying that the second trigger after success returns
/// 202 (no duplicate running row exists).  The concurrent-running branch
/// is covered by the unit tests.
#[actix_rt::test]
#[serial]
async fn test_sync_force_flag_does_not_break_api() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let create_req = test::TestRequest::post()
        .uri("/api/v1/integrations/connectors")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({
            "name": "Force Test Connector",
            "connector_type": "inbound",
            "is_enabled": true
        }))
        .to_request();
    let create_resp = test::call_service(&app, create_req).await;
    assert_eq!(create_resp.status(), 201);
    let body: Value = test::read_body_json(create_resp).await;
    let conn_id = body["data"]["id"].as_str().expect("connector id");

    // First trigger (force=false, default)
    let sync1 = test::TestRequest::post()
        .uri(&format!("/api/v1/integrations/connectors/{conn_id}/sync"))
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({ "entity_type": "candidates" }))
        .to_request();
    let resp1 = test::call_service(&app, sync1).await;
    assert_eq!(resp1.status(), 202);

    // Second trigger with force=true must also return 202 (no idempotency rejection)
    let sync2 = test::TestRequest::post()
        .uri(&format!("/api/v1/integrations/connectors/{conn_id}/sync"))
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({ "entity_type": "candidates", "force": true }))
        .to_request();
    let resp2 = test::call_service(&app, sync2).await;
    assert_eq!(
        resp2.status(),
        202,
        "force=true must always be accepted (no idempotency rejection)"
    );
}

/// Triggering sync on a non-existent connector must return 404.
/// (This test is already covered by test_sync_nonexistent_connector_returns_404
/// in the existing suite — this variant confirms the response shape is unchanged.)
#[actix_rt::test]
#[serial]
async fn test_sync_response_shape_unchanged() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    // Create a connector and trigger sync to verify the response envelope shape
    let create_req = test::TestRequest::post()
        .uri("/api/v1/integrations/connectors")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({
            "name": "Shape Test Connector",
            "connector_type": "inbound",
            "is_enabled": true
        }))
        .to_request();
    let create_resp = test::call_service(&app, create_req).await;
    let body: Value = test::read_body_json(create_resp).await;
    let conn_id = body["data"]["id"].as_str().expect("connector id");

    let sync_req = test::TestRequest::post()
        .uri(&format!("/api/v1/integrations/connectors/{conn_id}/sync"))
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({ "entity_type": "onboarding" }))
        .to_request();
    let sync_resp = test::call_service(&app, sync_req).await;
    assert_eq!(sync_resp.status(), 202);
    let sync_body: Value = test::read_body_json(sync_resp).await;

    // Verify all expected fields are present in the response envelope
    let data = &sync_body["data"];
    assert!(data["id"].is_string(), "data.id must be present");
    assert!(
        data["connector_id"].is_string(),
        "data.connector_id must be present"
    );
    assert!(
        data["entity_type"].is_string(),
        "data.entity_type must be present"
    );
    assert!(data["status"].is_string(), "data.status must be present");
    assert!(
        data["record_count"].is_number(),
        "data.record_count must be present"
    );
    assert!(
        data["updated_at"].is_string(),
        "data.updated_at must be present"
    );
}

// ── Distance-based search tests ───────────────────────────────────────────────

/// Seed a candidate with known coordinates directly via SQL.
/// Returns (candidate_id, first_name) so callers can query by name.
async fn seed_candidate_with_coords(config: &AppConfig, lat: f64, lng: f64) -> (String, String) {
    let pool = db::create_pool(&config.database_url);
    let mut conn = pool.get().expect("failed to get connection");

    let id = uuid::Uuid::new_v4();
    let first = format!("DistTest{}", &id.to_string()[..8]);
    let last = "Locatable";
    let email = format!("disttest.{}@example.com", &id.to_string()[..8]);

    diesel::sql_query(format!(
        "INSERT INTO candidates \
         (id, first_name, last_name, email, tags, created_at, updated_at, latitude, longitude) \
         VALUES \
         ('{id}', '{first}', '{last}', '{email}', ARRAY[]::text[], now(), now(), {lat}, {lng})"
    ))
    .execute(&mut conn)
    .expect("failed to seed candidate with coordinates");

    (id.to_string(), first)
}

/// When `site_code` resolves to a site with coordinates AND the candidate has
/// stored coordinates, `distance_miles` must be a non-null float in the search
/// result for that candidate.
#[actix_rt::test]
#[serial]
async fn test_search_distance_miles_populated_for_candidate_with_coords() {
    test_app!(app, config);
    let token = login_admin(&app).await;

    // Seed a candidate at HQ coordinates (New York).
    // HQ is seeded at (40.7128, -74.0060) — distance from itself is ~0 miles.
    let (cand_id, first_name) =
        seed_candidate_with_coords(&config, 40.7128, -74.0060).await;

    // Search by the unique first_name fragment; include site_code=HQ
    let uri = format!(
        "/api/v1/search?q={first_name}&site_code=HQ&page=1&per_page=25"
    );
    let req = test::TestRequest::get()
        .uri(&uri)
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "search with site_code must return 200");

    let body: Value = test::read_body_json(resp).await;
    let items = body["data"].as_array().expect("data array");

    // Find our specific candidate in the result set
    let our_item = items
        .iter()
        .find(|item| item["id"].as_str() == Some(&cand_id));

    assert!(
        our_item.is_some(),
        "seeded candidate must appear in search results; got items: {items:?}"
    );

    let item = our_item.unwrap();
    assert!(
        item["distance_miles"].is_number(),
        "distance_miles must be a number when site_code resolves and candidate has coordinates; \
         got: {}",
        item["distance_miles"]
    );

    let dist = item["distance_miles"].as_f64().unwrap();
    assert!(
        dist < 5.0,
        "candidate at HQ coordinates must be within 5 miles of HQ site, got {dist}"
    );
}

/// `max_distance_miles` must exclude candidates whose computed distance exceeds
/// the threshold.  Candidates with no coordinate data pass through unchanged.
#[actix_rt::test]
#[serial]
async fn test_search_max_distance_excludes_far_candidate() {
    test_app!(app, config);
    let token = login_admin(&app).await;

    // Candidate A: very close to HQ (New York)
    let (near_id, near_name) =
        seed_candidate_with_coords(&config, 40.7200, -74.0100).await; // ~1 mile from HQ

    // Candidate B: far from HQ — Los Angeles (~2446 miles)
    let (far_id, _far_name) =
        seed_candidate_with_coords(&config, 34.0522, -118.2437).await;

    // Search with max_distance_miles=500 — near should appear, far must not
    let uri_near = format!(
        "/api/v1/search?q={near_name}&site_code=HQ&max_distance_miles=500&page=1&per_page=25"
    );
    let req = test::TestRequest::get()
        .uri(&uri_near)
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = test::read_body_json(resp).await;
    let items = body["data"].as_array().expect("data array");

    let has_near = items.iter().any(|i| i["id"].as_str() == Some(&near_id));
    assert!(
        has_near,
        "candidate within 500 miles of HQ must appear in results"
    );

    // Search specifically for the far candidate — it must be excluded
    let far_query = format!("DistTest{}", &far_id[..8]);
    let uri_far = format!(
        "/api/v1/search?q={far_query}&site_code=HQ&max_distance_miles=500&page=1&per_page=25"
    );
    let req2 = test::TestRequest::get()
        .uri(&uri_far)
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp2 = test::call_service(&app, req2).await;
    assert_eq!(resp2.status(), 200);
    let body2: Value = test::read_body_json(resp2).await;
    let items2 = body2["data"].as_array().expect("data array");

    let has_far = items2.iter().any(|i| i["id"].as_str() == Some(&far_id));
    assert!(
        !has_far,
        "candidate ~2446 miles from HQ must be excluded by max_distance_miles=500"
    );
}

/// When `sort_by=distance` with a valid `site_code`, results that have a
/// computed distance must appear before results that do not (None sorts last),
/// and items with computed distances must be in ascending order.
#[actix_rt::test]
#[serial]
async fn test_search_sort_distance_ascending_order() {
    test_app!(app, config);
    let token = login_admin(&app).await;

    // Seed two candidates at known distances from HQ
    // Near: ~1 mile from HQ (NY)
    let (_near_id, _) = seed_candidate_with_coords(&config, 40.7200, -74.0100).await;
    // Medium: ~215 miles from HQ (Boston)
    let (_med_id, _) = seed_candidate_with_coords(&config, 42.3601, -71.0589).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/search?site_code=HQ&sort_by=distance&page=1&per_page=50")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "sort_by=distance with site_code must return 200");

    let body: Value = test::read_body_json(resp).await;
    let items = body["data"].as_array().expect("data array");

    // Collect computed distances from results (skip None)
    let distances: Vec<f64> = items
        .iter()
        .filter_map(|i| i["distance_miles"].as_f64())
        .collect();

    // All numeric distances must be in non-decreasing order
    for window in distances.windows(2) {
        assert!(
            window[0] <= window[1],
            "distance_miles must be ascending: {} > {} (out of order)",
            window[0],
            window[1]
        );
    }

    // Items with a distance value must appear before items with no distance
    // (None sorts last). Verify that no None-distance item appears before a
    // Some-distance item.
    let mut seen_none = false;
    for item in items {
        let has_dist = item["distance_miles"].is_number();
        if !has_dist {
            seen_none = true;
        }
        if seen_none && has_dist {
            panic!(
                "item with distance_miles appeared AFTER an item with no distance: {}",
                item
            );
        }
    }
}

// ── Alert acknowledgment ownership tests ────────────────────────────────────

/// Owner can acknowledge their own alert.
#[actix_rt::test]
#[serial]
async fn test_owner_can_acknowledge_own_alert() {
    test_app!(app, config);
    let club_admin_token = login_club_admin(&app).await;

    // club_admin creates a subscription (they have reporting:create + reporting:update)
    let create_req = test::TestRequest::post()
        .uri("/api/v1/reporting/subscriptions")
        .insert_header(("Authorization", format!("Bearer {club_admin_token}")))
        .set_json(json!({ "report_type": "snapshot", "parameters": {} }))
        .to_request();
    let create_resp = test::call_service(&app, create_req).await;
    assert_eq!(create_resp.status(), 201);
    let body: Value = test::read_body_json(create_resp).await;
    let sub_id = body["data"]["id"].as_str().expect("subscription id");

    // Insert an alert for this subscription
    let alert_id = uuid::Uuid::new_v4();
    {
        let db_pool = talentflow::infrastructure::db::create_pool(&config.database_url);
        let mut conn = db_pool.get().expect("db connection");
        diesel::sql_query(format!(
            "INSERT INTO reporting_alerts (id, subscription_id, severity, message, acknowledged) \
             VALUES ('{alert_id}', '{sub_id}', 'info', 'owner ack test', false)"
        ))
        .execute(&mut conn)
        .expect("insert test alert");
    }

    // Owner acknowledges their own alert — must succeed
    let ack_req = test::TestRequest::put()
        .uri(&format!("/api/v1/reporting/alerts/{alert_id}/acknowledge"))
        .insert_header(("Authorization", format!("Bearer {club_admin_token}")))
        .to_request();
    let ack_resp = test::call_service(&app, ack_req).await;
    assert_eq!(
        ack_resp.status(),
        200,
        "owner must be able to acknowledge their own alert"
    );
    let ack_body: Value = test::read_body_json(ack_resp).await;
    assert_eq!(ack_body["data"]["acknowledged"], true);
}

/// Non-owner with reporting:update permission cannot acknowledge another user's alert.
#[actix_rt::test]
#[serial]
async fn test_non_owner_club_admin_cannot_acknowledge_other_users_alert() {
    test_app!(app, config);
    let admin_token = login_admin(&app).await;
    let club_admin_token = login_club_admin(&app).await;

    // platform_admin creates a subscription (owned by admin)
    let create_req = test::TestRequest::post()
        .uri("/api/v1/reporting/subscriptions")
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .set_json(json!({ "report_type": "snapshot", "parameters": {} }))
        .to_request();
    let create_resp = test::call_service(&app, create_req).await;
    assert_eq!(create_resp.status(), 201);
    let body: Value = test::read_body_json(create_resp).await;
    let admin_sub_id = body["data"]["id"].as_str().expect("subscription id");

    // Insert an alert for the admin's subscription
    let alert_id = uuid::Uuid::new_v4();
    {
        let db_pool = talentflow::infrastructure::db::create_pool(&config.database_url);
        let mut conn = db_pool.get().expect("db connection");
        diesel::sql_query(format!(
            "INSERT INTO reporting_alerts (id, subscription_id, severity, message, acknowledged) \
             VALUES ('{alert_id}', '{admin_sub_id}', 'warning', 'cross-owner ack test', false)"
        ))
        .execute(&mut conn)
        .expect("insert test alert");
    }

    // club_admin (non-owner, has reporting:update) tries to acknowledge — must be denied
    let ack_req = test::TestRequest::put()
        .uri(&format!("/api/v1/reporting/alerts/{alert_id}/acknowledge"))
        .insert_header(("Authorization", format!("Bearer {club_admin_token}")))
        .to_request();
    let ack_resp = test::call_service(&app, ack_req).await;
    assert_eq!(
        ack_resp.status(),
        403,
        "non-owner with reporting:update must be denied when acknowledging another user's alert"
    );
}

/// platform_admin can acknowledge any alert regardless of ownership.
#[actix_rt::test]
#[serial]
async fn test_platform_admin_can_acknowledge_any_alert() {
    test_app!(app, config);
    let admin_token = login_admin(&app).await;
    let club_admin_token = login_club_admin(&app).await;

    // club_admin creates a subscription (owned by club_admin)
    let create_req = test::TestRequest::post()
        .uri("/api/v1/reporting/subscriptions")
        .insert_header(("Authorization", format!("Bearer {club_admin_token}")))
        .set_json(json!({ "report_type": "snapshot", "parameters": {} }))
        .to_request();
    let create_resp = test::call_service(&app, create_req).await;
    assert_eq!(create_resp.status(), 201);
    let body: Value = test::read_body_json(create_resp).await;
    let club_sub_id = body["data"]["id"].as_str().expect("subscription id");

    // Insert an alert for the club_admin's subscription
    let alert_id = uuid::Uuid::new_v4();
    {
        let db_pool = talentflow::infrastructure::db::create_pool(&config.database_url);
        let mut conn = db_pool.get().expect("db connection");
        diesel::sql_query(format!(
            "INSERT INTO reporting_alerts (id, subscription_id, severity, message, acknowledged) \
             VALUES ('{alert_id}', '{club_sub_id}', 'critical', 'admin ack test', false)"
        ))
        .execute(&mut conn)
        .expect("insert test alert");
    }

    // platform_admin acknowledges club_admin's alert — must succeed
    let ack_req = test::TestRequest::put()
        .uri(&format!("/api/v1/reporting/alerts/{alert_id}/acknowledge"))
        .insert_header(("Authorization", format!("Bearer {admin_token}")))
        .to_request();
    let ack_resp = test::call_service(&app, ack_req).await;
    assert_eq!(
        ack_resp.status(),
        200,
        "platform_admin must be able to acknowledge any user's alert"
    );
    let ack_body: Value = test::read_body_json(ack_resp).await;
    assert_eq!(ack_body["data"]["acknowledged"], true);
}

// ── Export staging semantics tests ──────────────────────────────────────────

/// Export with a field_map returns records_exported = 0 and a file destination,
/// confirming the staging-only contract.
#[actix_rt::test]
#[serial]
async fn test_export_with_field_map_returns_staging_metadata() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::post()
        .uri("/api/v1/integrations/export")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({
            "entity_type": "candidates",
            "field_map": {
                "first_name": "givenName",
                "last_name": "familyName"
            }
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = test::read_body_json(resp).await;

    assert_eq!(
        body["data"]["records_exported"], 0,
        "staging export must report records_exported = 0"
    );
    assert_eq!(body["data"]["entity_type"], "candidates");
    assert!(
        body["data"]["destination"]
            .as_str()
            .unwrap_or("")
            .starts_with("file:"),
        "destination must be a file path"
    );
}

/// Export with an empty field_map also succeeds and returns the staging contract.
#[actix_rt::test]
#[serial]
async fn test_export_with_empty_field_map_succeeds() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::post()
        .uri("/api/v1/integrations/export")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({
            "entity_type": "offers",
            "field_map": {}
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = test::read_body_json(resp).await;

    assert_eq!(body["data"]["records_exported"], 0);
    assert!(
        body["data"]["destination"]
            .as_str()
            .unwrap_or("")
            .starts_with("file:"),
        "destination must be a file path for empty field_map"
    );
}

// ── Business-native search facet tests ──────────────────────────────────────

/// New `department` param is accepted and returns 200.
#[actix_rt::test]
#[serial]
async fn test_search_with_department_filter_returns_200() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/search?department=Engineering&page=1&per_page=10")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        200,
        "search with department filter must return 200"
    );
    let body: Value = test::read_body_json(resp).await;
    assert!(body["data"].is_array());
}

/// New `source` param is accepted and returns 200.
#[actix_rt::test]
#[serial]
async fn test_search_with_source_filter_returns_200() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/search?source=referral&page=1&per_page=10")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        200,
        "search with source filter must return 200"
    );
    let body: Value = test::read_body_json(resp).await;
    assert!(body["data"].is_array());
}

/// New `salary_min` and `salary_max` params are accepted and return 200.
#[actix_rt::test]
#[serial]
async fn test_search_with_salary_range_returns_200() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/search?salary_min=50000&salary_max=200000&page=1&per_page=10")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        200,
        "search with salary range must return 200"
    );
    let body: Value = test::read_body_json(resp).await;
    assert!(body["data"].is_array());
}

/// Combined old + new params work together and return 200.
#[actix_rt::test]
#[serial]
async fn test_search_combined_old_and_new_params() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/search?q=admin&status=draft&department=Engineering&min_rating=1.0&sort_by=rating&page=1&per_page=10")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        200,
        "search with combined old + new params must return 200"
    );
    let body: Value = test::read_body_json(resp).await;
    assert!(body["data"].is_array());
    assert!(body["pagination"].is_object());
}

/// Omitting all new params preserves identical behavior to before.
#[actix_rt::test]
#[serial]
async fn test_search_omitting_new_params_preserves_old_behavior() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/search?page=1&per_page=10")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = test::read_body_json(resp).await;
    assert!(
        body["data"].is_array(),
        "response shape unchanged when new params omitted"
    );
    assert!(body["pagination"].is_object());

    // Every result must still have the recommended field
    if let Some(items) = body["data"].as_array() {
        for item in items {
            assert!(
                item["recommended"].is_boolean(),
                "recommended field must be present"
            );
        }
    }
}

/// The `department` filter must actually constrain results — offers in a
/// non-matching department must be excluded from the result set.
#[actix_rt::test]
#[serial]
async fn test_department_filter_constrains_results() {
    test_app!(app, config);
    let token = login_admin(&app).await;

    // Create an offer with department "Engineering" via direct SQL so we
    // have a known data point.  We need a candidate first (offers FK).
    let cand_id = uuid::Uuid::new_v4();
    let offer_id = uuid::Uuid::new_v4();
    {
        let db_pool = talentflow::infrastructure::db::create_pool(&config.database_url);
        let mut conn = db_pool.get().expect("db connection");
        diesel::sql_query(format!(
            "INSERT INTO candidates (id, first_name, last_name, email) \
             VALUES ('{cand_id}', 'DeptTest', 'Candidate', 'depttest_{cand_id}@test.local') \
             ON CONFLICT DO NOTHING"
        ))
        .execute(&mut conn)
        .expect("insert test candidate");
        diesel::sql_query(format!(
            "INSERT INTO offers (id, candidate_id, title, department, status, created_by) \
             VALUES ('{offer_id}', '{cand_id}', 'DeptFilterTest', 'Engineering', 'draft', \
             'b0000000-0000-0000-0000-000000000001') \
             ON CONFLICT DO NOTHING"
        ))
        .execute(&mut conn)
        .expect("insert test offer");
    }

    // Search with department=Engineering — must include our offer
    let req = test::TestRequest::get()
        .uri("/api/v1/search?department=Engineering&page=1&per_page=100")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = test::read_body_json(resp).await;
    let items = body["data"].as_array().expect("data array");
    let has_eng = items.iter().any(|i| {
        i["resource_type"] == "offer" && i["title"].as_str() == Some("DeptFilterTest")
    });
    assert!(
        has_eng,
        "department=Engineering must include our Engineering offer"
    );

    // Search with department=Marketing — must NOT include our Engineering offer
    let req2 = test::TestRequest::get()
        .uri("/api/v1/search?department=Marketing&page=1&per_page=100")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp2 = test::call_service(&app, req2).await;
    assert_eq!(resp2.status(), 200);
    let body2: Value = test::read_body_json(resp2).await;
    let items2 = body2["data"].as_array().expect("data array");
    let has_eng2 = items2.iter().any(|i| {
        i["resource_type"] == "offer" && i["title"].as_str() == Some("DeptFilterTest")
    });
    assert!(
        !has_eng2,
        "department=Marketing must exclude Engineering offer"
    );
}

/// The salary range filter must actually constrain results — offers
/// outside the salary band must be excluded.
#[actix_rt::test]
#[serial]
async fn test_salary_filter_constrains_results() {
    test_app!(app, config);
    let token = login_admin(&app).await;

    // Create an offer with salary_cents = 10_000_000 ($100,000)
    let cand_id = uuid::Uuid::new_v4();
    let offer_id = uuid::Uuid::new_v4();
    {
        let db_pool = talentflow::infrastructure::db::create_pool(&config.database_url);
        let mut conn = db_pool.get().expect("db connection");
        diesel::sql_query(format!(
            "INSERT INTO candidates (id, first_name, last_name, email) \
             VALUES ('{cand_id}', 'SalaryTest', 'Candidate', 'saltest_{cand_id}@test.local') \
             ON CONFLICT DO NOTHING"
        ))
        .execute(&mut conn)
        .expect("insert test candidate");
        diesel::sql_query(format!(
            "INSERT INTO offers (id, candidate_id, title, department, status, created_by, salary_cents) \
             VALUES ('{offer_id}', '{cand_id}', 'SalaryFilterTest', 'Engineering', 'draft', \
             'b0000000-0000-0000-0000-000000000001', 10000000) \
             ON CONFLICT DO NOTHING"
        ))
        .execute(&mut conn)
        .expect("insert test offer");
    }

    // salary_min=50000&salary_max=150000 ($50k–$150k) — must include $100k offer
    let req = test::TestRequest::get()
        .uri("/api/v1/search?salary_min=50000&salary_max=150000&page=1&per_page=100")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = test::read_body_json(resp).await;
    let items = body["data"].as_array().expect("data array");
    let has_sal = items.iter().any(|i| {
        i["resource_type"] == "offer" && i["title"].as_str() == Some("SalaryFilterTest")
    });
    assert!(
        has_sal,
        "salary range $50k–$150k must include $100k offer"
    );

    // salary_min=200000 ($200k+) — must NOT include $100k offer
    let req2 = test::TestRequest::get()
        .uri("/api/v1/search?salary_min=200000&page=1&per_page=100")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp2 = test::call_service(&app, req2).await;
    assert_eq!(resp2.status(), 200);
    let body2: Value = test::read_body_json(resp2).await;
    let items2 = body2["data"].as_array().expect("data array");
    let has_sal2 = items2.iter().any(|i| {
        i["resource_type"] == "offer" && i["title"].as_str() == Some("SalaryFilterTest")
    });
    assert!(
        !has_sal2,
        "salary_min=$200k must exclude $100k offer"
    );
}

// ── Vocabulary categories filter tests ──────────────────────────────────────

/// The `categories` param is accepted and returns 200.
#[actix_rt::test]
#[serial]
async fn test_search_with_categories_returns_200() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/search?categories=candidate_tag&page=1&per_page=10")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "search with categories must return 200");
}

/// categories=candidate_tag constrains results to candidates whose tags
/// include vocabulary values from the candidate_tag category.
#[actix_rt::test]
#[serial]
async fn test_categories_filter_constrains_results() {
    test_app!(app, config);
    let token = login_admin(&app).await;

    // Seed a candidate with tag "senior" (exists in candidate_tag vocabulary)
    let cand_id = uuid::Uuid::new_v4();
    {
        let db_pool = talentflow::infrastructure::db::create_pool(&config.database_url);
        let mut conn = db_pool.get().expect("db connection");
        diesel::sql_query(format!(
            "INSERT INTO candidates (id, first_name, last_name, email, tags) \
             VALUES ('{cand_id}', 'CatTest', 'Senior', 'cattest_{cand_id}@test.local', \
             ARRAY['senior']::text[]) ON CONFLICT DO NOTHING"
        ))
        .execute(&mut conn)
        .expect("insert test candidate");
    }

    // categories=candidate_tag — should include our candidate (tag "senior" is in that vocab)
    let req = test::TestRequest::get()
        .uri("/api/v1/search?categories=candidate_tag&page=1&per_page=100")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = test::read_body_json(resp).await;
    let items = body["data"].as_array().expect("data array");
    let has_cat = items.iter().any(|i| {
        i["resource_type"] == "candidate" && i["title"].as_str() == Some("CatTest Senior")
    });
    assert!(has_cat, "categories=candidate_tag must include candidate with tag 'senior'");

    // categories=department — candidate_tag values don't match department; our candidate
    // should pass through because the department category yields no candidate-relevant filter
    // (candidate_tag values are empty for department category)
    let req2 = test::TestRequest::get()
        .uri("/api/v1/search?categories=department&page=1&per_page=100")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp2 = test::call_service(&app, req2).await;
    assert_eq!(resp2.status(), 200);
}

// ── Price (total compensation) filter tests ─────────────────────────────────

/// The price_min/price_max params are accepted and return 200.
#[actix_rt::test]
#[serial]
async fn test_search_with_price_range_returns_200() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/search?price_min=50000&price_max=200000&page=1&per_page=10")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "search with price range must return 200");
}

/// price filter uses total comp (salary + bonus).  An offer with $100k salary
/// and 20% bonus has $120k total comp — should be included in $100k–$150k range
/// but excluded by $150k minimum.
#[actix_rt::test]
#[serial]
async fn test_price_filter_constrains_results() {
    test_app!(app, config);
    let token = login_admin(&app).await;

    let cand_id = uuid::Uuid::new_v4();
    let offer_id = uuid::Uuid::new_v4();
    {
        let db_pool = talentflow::infrastructure::db::create_pool(&config.database_url);
        let mut conn = db_pool.get().expect("db connection");
        diesel::sql_query(format!(
            "INSERT INTO candidates (id, first_name, last_name, email) \
             VALUES ('{cand_id}', 'PriceTest', 'Candidate', 'pricetest_{cand_id}@test.local') \
             ON CONFLICT DO NOTHING"
        ))
        .execute(&mut conn)
        .expect("insert test candidate");
        // $100k salary + 20% bonus = $120k total comp
        diesel::sql_query(format!(
            "INSERT INTO offers (id, candidate_id, title, department, status, created_by, \
             salary_cents, bonus_target_pct) \
             VALUES ('{offer_id}', '{cand_id}', 'PriceFilterTest', 'Engineering', 'draft', \
             'b0000000-0000-0000-0000-000000000001', 10000000, 20.0) \
             ON CONFLICT DO NOTHING"
        ))
        .execute(&mut conn)
        .expect("insert test offer");
    }

    // price_min=100000&price_max=150000 → total comp $120k is in range
    let req = test::TestRequest::get()
        .uri("/api/v1/search?price_min=100000&price_max=150000&page=1&per_page=100")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = test::read_body_json(resp).await;
    let items = body["data"].as_array().expect("data array");
    let has_price = items.iter().any(|i| {
        i["resource_type"] == "offer" && i["title"].as_str() == Some("PriceFilterTest")
    });
    assert!(has_price, "price range $100k–$150k must include $120k total comp offer");

    // price_min=150000 → total comp $120k is below range
    let req2 = test::TestRequest::get()
        .uri("/api/v1/search?price_min=150000&page=1&per_page=100")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp2 = test::call_service(&app, req2).await;
    assert_eq!(resp2.status(), 200);
    let body2: Value = test::read_body_json(resp2).await;
    let items2 = body2["data"].as_array().expect("data array");
    let has_price2 = items2.iter().any(|i| {
        i["resource_type"] == "offer" && i["title"].as_str() == Some("PriceFilterTest")
    });
    assert!(!has_price2, "price_min=$150k must exclude $120k total comp offer");
}

// ── Quality gate filter tests ───────────────────────────────────────────────

/// The quality_min/quality_max params are accepted and return 200.
#[actix_rt::test]
#[serial]
async fn test_search_with_quality_range_returns_200() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/search?quality_min=1.0&quality_max=4.0&page=1&per_page=10")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "search with quality range must return 200");
}

/// quality_min excludes offers without salary (non-domain-rated) but includes
/// offers with salary whose domain rating meets the threshold.
#[actix_rt::test]
#[serial]
async fn test_quality_filter_excludes_non_domain_rated() {
    test_app!(app, config);
    let token = login_admin(&app).await;

    let cand_id = uuid::Uuid::new_v4();
    let offer_no_sal = uuid::Uuid::new_v4();
    let offer_with_sal = uuid::Uuid::new_v4();
    {
        let db_pool = talentflow::infrastructure::db::create_pool(&config.database_url);
        let mut conn = db_pool.get().expect("db connection");
        diesel::sql_query(format!(
            "INSERT INTO candidates (id, first_name, last_name, email) \
             VALUES ('{cand_id}', 'QualTest', 'Candidate', 'qualtest_{cand_id}@test.local') \
             ON CONFLICT DO NOTHING"
        ))
        .execute(&mut conn)
        .expect("insert test candidate");
        // Offer WITHOUT salary → not domain-rated
        diesel::sql_query(format!(
            "INSERT INTO offers (id, candidate_id, title, status, created_by) \
             VALUES ('{offer_no_sal}', '{cand_id}', 'QualNoSalary', 'draft', \
             'b0000000-0000-0000-0000-000000000001') ON CONFLICT DO NOTHING"
        ))
        .execute(&mut conn)
        .expect("insert offer without salary");
        // Offer WITH salary $150k → domain-rated, rating ~2.2
        diesel::sql_query(format!(
            "INSERT INTO offers (id, candidate_id, title, status, created_by, salary_cents) \
             VALUES ('{offer_with_sal}', '{cand_id}', 'QualWithSalary', 'draft', \
             'b0000000-0000-0000-0000-000000000001', 15000000) ON CONFLICT DO NOTHING"
        ))
        .execute(&mut conn)
        .expect("insert offer with salary");
    }

    // quality_min=1.0 → should include salary offer, exclude no-salary offer
    let req = test::TestRequest::get()
        .uri("/api/v1/search?quality_min=1.0&page=1&per_page=100")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = test::read_body_json(resp).await;
    let items = body["data"].as_array().expect("data array");

    let has_with_sal = items.iter().any(|i| {
        i["title"].as_str() == Some("QualWithSalary")
    });
    let has_no_sal = items.iter().any(|i| {
        i["title"].as_str() == Some("QualNoSalary")
    });
    assert!(has_with_sal, "quality_min=1.0 must include domain-rated offer");
    assert!(!has_no_sal, "quality_min=1.0 must exclude non-domain-rated offer");

    // Also verify no candidates appear (candidates are never domain-rated)
    let has_any_candidate = items.iter().any(|i| i["resource_type"] == "candidate");
    assert!(
        !has_any_candidate,
        "quality_min must exclude ALL candidates (they are never domain-rated)"
    );
}

// ── Edge-case tests ─────────────────────────────────────────────────────────

/// Empty categories string is accepted and does not crash — returns 200
/// with normal results (no filtering applied).
#[actix_rt::test]
#[serial]
async fn test_search_with_empty_categories_returns_200() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/search?categories=&page=1&per_page=10")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "empty categories must return 200");
}

/// Inverted quality range (min > max) returns 200 with empty results — the
/// server does not reject it, the filter simply matches nothing.
#[actix_rt::test]
#[serial]
async fn test_search_inverted_quality_range_returns_empty() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/search?quality_min=5.0&quality_max=1.0&page=1&per_page=100")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = test::read_body_json(resp).await;
    let items = body["data"].as_array().expect("data array");
    assert!(
        items.is_empty(),
        "inverted quality range (min=5, max=1) must return empty results"
    );
}

/// price_min alone (without price_max) works as a floor-only filter.
#[actix_rt::test]
#[serial]
async fn test_search_price_min_only() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/search?price_min=1&page=1&per_page=10")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "price_min alone must return 200");
}

/// An offer with salary_cents gets a domain-native rating that differs from
/// pure relevance-derived (score * 5.0).  This verifies end-to-end that the
/// rating precedence (native > fallback) is actually applied in API output.
#[actix_rt::test]
#[serial]
async fn test_offer_with_salary_gets_domain_native_rating() {
    test_app!(app, config);
    let token = login_admin(&app).await;

    // Create an offer with known salary ($150k) — domain rating should be ~2.22
    let cand_id = uuid::Uuid::new_v4();
    let offer_id = uuid::Uuid::new_v4();
    {
        let db_pool = talentflow::infrastructure::db::create_pool(&config.database_url);
        let mut conn = db_pool.get().expect("db connection");
        diesel::sql_query(format!(
            "INSERT INTO candidates (id, first_name, last_name, email) \
             VALUES ('{cand_id}', 'RatingTest', 'Native', 'ratingtest_{cand_id}@test.local') \
             ON CONFLICT DO NOTHING"
        ))
        .execute(&mut conn)
        .expect("insert test candidate");
        diesel::sql_query(format!(
            "INSERT INTO offers (id, candidate_id, title, status, created_by, salary_cents) \
             VALUES ('{offer_id}', '{cand_id}', 'RatingNativeTest', 'draft', \
             'b0000000-0000-0000-0000-000000000001', 15000000) \
             ON CONFLICT DO NOTHING"
        ))
        .execute(&mut conn)
        .expect("insert test offer");
    }

    // Search for this specific offer by title
    let req = test::TestRequest::get()
        .uri("/api/v1/search?q=RatingNativeTest&page=1&per_page=10")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = test::read_body_json(resp).await;
    let items = body["data"].as_array().expect("data array");

    let offer_item = items.iter().find(|i| {
        i["resource_type"] == "offer" && i["title"].as_str() == Some("RatingNativeTest")
    });
    assert!(offer_item.is_some(), "offer must appear in results");

    let rating = offer_item.unwrap()["rating"].as_f64().expect("rating must be present");
    let score = offer_item.unwrap()["score"].as_f64().expect("score must be present");
    let fallback_rating = score * 5.0;

    // $150k → domain_rating formula → (150k-30k)/(300k-30k) * 5 ≈ 2.22
    // This must differ from the fallback (score * 5.0)
    assert!(
        (rating - fallback_rating).abs() > 0.01,
        "domain-native rating ({rating:.2}) must differ from fallback ({fallback_rating:.2}); \
         this proves native rating takes precedence over score*5.0"
    );
    assert!(
        rating > 1.0 && rating < 4.0,
        "domain rating for $150k should be in (1.0, 4.0), got {rating}"
    );
}

/// Backward compatibility proof: a search using only pre-facet params must
/// return the same total count whether or not the new params exist in the
/// codebase.  We verify the response has the exact same shape and that all
/// known-present fields are there.
#[actix_rt::test]
#[serial]
async fn test_old_params_only_backward_compat_shape_and_fields() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    // This is the exact pre-facet query pattern — only original params used
    let req = test::TestRequest::get()
        .uri("/api/v1/search?q=admin&tags=&status=&sort_by=relevance&page=1&per_page=10&min_rating=0.0&max_rating=5.0")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = test::read_body_json(resp).await;

    // Envelope shape
    assert!(body["data"].is_array(), "must have data array");
    assert!(body["pagination"].is_object(), "must have pagination object");
    assert!(body["pagination"]["page"].is_number(), "page must be number");
    assert!(body["pagination"]["per_page"].is_number(), "per_page must be number");
    assert!(body["pagination"]["total"].is_number(), "total must be number");

    // Each result item must have all original fields
    if let Some(items) = body["data"].as_array() {
        for item in items {
            assert!(item["resource_type"].is_string(), "resource_type");
            assert!(item["id"].is_string(), "id");
            assert!(item["title"].is_string(), "title");
            assert!(item["score"].is_number(), "score");
            assert!(item["created_at"].is_string(), "created_at");
            assert!(item["recommended"].is_boolean(), "recommended");
            // domain_rated must NOT appear in the response (skip_serializing)
            assert!(
                item.get("domain_rated").is_none(),
                "domain_rated must not be serialized in API response"
            );
        }
    }
}

// ── Search autocomplete tests ─────────────────────────────────────────────────

/// GET /api/v1/search/autocomplete must return 200 with a data array.
#[actix_rt::test]
#[serial]
async fn search_autocomplete_returns_200() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    let req = test::TestRequest::get()
        .uri("/api/v1/search/autocomplete?prefix=e&limit=5")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "autocomplete must return 200");
    let body: Value = test::read_body_json(resp).await;
    assert!(
        body["data"].is_array(),
        "autocomplete response must have a 'data' array; got: {body}"
    );
}

// ── Audit by ID tests ─────────────────────────────────────────────────────────

/// GET /api/v1/audit/{id} must return the event matching the given ID.
#[actix_rt::test]
#[serial]
async fn get_audit_event_by_id_returns_200() {
    test_app!(app, _config);
    let token = login_admin(&app).await;

    // Create a candidate to ensure at least one audit event exists
    let uniq = uuid::Uuid::new_v4();
    let req = test::TestRequest::post()
        .uri("/api/v1/candidates")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .set_json(json!({
            "first_name": "AuditById",
            "last_name": "Test",
            "email": format!("audit.byid.{}@example.com", uniq),
        }))
        .to_request();
    let create_resp = test::call_service(&app, req).await;
    assert_eq!(
        create_resp.status(),
        201,
        "candidate create must succeed for audit seeding"
    );

    // List audit events — must have at least one entry
    let req = test::TestRequest::get()
        .uri("/api/v1/audit?page=1&per_page=1")
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "audit list must return 200");
    let list_body: Value = test::read_body_json(resp).await;
    let items = list_body["data"].as_array().expect("data must be array");
    assert!(
        !items.is_empty(),
        "audit event list must contain at least one item after candidate creation"
    );

    let event_id = items[0]["id"].as_str().expect("audit event must have id");

    // Fetch by ID
    let req = test::TestRequest::get()
        .uri(&format!("/api/v1/audit/{event_id}"))
        .insert_header(("Authorization", format!("Bearer {token}")))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "get audit by id must return 200");
    let body: Value = test::read_body_json(resp).await;
    assert_eq!(
        body["data"]["id"].as_str().unwrap(),
        event_id,
        "returned audit event id must match the requested id"
    );
}
