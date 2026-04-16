/// Unit tests for BookingStatus state machine transitions.
use talentflow::domain::bookings::models::BookingStatus;

#[test]
fn pending_can_transition_to_confirmed() {
    assert!(BookingStatus::PendingConfirmation.can_transition_to(BookingStatus::Confirmed));
}

#[test]
fn pending_can_be_canceled() {
    assert!(BookingStatus::PendingConfirmation.can_transition_to(BookingStatus::Canceled));
}

#[test]
fn pending_cannot_skip_to_in_progress() {
    assert!(!BookingStatus::PendingConfirmation.can_transition_to(BookingStatus::InProgress));
}

#[test]
fn confirmed_can_start() {
    assert!(BookingStatus::Confirmed.can_transition_to(BookingStatus::InProgress));
}

#[test]
fn confirmed_can_be_canceled() {
    assert!(BookingStatus::Confirmed.can_transition_to(BookingStatus::Canceled));
}

#[test]
fn confirmed_can_go_to_exception() {
    assert!(BookingStatus::Confirmed.can_transition_to(BookingStatus::Exception));
}

#[test]
fn in_progress_can_complete() {
    assert!(BookingStatus::InProgress.can_transition_to(BookingStatus::Completed));
}

#[test]
fn in_progress_can_go_to_exception() {
    assert!(BookingStatus::InProgress.can_transition_to(BookingStatus::Exception));
}

#[test]
fn in_progress_cannot_go_back_to_confirmed() {
    assert!(!BookingStatus::InProgress.can_transition_to(BookingStatus::Confirmed));
}

#[test]
fn exception_can_be_completed() {
    assert!(BookingStatus::Exception.can_transition_to(BookingStatus::Completed));
}

#[test]
fn exception_can_be_canceled() {
    assert!(BookingStatus::Exception.can_transition_to(BookingStatus::Canceled));
}

#[test]
fn completed_is_terminal() {
    for target in [
        BookingStatus::PendingConfirmation,
        BookingStatus::Confirmed,
        BookingStatus::InProgress,
        BookingStatus::Canceled,
        BookingStatus::Exception,
    ] {
        assert!(
            !BookingStatus::Completed.can_transition_to(target),
            "completed should not transition to {target:?}"
        );
    }
}

#[test]
fn canceled_is_terminal() {
    for target in [
        BookingStatus::PendingConfirmation,
        BookingStatus::Confirmed,
        BookingStatus::InProgress,
        BookingStatus::Completed,
        BookingStatus::Exception,
    ] {
        assert!(
            !BookingStatus::Canceled.can_transition_to(target),
            "canceled should not transition to {target:?}"
        );
    }
}

#[test]
fn as_str_round_trip() {
    let statuses = [
        (BookingStatus::PendingConfirmation, "pending_confirmation"),
        (BookingStatus::Confirmed, "confirmed"),
        (BookingStatus::InProgress, "in_progress"),
        (BookingStatus::Completed, "completed"),
        (BookingStatus::Canceled, "cancelled"),
        (BookingStatus::Exception, "exception"),
    ];
    for (status, expected) in statuses {
        assert_eq!(status.as_str(), expected);
        assert_eq!(BookingStatus::from_str(expected), Some(status));
    }
}

#[test]
fn draft_parses_as_pending_confirmation() {
    // Backwards compatibility: old "draft" rows map to PendingConfirmation
    assert_eq!(
        BookingStatus::from_str("draft"),
        Some(BookingStatus::PendingConfirmation)
    );
}

#[test]
fn is_terminal_flags() {
    assert!(BookingStatus::Completed.is_terminal());
    assert!(BookingStatus::Canceled.is_terminal());
    assert!(!BookingStatus::PendingConfirmation.is_terminal());
    assert!(!BookingStatus::Confirmed.is_terminal());
    assert!(!BookingStatus::InProgress.is_terminal());
    assert!(!BookingStatus::Exception.is_terminal());
}
