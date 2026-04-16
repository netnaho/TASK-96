/// Unit tests for the OfferStatus state machine transitions.
use talentflow::domain::offers::models::OfferStatus;

#[test]
fn draft_can_transition_to_pending_approval() {
    assert!(OfferStatus::Draft.can_transition_to(OfferStatus::PendingApproval));
}

#[test]
fn draft_can_be_withdrawn() {
    assert!(OfferStatus::Draft.can_transition_to(OfferStatus::Withdrawn));
}

#[test]
fn draft_cannot_skip_to_approved() {
    assert!(!OfferStatus::Draft.can_transition_to(OfferStatus::Approved));
}

#[test]
fn pending_approval_can_be_approved() {
    assert!(OfferStatus::PendingApproval.can_transition_to(OfferStatus::Approved));
}

#[test]
fn pending_approval_can_be_withdrawn() {
    assert!(OfferStatus::PendingApproval.can_transition_to(OfferStatus::Withdrawn));
}

#[test]
fn approved_can_be_sent() {
    assert!(OfferStatus::Approved.can_transition_to(OfferStatus::Sent));
}

#[test]
fn sent_can_be_accepted() {
    assert!(OfferStatus::Sent.can_transition_to(OfferStatus::Accepted));
}

#[test]
fn sent_can_be_declined() {
    assert!(OfferStatus::Sent.can_transition_to(OfferStatus::Declined));
}

#[test]
fn sent_can_be_withdrawn() {
    assert!(OfferStatus::Sent.can_transition_to(OfferStatus::Withdrawn));
}

#[test]
fn sent_can_expire() {
    assert!(OfferStatus::Sent.can_transition_to(OfferStatus::Expired));
}

#[test]
fn accepted_is_terminal() {
    for target in [
        OfferStatus::Draft,
        OfferStatus::PendingApproval,
        OfferStatus::Approved,
        OfferStatus::Sent,
        OfferStatus::Declined,
        OfferStatus::Withdrawn,
        OfferStatus::Expired,
    ] {
        assert!(
            !OfferStatus::Accepted.can_transition_to(target),
            "accepted should not transition to {target:?}"
        );
    }
}

#[test]
fn withdrawn_is_terminal() {
    for target in [
        OfferStatus::Draft,
        OfferStatus::PendingApproval,
        OfferStatus::Approved,
        OfferStatus::Sent,
        OfferStatus::Accepted,
        OfferStatus::Declined,
        OfferStatus::Expired,
    ] {
        assert!(
            !OfferStatus::Withdrawn.can_transition_to(target),
            "withdrawn should not transition to {target:?}"
        );
    }
}

#[test]
fn as_str_round_trip() {
    let statuses = [
        (OfferStatus::Draft, "draft"),
        (OfferStatus::PendingApproval, "pending_approval"),
        (OfferStatus::Approved, "approved"),
        (OfferStatus::Sent, "sent"),
        (OfferStatus::Accepted, "accepted"),
        (OfferStatus::Declined, "declined"),
        (OfferStatus::Withdrawn, "withdrawn"),
        (OfferStatus::Expired, "expired"),
    ];
    for (status, expected) in statuses {
        assert_eq!(status.as_str(), expected);
    }
}
