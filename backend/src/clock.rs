//! What time it is, and which prompt day that falls in.
//!
//! A prompt day runs midnight to midnight in one timezone, Pacific by default
//! (`JUSTTYPE_PROMPT_TZ`), so development and production agree on when a day
//! opens. It follows daylight saving: the day the clocks go forward is 23
//! hours long, the day they go back 25.
//!
//! The clock can be moved forward for the smoke walkthrough and nothing else:
//! `JUSTTYPE_TEST_CLOCK=1` mounts a route that sets an offset (see
//! `routes::admin::set_test_clock`). Everything that asks "what day is it" for
//! Daily Prompts asks [`now`], so moving it moves the whole feature at once.
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::OnceLock;

use chrono::{DateTime, Duration, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

/// Seconds added to the real time. Zero except under the test clock.
static OFFSET_SECONDS: AtomicI64 = AtomicI64::new(0);

pub fn now() -> DateTime<Utc> {
    Utc::now() + Duration::seconds(OFFSET_SECONDS.load(Ordering::Relaxed))
}

/// Moves [`now`] so that it reads `to`. Only the test clock route calls this.
pub fn set_now(to: DateTime<Utc>) {
    OFFSET_SECONDS.store((to - Utc::now()).num_seconds(), Ordering::Relaxed);
}

/// `now()` as the ISO string every row stores.
pub fn now_iso() -> String {
    now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// The prompt day's timezone.
pub fn prompt_tz() -> Tz {
    static TZ: OnceLock<Tz> = OnceLock::new();
    *TZ.get_or_init(|| {
        std::env::var("JUSTTYPE_PROMPT_TZ")
            .ok()
            .and_then(|name| name.parse().ok())
            .unwrap_or(chrono_tz::America::Los_Angeles)
    })
}

/// The prompt day `at` falls in.
pub fn prompt_day(at: DateTime<Utc>) -> NaiveDate {
    prompt_day_in(prompt_tz(), at)
}

pub fn prompt_day_in(tz: Tz, at: DateTime<Utc>) -> NaiveDate {
    at.with_timezone(&tz).date_naive()
}

/// When `day` opens: its first instant, in UTC.
pub fn day_start(day: NaiveDate) -> DateTime<Utc> {
    day_start_in(prompt_tz(), day)
}

pub fn day_start_in(tz: Tz, day: NaiveDate) -> DateTime<Utc> {
    resolve_local(tz, day.and_time(NaiveTime::MIN))
}

/// When `day` closes, which is when the next one opens.
pub fn day_end(day: NaiveDate) -> DateTime<Utc> {
    day_start(day.succ_opt().unwrap_or(day))
}

/// A wall-clock time in `tz` as an instant. One that happens twice (when the
/// clocks go back) is the first; one that never happens (when they go
/// forward) is the first minute after the gap.
pub fn resolve_local(tz: Tz, local: NaiveDateTime) -> DateTime<Utc> {
    let mut probe = local;
    // A gap is at most a few hours anywhere; a day of minutes is far more than
    // enough to step over it.
    for _ in 0..(24 * 60) {
        match tz.from_local_datetime(&probe) {
            LocalResult::Single(t) => return t.with_timezone(&Utc),
            LocalResult::Ambiguous(first, _) => return first.with_timezone(&Utc),
            LocalResult::None => probe += Duration::minutes(1),
        }
    }
    Utc.from_utc_datetime(&local)
}

/// `YYYY-MM-DD`.
pub fn day_string(day: NaiveDate) -> String {
    day.format("%Y-%m-%d").to_string()
}

pub fn parse_day(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()
}

/// `HH:MM`.
pub fn parse_time(value: &str) -> Option<NaiveTime> {
    if value.len() != 5 {
        return None;
    }
    NaiveTime::parse_from_str(value, "%H:%M").ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::America::Los_Angeles as LA;

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }
    fn day(s: &str) -> NaiveDate {
        parse_day(s).unwrap()
    }

    #[test]
    fn the_day_turns_at_pacific_midnight() {
        // 23:59 and 00:00 Pacific, in summer (UTC−7).
        assert_eq!(
            prompt_day_in(LA, utc("2026-07-13T06:59:59Z")),
            day("2026-07-12")
        );
        assert_eq!(
            prompt_day_in(LA, utc("2026-07-13T07:00:00Z")),
            day("2026-07-13")
        );
        // And in winter (UTC−8).
        assert_eq!(
            prompt_day_in(LA, utc("2026-01-13T07:59:59Z")),
            day("2026-01-12")
        );
        assert_eq!(
            prompt_day_in(LA, utc("2026-01-13T08:00:00Z")),
            day("2026-01-13")
        );
        assert_eq!(
            day_start_in(LA, day("2026-07-13")),
            utc("2026-07-13T07:00:00Z")
        );
    }

    #[test]
    fn the_day_the_clocks_go_forward_is_23_hours() {
        // 2026-03-08: 02:00 PST becomes 03:00 PDT.
        let start = day_start_in(LA, day("2026-03-08"));
        let end = day_start_in(LA, day("2026-03-09"));
        assert_eq!(start, utc("2026-03-08T08:00:00Z"));
        assert_eq!(end - start, Duration::hours(23));
        assert_eq!(
            prompt_day_in(LA, utc("2026-03-09T06:59:59Z")),
            day("2026-03-08")
        );
        assert_eq!(
            prompt_day_in(LA, utc("2026-03-09T07:00:00Z")),
            day("2026-03-09")
        );
    }

    #[test]
    fn the_day_the_clocks_go_back_is_25_hours() {
        // 2026-11-01: 02:00 PDT becomes 01:00 PST.
        let start = day_start_in(LA, day("2026-11-01"));
        let end = day_start_in(LA, day("2026-11-02"));
        assert_eq!(start, utc("2026-11-01T07:00:00Z"));
        assert_eq!(end - start, Duration::hours(25));
        assert_eq!(
            prompt_day_in(LA, utc("2026-11-02T07:59:59Z")),
            day("2026-11-01")
        );
        assert_eq!(
            prompt_day_in(LA, utc("2026-11-02T08:00:00Z")),
            day("2026-11-02")
        );
    }

    #[test]
    fn skipped_and_repeated_local_times_resolve_once() {
        // 02:30 does not exist on the spring-forward night: the first minute
        // after the gap is 03:00 PDT.
        let skipped = resolve_local(LA, day("2026-03-08").and_hms_opt(2, 30, 0).unwrap());
        assert_eq!(skipped, utc("2026-03-08T10:00:00Z"));
        // 01:30 happens twice on the fall-back night: the first is PDT.
        let repeated = resolve_local(LA, day("2026-11-01").and_hms_opt(1, 30, 0).unwrap());
        assert_eq!(repeated, utc("2026-11-01T08:30:00Z"));
    }

    #[test]
    fn times_and_days_parse_strictly() {
        assert!(parse_time("07:00").is_some());
        assert!(parse_time("7:00").is_none());
        assert!(parse_time("24:00").is_none());
        assert!(parse_day("2026-02-30").is_none());
    }
}
