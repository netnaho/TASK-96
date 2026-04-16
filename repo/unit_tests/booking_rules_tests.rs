/// Unit tests for BookingOrder time-based rules (24h cutoff, hold expiry).
use chrono::{Duration, NaiveDate, NaiveTime, Utc};
use talentflow::domain::bookings::models::{
    BookingOrder, BookingStatus, BreachReasonCode, EligibilityCheck, EligibilityResult,
    InventorySlot, CANCEL_CUTOFF_HOURS, HOLD_DURATION_MINUTES,
};
use uuid::Uuid;

fn make_order(scheduled_date: NaiveDate, start_time: NaiveTime) -> BookingOrder {
    let now = Utc::now();
    BookingOrder {
        id: Uuid::new_v4(),
        candidate_id: Uuid::new_v4(),
        site_id: Uuid::new_v4(),
        status: BookingStatus::Confirmed,
        scheduled_date,
        scheduled_time_start: Some(start_time),
        scheduled_time_end: Some(NaiveTime::from_hms_opt(17, 0, 0).unwrap()),
        notes: None,
        slot_id: Some(Uuid::new_v4()),
        hold_expires_at: None,
        agreement_evidence: None,
        breach_reason: None,
        breach_reason_code: None,
        exception_detail: None,
        idempotency_key: None,
        created_by: Uuid::new_v4(),
        created_at: now,
        updated_at: now,
    }
}

#[test]
fn within_cancel_cutoff_24h_before() {
    // Booking starts 23 hours from now — within cutoff
    let start = Utc::now() + Duration::hours(23);
    let date = start.date_naive();
    let time = start.time();
    let order = make_order(date, time);

    assert!(order.is_within_cancel_cutoff(Utc::now()));
}

#[test]
fn outside_cancel_cutoff_48h_before() {
    // Booking starts 48 hours from now — outside cutoff
    let start = Utc::now() + Duration::hours(48);
    let date = start.date_naive();
    let time = start.time();
    let order = make_order(date, time);

    assert!(!order.is_within_cancel_cutoff(Utc::now()));
}

#[test]
fn exactly_at_cutoff_boundary_is_within() {
    // Booking starts exactly 24 hours from now — the cutoff is >=, so this is within
    let start = Utc::now() + Duration::hours(CANCEL_CUTOFF_HOURS);
    let date = start.date_naive();
    let time = start.time();
    let order = make_order(date, time);

    assert!(order.is_within_cancel_cutoff(Utc::now()));
}

#[test]
fn hold_expired_when_past() {
    let mut order = make_order(
        NaiveDate::from_ymd_opt(2030, 1, 1).unwrap(),
        NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
    );
    order.hold_expires_at = Some(Utc::now() - Duration::minutes(1));

    assert!(order.is_hold_expired(Utc::now()));
}

#[test]
fn hold_not_expired_when_future() {
    let mut order = make_order(
        NaiveDate::from_ymd_opt(2030, 1, 1).unwrap(),
        NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
    );
    order.hold_expires_at = Some(Utc::now() + Duration::minutes(HOLD_DURATION_MINUTES));

    assert!(!order.is_hold_expired(Utc::now()));
}

#[test]
fn hold_not_expired_when_none() {
    let order = make_order(
        NaiveDate::from_ymd_opt(2030, 1, 1).unwrap(),
        NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
    );

    assert!(!order.is_hold_expired(Utc::now()));
}

#[test]
fn inventory_slot_available_capacity() {
    let slot = InventorySlot {
        id: Uuid::new_v4(),
        site_id: Uuid::new_v4(),
        slot_date: NaiveDate::from_ymd_opt(2030, 1, 1).unwrap(),
        start_time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        end_time: NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
        capacity: 5,
        booked_count: 3,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    assert_eq!(slot.available_capacity(), 2);
    assert!(slot.is_available());
}

#[test]
fn inventory_slot_full() {
    let slot = InventorySlot {
        id: Uuid::new_v4(),
        site_id: Uuid::new_v4(),
        slot_date: NaiveDate::from_ymd_opt(2030, 1, 1).unwrap(),
        start_time: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        end_time: NaiveTime::from_hms_opt(10, 0, 0).unwrap(),
        capacity: 5,
        booked_count: 5,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    assert_eq!(slot.available_capacity(), 0);
    assert!(!slot.is_available());
}

#[test]
fn eligibility_result_all_pass() {
    let checks = vec![
        EligibilityCheck {
            name: "a",
            passed: true,
            detail: "ok".into(),
        },
        EligibilityCheck {
            name: "b",
            passed: true,
            detail: "ok".into(),
        },
    ];
    let result = EligibilityResult::from_checks(checks);
    assert!(result.eligible);
    assert!(result.failed_checks().is_empty());
}

#[test]
fn eligibility_result_one_fail() {
    let checks = vec![
        EligibilityCheck {
            name: "a",
            passed: true,
            detail: "ok".into(),
        },
        EligibilityCheck {
            name: "b",
            passed: false,
            detail: "nope".into(),
        },
    ];
    let result = EligibilityResult::from_checks(checks);
    assert!(!result.eligible);
    assert_eq!(result.failed_checks().len(), 1);
    assert_eq!(result.failed_checks()[0].name, "b");
}

#[test]
fn breach_reason_code_round_trip() {
    let codes = [
        (BreachReasonCode::LateCancellation, "late_cancellation"),
        (BreachReasonCode::NoShow, "no_show"),
        (BreachReasonCode::PolicyViolation, "policy_violation"),
        (BreachReasonCode::Other, "other"),
    ];
    for (code, expected) in codes {
        assert_eq!(code.as_str(), expected);
        assert_eq!(BreachReasonCode::from_str(expected), Some(code));
    }
}
