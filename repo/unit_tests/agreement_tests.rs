/// Unit tests for AgreementEvidence hash computation and verification.
use chrono::Utc;
use talentflow::domain::bookings::models::AgreementEvidence;
use uuid::Uuid;

#[test]
fn agreement_hash_is_deterministic() {
    let now = Utc::now();
    let candidate_id = Uuid::new_v4();
    let booking_id = Uuid::new_v4();

    let a = AgreementEvidence::new("Jane Doe".into(), now, candidate_id, booking_id);
    let b = AgreementEvidence::new("Jane Doe".into(), now, candidate_id, booking_id);

    assert_eq!(a.hash, b.hash);
}

#[test]
fn agreement_hash_changes_with_different_name() {
    let now = Utc::now();
    let candidate_id = Uuid::new_v4();
    let booking_id = Uuid::new_v4();

    let a = AgreementEvidence::new("Jane Doe".into(), now, candidate_id, booking_id);
    let b = AgreementEvidence::new("John Doe".into(), now, candidate_id, booking_id);

    assert_ne!(a.hash, b.hash);
}

#[test]
fn agreement_hash_changes_with_different_booking() {
    let now = Utc::now();
    let candidate_id = Uuid::new_v4();

    let a = AgreementEvidence::new("Jane Doe".into(), now, candidate_id, Uuid::new_v4());
    let b = AgreementEvidence::new("Jane Doe".into(), now, candidate_id, Uuid::new_v4());

    assert_ne!(a.hash, b.hash);
}

#[test]
fn agreement_verify_succeeds_with_matching_ids() {
    let candidate_id = Uuid::new_v4();
    let booking_id = Uuid::new_v4();
    let evidence = AgreementEvidence::new("Jane Doe".into(), Utc::now(), candidate_id, booking_id);

    assert!(evidence.verify(candidate_id, booking_id));
}

#[test]
fn agreement_verify_fails_with_wrong_candidate() {
    let candidate_id = Uuid::new_v4();
    let booking_id = Uuid::new_v4();
    let evidence = AgreementEvidence::new("Jane Doe".into(), Utc::now(), candidate_id, booking_id);

    assert!(!evidence.verify(Uuid::new_v4(), booking_id));
}

#[test]
fn agreement_verify_fails_with_wrong_booking() {
    let candidate_id = Uuid::new_v4();
    let booking_id = Uuid::new_v4();
    let evidence = AgreementEvidence::new("Jane Doe".into(), Utc::now(), candidate_id, booking_id);

    assert!(!evidence.verify(candidate_id, Uuid::new_v4()));
}

#[test]
fn agreement_is_present_checks_non_empty() {
    let evidence = AgreementEvidence::new(
        "Jane Doe".into(),
        Utc::now(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    );
    assert!(evidence.is_present());
}

#[test]
fn agreement_hash_is_64_hex_chars() {
    let evidence = AgreementEvidence::new(
        "Jane Doe".into(),
        Utc::now(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    );
    assert_eq!(evidence.hash.len(), 64); // SHA-256 hex = 64 chars
    assert!(evidence.hash.chars().all(|c| c.is_ascii_hexdigit()));
}
