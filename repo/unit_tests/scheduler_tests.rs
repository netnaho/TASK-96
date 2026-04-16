/// Unit tests for DST-safe local-time snapshot scheduling.
///
/// These tests exercise `next_local_run_utc_from` with injected `now` values
/// so they run without any I/O or system-clock dependency.
use chrono::{NaiveTime, TimeZone as _, Utc};
use chrono_tz::Tz;
use talentflow::infrastructure::jobs::time_helpers::{next_local_run_utc_from, parse_hhmm};

fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(y, mo, d, h, mi, s).unwrap()
}

fn t(h: u32, m: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(h, m, 0).unwrap()
}

// ── UTC timezone ─────────────────────────────────────────────────────────────

#[test]
fn utc_before_target_schedules_today() {
    let tz: Tz = "UTC".parse().unwrap();
    let now = utc(2024, 6, 15, 5, 59, 0); // 05:59 UTC
    let next = next_local_run_utc_from(tz, t(6, 0), now);
    assert_eq!(next, utc(2024, 6, 15, 6, 0, 0));
}

#[test]
fn utc_after_target_schedules_tomorrow() {
    let tz: Tz = "UTC".parse().unwrap();
    let now = utc(2024, 6, 15, 6, 0, 1); // 1 second after target
    let next = next_local_run_utc_from(tz, t(6, 0), now);
    assert_eq!(next, utc(2024, 6, 16, 6, 0, 0));
}

#[test]
fn utc_exactly_at_target_schedules_tomorrow() {
    let tz: Tz = "UTC".parse().unwrap();
    let now = utc(2024, 6, 15, 6, 0, 0); // exactly at target
    let next = next_local_run_utc_from(tz, t(6, 0), now);
    // now == target → not strictly after → rolls to tomorrow
    assert_eq!(next, utc(2024, 6, 16, 6, 0, 0));
}

// ── Non-UTC timezone (UTC+3, no DST) ─────────────────────────────────────────

#[test]
fn addis_ababa_before_target_schedules_same_day() {
    // Africa/Addis_Ababa = UTC+3
    let tz: Tz = "Africa/Addis_Ababa".parse().unwrap();
    // 02:30 UTC = 05:30 local; target 06:00 local in 30 min = 03:00 UTC
    let now = utc(2024, 9, 1, 2, 30, 0);
    let next = next_local_run_utc_from(tz, t(6, 0), now);
    assert_eq!(next, utc(2024, 9, 1, 3, 0, 0));
}

#[test]
fn addis_ababa_after_target_schedules_tomorrow() {
    let tz: Tz = "Africa/Addis_Ababa".parse().unwrap();
    // 04:00 UTC = 07:00 local → past 06:00 local; next run = tomorrow 03:00 UTC
    let now = utc(2024, 9, 1, 4, 0, 0);
    let next = next_local_run_utc_from(tz, t(6, 0), now);
    assert_eq!(next, utc(2024, 9, 2, 3, 0, 0));
}

#[test]
fn new_york_before_target_schedules_today() {
    // EST (UTC-5); target 06:00 local = 11:00 UTC
    let tz: Tz = "America/New_York".parse().unwrap();
    let now = utc(2024, 1, 15, 10, 0, 0); // 05:00 EST
    let next = next_local_run_utc_from(tz, t(6, 0), now);
    assert_eq!(next, utc(2024, 1, 15, 11, 0, 0));
}

// ── DST spring-forward (America/New_York 2024-03-10: 02:00 → 03:00) ─────────

#[test]
fn dst_spring_forward_target_in_gap_is_advanced() {
    let tz: Tz = "America/New_York".parse().unwrap();
    // Target 02:30 — that local time doesn't exist on this date.
    // Expected: runs at 03:30 local (= 07:30 UTC; UTC-4 after spring-forward).
    let now = utc(2024, 3, 10, 5, 0, 0); // 00:00 EST — well before the gap
    let next = next_local_run_utc_from(tz, t(2, 30), now);
    assert_eq!(
        next,
        utc(2024, 3, 10, 7, 30, 0),
        "spring-forward gap: should advance 1h"
    );
}

#[test]
fn dst_spring_forward_target_before_gap_runs_normally() {
    let tz: Tz = "America/New_York".parse().unwrap();
    // Target 01:00 — this time exists (before the gap at 02:00).
    let now = utc(2024, 3, 10, 5, 0, 0); // 00:00 EST
    let next = next_local_run_utc_from(tz, t(1, 0), now);
    // 01:00 EST = 06:00 UTC
    assert_eq!(next, utc(2024, 3, 10, 6, 0, 0));
}

// ── DST fall-back (America/New_York 2024-11-03: 02:00 → 01:00) ──────────────

#[test]
fn dst_fall_back_uses_first_occurrence() {
    let tz: Tz = "America/New_York".parse().unwrap();
    // Target 01:30 — occurs twice: once at 05:30 UTC (EDT) and once at 06:30 UTC (EST).
    // We expect the *first* (earlier, 05:30 UTC) to avoid double-fire.
    let now = utc(2024, 11, 3, 0, 0, 0); // 20:00 EDT the day before
    let next = next_local_run_utc_from(tz, t(1, 30), now);
    assert_eq!(
        next,
        utc(2024, 11, 3, 5, 30, 0),
        "fall-back overlap: should use first (earlier) occurrence"
    );
}

// ── parse_hhmm helper ─────────────────────────────────────────────────────────

#[test]
fn parse_hhmm_parses_correctly() {
    assert_eq!(parse_hhmm("06:00"), t(6, 0));
    assert_eq!(parse_hhmm("00:00"), t(0, 0));
    assert_eq!(parse_hhmm("23:59"), t(23, 59));
}

#[test]
#[should_panic(expected = "SNAPSHOT_TIME_LOCAL must be HH:MM")]
fn parse_hhmm_rejects_bad_format() {
    parse_hhmm("6am");
}
