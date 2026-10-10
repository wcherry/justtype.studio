//! The rules of Daily Prompts that are only arithmetic: the evergreen
//! prompts a day falls back on, a project's title from its prompt, and when a
//! writer's email is due.
use chrono::{DateTime, Duration, NaiveDate, NaiveTime, Timelike, Utc};
use chrono_tz::Tz;

use crate::clock;

/// For a day when nothing was queued and the model could not be reached
/// either. Picked by the day, so the same day always gets the same one.
pub const EVERGREEN: &[&str] = &[
    "Write about a door that someone has been told never to open.",
    "A character finds a letter addressed to them, dated twenty years from now.",
    "Describe a kitchen at three in the morning, and who is in it.",
    "Two people share an umbrella. Only one of them wants to.",
    "Write the last conversation in a shop that is closing for good.",
    "Something small goes missing, and it matters more than it should.",
    "A journey that should take an hour takes all day.",
    "Write about a promise kept too well.",
    "Someone hears their name called in a crowd where nobody knows them.",
    "The first warm day of the year, and the news it brings.",
    "An heirloom nobody in the family wants.",
    "Write a scene that ends just before the thing everyone is waiting for.",
];

pub fn evergreen_for(day: NaiveDate) -> &'static str {
    let n = day.num_days_from_ce().unsigned_abs() as usize;
    EVERGREEN[n % EVERGREEN.len()]
}

use chrono::Datelike;

/// A new project's title: the first line of its prompt, shortened.
pub fn title_from_prompt(prompt: &str) -> String {
    let first = prompt
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let title = super::split::shorten(first, 60);
    if title.is_empty() {
        "Daily prompt".into()
    } else {
        title
    }
}

/// When the prompt for `day` should reach a writer who asked for it at
/// `local` in `tz`: the first time that wall-clock time comes round after the
/// day opens. A time the clocks skip that day is sent the first minute after
/// the gap; a time that happens twice, the first time. Should the first
/// occurrence fall after the day has closed (only possible on the 23-hour
/// day), it is sent as the day opens instead, so no day is missed.
pub fn send_instant(day: NaiveDate, local: NaiveTime, tz: Tz) -> DateTime<Utc> {
    let opens = clock::day_start(day);
    let closes = clock::day_end(day);
    let local_date = opens.with_timezone(&tz).date_naive();
    for offset in 0..3 {
        let date = local_date + Duration::days(offset);
        let at = clock::resolve_local(tz, date.and_time(local));
        if at >= opens {
            return if at < closes { at } else { opens };
        }
    }
    opens
}

/// The default time for a writer in `tz`: the first whole hour after the
/// prompt opens there, and never before 07:00.
pub fn default_send_time(tz: Tz, today: NaiveDate) -> NaiveTime {
    let opens = clock::day_start(today).with_timezone(&tz);
    let mut hour = opens.hour();
    if opens.minute() > 0 || opens.second() > 0 {
        hour = (hour + 1) % 24;
    }
    let hour = hour.max(7);
    NaiveTime::from_hms_opt(hour, 0, 0).expect("an hour of the day")
}

/// When the prompt opens on `day`, as a wall-clock time in `tz`, for the note
/// beside the time picker.
pub fn opens_local(day: NaiveDate, tz: Tz) -> NaiveTime {
    clock::day_start(day).with_timezone(&tz).time()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::{America::New_York, Asia::Tokyo, Europe::London};

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }
    fn day(s: &str) -> NaiveDate {
        clock::parse_day(s).unwrap()
    }
    fn t(s: &str) -> NaiveTime {
        clock::parse_time(s).unwrap()
    }

    #[test]
    fn three_timezones_on_an_ordinary_day() {
        // 2026-07-13 opens at 07:00Z (midnight PDT).
        let d = day("2026-07-13");
        // 08:00 in New York (EDT, −4) = 12:00Z.
        assert_eq!(
            send_instant(d, t("08:00"), New_York),
            utc("2026-07-13T12:00:00Z")
        );
        // 07:00 in London (BST, +1) is 06:00Z, before the day opens, so the
        // next morning's 07:00 is the one in this day.
        assert_eq!(
            send_instant(d, t("07:00"), London),
            utc("2026-07-14T06:00:00Z")
        );
        // 17:00 in Tokyo (+9) = 08:00Z: an hour after it opens.
        assert_eq!(
            send_instant(d, t("17:00"), Tokyo),
            utc("2026-07-13T08:00:00Z")
        );
        // Every send falls inside its day.
        for (time, tz) in [("00:00", Tokyo), ("23:59", London), ("06:59", New_York)] {
            let at = send_instant(d, t(time), tz);
            assert!(
                at >= clock::day_start(d) && at < clock::day_end(d),
                "{time} {tz}"
            );
        }
    }

    #[test]
    fn a_local_time_the_clocks_skip_is_sent_after_the_gap() {
        // New York springs forward on 2026-03-08 at 02:00 EST → 03:00 EDT.
        // That Pacific day opens at 08:00Z = 03:00 EDT, so 02:30 has already
        // gone; the next 02:30 (on the 9th) is a real time. Ask instead about
        // the Pacific day before, which contains New York's gap.
        let at = send_instant(day("2026-03-07"), t("02:30"), New_York);
        // 02:30 on the 8th does not exist: 03:00 EDT, 07:00Z.
        assert_eq!(at, utc("2026-03-08T07:00:00Z"));
        assert_eq!(clock::prompt_day(at), day("2026-03-07"));
    }

    #[test]
    fn a_local_time_that_happens_twice_is_sent_the_first_time() {
        // London falls back on 2026-10-25: 01:30 happens in BST and in GMT.
        // The Pacific day of 2026-10-24 runs 07:00Z on the 24th to 07:00Z on
        // the 25th; 01:30 BST on the 25th is 00:30Z, inside it.
        let at = send_instant(day("2026-10-24"), t("01:30"), London);
        assert_eq!(at, utc("2026-10-25T00:30:00Z"));
    }

    #[test]
    fn pacific_daylight_saving_moves_the_window_not_the_writer() {
        // The 23-hour Pacific day: 2026-03-08 opens 08:00Z, closes 07:00Z.
        let d = day("2026-03-08");
        // 09:00 in Tokyo is 00:00Z on the 9th: inside.
        assert_eq!(
            send_instant(d, t("09:00"), Tokyo),
            utc("2026-03-09T00:00:00Z")
        );
        // 16:30 Tokyo = 07:30Z: the 8th's is before it opens and the 9th's is
        // after it closes, so it goes as the day opens.
        assert_eq!(send_instant(d, t("16:30"), Tokyo), clock::day_start(d));
        // The 25-hour day: 2026-11-01 opens 07:00Z, closes 08:00Z on the 2nd.
        let d = day("2026-11-01");
        assert_eq!(
            send_instant(d, t("07:30"), London),
            utc("2026-11-01T07:30:00Z")
        );
    }

    #[test]
    fn the_default_time_is_the_first_hour_after_opening_but_not_before_seven() {
        let d = day("2026-07-13");
        // Opens 08:00 in London: 08:00.
        assert_eq!(default_send_time(London, d), t("08:00"));
        // Opens 16:00 in Tokyo: 16:00.
        assert_eq!(default_send_time(Tokyo, d), t("16:00"));
        // Opens 03:00 in New York: not before 07:00.
        assert_eq!(default_send_time(New_York, d), t("07:00"));
        // Opens at a half hour (India, +5:30 → 12:30): rounded up.
        assert_eq!(default_send_time(chrono_tz::Asia::Kolkata, d), t("13:00"));
    }

    #[test]
    fn evergreen_prompts_and_titles() {
        assert_eq!(
            evergreen_for(day("2026-07-13")),
            evergreen_for(day("2026-07-13"))
        );
        assert_ne!(
            evergreen_for(day("2026-07-13")),
            evergreen_for(day("2026-07-14"))
        );
        assert_eq!(title_from_prompt("\n  A door.\nMore."), "A door.");
        assert!(title_from_prompt(&"word ".repeat(40)).chars().count() <= 61);
        assert_eq!(title_from_prompt("  "), "Daily prompt");
    }
}
