/// Unit tests for the reporting delivery gateway adapters.
///
/// These tests run without a database or network — all adapters are exercised
/// in their no-op (Skipped) configuration.
use talentflow::infrastructure::{
    config::ReportingDeliveryConfig,
    reporting_delivery::{
        build_gateway, AlertPayload, DeliveryGateway, DeliveryOutcome, LocalEmailGatewayAdapter,
        LocalImGatewayAdapter,
    },
};
use uuid::Uuid;

fn dummy_payload() -> AlertPayload {
    AlertPayload {
        alert_id: Uuid::new_v4(),
        subscription_id: Uuid::new_v4(),
        severity: "warning".into(),
        message: "test alert message".into(),
    }
}

// ── LocalEmailGatewayAdapter ──────────────────────────────────────────────────

#[test]
fn test_email_adapter_skipped_when_url_is_none() {
    let adapter = LocalEmailGatewayAdapter::new(None);
    let outcome = adapter.deliver(&dummy_payload());
    assert_eq!(outcome, DeliveryOutcome::Skipped);
}

#[test]
fn test_email_adapter_name() {
    let adapter = LocalEmailGatewayAdapter::new(None);
    assert_eq!(adapter.name(), "local-email");
}

#[test]
fn test_email_adapter_error_on_unreachable_url() {
    // Port 1 is reserved and should be unreachable, producing an Error outcome.
    let adapter = LocalEmailGatewayAdapter::new(Some("http://127.0.0.1:1/send".into()));
    let outcome = adapter.deliver(&dummy_payload());
    assert!(
        matches!(outcome, DeliveryOutcome::Error(_)),
        "unreachable URL must produce Error, got: {outcome:?}"
    );
}

// ── LocalImGatewayAdapter ─────────────────────────────────────────────────────

#[test]
fn test_im_adapter_skipped_when_url_is_none() {
    let adapter = LocalImGatewayAdapter::new(None);
    let outcome = adapter.deliver(&dummy_payload());
    assert_eq!(outcome, DeliveryOutcome::Skipped);
}

#[test]
fn test_im_adapter_name() {
    let adapter = LocalImGatewayAdapter::new(None);
    assert_eq!(adapter.name(), "local-im");
}

#[test]
fn test_im_adapter_error_on_unreachable_url() {
    let adapter = LocalImGatewayAdapter::new(Some("http://127.0.0.1:1/hooks/alerts".into()));
    let outcome = adapter.deliver(&dummy_payload());
    assert!(
        matches!(outcome, DeliveryOutcome::Error(_)),
        "unreachable URL must produce Error, got: {outcome:?}"
    );
}

// ── DeliveryOutcome::to_json ──────────────────────────────────────────────────

#[test]
fn test_delivered_outcome_to_json() {
    let json = DeliveryOutcome::Delivered.to_json();
    assert_eq!(json["status"], "delivered");
    assert!(
        json.get("detail").is_none(),
        "Delivered must not include detail"
    );
}

#[test]
fn test_skipped_outcome_to_json() {
    let json = DeliveryOutcome::Skipped.to_json();
    assert_eq!(json["status"], "skipped");
    assert!(
        json.get("detail").is_none(),
        "Skipped must not include detail"
    );
}

#[test]
fn test_error_outcome_to_json_includes_detail() {
    let json = DeliveryOutcome::Error("connect failed".into()).to_json();
    assert_eq!(json["status"], "error");
    assert_eq!(json["detail"], "connect failed");
}

// ── build_gateway (CompositeDeliveryGateway) ──────────────────────────────────

#[test]
fn test_build_gateway_disabled_produces_all_skipped() {
    let config = ReportingDeliveryConfig {
        enabled: false,
        email_gateway_url: Some("http://localhost:9999/send".into()),
        im_gateway_url: Some("http://localhost:9998/hooks".into()),
    };
    let gateway = build_gateway(&config);
    let outcomes = gateway.deliver_all(&dummy_payload());

    // Both adapters must skip when delivery is disabled
    assert_eq!(
        outcomes["local-email"]["status"], "skipped",
        "email must be skipped when delivery disabled"
    );
    assert_eq!(
        outcomes["local-im"]["status"], "skipped",
        "IM must be skipped when delivery disabled"
    );
}

#[test]
fn test_build_gateway_no_urls_produces_all_skipped() {
    let config = ReportingDeliveryConfig {
        enabled: true,
        email_gateway_url: None,
        im_gateway_url: None,
    };
    let gateway = build_gateway(&config);
    let outcomes = gateway.deliver_all(&dummy_payload());

    assert_eq!(outcomes["local-email"]["status"], "skipped");
    assert_eq!(outcomes["local-im"]["status"], "skipped");
}

#[test]
fn test_build_gateway_deliver_all_returns_both_adapter_keys() {
    let config = ReportingDeliveryConfig {
        enabled: false,
        email_gateway_url: None,
        im_gateway_url: None,
    };
    let gateway = build_gateway(&config);
    let outcomes = gateway.deliver_all(&dummy_payload());

    assert!(
        outcomes.get("local-email").is_some(),
        "outcomes must contain 'local-email' key"
    );
    assert!(
        outcomes.get("local-im").is_some(),
        "outcomes must contain 'local-im' key"
    );
}
