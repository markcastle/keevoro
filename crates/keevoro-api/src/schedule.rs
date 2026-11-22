//! A backup that runs by itself on a schedule: `schedule.show`,
//! `schedule.set` and `schedule.skip_next`, what Keevoro remembers of each
//! run, and the scheduler the app runs while it is open.
//!
//! Each backup keeps its schedule in its own catalog's settings, so it
//! travels with the backup's record, and older versions leave keys they do
//! not know alone (nothing for this goes in `settings.json`):
//!
//! - `schedule`: the person's choice (`keevoro_core::schedule::Stored`),
//!   written only by `schedule.set`. Missing or empty means none. One that
//!   cannot be read counts as none, is said in the log, and is kept as it
//!   was under `schedule_unreadable`.
//! - `schedule_state`: Keevoro's own record ([`Record`]), written only by
//!   the runs and by `schedule.set` and `schedule.skip_next`.
//!
//! The rules are pure, in `keevoro_core::schedule`, and are given the time
//! and the time zone; only the edges here read the clock.

use crate::backups::Named;
use crate::error::{Error, Result};
use crate::jobs::JobHandle;
use crate::keevoro::Keevoro;
use jiff::tz::TimeZone;
use jiff::Timestamp;
use keevoro_catalog::Catalog;
use keevoro_core::schedule::{self as rules, Due, Every, Schedule};
use keevoro_core::unattended::Occasion;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

/// The catalog setting holding the person's choice.
pub const SCHEDULE: &str = "schedule";
/// Where a choice that could not be read is kept, as it was.
pub const UNREADABLE: &str = "schedule_unreadable";
/// The catalog setting holding Keevoro's record of the runs.
pub const STATE: &str = "schedule_state";

/// How many runs are remembered, for "the last few" on the backup screen.
pub const LAST_KEPT: usize = 10;

/// A backup's schedule is read again from its catalog at most this often
/// by the scheduler, or at once when it is changed here.
pub const READ_AGAIN_EVERY: i64 = 15 * 60;

/// After Keevoro opens, or the computer wakes, nothing is caught up for
/// this long, so a person who has just sat down is not met by a backup.
pub const GRACE: i64 = 5 * 60;

/// A backup that found another program working on it is tried again after
/// this long.
pub const BUSY_RETRY: i64 = 15 * 60;

/// The time and the computer's time zone, read at the edges only: the
/// computer's own, or a test's.
pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
    fn zone(&self) -> TimeZone;
}

/// The computer's own clock and time zone, read each time, so a change
/// of time zone in the system's settings counts at once.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
    fn zone(&self) -> TimeZone {
        TimeZone::system()
    }
}

/// Keevoro's own record of a schedule's runs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    #[serde(default)]
    pub v: u32,
    /// Every slot up to here is accounted for, in seconds since 1970.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handled_through: Option<i64>,
    /// The one slot to skip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip: Option<i64>,
    /// When it last ran by itself, for any reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_ran: Option<i64>,
    /// Set while a run is going; left set by a run cut short when Keevoro
    /// quit or the computer shut down, which the next run says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub running_since: Option<i64>,
    /// The last few runs, newest first, including those that found
    /// nothing new, which the history does not keep.
    #[serde(default)]
    pub last: Vec<Attempt>,
}

/// One run by itself, or one passed over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attempt {
    /// When it ended, in seconds since 1970.
    pub at: i64,
    /// Why it started, in a few words: "on its schedule".
    pub why: String,
    /// How it went, in a word: `worked`, `someFiles`, `unreachable`,
    /// `refused`, `stopped`, `heldBack`, `notNow`, `skipped`.
    pub outcome: String,
    /// What it said, in a line.
    pub said: String,
    /// How many runs due before it were missed (the computer off or
    /// asleep, or Keevoro not running) that it caught up.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub missed: usize,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

impl Record {
    pub(crate) fn rules(&self) -> rules::State {
        rules::State {
            handled_through: self.handled_through,
            skip: self.skip,
            last_ran: self.last_ran,
        }
    }

    /// Notes a run at the top of the last few. One that waited for a good
    /// moment again, for the same reason as the one before, only moves
    /// that one's time on: a laptop on battery all night is one line.
    pub(crate) fn note(&mut self, attempt: Attempt) {
        if let Some(latest) = self.last.first_mut() {
            if attempt.outcome == "notNow" && latest.outcome == "notNow" && latest.said == attempt.said {
                latest.at = attempt.at;
                return;
            }
        }
        self.last.insert(0, attempt);
        self.last.truncate(LAST_KEPT);
    }
}

/// The schedule kept in a catalog, if there is one it can read.
pub(crate) fn schedule_of(catalog: &Catalog) -> Result<Option<Schedule>> {
    let Some(text) = catalog.setting(SCHEDULE)?.filter(|t| !t.trim().is_empty()) else {
        return Ok(None);
    };
    Ok(serde_json::from_str::<rules::Stored>(&text)
        .ok()
        .and_then(|stored| Schedule::from_stored(&stored)))
}

/// The record kept in a catalog; one that cannot be read starts afresh.
pub(crate) fn record_of(catalog: &Catalog) -> Result<Record> {
    Ok(catalog
        .setting(STATE)?
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default())
}

pub(crate) fn keep_record(catalog: &Catalog, record: &Record) -> Result<()> {
    let record = Record { v: 1, ..record.clone() };
    catalog.set_setting(
        STATE,
        &serde_json::to_string(&record).map_err(|e| Error::failed(e.to_string()))?,
    )?;
    Ok(())
}

/// `schedule.set`: a schedule, or none, or pausing and carrying on.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetScheduleRequest {
    /// `day`, `week` or `month`, or `off` for no schedule. Left out, only
    /// `paused` is changed.
    #[serde(default)]
    pub every: Option<String>,
    /// The time, `HH:MM`, in this computer's own time. 02:30 unless given.
    #[serde(default)]
    pub at: Option<String>,
    /// For `week`: `mon` to `sun`. Sunday unless given.
    #[serde(default)]
    pub weekday: Option<String>,
    /// For `month`: the day, 1 to 31 (the last day of shorter months).
    /// The 1st unless given.
    #[serde(default)]
    pub day: Option<u8>,
    /// True to pause it, false to carry on. Carrying on never catches up
    /// what was missed while paused.
    #[serde(default)]
    pub paused: Option<bool>,
}

/// `schedule.skip_next`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkipNextRequest {
    /// True to stop skipping it.
    #[serde(default)]
    pub undo: bool,
}

/// `schedule.show`: when a backup runs by itself, when next, and its last
/// few runs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleView {
    /// The schedule in words: "Every day at 02:30". Absent when there is
    /// none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub every: Option<String>,
    /// The choice as it is kept, for a screen to set its controls from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub choice: Option<rules::Stored>,
    pub paused: bool,
    /// When it next runs, in words ("Tonight at 02:30"), and in seconds
    /// since 1970.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_at: Option<i64>,
    /// True when that next run will be skipped, as asked.
    pub skipping: bool,
    /// The last few runs, newest first.
    pub last: Vec<Attempt>,
    /// True when a choice was kept that could not be read; it is kept as it
    /// was, and counts as none.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub unreadable: bool,
}

/// The view of a backup's schedule at `now` in `zone`.
pub(crate) fn view(catalog: &Catalog, now: Timestamp, zone: &TimeZone) -> Result<ScheduleView> {
    let schedule = schedule_of(catalog)?;
    let record = record_of(catalog)?;
    let unreadable = schedule.is_none() && catalog.setting(SCHEDULE)?.is_some_and(|t| !t.trim().is_empty());
    let Some(schedule) = schedule else {
        return Ok(ScheduleView {
            last: record.last,
            unreadable,
            ..ScheduleView::default()
        });
    };
    let next = match rules::due(&schedule, &record.rules(), zone, now) {
        Due::Paused => None,
        Due::NotYet { next } => next,
        Due::Pass { slot, .. } | Due::Now { slot, .. } => Some(slot),
    };
    Ok(ScheduleView {
        every: Some(rules::every_words(&schedule)),
        choice: Some(schedule.stored()),
        paused: schedule.paused,
        next: next.map(|t| rules::next_words(t, now, zone)),
        next_at: next.map(Timestamp::as_second),
        skipping: next.is_some_and(|t| record.skip == Some(t.as_second())),
        last: record.last,
        unreadable: false,
    })
}

/// The schedule a request asks for, given the one there is.
fn asked_for(request: &SetScheduleRequest, now: Option<Schedule>) -> Result<Option<Schedule>> {
    let not_understood = |what: &str| Error::not_understood(what);
    let Some(every) = request.every.as_deref() else {
        let Some(mut schedule) = now else {
            return Err(Error::refused(
                "This backup has no schedule to pause or carry on. Set one first.",
            ));
        };
        schedule.paused = request.paused.unwrap_or(schedule.paused);
        return Ok(Some(schedule));
    };
    let at = match request.at.as_deref() {
        Some(at) => rules::time_named(at).ok_or_else(|| not_understood("the time should be written like 02:30"))?,
        None => rules::time_named("02:30").unwrap_or_default(),
    };
    let every = match every {
        "off" => return Ok(None),
        "day" => Every::Day,
        "week" => Every::Week(
            rules::weekday_named(request.weekday.as_deref().unwrap_or("sun")).ok_or_else(|| {
                not_understood("the day of the week should be one of mon, tue, wed, thu, fri, sat, sun")
            })?,
        ),
        "month" => match request.day.unwrap_or(1) {
            day @ 1..=31 => Every::Month(day),
            _ => return Err(not_understood("the day of the month should be 1 to 31")),
        },
        _ => return Err(not_understood("every should be day, week, month or off")),
    };
    Ok(Some(Schedule {
        every,
        at,
        paused: request.paused.unwrap_or(false),
    }))
}

/// `schedule.away`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwayRequest {
    /// Since when, in seconds since 1970. Left out, the last `days` days.
    #[serde(default)]
    pub since: Option<i64>,
    /// How many days back, when `since` is left out. 7 unless given.
    #[serde(default)]
    pub days: Option<u32>,
}

/// `schedule.away`: what every backup did by itself since a time, and what
/// is waiting for the person's say. The app shows it as "While you were
/// away" when its window opens; the command line says it with
/// `keevoro away`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AwayView {
    /// Since when, in seconds since 1970.
    pub since: i64,
    /// Each backup that ran by itself, or missed a run, since then.
    pub backups: Vec<AwayBackup>,
    /// What waits for the person's say, a sentence each, newest first.
    pub waiting: Vec<AwayWaiting>,
}

impl AwayView {
    /// True when there is nothing to say.
    pub fn is_empty(&self) -> bool {
        self.backups.is_empty() && self.waiting.is_empty()
    }
}

/// One backup, in "While you were away".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AwayBackup {
    pub backup: String,
    pub id: String,
    /// Its runs since then, oldest first.
    pub runs: Vec<AwayRun>,
    /// A run due that has not happened yet, perhaps because the computer
    /// was off: when it was due, and that it will run once, soon.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub missed: Option<String>,
}

/// One run by itself, in "While you were away".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AwayRun {
    /// When it ended, in seconds since 1970, and in words: "Yesterday at
    /// 02:31".
    pub at: i64,
    pub when: String,
    /// How it went, in a word, as in [`Attempt::outcome`].
    pub outcome: String,
    /// What it said, in a line.
    pub said: String,
    /// What it caught up, when it caught up runs that were missed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caught_up: Option<String>,
}

/// One thing waiting for the person's say.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AwayWaiting {
    pub backup: String,
    pub id: String,
    pub said: String,
}

/// A run due this long ago, in seconds, that has not happened was missed.
const MISSED_AFTER: i64 = 10 * 60;

/// How a run ended that leaves something for the person: what it held
/// back, or a problem only they can put right.
const WAITS_FOR_YOU: &[&str] = &["heldBack"];

/// What one backup did by itself since `since`, from its schedule and
/// record, as "While you were away" says it. Pure.
pub(crate) fn away_of(
    backup: &Named,
    schedule: Option<&Schedule>,
    record: &Record,
    since: i64,
    now: Timestamp,
    zone: &TimeZone,
) -> (Option<AwayBackup>, Option<AwayWaiting>) {
    let mut runs: Vec<AwayRun> = record
        .last
        .iter()
        .filter(|a| a.at > since)
        .map(|a| AwayRun {
            at: a.at,
            when: Timestamp::from_second(a.at).map_or_else(|_| String::new(), |t| rules::when_words(t, now, zone)),
            outcome: a.outcome.clone(),
            said: a.said.clone(),
            caught_up: rules::caught_up_words(a.missed),
        })
        .collect();
    runs.reverse();
    let missed = schedule.and_then(|schedule| match rules::due(schedule, &record.rules(), zone, now) {
        // A run due a moment ago is about to start; one due a while ago did
        // not happen when it should have.
        Due::Now { slot, .. } if now.as_second() - slot.as_second() > MISSED_AFTER => Some(format!(
            "The run due {} didn't happen, perhaps because the computer was off or asleep. It will run once, in a few minutes.",
            rules::when_words(slot, now, zone).to_lowercase()
        )),
        _ => None,
    });
    let waiting = record
        .last
        .iter()
        .find(|a| a.at > since && WAITS_FOR_YOU.contains(&a.outcome.as_str()))
        .map(|a| AwayWaiting {
            backup: backup.name.clone(),
            id: backup.id.clone(),
            said: a.said.clone(),
        });
    let away = (!runs.is_empty() || missed.is_some()).then(|| AwayBackup {
        backup: backup.name.clone(),
        id: backup.id.clone(),
        runs,
        missed,
    });
    (away, waiting)
}

/// What the scheduler remembers of each backup between looks.
#[derive(Default)]
pub(crate) struct Known {
    backups: Mutex<HashMap<String, Seen>>,
}

#[derive(Clone)]
struct Seen {
    schedule: Option<Schedule>,
    record: Record,
    read_at: i64,
    /// Not tried again before this, after finding another program busy.
    not_before: i64,
}

impl Known {
    pub(crate) fn forget(&self, backup: &str) {
        self.backups
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(backup);
    }
}

/// When the scheduler looks: every minute, except within [`GRACE`] of
/// Keevoro opening or the computer waking (a minute's look that comes
/// late by more than a few minutes means it slept). Pure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ticker {
    quiet_until: i64,
    last: i64,
}

impl Ticker {
    pub fn started_at(now: Timestamp) -> Self {
        Self {
            quiet_until: now.as_second() + GRACE,
            last: now.as_second(),
        }
    }

    /// True when it may start runs now.
    pub fn may_run(&mut self, now: Timestamp) -> bool {
        let now = now.as_second();
        // A look a minute after the last that comes three minutes or more
        // late, or one from before the last (the clock was set back), is a
        // wake: the person may have just opened the lid.
        if now - self.last >= 3 * 60 || now < self.last {
            self.quiet_until = now + GRACE;
        }
        self.last = now;
        now >= self.quiet_until
    }
}

impl Keevoro {
    /// `schedule.show`.
    pub fn schedule(&self, backup: &Named) -> Result<ScheduleView> {
        let catalog = Catalog::open_in_library(&backup.home)?;
        view(&catalog, self.clock.now(), &self.clock.zone())
    }

    /// `schedule.set`. Saving a schedule never runs it at once: every slot
    /// up to now is accounted for. Carrying on after a pause does the same,
    /// so a month's pause is never caught up.
    pub fn set_schedule(&self, backup: &Named, request: &SetScheduleRequest) -> Result<ScheduleView> {
        self.set_schedule_at(backup, request, self.clock.now(), &self.clock.zone())
    }

    pub(crate) fn set_schedule_at(
        &self,
        backup: &Named,
        request: &SetScheduleRequest,
        now: Timestamp,
        zone: &TimeZone,
    ) -> Result<ScheduleView> {
        let catalog = Catalog::open_in_library(&backup.home)?;
        let before = schedule_of(&catalog)?;
        let after = asked_for(request, before)?;
        // A choice that could not be read is kept as it was before anything
        // is written over it, so whatever was set there can still be found.
        if let Some(text) = catalog.setting(SCHEDULE)?.filter(|t| !t.trim().is_empty()) {
            if before.is_none() {
                catalog.set_setting(UNREADABLE, &text)?;
                self.log().say(&format!(
                    "\"{}\" had a schedule this version of Keevoro couldn't read; it is kept as {UNREADABLE}",
                    backup.name
                ));
            }
        }
        let mut record = record_of(&catalog)?;
        match &after {
            Some(schedule) => {
                let text = serde_json::to_string(&schedule.stored()).map_err(|e| Error::failed(e.to_string()))?;
                catalog.set_setting(SCHEDULE, &text)?;
            }
            None => catalog.set_setting(SCHEDULE, "")?,
        }
        record.handled_through = Some(now.as_second());
        record.skip = None;
        keep_record(&catalog, &record)?;
        self.schedules.forget(&backup.id);
        self.log().say(&match &after {
            Some(s) if s.paused => format!("Paused \"{}\" backing up by itself", backup.name),
            Some(s) => format!("\"{}\" backs up by itself: {}", backup.name, rules::every_words(s)),
            None => format!("\"{}\" no longer backs up by itself", backup.name),
        });
        view(&catalog, now, zone)
    }

    /// `schedule.skip_next`: skips the next run by itself, or stops
    /// skipping it.
    pub fn skip_next_run(&self, backup: &Named, undo: bool) -> Result<ScheduleView> {
        self.skip_next_run_at(backup, undo, self.clock.now(), &self.clock.zone())
    }

    pub(crate) fn skip_next_run_at(
        &self,
        backup: &Named,
        undo: bool,
        now: Timestamp,
        zone: &TimeZone,
    ) -> Result<ScheduleView> {
        let catalog = Catalog::open_in_library(&backup.home)?;
        let Some(schedule) = schedule_of(&catalog)? else {
            return Err(Error::refused(
                "This backup has no schedule, so there is no next run to skip.",
            ));
        };
        let mut record = record_of(&catalog)?;
        record.skip = if undo {
            None
        } else {
            let shown = view(&catalog, now, zone)?;
            shown
                .next_at
                .or_else(|| rules::next_slot_after(&schedule, zone, now).map(Timestamp::as_second))
        };
        keep_record(&catalog, &record)?;
        self.schedules.forget(&backup.id);
        view(&catalog, now, zone)
    }

    /// `schedule.away`: what every backup did by itself since `since`
    /// (seconds since 1970), or the last `days` days, and what waits for
    /// the person's say. A backup whose catalog cannot be opened now (its
    /// drive unplugged) is left out.
    pub fn away(&self, request: &AwayRequest) -> AwayView {
        let (now, zone) = (self.clock.now(), self.clock.zone());
        let since = request
            .since
            .unwrap_or_else(|| now.as_second() - i64::from(request.days.unwrap_or(7)) * 86_400);
        let mut view = AwayView {
            since,
            ..AwayView::default()
        };
        for backup in self.backups().all() {
            let Some(catalog) = crate::backup::existing_catalog(&backup.home) else {
                continue;
            };
            let schedule = schedule_of(&catalog).ok().flatten();
            let Ok(record) = record_of(&catalog) else { continue };
            let (away, waiting) = away_of(&backup, schedule.as_ref(), &record, since, now, &zone);
            view.backups.extend(away);
            view.waiting.extend(waiting);
        }
        view
    }

    /// The scheduler's look: starts a run by itself for each backup whose
    /// schedule is due at `now` in `zone`, and accounts for a slot passed
    /// over (skipped, or too soon after the last run) without one. Gives
    /// the runs it started. The app calls it every minute while it is open
    /// (see [`Ticker`]); the command line's `--if-due` asks the same rule
    /// for one backup.
    pub fn run_schedules_due(&self) -> Vec<JobHandle> {
        let (now, zone) = (self.clock.now(), self.clock.zone());
        let zone = &zone;
        let mut started = Vec::new();
        for backup in self.backups().all() {
            let Some(seen) = self.seen(&backup, now) else { continue };
            if seen.not_before > now.as_second() {
                continue;
            }
            let Some(schedule) = seen.schedule else { continue };
            match rules::due(&schedule, &seen.record.rules(), zone, now) {
                Due::Paused | Due::NotYet { .. } => {}
                Due::Pass { slot, why } => {
                    if let Err(e) = self.pass_over(&backup, &schedule, slot, why, zone, now) {
                        self.log()
                            .say(&format!("Couldn't note a run of \"{}\" passed over: {e}", backup.name));
                    }
                }
                Due::Now { missed, .. } => {
                    let occasion = if missed > 0 {
                        Occasion::CatchUp
                    } else {
                        Occasion::Schedule
                    };
                    match self.run_by_itself_if_due(&backup, occasion, 0, true, None) {
                        Ok(handle) => started.push(handle),
                        Err(e) if e.kind == crate::Kind::Busy => self.not_before(&backup, now.as_second() + BUSY_RETRY),
                        Err(e) => self
                            .log()
                            .say(&format!("Couldn't back up \"{}\" by itself: {e}", backup.name)),
                    }
                }
            }
        }
        started
    }

    /// Each backup with the schedule and record the scheduler knows for
    /// it, read again from its catalog when old: for saying at a glance
    /// what is next and how the last runs went, at no more cost than the
    /// scheduler's own look. A backup whose catalog cannot be opened (its
    /// drive unplugged, say) is left out.
    pub(crate) fn schedules_known(&self, now: Timestamp) -> Vec<(Named, Option<Schedule>, Record)> {
        self.backups()
            .all()
            .into_iter()
            .filter_map(|backup| {
                let seen = self.seen(&backup, now)?;
                Some((backup, seen.schedule, seen.record))
            })
            .collect()
    }

    /// What the scheduler knows of a backup, read again when old.
    fn seen(&self, backup: &Named, now: Timestamp) -> Option<Seen> {
        let mut all = self
            .schedules
            .backups
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let fresh = all
            .get(&backup.id)
            .is_some_and(|seen| now.as_second() - seen.read_at < READ_AGAIN_EVERY && now.as_second() >= seen.read_at);
        if !fresh {
            // Never made where it is not: a backup removed a moment ago, or
            // one whose drive is unplugged, is passed over.
            let catalog = crate::backup::existing_catalog(&backup.home)?;
            let not_before = all.get(&backup.id).map_or(0, |s| s.not_before);
            all.insert(
                backup.id.clone(),
                Seen {
                    schedule: schedule_of(&catalog).ok().flatten(),
                    record: record_of(&catalog).ok()?,
                    read_at: now.as_second(),
                    not_before,
                },
            );
        }
        all.get(&backup.id).cloned()
    }

    fn not_before(&self, backup: &Named, at: i64) {
        if let Some(seen) = self
            .schedules
            .backups
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_mut(&backup.id)
        {
            seen.not_before = at;
        }
    }

    /// Accounts for a slot passed over without a run, and says so in its
    /// last few.
    fn pass_over(
        &self,
        backup: &Named,
        schedule: &Schedule,
        slot: Timestamp,
        why: rules::Pass,
        zone: &TimeZone,
        now: Timestamp,
    ) -> Result<()> {
        let catalog = Catalog::open_in_library(&backup.home)?;
        let mut record = record_of(&catalog)?;
        record.handled_through = Some(rules::handled_after_run(schedule, zone, slot, now));
        if why == rules::Pass::Skipped {
            record.skip = None;
        }
        record.note(Attempt {
            at: now.as_second(),
            why: Occasion::Schedule.words().to_string(),
            outcome: "skipped".into(),
            said: match why {
                rules::Pass::Skipped => "Skipped, as you asked.".into(),
                rules::Pass::TooSoon => "Not run again so soon after the last run.".into(),
            },
            missed: 0,
        });
        keep_record(&catalog, &record)?;
        self.schedules.forget(&backup.id);
        self.log().say(&format!(
            "\"{}\": the run due by its schedule was passed over: {}",
            backup.name, record.last[0].said
        ));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::tests::{card_going_to_usb_for, one_at_a_time, plugged_in_and_quiet, Says};
    use std::sync::Arc;

    /// A clock the test moves, in London's time as POSIX writes it.
    #[derive(Clone)]
    struct Moved(Arc<Mutex<Timestamp>>);

    impl Moved {
        fn at(text: &str) -> Self {
            Self(Arc::new(Mutex::new(wall(text))))
        }
        fn to(&self, text: &str) {
            *self.0.lock().unwrap() = wall(text);
        }
    }

    impl Clock for Moved {
        fn now(&self) -> Timestamp {
            *self.0.lock().unwrap()
        }
        fn zone(&self) -> TimeZone {
            london()
        }
    }

    fn london() -> TimeZone {
        TimeZone::posix("GMT0BST,M3.5.0/1,M10.5.0").unwrap()
    }

    fn wall(text: &str) -> Timestamp {
        let civil: jiff::civil::DateTime = text.parse().unwrap();
        london().to_ambiguous_timestamp(civil).compatible().unwrap()
    }

    fn set(every: &str, at: &str) -> SetScheduleRequest {
        SetScheduleRequest {
            every: Some(every.into()),
            at: Some(at.into()),
            ..SetScheduleRequest::default()
        }
    }

    #[test]
    fn a_schedule_is_set_shown_skipped_paused_and_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let clock = Moved::at("2026-06-10T12:00");
        let keevoro = crate::keevoro::tests::keevoro(dir.path()).with_clock(clock.clone());
        let named = keevoro.start_backup("Family").unwrap();
        assert_eq!(
            keevoro.schedule(&named).unwrap(),
            ScheduleView::default(),
            "none until set"
        );

        let shown = keevoro.set_schedule(&named, &set("day", "02:30")).unwrap();
        assert_eq!(shown.every.as_deref(), Some("Every day at 02:30"));
        assert_eq!(shown.next.as_deref(), Some("Tonight at 02:30"));
        assert_eq!(shown.next_at, Some(wall("2026-06-11T02:30").as_second()));
        assert!(!shown.paused && !shown.skipping);

        assert!(keevoro.skip_next_run(&named, false).unwrap().skipping);
        assert!(!keevoro.skip_next_run(&named, true).unwrap().skipping);

        let paused = keevoro
            .set_schedule(
                &named,
                &SetScheduleRequest {
                    paused: Some(true),
                    ..SetScheduleRequest::default()
                },
            )
            .unwrap();
        assert!(paused.paused && paused.next.is_none());
        assert_eq!(paused.every.as_deref(), Some("Every day at 02:30"));

        let weekly = keevoro
            .set_schedule(
                &named,
                &SetScheduleRequest {
                    weekday: Some("sun".into()),
                    ..set("week", "09:00")
                },
            )
            .unwrap();
        assert_eq!(weekly.every.as_deref(), Some("Every week on Sunday at 09:00"));
        assert_eq!(weekly.next.as_deref(), Some("Sunday at 09:00"));

        let off = keevoro.set_schedule(&named, &set("off", "00:00")).unwrap();
        assert_eq!(off.every, None);
        for wrong in [set("fortnight", "02:30"), set("day", "25:61")] {
            let e = keevoro.set_schedule(&named, &wrong).unwrap_err();
            assert_eq!(e.kind, crate::Kind::NotUnderstood, "{e:?}");
        }
        let e = keevoro.skip_next_run(&named, false).unwrap_err();
        assert_eq!(e.kind, crate::Kind::Refused);
    }

    #[test]
    fn a_schedule_this_version_cannot_read_counts_as_none_and_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let keevoro = crate::keevoro::tests::keevoro(dir.path()).with_clock(Moved::at("2026-06-10T12:00"));
        let named = keevoro.start_backup("Family").unwrap();
        let catalog = Catalog::open_in_library(&named.home).unwrap();
        catalog.set_setting(SCHEDULE, r#"{"v":2,"every":"fortnight"}"#).unwrap();
        let shown = keevoro.schedule(&named).unwrap();
        assert!(shown.unreadable && shown.every.is_none());
        assert_eq!(keevoro.run_schedules_due(), Vec::new());
        keevoro.set_schedule(&named, &set("day", "02:30")).unwrap();
        assert_eq!(
            catalog.setting(UNREADABLE).unwrap().as_deref(),
            Some(r#"{"v":2,"every":"fortnight"}"#)
        );
    }

    #[test]
    fn the_scheduler_runs_a_backup_when_its_schedule_comes_round_once_however_many_were_missed() {
        let _turn = one_at_a_time();
        let dir = tempfile::tempdir().unwrap();
        let clock = Moved::at("2026-06-10T12:00");
        let keevoro = crate::jobs::tests::keevoro_for_tests(dir.path())
            .with_clock(clock.clone())
            .with_signals(plugged_in_and_quiet());
        let (named, _, usb) = card_going_to_usb_for(dir.path(), &keevoro);
        Catalog::open_in_library(&named.home)
            .unwrap()
            .set_arrange(keevoro_core::Arrange::ByDate)
            .unwrap();
        keevoro.set_schedule(&named, &set("day", "02:30")).unwrap();
        clock.to("2026-06-10T23:00");
        assert!(keevoro.run_schedules_due().is_empty(), "not yet");

        // Off for three nights, and the last run by itself was cut short
        // when the computer shut down: one run catches up, and says both.
        let catalog = Catalog::open_in_library(&named.home).unwrap();
        let mut record = record_of(&catalog).unwrap();
        record.running_since = Some(wall("2026-06-09T02:30").as_second());
        keep_record(&catalog, &record).unwrap();
        clock.to("2026-06-13T08:00");
        let started = keevoro.run_schedules_due();
        assert_eq!(started.len(), 1);
        assert_eq!(started[0].op, Some("schedule.run"));
        keevoro.wait(started[0].id);
        keevoro.wait_for_jobs();
        assert!(usb.join("originals").is_dir(), "it backed up");
        let said = crate::backup::Backup::open(&named.home)
            .unwrap()
            .last_summary()
            .unwrap()
            .unwrap();
        let said = said["summary"].to_string();
        assert!(said.contains("Catching up: 2 runs did not happen"), "{said}");
        assert!(said.contains("was cut short"), "{said}");
        assert_eq!(record_of(&catalog).unwrap().running_since, None);
        let shown = keevoro.schedule(&named).unwrap();
        assert_eq!(shown.last.len(), 1);
        assert_eq!(
            (shown.last[0].why.as_str(), shown.last[0].outcome.as_str()),
            ("to catch up a run that was missed", "worked")
        );
        assert_eq!(shown.next.as_deref(), Some("Tonight at 02:30"));
        assert!(
            keevoro.run_schedules_due().is_empty(),
            "once, not once per night missed"
        );
        // While you were away: the one run, and what it caught up.
        let away = keevoro.away(&AwayRequest {
            since: Some(wall("2026-06-10T23:00").as_second()),
            days: None,
        });
        assert_eq!(away.backups.len(), 1, "{away:?}");
        let run = &away.backups[0].runs[0];
        assert_eq!((run.when.as_str(), run.outcome.as_str()), ("Today at 08:00", "worked"));
        assert_eq!(
            run.caught_up.as_deref(),
            Some("This caught up 2 runs that were missed, perhaps because the computer was off or asleep.")
        );
        assert_eq!(away.backups[0].missed, None, "nothing more to catch up");
        assert!(away.waiting.is_empty());

        // Skipped, as asked: accounted for, and said, with no run.
        keevoro.skip_next_run(&named, false).unwrap();
        clock.to("2026-06-14T03:00");
        assert!(keevoro.run_schedules_due().is_empty());
        let shown = keevoro.schedule(&named).unwrap();
        assert_eq!(
            (shown.last[0].outcome.as_str(), shown.last[0].said.as_str()),
            ("skipped", "Skipped, as you asked.")
        );
        assert!(!shown.skipping);

        // Another program holds the one-at-a-time lock: tried again later.
        let other = crate::jobs::tests::keevoro_for_tests(dir.path()).with_signals(Says(
            keevoro_core::background::StartFacts {
                on_battery: Some(true),
                ..Default::default()
            },
        ));
        let waiting = other.run_by_itself(&named, Occasion::CommandLine, 1, None).unwrap();
        clock.to("2026-06-15T03:00");
        assert!(keevoro.run_schedules_due().is_empty(), "busy elsewhere");
        other.stop(crate::Which::Job(waiting.id));
        other.wait(waiting.id);
        other.wait_for_jobs();
        clock.to("2026-06-15T03:05");
        assert!(keevoro.run_schedules_due().is_empty(), "not straight away");
        clock.to("2026-06-15T03:20");
        let again = keevoro.run_schedules_due();
        assert_eq!(again.len(), 1, "fifteen minutes later");
        keevoro.wait(again[0].id);
        keevoro.wait_for_jobs();
    }

    /// How long the scheduler's look takes with twenty backups, each with
    /// a schedule: the first reads every catalog, the next only memory.
    /// Run by hand: `cargo test -p keevoro-api how_long_a_look -- --ignored --nocapture`.
    #[test]
    #[ignore = "a measurement, run by hand"]
    fn how_long_a_look_at_twenty_schedules_takes() {
        let dir = tempfile::tempdir().unwrap();
        let clock = Moved::at("2026-06-10T12:00");
        let keevoro = crate::keevoro::tests::keevoro(dir.path()).with_clock(clock.clone());
        for n in 0..20 {
            let named = keevoro.start_backup(&format!("Backup {n}")).unwrap();
            keevoro.set_schedule(&named, &set("day", "02:30")).unwrap();
        }
        clock.to("2026-06-10T12:01");
        let started = std::time::Instant::now();
        assert!(keevoro.run_schedules_due().is_empty());
        let first = started.elapsed();
        let started = std::time::Instant::now();
        for _ in 0..100 {
            keevoro.run_schedules_due();
        }
        println!(
            "The first look read 20 catalogs in {first:?}; each look after that took {:?}",
            started.elapsed() / 100
        );
    }

    #[test]
    fn the_scheduler_waits_a_while_after_keevoro_opens_or_the_computer_wakes() {
        let mut ticker = Ticker::started_at(wall("2026-06-10T08:00"));
        assert!(!ticker.may_run(wall("2026-06-10T08:01")));
        assert!(!ticker.may_run(wall("2026-06-10T08:03")));
        assert!(ticker.may_run(wall("2026-06-10T08:05")));
        assert!(ticker.may_run(wall("2026-06-10T08:06")));
        // Asleep overnight: the first look after waking is late.
        assert!(!ticker.may_run(wall("2026-06-11T07:00")));
        assert!(!ticker.may_run(wall("2026-06-11T07:02")));
        assert!(!ticker.may_run(wall("2026-06-11T07:04")));
        assert!(ticker.may_run(wall("2026-06-11T07:05")));
        // The clock set back is treated as a wake too.
        assert!(!ticker.may_run(wall("2026-06-11T06:00")));
    }

    fn named(name: &str) -> Named {
        Named {
            id: "backup-1".into(),
            name: name.into(),
            home: std::path::PathBuf::from("unused"),
        }
    }

    fn attempt(at: &str, outcome: &str, said: &str) -> Attempt {
        Attempt {
            at: wall(at).as_second(),
            why: "on its schedule".into(),
            outcome: outcome.into(),
            said: said.into(),
            missed: 0,
        }
    }

    #[test]
    fn while_you_were_away_says_each_run_since_what_was_missed_and_what_waits() {
        let schedule = Schedule {
            every: Every::Day,
            at: rules::time_named("02:30").unwrap(),
            paused: false,
        };
        let mut record = Record {
            handled_through: Some(wall("2026-06-09T02:30").as_second()),
            ..Record::default()
        };
        record.note(attempt("2026-06-08T02:31", "worked", "Long ago."));
        record.note(attempt(
            "2026-06-09T02:40",
            "heldBack",
            "4 new pictures were set aside as probably not photos, and not backed up.",
        ));
        let since = wall("2026-06-08T12:00").as_second();
        let now = wall("2026-06-11T09:00");
        let (away, waiting) = away_of(&named("Family photos"), Some(&schedule), &record, since, now, &london());
        let away = away.unwrap();
        assert_eq!(away.runs.len(), 1, "only since then");
        assert_eq!(away.runs[0].when, "Tuesday at 02:40");
        assert_eq!(
            away.missed.as_deref(),
            Some("The run due today at 02:30 didn't happen, perhaps because the computer was off or asleep. It will run once, in a few minutes.")
        );
        assert_eq!(
            waiting.unwrap().said,
            "4 new pictures were set aside as probably not photos, and not backed up."
        );

        // Nothing since: nothing to say.
        let later = wall("2026-06-12T00:00").as_second();
        let record = Record {
            handled_through: Some(now.as_second()),
            ..record
        };
        assert_eq!(
            away_of(&named("Family photos"), Some(&schedule), &record, later, now, &london()),
            (None, None)
        );
    }

    #[test]
    fn a_record_kept_before_runs_said_what_they_caught_up_reads_as_catching_up_nothing() {
        let kept = r#"{"v":1,"handledThrough":5,"last":[{"at":4,"why":"on its schedule","outcome":"worked","said":"Nothing new to back up."}]}"#;
        let record: Record = serde_json::from_str(kept).unwrap();
        assert_eq!(record.last[0].missed, 0);
        assert!(
            !serde_json::to_string(&record).unwrap().contains("missed"),
            "nothing new written"
        );
    }

    #[test]
    fn a_run_that_waits_again_for_the_same_reason_is_one_line_in_the_last_few() {
        let mut record = Record::default();
        let waiting = "Not backed up then. Waiting while this computer is on battery.";
        record.note(attempt("2026-06-10T02:30", "notNow", waiting));
        record.note(attempt("2026-06-10T02:31", "notNow", waiting));
        record.note(attempt("2026-06-10T02:32", "notNow", waiting));
        assert_eq!(record.last.len(), 1);
        assert_eq!(record.last[0].at, wall("2026-06-10T02:32").as_second());
        record.note(attempt(
            "2026-06-10T03:00",
            "worked",
            "Safely backed up 23 files (41.0 MB).",
        ));
        record.note(attempt("2026-06-11T02:30", "notNow", waiting));
        assert_eq!(record.last.len(), 3);
    }
}
