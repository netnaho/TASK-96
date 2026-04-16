/// DST-safe local-time scheduling helpers.
///
/// The daily reporting snapshot must fire at a configurable *local* time (e.g.
/// 06:00 in Africa/Addis_Ababa).  A plain UTC cron expression drifts by an
/// hour every DST transition and is therefore incorrect for non-UTC zones.
///
/// This module provides [`next_local_run_utc_from`] which, given a reference
/// `now` instant, computes the next UTC instant at which the target local time
/// occurs in the configured timezone, with explicit DST handling:
///
/// - **Spring-forward gap** (target local time does not exist): the function
///   advances by one hour to land in the next valid local time, so the job runs
///   once at the post-gap moment rather than being skipped.
/// - **Fall-back overlap** (target local time occurs twice): the function uses
///   the *first* (earlier, pre-rollback) occurrence to prevent a double-fire.
use chrono::{DateTime, Duration, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

/// Compute the next UTC instant at which `target` occurs in `tz`, measured
/// from `now`.
///
/// # Arguments
/// * `tz` — IANA timezone (e.g. `chrono_tz::Africa::Addis_Ababa`)
/// * `target` — desired local time of day (e.g. `NaiveTime::from_hms_opt(6, 0, 0)`)
/// * `now` — reference point (usually `Utc::now()`; injected for testability)
pub fn next_local_run_utc_from(tz: Tz, target: NaiveTime, now: DateTime<Utc>) -> DateTime<Utc> {
    let now_local = now.with_timezone(&tz);

    // Try today first (may already be past)
    let today_naive = now_local.date_naive().and_time(target);
    let run_today = resolve_dst(tz, today_naive, now);

    if run_today > now {
        return run_today;
    }

    // Not yet today (or already past) → schedule for tomorrow
    let tomorrow_naive = (now_local.date_naive() + Duration::days(1)).and_time(target);
    resolve_dst(tz, tomorrow_naive, now)
}

/// Thin wrapper over `next_local_run_utc_from` using the real current time.
pub fn next_local_run_utc(tz: Tz, target: NaiveTime) -> DateTime<Utc> {
    next_local_run_utc_from(tz, target, Utc::now())
}

/// Resolve a naive local datetime to UTC, handling DST transitions.
fn resolve_dst(tz: Tz, naive: chrono::NaiveDateTime, fallback: DateTime<Utc>) -> DateTime<Utc> {
    match tz.from_local_datetime(&naive) {
        // Unambiguous — the usual case
        chrono::LocalResult::Single(dt) => dt.with_timezone(&Utc),

        // Fall-back overlap (DST ending): the same wall-clock instant occurs
        // twice.  Use the *first* (earlier UTC) occurrence to avoid double-fire.
        chrono::LocalResult::Ambiguous(first, _second) => first.with_timezone(&Utc),

        // Spring-forward gap (DST beginning): the target time does not exist
        // locally.  Advance by one hour to land at the first valid local
        // instant after the gap.
        chrono::LocalResult::None => {
            let shifted = naive + Duration::hours(1);
            tz.from_local_datetime(&shifted)
                .earliest()
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|| fallback + Duration::hours(1))
        }
    }
}

/// Parse a `"HH:MM"` string into a [`NaiveTime`].
///
/// # Panics
/// Panics at startup if `SNAPSHOT_TIME_LOCAL` is not in `HH:MM` format.
pub fn parse_hhmm(s: &str) -> NaiveTime {
    NaiveTime::parse_from_str(s, "%H:%M")
        .unwrap_or_else(|_| panic!("SNAPSHOT_TIME_LOCAL must be HH:MM, got: {s:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone as _;
    use chrono_tz::Tz;

    fn utc_dt(y: i32, mo: u32, d: u32, h: u32, min: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, min, s).unwrap()
    }

    #[test]
    fn utc_zone_schedules_same_day_when_before_target() {
        let tz: Tz = "UTC".parse().unwrap();
        let target = NaiveTime::from_hms_opt(6, 0, 0).unwrap();
        let now = utc_dt(2024, 6, 1, 5, 0, 0); // 05:00 UTC
        let next = next_local_run_utc_from(tz, target, now);
        assert_eq!(next, utc_dt(2024, 6, 1, 6, 0, 0), "should fire in 1h");
    }

    #[test]
    fn utc_zone_rolls_to_tomorrow_when_past_target() {
        let tz: Tz = "UTC".parse().unwrap();
        let target = NaiveTime::from_hms_opt(6, 0, 0).unwrap();
        let now = utc_dt(2024, 6, 1, 7, 0, 0); // 07:00 UTC — past 06:00
        let next = next_local_run_utc_from(tz, target, now);
        assert_eq!(next, utc_dt(2024, 6, 2, 6, 0, 0), "should fire tomorrow");
    }

    #[test]
    fn non_utc_zone_offsets_correctly() {
        // Africa/Addis_Ababa = UTC+3 (no DST)
        let tz: Tz = "Africa/Addis_Ababa".parse().unwrap();
        let target = NaiveTime::from_hms_opt(6, 0, 0).unwrap();
        // 02:30 UTC = 05:30 local → target 06:00 local is in 30 min = 03:00 UTC
        let now = utc_dt(2024, 6, 1, 2, 30, 0);
        let next = next_local_run_utc_from(tz, target, now);
        assert_eq!(next, utc_dt(2024, 6, 1, 3, 0, 0), "06:00 Addis = 03:00 UTC");
    }

    #[test]
    fn non_utc_zone_rolls_to_tomorrow_when_past() {
        // Africa/Addis_Ababa = UTC+3
        let tz: Tz = "Africa/Addis_Ababa".parse().unwrap();
        let target = NaiveTime::from_hms_opt(6, 0, 0).unwrap();
        // 04:00 UTC = 07:00 local → already past 06:00 local
        let now = utc_dt(2024, 6, 1, 4, 0, 0);
        let next = next_local_run_utc_from(tz, target, now);
        assert_eq!(
            next,
            utc_dt(2024, 6, 2, 3, 0, 0),
            "should fire tomorrow 06:00 Addis"
        );
    }

    /// DST spring-forward: America/New_York on 2024-03-10 clocks jump from
    /// 02:00 → 03:00.  Target 02:30 local does not exist → should advance to
    /// 03:00 local = 07:00 UTC (UTC-4 after spring-forward).
    #[test]
    fn dst_spring_forward_advances_to_post_gap_time() {
        let tz: Tz = "America/New_York".parse().unwrap();
        let target = NaiveTime::from_hms_opt(2, 30, 0).unwrap();
        // Just before the spring-forward (still UTC-5) → 01:00 local
        let now = utc_dt(2024, 3, 10, 6, 0, 0); // 01:00 EST
        let next = next_local_run_utc_from(tz, target, now);
        // 02:30 doesn't exist → advance to 03:30 local (UTC-4) = 07:30 UTC
        assert_eq!(
            next,
            utc_dt(2024, 3, 10, 7, 30, 0),
            "spring-forward: should advance 1h past the gap"
        );
    }

    /// DST fall-back: America/New_York on 2024-11-03 clocks fall from
    /// 02:00 → 01:00.  Target 01:30 local is ambiguous (occurs at both
    /// 05:30 UTC and 06:30 UTC).  Should use the *first* (05:30 UTC).
    #[test]
    fn dst_fall_back_uses_first_occurrence() {
        let tz: Tz = "America/New_York".parse().unwrap();
        let target = NaiveTime::from_hms_opt(1, 30, 0).unwrap();
        // Well before 01:30 on fall-back day (e.g., midnight UTC = 20:00 EST prev day)
        let now = utc_dt(2024, 11, 3, 0, 0, 0);
        let next = next_local_run_utc_from(tz, target, now);
        // First occurrence of 01:30 EDT = 05:30 UTC
        assert_eq!(
            next,
            utc_dt(2024, 11, 3, 5, 30, 0),
            "fall-back: should use the first (earlier) occurrence"
        );
    }

    #[test]
    fn parse_hhmm_valid() {
        let t = parse_hhmm("06:00");
        assert_eq!(t, NaiveTime::from_hms_opt(6, 0, 0).unwrap());
    }

    #[test]
    #[should_panic(expected = "SNAPSHOT_TIME_LOCAL must be HH:MM")]
    fn parse_hhmm_invalid_panics() {
        parse_hhmm("6am");
    }
}
