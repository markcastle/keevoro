//! When a backup runs by itself: every day, week or month at a chosen
//! time, in the computer's own time wherever it is.
//!
//! Pure: the time zone and the moment "now" are always passed in, so every
//! rule is tested with made-up clocks, including days when the clocks
//! change and a computer asleep across several runs. Nothing here reads
//! the clock.
//!
//! The rules:
//!
//! - **A slot** is the moment a run is due: the chosen time on a day the
//!   schedule names. A time the clocks skip (01:30 on the spring morning in
//!   London) is moved on by the length of the gap, to 02:30; a time that
//!   happens twice (the autumn hour) is the first of the two. The 31st is
//!   the last day of a shorter month.
//! - **Due** is the latest slot at or before now that is after the last
//!   one accounted for. However many were missed (a computer off for a
//!   month), it is one run, said as catching up.
//! - **Saving** a schedule, or carrying on after a pause, accounts for
//!   every slot up to then, so nothing runs at once and a month's pause is
//!   never caught up. Run now is how to run at once.
//! - **Skipping** the next one marks that slot; when it comes it is
//!   accounted for, and said, without a run.
//! - **Never twice in quick succession:** a slot within half a period of
//!   the last run (12 hours for a daily schedule) is accounted for without
//!   a run, so a journey west, or Run now an hour before, does not give two.
//! - **A clock that was wrong:** a record more than a day in the future is
//!   read as now, or a computer whose clock once said 2030 would never run
//!   again.

use jiff::civil::{Date, Time, Weekday};
use jiff::tz::TimeZone;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

/// How often.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Every {
    Day,
    Week(Weekday),
    /// On this day of the month, 1 to 31; the last day of a shorter month.
    Month(u8),
}

/// A backup's schedule, as the person chose it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schedule {
    pub every: Every,
    /// The time, in the computer's own time zone wherever it is.
    pub at: Time,
    /// Paused: no slot is due until it carries on.
    pub paused: bool,
}

/// How a schedule is kept, as JSON in the backup's catalog. `weekday` is
/// only for `week` and `day` only for `month`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stored {
    pub v: u32,
    /// `day`, `week` or `month`.
    pub every: String,
    /// `HH:MM`.
    pub at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weekday: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day: Option<u8>,
    #[serde(default)]
    pub paused: bool,
}

const WEEKDAYS: [(Weekday, &str, &str); 7] = [
    (Weekday::Monday, "mon", "Monday"),
    (Weekday::Tuesday, "tue", "Tuesday"),
    (Weekday::Wednesday, "wed", "Wednesday"),
    (Weekday::Thursday, "thu", "Thursday"),
    (Weekday::Friday, "fri", "Friday"),
    (Weekday::Saturday, "sat", "Saturday"),
    (Weekday::Sunday, "sun", "Sunday"),
];

/// A weekday from its short name, `mon` to `sun`, or its whole name.
pub fn weekday_named(name: &str) -> Option<Weekday> {
    let name = name.trim().to_ascii_lowercase();
    WEEKDAYS
        .iter()
        .find(|(_, short, long)| name == *short || name == long.to_ascii_lowercase())
        .map(|(day, _, _)| *day)
}

fn weekday_short(day: Weekday) -> &'static str {
    WEEKDAYS
        .iter()
        .find(|(d, _, _)| *d == day)
        .map_or("sun", |(_, short, _)| short)
}

/// "Sunday".
pub fn weekday_words(day: Weekday) -> &'static str {
    WEEKDAYS
        .iter()
        .find(|(d, _, _)| *d == day)
        .map_or("Sunday", |(_, _, long)| long)
}

/// A time of day from `HH:MM`.
pub fn time_named(text: &str) -> Option<Time> {
    let (h, m) = text.trim().split_once(':')?;
    let (h, m): (i8, i8) = (h.parse().ok()?, m.parse().ok()?);
    Time::new(h, m, 0, 0).ok()
}

/// `HH:MM`.
pub fn time_words(at: Time) -> String {
    format!("{:02}:{:02}", at.hour(), at.minute())
}

impl Schedule {
    /// Read from how it is kept. `None` when it cannot be understood.
    pub fn from_stored(stored: &Stored) -> Option<Self> {
        let every = match stored.every.as_str() {
            "day" => Every::Day,
            "week" => Every::Week(weekday_named(stored.weekday.as_deref()?)?),
            "month" => match stored.day? {
                day @ 1..=31 => Every::Month(day),
                _ => return None,
            },
            _ => return None,
        };
        Some(Self {
            every,
            at: time_named(&stored.at)?,
            paused: stored.paused,
        })
    }

    /// How it is kept.
    pub fn stored(&self) -> Stored {
        let (every, weekday, day) = match self.every {
            Every::Day => ("day", None, None),
            Every::Week(w) => ("week", Some(weekday_short(w).to_string()), None),
            Every::Month(d) => ("month", None, Some(d)),
        };
        Stored {
            v: 1,
            every: every.to_string(),
            at: time_words(self.at),
            weekday,
            day,
            paused: self.paused,
        }
    }

    /// About how long between two runs, in seconds, for the rule that two
    /// never come in quick succession.
    fn period(&self) -> i64 {
        match self.every {
            Every::Day => 86_400,
            Every::Week(_) => 7 * 86_400,
            Every::Month(_) => 28 * 86_400,
        }
    }
}

/// Is a run due on this date, by the calendar alone?
fn falls_on(date: Date, every: Every) -> bool {
    match every {
        Every::Day => true,
        Every::Week(day) => date.weekday() == day,
        Every::Month(day) => {
            let last = date.days_in_month();
            date.day() == i8::try_from(day).unwrap_or(31).min(last)
        }
    }
}

/// The moment of the run on this date, if the schedule has one then. A
/// time the clocks skip is moved on by the gap; one they repeat is the
/// first.
pub fn slot_on(date: Date, schedule: &Schedule, zone: &TimeZone) -> Option<Timestamp> {
    if !falls_on(date, schedule.every) {
        return None;
    }
    zone.to_ambiguous_timestamp(date.to_datetime(schedule.at))
        .compatible()
        .ok()
}

/// The furthest from a date a slot can be: a month and a few days.
const LOOK_AT_MOST: i32 = 40;

/// The first slot after `t`.
pub fn next_slot_after(schedule: &Schedule, zone: &TimeZone, t: Timestamp) -> Option<Timestamp> {
    let mut date = t.to_zoned(zone.clone()).date().yesterday().ok()?;
    for _ in 0..LOOK_AT_MOST {
        if let Some(slot) = slot_on(date, schedule, zone).filter(|slot| *slot > t) {
            return Some(slot);
        }
        date = date.tomorrow().ok()?;
    }
    None
}

/// The latest slot at or before `t`.
pub fn latest_slot_at_or_before(schedule: &Schedule, zone: &TimeZone, t: Timestamp) -> Option<Timestamp> {
    let mut date = t.to_zoned(zone.clone()).date().tomorrow().ok()?;
    for _ in 0..LOOK_AT_MOST {
        if let Some(slot) = slot_on(date, schedule, zone).filter(|slot| *slot <= t) {
            return Some(slot);
        }
        date = date.yesterday().ok()?;
    }
    None
}

/// What Keevoro remembers of a schedule's runs, for deciding the next.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct State {
    /// Every slot up to here is accounted for (seconds since 1970).
    pub handled_through: Option<i64>,
    /// The one slot to skip.
    pub skip: Option<i64>,
    /// When a backup last ran by itself, for any reason.
    pub last_ran: Option<i64>,
}

/// Whether a backup's schedule wants a run now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Due {
    /// Paused by the person.
    Paused,
    /// Nothing is due; the next slot is then.
    NotYet { next: Option<Timestamp> },
    /// A slot is due, but is accounted for without a run: the person asked
    /// to skip it, or a run came too recently.
    Pass { slot: Timestamp, why: Pass },
    /// Run now, for this slot, having missed `missed` before it.
    Now { slot: Timestamp, missed: usize },
}

/// Why a slot is passed without a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    Skipped,
    TooSoon,
}

/// A record more than this far in the future means the clock was wrong.
const FUTURE_IS_WRONG: i64 = 86_400;

/// Whether a run is due now.
pub fn due(schedule: &Schedule, state: &State, zone: &TimeZone, now: Timestamp) -> Due {
    if schedule.paused {
        return Due::Paused;
    }
    let now_s = now.as_second();
    let wrong = |at: i64| at > now_s + FUTURE_IS_WRONG;
    let handled = state.handled_through.map(|at| if wrong(at) { now_s } else { at });
    let next = || {
        next_slot_after(
            schedule,
            zone,
            Timestamp::from_second(handled.unwrap_or(now_s).max(now_s)).unwrap_or(now),
        )
    };
    let Some(slot) = latest_slot_at_or_before(schedule, zone, now) else {
        return Due::NotYet { next: next() };
    };
    // Never set: nothing before now is owed.
    let Some(handled) = handled else {
        return Due::NotYet { next: next() };
    };
    if slot.as_second() <= handled {
        return Due::NotYet { next: next() };
    }
    if state.skip == Some(slot.as_second()) {
        return Due::Pass {
            slot,
            why: Pass::Skipped,
        };
    }
    let last_ran = state.last_ran.filter(|at| !wrong(*at));
    if last_ran.is_some_and(|at| slot.as_second() - at < schedule.period() / 2 && at <= now_s) {
        return Due::Pass {
            slot,
            why: Pass::TooSoon,
        };
    }
    Due::Now {
        slot,
        missed: missed_between(schedule, zone, handled, slot),
    }
}

/// How many slots came after `handled` and before `slot`.
fn missed_between(schedule: &Schedule, zone: &TimeZone, handled: i64, slot: Timestamp) -> usize {
    let Ok(mut t) = Timestamp::from_second(handled) else {
        return 0;
    };
    let mut missed = 0;
    while let Some(next) = next_slot_after(schedule, zone, t) {
        if next >= slot || missed > 1_000 {
            break;
        }
        missed += 1;
        t = next;
    }
    missed
}

/// Everything accounted for once a run for `slot` has ended at `ended`:
/// a run longer than the period never starts again the moment it ends.
pub fn handled_after_run(schedule: &Schedule, zone: &TimeZone, slot: Timestamp, ended: Timestamp) -> i64 {
    latest_slot_at_or_before(schedule, zone, ended)
        .map_or(slot.as_second(), |latest| latest.as_second().max(slot.as_second()))
}

/// How a run by itself ended, for deciding what is accounted for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ran {
    /// It sent something, or there was nothing new.
    Worked,
    /// Some files could not be sent.
    SomeFiles,
    /// The person stopped it.
    StoppedByPerson,
    /// A destination was out of reach.
    Unreachable,
    /// No room, keys not working, nowhere to back up to.
    Refused,
    /// Something needs the person's say.
    HeldBack,
    /// On battery, busy, or the person in Keevoro: not now.
    NotNow,
    /// Another program was working on the backup.
    Busy,
}

/// Whether a run that ended so accounts for its slot. One that did not
/// run now, or found the backup busy elsewhere, does not: it is tried
/// again, and once the next slot passes the two count as one. Everything
/// else does, problems included: the next slot tries again, and nothing
/// retries in a loop. (A run cut short when Keevoro quits or the computer
/// shuts down never gets here, so its slot is caught up once.)
pub fn accounts_for_its_slot(ran: Ran) -> bool {
    !matches!(ran, Ran::NotNow | Ran::Busy)
}

/// "Every day at 02:30", "Every week on Sunday at 09:00", "Every month on
/// the 31st (or the last day of shorter months) at 02:30".
pub fn every_words(schedule: &Schedule) -> String {
    let at = time_words(schedule.at);
    match schedule.every {
        Every::Day => format!("Every day at {at}"),
        Every::Week(day) => format!("Every week on {} at {at}", weekday_words(day)),
        Every::Month(day) if day > 28 => {
            format!(
                "Every month on the {} (or the last day of shorter months) at {at}",
                ordinal(day)
            )
        }
        Every::Month(day) => format!("Every month on the {} at {at}", ordinal(day)),
    }
}

/// "1st", "2nd", "23rd", "31st".
pub fn ordinal(n: u8) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// When the next run is, as a person says it: "Tonight at 02:30",
/// "Tomorrow at 09:00", "Sunday at 09:00", "On 31 December at 02:30".
pub fn next_words(next: Timestamp, now: Timestamp, zone: &TimeZone) -> String {
    let (then, now) = (next.to_zoned(zone.clone()), now.to_zoned(zone.clone()));
    let at = time_words(then.time());
    let days = (then.date() - now.date()).get_days();
    let evening = |hour: i8| hour >= 18;
    let small_hours = then.hour() < 6;
    match days {
        0 if evening(then.hour()) => format!("Tonight at {at}"),
        0 if small_hours => format!("Tonight at {at}"),
        0 => format!("Today at {at}"),
        // The small hours of tomorrow are the coming night.
        1 if small_hours && now.hour() >= 6 => format!("Tonight at {at}"),
        1 => format!("Tomorrow at {at}"),
        2..=6 => format!("{} at {at}", weekday_words(then.weekday())),
        _ => format!(
            "On {} {} at {at}",
            then.day(),
            MONTHS[usize::try_from(then.month() - 1).unwrap_or(0)]
        ),
    }
}

/// When something already happened, as a person says it: "Today at
/// 02:31", "Yesterday at 02:31", "Tuesday at 02:31", "On 3 June at 02:31".
pub fn when_words(then: Timestamp, now: Timestamp, zone: &TimeZone) -> String {
    let (then, now) = (then.to_zoned(zone.clone()), now.to_zoned(zone.clone()));
    let at = time_words(then.time());
    match (now.date() - then.date()).get_days() {
        0 => format!("Today at {at}"),
        1 => format!("Yesterday at {at}"),
        2..=6 => format!("{} at {at}", weekday_words(then.weekday())),
        _ => format!(
            "On {} {} at {at}",
            then.day(),
            MONTHS[usize::try_from(then.month() - 1).unwrap_or(0)]
        ),
    }
}

/// What a run that caught up says of what it caught up, for "While you
/// were away".
pub fn caught_up_words(missed: usize) -> Option<String> {
    match missed {
        0 => None,
        1 => Some("This caught up a run that was missed, perhaps because the computer was off or asleep.".into()),
        n => Some(format!(
            "This caught up {n} runs that were missed, perhaps because the computer was off or asleep."
        )),
    }
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// When a run due happened, as a person says it, for "Catching up: the
/// run due on Tuesday did not happen because the computer was off".
pub fn catching_up_words(missed: usize) -> Option<String> {
    match missed {
        0 => None,
        1 => Some("Catching up: the run before this one did not happen, perhaps because the computer was off. This one run catches up.".into()),
        n => Some(format!(
            "Catching up: {n} runs did not happen, perhaps because the computer was off. This one run catches up all of them."
        )),
    }
}

#[cfg(test)]
mod tests;
