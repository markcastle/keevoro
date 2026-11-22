//! `schedule.run`: a backup that runs by itself, with nobody watching.
//!
//! It is the jobs a person would run, one after another, in one job (an
//! ordinary backup to every other program, marked with why it started):
//! wait for a good moment, look through the sources again, hold back what
//! needs the person's say, send what is new, and write down what happened.
//! The rules for what waits are pure, in `keevoro_core::unattended` and
//! `keevoro_core::background::may_start`; this gathers the facts.
//!
//! Every way it could do the wrong thing with nobody there is stopped or
//! said:
//!
//! - a source that is not plugged in is passed over and said, and what was
//!   found on it is kept (`looking::sources_to_look_again`);
//! - another drive at a source's place is passed over, never taken for it;
//! - a destination that looks emptied or replaced is not sent to
//!   (`HowToSend::unattended` in the engine);
//! - a backup never asked how to arrange its files, or reading from cloud
//!   storage that costs about £1 or more, sends nothing;
//! - on battery, or while the computer is busy, it waits, and says why;
//! - only one runs at a time on the computer (`unattended.lock`), and the
//!   usual lock keeps every other program off the backup while it runs.

use super::backing_up::{send_and_record, ByItselfSoFar};
use super::looking::{self, ToLook};
use super::{spawn, Context, Job, JobHandle, JobKind, Listener, Outcome, Said, SendEvent};
use crate::backup::Backup;
use crate::backups::Named;
use crate::error::{Error, Result};
use crate::keevoro::Keevoro;
use keevoro_catalog::Catalog;
use keevoro_core::background::{may_start, StartFacts};
use keevoro_core::estimate::Tracker;
use keevoro_core::unattended::{what_waits, Facts, Occasion};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The file in Keevoro's data folder held for the length of every backup
/// that runs by itself, so only one runs at a time on the computer. The
/// operating system lets go of it if the program stops, so it is never
/// left taken.
pub const LOCK: &str = "unattended.lock";

/// How often a run that waits for a good moment looks again.
const LOOK_AGAIN_EVERY: Duration = Duration::from_secs(30);

/// What the computer says about itself, for a backup that runs by itself.
/// Only `on_battery`, `others_cpu` and `metered` are filled in; the run
/// fills in the rest.
pub trait Signals: Send + Sync {
    fn now(&self) -> StartFacts;
}

/// The real computer. Made when first asked, so opening Keevoro costs
/// nothing; the first answer takes five readings a second apart, so a
/// moment's spike is not taken for a busy computer, and each later one a
/// single fresh reading, averaged with the last few.
#[derive(Default)]
pub struct ThisComputer(std::sync::Mutex<Option<(keevoro_engine::background::Computer, usize)>>);

impl Signals for ThisComputer {
    fn now(&self) -> StartFacts {
        let mut held = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (computer, readings) = held.get_or_insert_with(|| (keevoro_engine::background::Computer::new(), 0));
        let mut conditions = computer.conditions(false, 1);
        *readings += 1;
        while *readings < 5 {
            std::thread::sleep(Duration::from_secs(1));
            conditions = computer.conditions(false, 1);
            *readings += 1;
        }
        StartFacts {
            on_battery: conditions.on_battery,
            others_cpu: conditions.others_cpu,
            // Not known yet on any computer: never a reason to wait.
            metered: None,
            ..StartFacts::default()
        }
    }
}

/// The computer-wide lock, held while a backup runs by itself.
pub(crate) struct OneAtATime(#[allow(dead_code)] std::fs::File);

impl OneAtATime {
    /// Takes it, or `None` when another run holds it.
    fn take(data: &Path) -> Result<Option<Self>> {
        let failed =
            |e: std::io::Error| Error::failed(format!("Keevoro couldn't note that it is backing up by itself: {e}."));
        std::fs::create_dir_all(data).map_err(failed)?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(data.join(LOCK))
            .map_err(failed)?;
        match fs4::FileExt::try_lock(&file) {
            Ok(()) => Ok(Some(Self(file))),
            Err(_) => Ok(None),
        }
    }
}

/// Refused while another backup runs by itself.
pub const ONE_AT_A_TIME: &str =
    "Keevoro is already backing up by itself, and runs one backup at a time like that. Try again when it has finished.";

impl Keevoro {
    /// `schedule.run`: backs up by itself. Waits at most `wait_minutes`
    /// for a good moment (none means it gives up for now at once).
    pub fn run_by_itself(
        &self,
        backup: &Named,
        occasion: Occasion,
        wait_minutes: u32,
        listener: Option<Listener>,
    ) -> Result<JobHandle> {
        let lock = OneAtATime::take(&self.data)?.ok_or_else(|| Error::busy(ONE_AT_A_TIME))?;
        let ctx = self.context();
        let job = ctx.jobs.begin_by_itself(backup, occasion, listener)?;
        let handle = job.handle.clone();
        let tracker = Tracker::new(crate::rates::remembered(&ctx.rates));
        job.track(tracker.clone());
        let opening = "Getting ready to back up by itself";
        job.note_line(opening);
        ctx.jobs.heard(&job, Said::Send(SendEvent::progress(opening)), true);
        let _ = Backup::open(&backup.home).and_then(|b| b.forget_last_summary());
        ctx.log.say(&format!(
            "Backing up \"{}\" by itself, {}",
            backup.name,
            occasion.words()
        ));
        let run = Run {
            backup: backup.clone(),
            occasion,
            wait: Duration::from_secs(u64::from(wait_minutes) * 60),
            signals: self.signals.clone(),
            window_used: self.window_used.clone(),
            how: self.how_to_look(backup),
        };
        spawn(&ctx, &job, move |ctx, job, ending| {
            let _lock = lock;
            // A guest's work: everything else on the computer comes first.
            keevoro_frame::quiet::lower_this_threads_priority();
            let last = run.go(ctx, job, &tracker);
            super::duplicates::start(ctx, &run.backup, None, super::duplicates::Why::AfterRunningByItself);
            ending.with(Some(last));
        })?;
        Ok(handle)
    }
}

/// One run, and what it needs on its thread.
struct Run {
    backup: Named,
    occasion: Occasion,
    wait: Duration,
    signals: Arc<dyn Signals>,
    window_used: Arc<AtomicI64>,
    how: looking::HowToLook,
}

impl Run {
    fn go(&self, ctx: &Context, job: &Arc<Job>, tracker: &Tracker) -> Said {
        if let Some(not_now) = self.wait_for_a_good_moment(job) {
            return self.end(ctx, job, not_now);
        }
        if job.stopped() {
            return self.end(ctx, job, Ending::Stopped);
        }
        let home = &self.backup.home;
        let found_before = found_paths(home);
        let to_look = looking::what_to_look_again(&self.backup, None, None)
            .map(|(to_look, _)| to_look)
            .unwrap_or_else(|e| ToLook {
                passed_over: vec![e.message],
                ..ToLook::default()
            });
        let mut looked_again = Vec::new();
        let mut passed_over = to_look.passed_over.clone();
        for step in &to_look.steps {
            match looking::get_ready(ctx, home, step) {
                Ok(said) => passed_over.extend(said),
                Err(e) => {
                    passed_over.push(e.message);
                    continue;
                }
            }
            let place = looking::place_of(step);
            let said = looking::look_through(ctx, job, home, place, &self.how.kinds, &self.how.skip);
            let real = std::fs::canonicalize(place).unwrap_or_else(|_| place.to_path_buf());
            keevoro_engine::surveys::mark_out_of_date(&ctx.cache, &real);
            if said.stopped || job.stopped() {
                return self.end(ctx, job, Ending::Stopped);
            }
            if let Some(problem) = &said.problem {
                passed_over.push(format!("{} couldn't be looked through: {problem}", said.name));
            } else if let Some(unplugged) = &said.unplugged {
                passed_over.push(unplugged.clone());
                looked_again.push(said.name.clone());
            } else {
                looked_again.push(said.name.clone());
            }
            ctx.jobs.heard(job, Said::Scan(said), true);
        }
        for different in &to_look.different {
            ctx.log.say(&different.said);
        }
        let (facts, found) = match self.facts(&to_look, &found_before) {
            Ok(facts) => facts,
            Err(e) => return self.end(ctx, job, Ending::Problem(e.message)),
        };
        let waits = what_waits(&facts);
        let record = keevoro_engine::history::ByItself {
            why: self.occasion.words().to_string(),
            looked_again,
            passed_over: passed_over.clone(),
            found,
            waiting: Vec::new(),
        };
        if waits.iter().any(|w| w.holds_the_run) {
            let waiting: Vec<String> = waits.iter().map(|w| w.said.clone()).collect();
            return self.end(ctx, job, Ending::HeldBack { record, waiting });
        }
        send_and_record(
            ctx,
            job,
            &self.backup,
            tracker,
            JobKind::BackUp,
            Some(ByItselfSoFar {
                record,
                said_first: passed_over,
                waiting: waits.into_iter().map(|w| w.said).collect(),
            }),
        )
    }

    /// Waits while the computer is on battery or busy, or the person is in
    /// Keevoro's window, for at most the time allowed. Gives why it did not
    /// run now, if it gave up.
    fn wait_for_a_good_moment(&self, job: &Arc<Job>) -> Option<Ending> {
        let started = Instant::now();
        loop {
            let facts = StartFacts {
                needs_internet: needs_internet(&self.backup.home),
                window_used: self.window_used_ago(),
                person_asked: self.occasion.person_asked(),
                ..self.signals.now()
            };
            let Err(wait) = may_start(&facts) else {
                return None;
            };
            if job.stopped() {
                return Some(Ending::Stopped);
            }
            if started.elapsed() >= self.wait {
                return Some(Ending::NotNow(wait.words().to_string()));
            }
            job.note_line(wait.words());
            let until = Instant::now() + LOOK_AGAIN_EVERY;
            while Instant::now() < until && !job.stopped() {
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    }

    fn window_used_ago(&self) -> Option<u64> {
        let at = self.window_used.load(Ordering::SeqCst);
        (at > 0).then(|| u64::try_from(super::seconds_now().saturating_sub(at)).unwrap_or(0))
    }

    /// What the run knows before it sends anything, and what looking again
    /// found new, in words.
    fn facts(&self, to_look: &ToLook, before: &HashSet<PathBuf>) -> Result<(Facts, Vec<String>)> {
        let catalog = Catalog::open_in_library(&self.backup.home)?;
        let reports = catalog.load_reports().map_err(Error::from)?;
        let mut copies: HashMap<&str, usize> = HashMap::new();
        for item in reports.iter().flat_map(|r| &r.items) {
            if let Some(hash) = item.blake3.as_deref() {
                *copies.entry(hash).or_default() += 1;
            }
        }
        let (mut photos, mut videos, mut others, mut set_aside, mut new_copies) = (0, 0, 0, 0, 0);
        for item in reports
            .iter()
            .flat_map(|r| &r.items)
            .filter(|i| !before.contains(&i.path))
        {
            match item.kind {
                keevoro_core::MediaKind::Photo => photos += 1,
                keevoro_core::MediaKind::Video => videos += 1,
                keevoro_core::MediaKind::Other => others += 1,
            }
            if item.junk.is_some() && !keevoro_engine::wanted(&catalog, item).unwrap_or(false) {
                set_aside += 1;
            }
            if item
                .blake3
                .as_deref()
                .is_some_and(|h| copies.get(h).copied().unwrap_or(0) > 1)
            {
                new_copies += 1;
            }
        }
        let found = if photos + videos + others == 0 {
            vec!["Nothing new was found.".to_string()]
        } else {
            vec![format!(
                "New since the last look: {}.",
                keevoro_core::words::files(photos, videos, others)
            )]
        };
        let reads_a_bucket = catalog.sources()?.iter().any(|s| s.bucket.is_some());
        let cost = if reads_a_bucket {
            let discover = keevoro_engine::discover(&self.backup.home, &catalog).map_err(Error::from)?;
            #[allow(clippy::cast_precision_loss)]
            let pounds = discover.from_cloud.iter().map(|c| c.pence).sum::<u64>() as f64 / 100.0;
            let said: Vec<String> = discover.from_cloud.iter().map(|c| c.cost.clone()).collect();
            (!discover.from_cloud.is_empty()).then(|| (pounds, said.join(" ")))
        } else {
            None
        };
        Ok((
            Facts {
                backup: self.backup.name.clone(),
                arrange_asked: catalog.arrange()?.is_some(),
                new_set_aside: set_aside,
                new_copies,
                different_drives: to_look.different.iter().map(|d| d.name.clone()).collect(),
                cost,
            },
            found,
        ))
    }

    /// Ends a run that sent nothing, saying why.
    fn end(&self, ctx: &Context, job: &Arc<Job>, how: Ending) -> Said {
        ctx.jobs.release(job);
        let (said, outcome, problem) = match &how {
            Ending::NotNow(why) => (format!("Not backed up now. {why}."), Outcome::Stopped, None),
            Ending::Stopped => (keevoro_engine::send::STOPPED.to_string(), Outcome::Stopped, None),
            Ending::Problem(problem) => (String::new(), Outcome::Refused, Some(problem.clone())),
            Ending::HeldBack { waiting, .. } => (
                format!("Nothing was backed up. {}", waiting.join(" ")),
                Outcome::Worked,
                None,
            ),
        };
        let mut last = SendEvent::finished(said.as_bytes(), problem, outcome);
        match how {
            Ending::NotNow(why) => last.not_now = Some(why),
            Ending::HeldBack { mut record, waiting } => {
                record.waiting.clone_from(&waiting);
                last.held_back = waiting;
                if let Err(e) = record_held_back(&self.backup.home, job, record, &last) {
                    ctx.log.say(&format!("Couldn't write down how this backup went: {e:#}"));
                }
            }
            Ending::Stopped | Ending::Problem(_) => {}
        }
        ctx.log.say(&format!(
            "Backing up \"{}\" by itself ended: {}",
            self.backup.name,
            last.summary.join(" ")
        ));
        let _ = Backup::open(&self.backup.home).and_then(|b| b.remember_last_summary(&last));
        let last = Said::Send(last);
        ctx.jobs.heard(job, last.clone(), true);
        last
    }
}

/// How a run that sent nothing ended.
enum Ending {
    /// Not now: on battery, busy, or the person in Keevoro, past the wait.
    NotNow(String),
    Stopped,
    Problem(String),
    /// Something that needs the person's say holds the whole run back.
    HeldBack {
        record: keevoro_engine::history::ByItself,
        waiting: Vec<String>,
    },
}

/// Every file found on the backup's sources now, by where it is.
fn found_paths(home: &Path) -> HashSet<PathBuf> {
    Catalog::open_in_library(home)
        .and_then(|c| c.load_reports_as_found())
        .map(|reports| reports.into_iter().flat_map(|r| r.items).map(|i| i.path).collect())
        .unwrap_or_default()
}

/// True when the backup reads or sends over the internet: a destination in
/// cloud storage or across a network, or a bucket source.
fn needs_internet(home: &Path) -> bool {
    let Ok(catalog) = Catalog::open_in_library(home) else {
        return false;
    };
    let cloud = keevoro_engine::connect::destinations(&catalog)
        .map(|all| all.iter().any(|d| d.kind != "folder"))
        .unwrap_or(false);
    cloud
        || catalog
            .sources()
            .is_ok_and(|all| all.iter().any(|s| s.bucket.is_some()))
}

/// Writes a run held back into the backup's history, so it can be looked
/// back on.
fn record_held_back(
    home: &Path,
    job: &Job,
    record: keevoro_engine::history::ByItself,
    last: &SendEvent,
) -> anyhow::Result<()> {
    use keevoro_engine::history::{Ended, RunDetail, Sent};
    let catalog = Catalog::open_in_library(home)?;
    let to: Vec<String> = keevoro_engine::connect::destinations(&catalog)?
        .iter()
        .map(keevoro_engine::connect::Saved::describe)
        .collect();
    let now = super::seconds_now();
    let entry = Sent {
        finished_at: now,
        seconds: u64::try_from(now.saturating_sub(job.handle.started_at)).unwrap_or(0),
        bytes: 0,
        to: to.join(" and "),
        stopped: false,
        detail: Some(Box::new(RunDetail {
            started_at: job.handle.started_at,
            paused_seconds: 0,
            ended: Ended::Finished,
            problem: None,
            started_by: job.handle.started_by.name().replace("the Keevoro app", "the app"),
            version: env!("CARGO_PKG_VERSION").to_string(),
            destinations: Vec::new(),
            stages: Vec::new(),
            duplicates: 0,
            duplicate_bytes: 0,
            previews: None,
            summary: last.summary.clone(),
            by_itself: Some(record),
        })),
    };
    keevoro_engine::history::record(&catalog, &entry)
}
