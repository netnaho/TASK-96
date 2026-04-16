/// Unit tests for onboarding ReadinessReport calculation.
use chrono::Utc;
use talentflow::domain::onboarding::models::{
    OnboardingItem, OnboardingItemStatus, ReadinessReport,
};
use uuid::Uuid;

fn make_item(required: bool, status: OnboardingItemStatus) -> OnboardingItem {
    let now = Utc::now();
    OnboardingItem {
        id: Uuid::new_v4(),
        checklist_id: Uuid::new_v4(),
        title: "test item".to_string(),
        description: None,
        item_order: 1,
        status,
        requires_upload: false,
        upload_storage_key: None,
        health_attestation_encrypted: None,
        required,
        item_due_date: None,
        completed_at: None,
        completed_by: None,
        created_at: now,
        updated_at: now,
    }
}

#[test]
fn empty_checklist_is_100_pct() {
    let id = Uuid::new_v4();
    let report = ReadinessReport::compute(id, &[]);
    assert_eq!(report.readiness_pct, 100);
    assert_eq!(report.total_required, 0);
    assert_eq!(report.required_completed, 0);
}

#[test]
fn no_required_items_is_100_pct() {
    let id = Uuid::new_v4();
    let items = vec![
        make_item(false, OnboardingItemStatus::NotStarted),
        make_item(false, OnboardingItemStatus::InProgress),
    ];
    let report = ReadinessReport::compute(id, &items);
    assert_eq!(report.readiness_pct, 100);
    assert_eq!(report.total_required, 0);
}

#[test]
fn all_required_completed_is_100_pct() {
    let id = Uuid::new_v4();
    let items = vec![
        make_item(true, OnboardingItemStatus::Completed),
        make_item(true, OnboardingItemStatus::Completed),
    ];
    let report = ReadinessReport::compute(id, &items);
    assert_eq!(report.readiness_pct, 100);
    assert_eq!(report.total_required, 2);
    assert_eq!(report.required_completed, 2);
}

#[test]
fn half_required_completed_is_50_pct() {
    let id = Uuid::new_v4();
    let items = vec![
        make_item(true, OnboardingItemStatus::Completed),
        make_item(true, OnboardingItemStatus::NotStarted),
    ];
    let report = ReadinessReport::compute(id, &items);
    assert_eq!(report.readiness_pct, 50);
    assert_eq!(report.required_completed, 1);
}

#[test]
fn optional_items_do_not_count_toward_required() {
    let id = Uuid::new_v4();
    let items = vec![
        make_item(true, OnboardingItemStatus::NotStarted), // required, incomplete
        make_item(false, OnboardingItemStatus::Completed), // optional, complete — must not inflate
    ];
    let report = ReadinessReport::compute(id, &items);
    assert_eq!(report.total_required, 1);
    assert_eq!(report.required_completed, 0);
    assert_eq!(report.readiness_pct, 0);
}

#[test]
fn one_of_three_complete_is_33_pct() {
    let id = Uuid::new_v4();
    let items = vec![
        make_item(true, OnboardingItemStatus::Completed),
        make_item(true, OnboardingItemStatus::InProgress),
        make_item(true, OnboardingItemStatus::NotStarted),
    ];
    let report = ReadinessReport::compute(id, &items);
    assert_eq!(report.readiness_pct, 33);
}

#[test]
fn checklist_id_is_preserved_in_report() {
    let id = Uuid::new_v4();
    let report = ReadinessReport::compute(id, &[]);
    assert_eq!(report.checklist_id, id);
}

#[test]
fn skipped_items_do_not_count_as_completed() {
    let id = Uuid::new_v4();
    let items = vec![
        make_item(true, OnboardingItemStatus::Skipped),
        make_item(true, OnboardingItemStatus::NotStarted),
    ];
    let report = ReadinessReport::compute(id, &items);
    assert_eq!(report.required_completed, 0);
    assert_eq!(report.readiness_pct, 0);
}
