//! The schedule's rules on made-up clocks, in made-up time zones written
//! the way POSIX writes them, so they need no time-zone files and say the
//! same on every computer.

use super::*;

fn london() -> TimeZone {
    TimeZone::posix("GMT0BST,M3.5.0/1,M10.5.0").unwrap()
}

fn new_york() -> TimeZone {
    TimeZone::posix("EST5EDT,M3.2.0,M11.1.0").unwrap()
}

fn sydney() -> TimeZone {
    TimeZone::posix("AEST-10AEDT,M10.1.0,M4.1.0/3").unwrap()
}

/// Cuba moves its clocks at midnight, so a day there can begin at 01:00.
fn havana() -> TimeZone {
    TimeZone::posix("CST5CDT,M3.2.0/0,M11.1.0/1").unwrap()
}

/// A moment, as the clock on the wall in `zone` says it.
fn at(zone: &TimeZone, text: &str) -> Timestamp {
    let civil: jiff::civil::DateTime = text.parse().unwrap();
    zone.to_ambiguous_timestamp(civil).compatible().unwrap()
}

/// A moment as the wall clock says it, back.
fn wall(zone: &TimeZone, t: Timestamp) -> String {
    t.to_zoned(zone.clone())
        .datetime()
        .strftime("%Y-%m-%dT%H:%M")
        .to_string()
}

fn daily(time: &str) -> Schedule {
    Schedule {
        every: Every::Day,
        at: time_named(time).unwrap(),
        paused: false,
    }
}

fn handled(t: Timestamp) -> State {
    State {
        handled_through: Some(t.as_second()),
        ..State::default()
    }
}

#[test]
fn every_day_week_and_month_at_the_time_chosen_in_three_zones() {
    for zone in [london(), new_york(), sydney()] {
        let s = daily("02:30");
        let next = next_slot_after(&s, &zone, at(&zone, "2026-06-10T12:00")).unwrap();
        assert_eq!(wall(&zone, next), "2026-06-11T02:30");
        let weekly = Schedule {
            every: Every::Week(Weekday::Sunday),
            ..daily("09:00")
        };
        // 10 June 2026 is a Wednesday.
        let next = next_slot_after(&weekly, &zone, at(&zone, "2026-06-10T12:00")).unwrap();
        assert_eq!(wall(&zone, next), "2026-06-14T09:00");
        let monthly = Schedule {
            every: Every::Month(1),
            ..daily("09:00")
        };
        let next = next_slot_after(&monthly, &zone, at(&zone, "2026-06-10T12:00")).unwrap();
        assert_eq!(wall(&zone, next), "2026-07-01T09:00");
    }
}

#[test]
fn a_time_the_clocks_skip_moves_on_by_the_gap_and_one_they_repeat_runs_once() {
    let zone = london();
    let s = daily("01:30");
    // 29 March 2026: 01:00 becomes 02:00.
    let spring = slot_on("2026-03-29".parse().unwrap(), &s, &zone).unwrap();
    assert_eq!(wall(&zone, spring), "2026-03-29T02:30");
    // 25 October 2026: 02:00 goes back to 01:00, so 01:30 happens twice.
    let autumn = slot_on("2026-10-25".parse().unwrap(), &s, &zone).unwrap();
    assert_eq!(
        autumn.to_zoned(zone.clone()).offset().seconds(),
        3600,
        "the first 01:30"
    );
    let state = handled(at(&zone, "2026-10-24T12:00"));
    let first = due(
        &s,
        &state,
        &zone,
        autumn.checked_add(jiff::Span::new().minutes(1)).unwrap(),
    );
    let Due::Now { slot, .. } = first else {
        panic!("{first:?}")
    };
    let after = State {
        handled_through: Some(handled_after_run(&s, &zone, slot, slot)),
        last_ran: Some(slot.as_second()),
        ..state
    };
    // The second 01:30, an hour later, is not another run.
    let second = autumn.checked_add(jiff::Span::new().minutes(61)).unwrap();
    assert!(matches!(due(&s, &after, &zone, second), Due::NotYet { .. }));
}

#[test]
fn a_day_that_begins_at_one_in_the_morning_still_has_its_midnight_run() {
    let zone = havana();
    let s = daily("00:00");
    // 8 March 2026, the second Sunday of March: midnight does not happen.
    let slot = slot_on("2026-03-08".parse().unwrap(), &s, &zone).unwrap();
    assert_eq!(wall(&zone, slot), "2026-03-08T01:00");
}

#[test]
fn the_31st_is_the_last_day_of_a_shorter_month() {
    let zone = london();
    let s = Schedule {
        every: Every::Month(31),
        ..daily("02:30")
    };
    let mut t = at(&zone, "2027-01-31T12:00");
    let mut seen = Vec::new();
    for _ in 0..4 {
        let next = next_slot_after(&s, &zone, t).unwrap();
        seen.push(wall(&zone, next));
        t = next;
    }
    assert_eq!(
        seen,
        [
            "2027-02-28T02:30",
            "2027-03-31T02:30",
            "2027-04-30T02:30",
            "2027-05-31T02:30"
        ]
    );
    let leap = next_slot_after(&s, &zone, at(&zone, "2028-02-01T00:00")).unwrap();
    assert_eq!(wall(&zone, leap), "2028-02-29T02:30");
}

#[test]
fn forty_days_off_is_caught_up_once_not_forty_times() {
    let zone = london();
    let s = daily("02:30");
    let off = at(&zone, "2026-06-01T03:00");
    let back = at(&zone, "2026-07-11T08:00");
    let state = handled(off);
    let Due::Now { slot, missed } = due(&s, &state, &zone, back) else {
        panic!()
    };
    assert_eq!(wall(&zone, slot), "2026-07-11T02:30");
    assert_eq!(missed, 39);
    assert!(catching_up_words(missed).unwrap().contains("39 runs did not happen"));
    let after = State {
        handled_through: Some(handled_after_run(&s, &zone, slot, back)),
        last_ran: Some(back.as_second()),
        ..state
    };
    assert_eq!(
        due(&s, &after, &zone, back),
        Due::NotYet {
            next: Some(at(&zone, "2026-07-12T02:30"))
        }
    );
}

#[test]
fn asleep_across_several_due_times_is_one_run_when_it_wakes() {
    let zone = new_york();
    let s = Schedule {
        every: Every::Week(Weekday::Sunday),
        ..daily("09:00")
    };
    let state = handled(at(&zone, "2026-05-01T12:00"));
    let woke = at(&zone, "2026-05-20T07:00");
    let Due::Now { slot, missed } = due(&s, &state, &zone, woke) else {
        panic!()
    };
    // Sundays 3 and 10 May were missed; 17 May is the one run.
    assert_eq!((wall(&zone, slot).as_str(), missed), ("2026-05-17T09:00", 2));
}

#[test]
fn skip_the_next_one_and_then_miss_one_and_pause_and_carry_on() {
    let zone = london();
    let s = daily("02:30");
    let saved = at(&zone, "2026-06-01T12:00");
    let next = next_slot_after(&s, &zone, saved).unwrap();
    let mut state = State {
        skip: Some(next.as_second()),
        ..handled(saved)
    };
    let woke = at(&zone, "2026-06-02T03:00");
    assert_eq!(
        due(&s, &state, &zone, woke),
        Due::Pass {
            slot: next,
            why: Pass::Skipped
        }
    );
    state.handled_through = Some(next.as_second());
    state.skip = None;
    // Skipped, then the computer is off for the next one: caught up once.
    let later = at(&zone, "2026-06-03T09:00");
    assert!(matches!(due(&s, &state, &zone, later), Due::Now { missed: 0, .. }));
    // Paused, nothing is due; carrying on accounts for everything up to then.
    let paused = Schedule { paused: true, ..s };
    assert_eq!(due(&paused, &state, &zone, at(&zone, "2026-07-01T09:00")), Due::Paused);
    let carried_on = handled(at(&zone, "2026-07-01T09:00"));
    assert!(matches!(
        due(&s, &carried_on, &zone, at(&zone, "2026-07-01T09:00")),
        Due::NotYet { .. }
    ));
}

#[test]
fn a_schedule_saved_or_changed_never_runs_at_once() {
    let zone = london();
    let now = at(&zone, "2026-06-01T03:00");
    for s in [daily("02:30"), daily("02:59")] {
        assert!(matches!(due(&s, &handled(now), &zone, now), Due::NotYet { .. }));
    }
    // Never set at all: nothing before now is owed either.
    assert!(matches!(
        due(&daily("02:30"), &State::default(), &zone, now),
        Due::NotYet { .. }
    ));
}

#[test]
fn a_run_longer_than_its_period_does_not_start_again_the_moment_it_ends() {
    let zone = london();
    let s = daily("02:30");
    let slot = at(&zone, "2026-06-01T02:30");
    let ended = at(&zone, "2026-06-02T11:00");
    let state = State {
        handled_through: Some(handled_after_run(&s, &zone, slot, ended)),
        last_ran: Some(slot.as_second()),
        ..State::default()
    };
    assert_eq!(
        due(&s, &state, &zone, ended),
        Due::NotYet {
            next: Some(at(&zone, "2026-06-03T02:30"))
        }
    );
}

#[test]
fn a_clock_that_once_said_2030_does_not_stop_every_run() {
    let zone = london();
    let s = daily("02:30");
    let state = State {
        handled_through: Some(at(&zone, "2030-01-01T00:00").as_second()),
        last_ran: Some(at(&zone, "2030-01-01T00:00").as_second()),
        ..State::default()
    };
    let now = at(&zone, "2026-06-02T08:00");
    assert!(matches!(due(&s, &state, &zone, now), Due::NotYet { .. }), "read as now");
    let next = next_slot_after(&s, &zone, now).unwrap();
    let state = State {
        handled_through: Some(now.as_second()),
        ..state
    };
    assert!(matches!(
        due(
            &s,
            &state,
            &zone,
            next.checked_add(jiff::Span::new().minutes(5)).unwrap()
        ),
        Due::Now { .. }
    ));
}

#[test]
fn travelling_west_or_running_now_never_gives_two_runs_close_together() {
    let s = daily("02:30");
    // Ran at 02:30 in London, then flew to New York the same morning.
    let ran = at(&london(), "2026-06-02T02:30");
    let zone = new_york();
    let state = State {
        handled_through: Some(ran.as_second()),
        last_ran: Some(ran.as_second()),
        ..State::default()
    };
    let new_york_slot = at(&zone, "2026-06-02T02:30");
    let woke = new_york_slot.checked_add(jiff::Span::new().minutes(1)).unwrap();
    assert_eq!(
        due(&s, &state, &zone, woke),
        Due::Pass {
            slot: new_york_slot,
            why: Pass::TooSoon
        }
    );
    // Run now an hour before the slot does the same.
    let zone = london();
    let run_now = at(&zone, "2026-06-03T01:30");
    let state = State {
        handled_through: Some(at(&zone, "2026-06-02T03:00").as_second()),
        last_ran: Some(run_now.as_second()),
        ..State::default()
    };
    assert!(matches!(
        due(&s, &state, &zone, at(&zone, "2026-06-03T02:31")),
        Due::Pass { why: Pass::TooSoon, .. }
    ));
}

#[test]
fn what_each_way_a_run_ends_accounts_for() {
    for ran in [
        Ran::Worked,
        Ran::SomeFiles,
        Ran::StoppedByPerson,
        Ran::Unreachable,
        Ran::Refused,
        Ran::HeldBack,
    ] {
        assert!(accounts_for_its_slot(ran), "{ran:?}");
    }
    for ran in [Ran::NotNow, Ran::Busy] {
        assert!(!accounts_for_its_slot(ran), "{ran:?}");
    }
}

#[test]
fn a_schedule_is_said_as_a_person_says_it() {
    assert_eq!(every_words(&daily("02:30")), "Every day at 02:30");
    let weekly = Schedule {
        every: Every::Week(Weekday::Sunday),
        ..daily("09:00")
    };
    assert_eq!(every_words(&weekly), "Every week on Sunday at 09:00");
    let monthly = Schedule {
        every: Every::Month(31),
        ..daily("02:30")
    };
    assert_eq!(
        every_words(&monthly),
        "Every month on the 31st (or the last day of shorter months) at 02:30"
    );
    let first = Schedule {
        every: Every::Month(1),
        ..daily("09:00")
    };
    assert_eq!(every_words(&first), "Every month on the 1st at 09:00");
    assert_eq!(
        [ordinal(2), ordinal(3), ordinal(11), ordinal(22)],
        ["2nd", "3rd", "11th", "22nd"]
    );

    let zone = london();
    let now = at(&zone, "2026-06-10T21:00");
    assert_eq!(
        next_words(at(&zone, "2026-06-11T02:30"), now, &zone),
        "Tonight at 02:30"
    );
    assert_eq!(
        next_words(at(&zone, "2026-06-11T09:00"), now, &zone),
        "Tomorrow at 09:00"
    );
    assert_eq!(
        next_words(at(&zone, "2026-06-10T22:00"), now, &zone),
        "Tonight at 22:00"
    );
    assert_eq!(
        next_words(at(&zone, "2026-06-11T02:30"), at(&zone, "2026-06-10T12:00"), &zone),
        "Tonight at 02:30"
    );
    assert_eq!(
        next_words(at(&zone, "2026-06-11T02:30"), at(&zone, "2026-06-10T03:00"), &zone),
        "Tomorrow at 02:30"
    );
    let morning = at(&zone, "2026-06-10T08:00");
    assert_eq!(
        next_words(at(&zone, "2026-06-10T14:00"), morning, &zone),
        "Today at 14:00"
    );
    assert_eq!(next_words(at(&zone, "2026-06-14T09:00"), now, &zone), "Sunday at 09:00");
    assert_eq!(
        next_words(at(&zone, "2026-12-31T02:30"), now, &zone),
        "On 31 December at 02:30"
    );
}

#[test]
fn a_schedule_is_kept_and_read_back_and_anything_else_is_no_schedule() {
    for s in [
        daily("02:30"),
        Schedule {
            every: Every::Week(Weekday::Friday),
            paused: true,
            ..daily("23:45")
        },
        Schedule {
            every: Every::Month(31),
            ..daily("00:00")
        },
    ] {
        let text = serde_json::to_string(&s.stored()).unwrap();
        let read: Stored = serde_json::from_str(&text).unwrap();
        assert_eq!(Schedule::from_stored(&read), Some(s), "{text}");
    }
    assert_eq!(
        serde_json::to_value(daily("02:30").stored()).unwrap(),
        serde_json::json!({ "v": 1, "every": "day", "at": "02:30", "paused": false })
    );
    for bad in [
        r#"{"v":1,"every":"fortnight","at":"02:30"}"#,
        r#"{"v":1,"every":"week","at":"02:30"}"#,
        r#"{"v":1,"every":"month","at":"02:30","day":32}"#,
        r#"{"v":1,"every":"day","at":"25:00"}"#,
    ] {
        let read: Stored = serde_json::from_str(bad).unwrap();
        assert_eq!(Schedule::from_stored(&read), None, "{bad}");
    }
}

#[test]
fn when_something_happened_is_said_the_way_a_person_says_it() {
    let zone = london();
    let now = at(&zone, "2026-06-11T09:00");
    assert_eq!(when_words(at(&zone, "2026-06-11T02:31"), now, &zone), "Today at 02:31");
    assert_eq!(
        when_words(at(&zone, "2026-06-10T23:59"), now, &zone),
        "Yesterday at 23:59"
    );
    assert_eq!(
        when_words(at(&zone, "2026-06-09T02:30"), now, &zone),
        "Tuesday at 02:30"
    );
    assert_eq!(
        when_words(at(&zone, "2026-06-03T02:30"), now, &zone),
        "On 3 June at 02:30"
    );
    assert_eq!(caught_up_words(0), None);
    assert_eq!(
        caught_up_words(2).as_deref(),
        Some("This caught up 2 runs that were missed, perhaps because the computer was off or asleep.")
    );
}
