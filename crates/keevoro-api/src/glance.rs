//! Keevoro at a glance: what it is doing, in one line and one of four
//! moods, for the icon by the clock and its menu.
//!
//! The words are worked out here, pure, from facts gathered by
//! [`Keevoro::at_a_glance`], so the app's icon, its tooltip and its menu say
//! the same as the window and the command line: the time a backup has left
//! is the estimator's own sentence, the next run is the schedule's own
//! words, and pausing is Keevoro's one Pause.
//!
//! Gathering costs nothing worth counting: the jobs are in memory, and each
//! backup's schedule and record are the ones the scheduler already keeps
//! (read from its catalog at most every 15 minutes, or when changed here).

use crate::jobs::{JobId, JobKind};
use crate::keevoro::Keevoro;
use jiff::civil::Time;
use jiff::tz::TimeZone;
use jiff::Timestamp;
use keevoro_core::schedule::{self as rules, Due};
use serde::Serialize;
use std::sync::atomic::Ordering;

/// What Keevoro is like now, which the icon shows by its shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Mood {
    /// Nothing is happening and nothing waits for the person.
    Calm,
    /// A backup, a search, a check or a survey is going.
    Working,
    /// The person paused Keevoro.
    Paused,
    /// A backup by itself held something back for the person's say, or
    /// could not back up, since they last had the window open.
    NeedsYou,
}

/// The job that says what Keevoro is doing, when several are going.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Doing {
    pub kind: JobKind,
    /// The backup it is for, by name.
    pub backup: Option<String>,
    /// For a backup: the time left in the estimator's words, "About 12
    /// minutes left".
    pub left: Option<String>,
}

/// Everything the line is made from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    pub paused: bool,
    /// When a pause for a while ends by itself.
    pub paused_until: Option<Timestamp>,
    pub doing: Option<Doing>,
    pub needs_you: bool,
    /// The soonest run by itself: the backup's name, and when.
    pub next: Option<(String, Timestamp)>,
    /// More than one backup has a schedule, so the next is named.
    pub several: bool,
    /// Every backup with a schedule last ran well, and at least one has run.
    pub all_well: bool,
    /// Some backup has a schedule, paused or not.
    pub any_schedule: bool,
}

/// What is said when something needs the person.
pub const NEEDS_YOU: &str = "Something needs your say";

/// The outcomes of a run by itself (in a schedule's last few) that need
/// the person: something held back, files not backed up, a destination
/// out of reach, or a run refused.
pub const NEEDS_YOU_AFTER: &[&str] = &["heldBack", "someFiles", "unreachable", "refused"];

/// The mood and the line, at `now` in `zone`. Pure.
pub fn said(facts: &Facts, now: Timestamp, zone: &TimeZone) -> (Mood, String) {
    if facts.paused {
        let line = match facts.paused_until {
            Some(until) => format!("Paused until {}", until_words(until, now, zone)),
            None => "Paused".to_string(),
        };
        return (Mood::Paused, line);
    }
    if let Some(doing) = &facts.doing {
        return (Mood::Working, doing_words(doing));
    }
    if facts.needs_you {
        return (Mood::NeedsYou, NEEDS_YOU.to_string());
    }
    let line = match (&facts.next, facts.any_schedule) {
        (Some((name, at)), _) => {
            let when = in_a_sentence(&rules::next_words(*at, now, zone));
            let next = if facts.several {
                format!("Next: {name} {when}")
            } else {
                format!("Next: {when}")
            };
            if facts.all_well {
                format!("All backed up. {next}")
            } else {
                next
            }
        }
        (None, true) => "Nothing due: backing up by itself is paused".to_string(),
        (None, false) => "Nothing running now".to_string(),
    };
    (Mood::Calm, line)
}

/// What a job is doing, in the words of the line.
fn doing_words(doing: &Doing) -> String {
    let name = doing.backup.as_deref();
    match (doing.kind, name) {
        (JobKind::BackUp, Some(name)) => match &doing.left {
            Some(left) => format!("Backing up {name}, {}", lower_first(left)),
            None => format!("Backing up {name}"),
        },
        (JobKind::BackUp, None) => "Backing up".to_string(),
        (JobKind::Check, Some(name)) => format!("Checking {name}"),
        (JobKind::Look | JobKind::LookAgain, Some(name)) => format!("Looking through a source for {name}"),
        (JobKind::Duplicates, Some(name)) => format!("Checking {name} for duplicates"),
        (JobKind::Check, None) => "Checking a backup".to_string(),
        (JobKind::Look | JobKind::LookAgain, None) => "Looking through a source".to_string(),
        (JobKind::Duplicates, None) => "Checking for duplicates".to_string(),
        (JobKind::Survey, _) => "Finding where your photos are".to_string(),
    }
}

/// Which of the jobs going says what Keevoro is doing: a backup first,
/// then a check, a search, a survey, and the check for duplicates last.
pub fn rank(kind: JobKind) -> u8 {
    match kind {
        JobKind::BackUp => 0,
        JobKind::Check => 1,
        JobKind::Look | JobKind::LookAgain => 2,
        JobKind::Survey => 3,
        JobKind::Duplicates => 4,
    }
}

/// When a pause ends, as the line says it: "15:42" today, "tomorrow at
/// 07:00", and the schedule's words further off.
pub fn until_words(until: Timestamp, now: Timestamp, zone: &TimeZone) -> String {
    let (then, today) = (until.to_zoned(zone.clone()), now.to_zoned(zone.clone()));
    let at = rules::time_words(then.time());
    match (then.date() - today.date()).get_days() {
        0 => at,
        1 => format!("tomorrow at {at}"),
        _ => in_a_sentence(&rules::next_words(until, now, zone)),
    }
}

/// The schedule's words for when, to go inside a sentence: "tonight at
/// 02:30", while a day of the week keeps its capital.
pub fn in_a_sentence(words: &str) -> String {
    let opening = words.split_whitespace().next().unwrap_or_default();
    if ["Tonight", "Today", "Tomorrow", "On"].contains(&opening) {
        lower_first(words)
    } else {
        words.to_string()
    }
}

fn lower_first(words: &str) -> String {
    let mut letters = words.chars();
    letters
        .next()
        .map(|c| c.to_lowercase().chain(letters).collect())
        .unwrap_or_default()
}

/// How long a pause from the icon by the clock lasts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseFor {
    AnHour,
    /// Until seven in the morning of the next day.
    UntilTomorrow,
    /// Until the person carries on.
    UntilCarryOn,
}

impl PauseFor {
    /// How it is offered.
    pub fn words(self) -> &'static str {
        match self {
            Self::AnHour => "For an hour",
            Self::UntilTomorrow => "Until tomorrow morning",
            Self::UntilCarryOn => "Until I carry on",
        }
    }

    /// When a pause asked for at `now` ends by itself, if it does. Pure.
    pub fn ends(self, now: Timestamp, zone: &TimeZone) -> Option<Timestamp> {
        match self {
            Self::AnHour => Some(now + jiff::SignedDuration::from_hours(1)),
            Self::UntilTomorrow => {
                let tomorrow = now.to_zoned(zone.clone()).date().tomorrow().ok()?;
                let morning = tomorrow.to_datetime(Time::constant(7, 0, 0, 0));
                zone.to_ambiguous_timestamp(morning).compatible().ok()
            }
            Self::UntilCarryOn => None,
        }
    }
}

/// One backup, as the menu offers it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlanceBackup {
    pub id: String,
    pub name: String,
    /// True while a search, a backup or a check runs on it, here or in
    /// another program, so it cannot be backed up now.
    pub busy: bool,
}

/// Keevoro at a glance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Glance {
    pub mood: Mood,
    /// What it is doing, in one line: "All backed up. Next: tonight at
    /// 02:30", "Backing up Family photos, about 12 minutes left", "Paused",
    /// "Something needs your say".
    pub line: String,
    pub paused: bool,
    /// True while something is going, or some backup has a schedule that
    /// is not paused: then there is something to pause.
    pub pausable: bool,
    /// Every backup, to back up now.
    pub backups: Vec<GlanceBackup>,
    /// The backup going, to stop it: its job and its backup's name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stoppable: Option<(JobId, String)>,
}

impl Keevoro {
    /// Keevoro at a glance, now. `seen_since` is when the person last had
    /// the window open, in seconds since 1970: what a backup by itself held
    /// back after that still needs their say.
    pub fn at_a_glance(&self, seen_since: i64) -> Glance {
        let (now, zone) = (self.clock.now(), self.clock.zone());
        let jobs: Vec<_> = self.jobs(None).into_iter().filter(|j| !j.ended).collect();
        let going = jobs.iter().min_by_key(|j| rank(j.handle.job));
        let doing = going.map(|job| Doing {
            kind: job.handle.job,
            backup: job.handle.backup.as_ref().map(|b| b.name.clone()),
            left: job.estimate.as_ref().map(|e| e.left.clone()).filter(|l| !l.is_empty()),
        });
        let stoppable = jobs
            .iter()
            .find(|j| j.handle.job == JobKind::BackUp)
            .and_then(|j| Some((j.handle.id, j.handle.backup.as_ref()?.name.clone())));
        let busy_on: Vec<&str> = jobs
            .iter()
            .filter(|j| j.handle.job != JobKind::Duplicates)
            .filter_map(|j| j.handle.backup.as_ref().map(|b| b.id.as_str()))
            .collect();
        let mut facts = Facts {
            paused: keevoro_core::pause::is_paused(),
            paused_until: self.paused_until(),
            doing,
            ..Facts::default()
        };
        let mut scheduled = 0;
        let mut ran_well = (0, 0);
        for (named, schedule, record) in self.schedules_known(now) {
            if let Some(last) = record
                .last
                .iter()
                .find(|a| a.outcome != "skipped" && a.outcome != "notDue")
            {
                if last.at > seen_since && NEEDS_YOU_AFTER.contains(&last.outcome.as_str()) {
                    facts.needs_you = true;
                }
                if schedule.is_some() {
                    ran_well.0 += 1;
                    if last.outcome == "worked" {
                        ran_well.1 += 1;
                    }
                }
            }
            let Some(schedule) = schedule else { continue };
            facts.any_schedule = true;
            scheduled += 1;
            let next = match rules::due(&schedule, &record.rules(), &zone, now) {
                Due::NotYet { next } => next,
                Due::Pass { slot, .. } | Due::Now { slot, .. } => Some(slot),
                Due::Paused => None,
            };
            if let Some(at) = next {
                if facts.next.as_ref().is_none_or(|(_, soonest)| at < *soonest) {
                    facts.next = Some((named.name.clone(), at));
                }
            }
        }
        facts.several = scheduled > 1;
        facts.all_well = ran_well.0 > 0 && ran_well.0 == ran_well.1;
        let (mood, line) = said(&facts, now, &zone);
        Glance {
            mood,
            line,
            paused: facts.paused,
            pausable: facts.doing.is_some() || facts.next.is_some(),
            backups: self
                .backups()
                .all()
                .into_iter()
                .map(|b| GlanceBackup {
                    busy: busy_on.contains(&b.id.as_str()),
                    id: b.id,
                    name: b.name,
                })
                .collect(),
            stoppable,
        }
    }

    /// Pauses all of Keevoro's heavy work, as [`Keevoro::pause`] does, until
    /// `until` (it then carries on by itself, when the app next looks), or
    /// until someone carries on.
    pub fn pause_until(&self, until: Option<Timestamp>) {
        self.pause();
        self.paused_until
            .store(until.map_or(0, Timestamp::as_second), Ordering::SeqCst);
    }

    /// When a pause for a while ends by itself, while Keevoro is paused.
    pub fn paused_until(&self) -> Option<Timestamp> {
        let until = self.paused_until.load(Ordering::SeqCst);
        if until == 0 || !keevoro_core::pause::is_paused() {
            return None;
        }
        Timestamp::from_second(until).ok()
    }

    /// Carries on if a pause for a while has run its course by now. True
    /// if it did. The app asks once a minute.
    pub fn carry_on_if_the_pause_is_over(&self) -> bool {
        let Some(until) = self.paused_until() else {
            return false;
        };
        if self.clock.now() < until {
            return false;
        }
        self.carry_on();
        self.log().say("The pause asked for ran its course; carrying on");
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn london() -> TimeZone {
        TimeZone::posix("GMT0BST,M3.5.0/1,M10.5.0").unwrap()
    }

    fn wall(text: &str) -> Timestamp {
        let civil: jiff::civil::DateTime = text.parse().unwrap();
        london().to_ambiguous_timestamp(civil).compatible().unwrap()
    }

    fn line(facts: &Facts) -> (Mood, String) {
        said(facts, wall("2026-06-10T12:00"), &london())
    }

    fn backing_up(left: Option<&str>) -> Doing {
        Doing {
            kind: JobKind::BackUp,
            backup: Some("Family photos".into()),
            left: left.map(str::to_string),
        }
    }

    #[test]
    fn the_line_says_the_next_run_in_the_schedules_own_words() {
        let next = Some(("Family photos".to_string(), wall("2026-06-11T02:30")));
        let calm = Facts {
            next: next.clone(),
            all_well: true,
            any_schedule: true,
            ..Facts::default()
        };
        assert_eq!(
            line(&calm),
            (Mood::Calm, "All backed up. Next: tonight at 02:30".into())
        );
        let several = Facts {
            several: true,
            ..calm.clone()
        };
        assert_eq!(line(&several).1, "All backed up. Next: Family photos tonight at 02:30");
        // Not said to be all backed up until a run has said so.
        let never_ran = Facts {
            all_well: false,
            ..calm.clone()
        };
        assert_eq!(line(&never_ran).1, "Next: tonight at 02:30");
        // A day of the week keeps its capital.
        let sunday = Facts {
            next: Some(("Family photos".into(), wall("2026-06-14T09:00"))),
            ..calm
        };
        assert_eq!(line(&sunday).1, "All backed up. Next: Sunday at 09:00");
        let paused_schedules = Facts {
            any_schedule: true,
            ..Facts::default()
        };
        assert_eq!(line(&paused_schedules).1, "Nothing due: backing up by itself is paused");
        assert_eq!(line(&Facts::default()), (Mood::Calm, "Nothing running now".into()));
    }

    #[test]
    fn a_backup_going_says_how_long_it_has_left_in_the_estimators_words() {
        let working = Facts {
            doing: Some(backing_up(Some("About 12 minutes left"))),
            needs_you: true,
            next: Some(("Family photos".into(), wall("2026-06-11T02:30"))),
            ..Facts::default()
        };
        assert_eq!(
            line(&working),
            (Mood::Working, "Backing up Family photos, about 12 minutes left".into())
        );
        let unsure = Facts {
            doing: Some(backing_up(None)),
            ..Facts::default()
        };
        assert_eq!(line(&unsure).1, "Backing up Family photos");
        for (kind, said) in [
            (JobKind::Check, "Checking Family photos"),
            (JobKind::Look, "Looking through a source for Family photos"),
            (JobKind::LookAgain, "Looking through a source for Family photos"),
            (JobKind::Duplicates, "Checking Family photos for duplicates"),
            (JobKind::Survey, "Finding where your photos are"),
        ] {
            let facts = Facts {
                doing: Some(Doing {
                    kind,
                    backup: Some("Family photos".into()),
                    left: None,
                }),
                ..Facts::default()
            };
            assert_eq!(line(&facts), (Mood::Working, said.to_string()));
        }
        assert!(rank(JobKind::BackUp) < rank(JobKind::Check));
        assert!(rank(JobKind::Survey) < rank(JobKind::Duplicates));
    }

    #[test]
    fn paused_comes_first_and_says_until_when() {
        let paused = Facts {
            paused: true,
            doing: Some(backing_up(Some("About 12 minutes left"))),
            needs_you: true,
            ..Facts::default()
        };
        assert_eq!(line(&paused), (Mood::Paused, "Paused".into()));
        let for_an_hour = Facts {
            paused_until: Some(wall("2026-06-10T13:00")),
            ..paused.clone()
        };
        assert_eq!(line(&for_an_hour).1, "Paused until 13:00");
        let till_tomorrow = Facts {
            paused_until: Some(wall("2026-06-11T07:00")),
            ..paused
        };
        assert_eq!(line(&till_tomorrow).1, "Paused until tomorrow at 07:00");
    }

    #[test]
    fn something_held_back_needs_the_person_until_nothing_is_going() {
        let needs = Facts {
            needs_you: true,
            next: Some(("Family photos".into(), wall("2026-06-11T02:30"))),
            all_well: true,
            ..Facts::default()
        };
        assert_eq!(line(&needs), (Mood::NeedsYou, NEEDS_YOU.to_string()));
    }

    #[test]
    fn a_pause_for_a_while_ends_an_hour_later_or_tomorrow_morning() {
        let now = wall("2026-06-10T22:15");
        assert_eq!(PauseFor::AnHour.ends(now, &london()), Some(wall("2026-06-10T23:15")));
        assert_eq!(
            PauseFor::UntilTomorrow.ends(now, &london()),
            Some(wall("2026-06-11T07:00"))
        );
        // The night the clocks go back, it is still seven by the clock.
        assert_eq!(
            PauseFor::UntilTomorrow.ends(wall("2026-10-24T23:00"), &london()),
            Some(wall("2026-10-25T07:00"))
        );
        assert_eq!(PauseFor::UntilCarryOn.ends(now, &london()), None);
        assert_eq!(PauseFor::UntilTomorrow.words(), "Until tomorrow morning");
    }

    /// A clock the test moves, in London.
    #[derive(Clone)]
    struct Moved(std::sync::Arc<std::sync::Mutex<Timestamp>>);

    impl crate::schedule::Clock for Moved {
        fn now(&self) -> Timestamp {
            *self.0.lock().unwrap()
        }
        fn zone(&self) -> TimeZone {
            london()
        }
    }

    fn ran(keevoro: &Keevoro, named: &crate::Named, at: &str, outcome: &str) {
        let catalog = keevoro_catalog::Catalog::open_in_library(&named.home).unwrap();
        let mut record = crate::schedule::record_of(&catalog).unwrap();
        record.note(crate::schedule::Attempt {
            at: wall(at).as_second(),
            why: "on its schedule".into(),
            outcome: outcome.into(),
            said: String::new(),
        });
        crate::schedule::keep_record(&catalog, &record).unwrap();
        keevoro.schedules.forget(&named.id);
    }

    #[test]
    fn keevoro_at_a_glance_follows_its_schedules_their_runs_and_its_pause() {
        let _turn = crate::jobs::tests::one_at_a_time();
        let dir = tempfile::tempdir().unwrap();
        let clock = Moved(std::sync::Arc::new(std::sync::Mutex::new(wall("2026-06-10T12:00"))));
        let keevoro = crate::keevoro::tests::keevoro(dir.path()).with_clock(clock.clone());
        let named = keevoro.start_backup("Family photos").unwrap();
        let glance = keevoro.at_a_glance(0);
        assert_eq!((glance.mood, glance.line.as_str()), (Mood::Calm, "Nothing running now"));
        assert!(!glance.pausable && !glance.paused && glance.stoppable.is_none());
        assert_eq!(
            glance.backups,
            vec![GlanceBackup {
                id: named.id.clone(),
                name: "Family photos".into(),
                busy: false
            }]
        );

        let request = crate::schedule::SetScheduleRequest {
            every: Some("day".into()),
            at: Some("02:30".into()),
            ..Default::default()
        };
        keevoro.set_schedule(&named, &request).unwrap();
        let glance = keevoro.at_a_glance(0);
        assert_eq!(glance.line, "Next: tonight at 02:30");
        assert!(glance.pausable, "a schedule can be paused");

        ran(&keevoro, &named, "2026-06-10T02:31", "worked");
        assert_eq!(keevoro.at_a_glance(0).line, "All backed up. Next: tonight at 02:30");

        // A run held something back after the window was last open.
        ran(&keevoro, &named, "2026-06-10T11:00", "heldBack");
        let since_before = wall("2026-06-10T10:00").as_second();
        let glance = keevoro.at_a_glance(since_before);
        assert_eq!((glance.mood, glance.line.as_str()), (Mood::NeedsYou, NEEDS_YOU));
        // Once seen, it is calm again, and no longer said to be all backed up.
        let glance = keevoro.at_a_glance(wall("2026-06-10T11:30").as_second());
        assert_eq!(
            (glance.mood, glance.line.as_str()),
            (Mood::Calm, "Next: tonight at 02:30")
        );

        // Paused for an hour from the icon: said, and over by itself.
        keevoro.pause_until(PauseFor::AnHour.ends(wall("2026-06-10T12:00"), &london()));
        let glance = keevoro.at_a_glance(0);
        assert_eq!(
            (glance.mood, glance.line.as_str()),
            (Mood::Paused, "Paused until 13:00")
        );
        assert!(glance.paused);
        assert!(!keevoro.carry_on_if_the_pause_is_over(), "not yet");
        *clock.0.lock().unwrap() = wall("2026-06-10T13:01");
        assert!(keevoro.carry_on_if_the_pause_is_over());
        assert!(!keevoro_core::pause::is_paused());
        assert_eq!(keevoro.paused_until(), None);
        // A pause until someone carries on never ends by itself.
        keevoro.pause_until(None);
        *clock.0.lock().unwrap() = wall("2026-06-12T13:01");
        assert!(!keevoro.carry_on_if_the_pause_is_over());
        assert_eq!(keevoro.at_a_glance(0).line, "Paused");
        keevoro.carry_on();
    }

    #[test]
    fn in_a_sentence_only_lowers_what_is_not_a_name() {
        assert_eq!(in_a_sentence("Tonight at 02:30"), "tonight at 02:30");
        assert_eq!(in_a_sentence("Tomorrow at 02:30"), "tomorrow at 02:30");
        assert_eq!(in_a_sentence("On 31 December at 02:30"), "on 31 December at 02:30");
        assert_eq!(in_a_sentence("Sunday at 09:00"), "Sunday at 09:00");
    }
}
