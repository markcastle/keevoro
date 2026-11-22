//! The background check for duplicates: reading in full the files a quick
//! search only sampled, so that true duplicates are known and a backup has
//! less to do.
//!
//! Only one runs at a time. It gives way to a search and to a backup: a
//! backup reads the same files, and fingerprints them as it goes, so
//! checking alongside it would read them twice over. Each of those starts
//! it again when it ends, and it carries on from where it was.
//!
//! After a search an AI agent asked for, it reads only files in folders
//! that agent may look in: the agent started it, so it is held to the
//! agent's folders like the search was. What it leaves is read by the
//! next check started any other way, or by a backup, which reads what it
//! needs as it sends.

use super::{spawn, CheckEvent, Context, Every, JobHandle, JobKind, Said};
use crate::agents::AgentAccess;
use crate::backups::Named;
use keevoro_catalog::Catalog;
use keevoro_engine::friendly_path;
use std::cell::Cell;
use std::time::{Duration, Instant};

/// How often the check looks to see whether another program has started
/// working on its backup. Between looks it trusts the last answer, so a
/// file is not read from disk between every two photos.
const LOOK_ELSEWHERE_EVERY: Duration = Duration::from_millis(500);

/// Whether the check should give way: asked between every two files. Work
/// in this program is asked about every time, which costs nothing; another
/// program, every so often.
struct GiveWayWhen<'a> {
    ctx: &'a Context,
    backup: &'a str,
    looked: Cell<Option<(Instant, bool)>>,
}

impl<'a> GiveWayWhen<'a> {
    fn new(ctx: &'a Context, backup: &'a str) -> Self {
        Self {
            ctx,
            backup,
            looked: Cell::new(None),
        }
    }

    fn now(&self) -> bool {
        self.ctx.jobs.busy_here() || self.elsewhere()
    }

    fn elsewhere(&self) -> bool {
        match self.looked.get() {
            Some((at, answer)) if at.elapsed() < LOOK_ELSEWHERE_EVERY => answer,
            _ => {
                let answer = self.ctx.jobs.held_elsewhere(self.backup);
                self.looked.set(Some((Instant::now(), answer)));
                answer
            }
        }
    }
}

/// Why the check is being started, which decides whether one the person
/// stopped may start again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Why {
    /// A search or a backup on the backup has just ended: new work the
    /// person asked for, so a check they stopped before starts again.
    AfterWork,
    /// Keevoro opened, or the backup was opened: it carries on with what is
    /// left, unless the person stopped it.
    CarryingOn,
    /// The person asked: "Check for duplicates now".
    Asked,
    /// A backup that ran by itself has just ended. As after any backup, but
    /// a guest's work, with nobody waiting for it: at low priority, and it
    /// gives way while the computer is on battery or busy, to carry on next
    /// time Keevoro opens.
    AfterRunningByItself,
}

/// How often a check after a backup that ran by itself asks whether the
/// computer is still free for it.
const ASK_THE_COMPUTER_EVERY: Duration = Duration::from_secs(30);

/// The catalog setting that says a person stopped the check, and when.
const STOPPED: &str = "duplicates_stopped";

/// True when a person stopped the check on this backup and nothing has
/// started it again since.
pub(crate) fn stopped_by_person(catalog: &Catalog) -> bool {
    catalog.setting(STOPPED).ok().flatten().is_some_and(|v| !v.is_empty())
}

/// How the check on a backup stands when none is running: how many files
/// are still to check, whether a person stopped it, and that in words.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicatesLeft {
    pub left: usize,
    pub stopped: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub said: Vec<String>,
}

impl DuplicatesLeft {
    pub(crate) fn of(catalog: &Catalog) -> Self {
        let left = catalog.unhashed_count().unwrap_or(0);
        let stopped = stopped_by_person(catalog);
        let mut said = if stopped && left > 0 {
            crate::words::check_stopped(left)
        } else {
            Vec::new()
        };
        if left > 0 {
            said.extend(waiting_for_sources(catalog).0);
        }
        Self { left, stopped, said }
    }
}

/// Each source with files not yet checked that is not plugged in now, by
/// the one rule a search and a backup use, said as a sentence each; and
/// how many files not yet checked are on sources that are there.
pub(crate) fn waiting_for_sources(catalog: &Catalog) -> (Vec<String>, usize) {
    let mut on = std::collections::BTreeMap::<std::path::PathBuf, usize>::new();
    for file in catalog.unhashed().unwrap_or_default() {
        *on.entry(file.source).or_default() += 1;
    }
    let mut there = 0;
    let gone: Vec<keevoro_engine::deep::Gone> = on
        .into_iter()
        .filter_map(|(source, left)| {
            if keevoro_engine::reach::is_there(&source) {
                there += left;
                None
            } else {
                Some(keevoro_engine::deep::Gone { source, left })
            }
        })
        .collect();
    (gone_words(catalog, &gone), there)
}

/// What the check says of the sources it could not reach: "KINGSTON (E:)
/// is not plugged in, so its 1,315 files not yet checked for duplicates
/// will be checked when it is back."
fn gone_words(catalog: &Catalog, gone: &[keevoro_engine::deep::Gone]) -> Vec<String> {
    let sources = catalog.sources().unwrap_or_default();
    gone.iter()
        .map(|g| {
            let (name, drive) = match sources.iter().find(|s| s.root == g.source) {
                Some(source) => {
                    super::looking::called(source, catalog.source_mark(&source.root).ok().flatten().as_ref())
                }
                None => (friendly_path(&g.source), false),
            };
            crate::words::check_waits_for(&name, drive, g.left)
        })
        .collect()
}

/// Starts the check on this backup, unless one is running already, or the
/// person stopped it and this is only Keevoro carrying on. With `within`,
/// the folders an AI agent may look in, only files inside them are read.
pub(crate) fn start(ctx: &Context, backup: &Named, within: Option<AgentAccess>, why: Why) -> Option<JobHandle> {
    let home = backup.home.clone();
    match why {
        Why::CarryingOn => {
            let stopped = Catalog::open_in_library(&home)
                .map(|c| stopped_by_person(&c))
                .unwrap_or(false);
            if stopped {
                return None;
            }
        }
        Why::AfterWork | Why::Asked | Why::AfterRunningByItself => {
            if let Ok(catalog) = Catalog::open_in_library(&home) {
                if stopped_by_person(&catalog) {
                    let _ = catalog.set_setting(STOPPED, "");
                }
            }
        }
    }
    let job = ctx.jobs.begin_only_one(JobKind::Duplicates, backup)?;
    let handle = job.handle.clone();
    let backup_id = backup.id.clone();
    job.note_line("Checking for duplicates");
    spawn(ctx, &job, move |ctx, job, ending| {
        let log = &ctx.log;
        let started = Instant::now();
        let mut every = Every::now(ctx.jobs.progress_every());
        let mut total = 0;
        let give_way = GiveWayWhen::new(ctx, &backup_id);
        let a_guest = why == Why::AfterRunningByItself;
        if a_guest {
            keevoro_frame::quiet::lower_this_threads_priority();
        }
        let computer_asked: Cell<Option<(Instant, bool)>> = Cell::new(None);
        // True while the computer is on battery or busy, asked every so
        // often, for a check that is a guest.
        let not_a_good_moment = || {
            if !a_guest {
                return false;
            }
            match computer_asked.get() {
                Some((at, answer)) if at.elapsed() < ASK_THE_COMPUTER_EVERY => answer,
                _ => {
                    let facts = ctx.signals.now();
                    let answer = keevoro_core::background::may_start(&facts).is_err();
                    computer_asked.set(Some((Instant::now(), answer)));
                    answer
                }
            }
        };
        // Paused on its own, it waits here between one file and the next,
        // as it would for Keevoro's own Pause, until it is let go on or
        // asked to stop.
        let keep_going = || {
            while job.held() && !job.stopped() {
                std::thread::sleep(Duration::from_millis(150));
            }
            !job.stopped() && !give_way.now() && !not_a_good_moment()
        };
        let result = Catalog::open_in_library(&home)
            .map_err(anyhow::Error::from)
            .and_then(|catalog| {
                let may_read = |file: &std::path::Path| within.as_ref().is_none_or(|agent| agent.may_look_in(file));
                keevoro_engine::deep::deep_look_where(
                    &catalog,
                    Some(&ctx.cache),
                    None,
                    &may_read,
                    &mut |p| {
                        total = p.total;
                        if p.done == 0 {
                            log.say(&format!("Checking {} files in the background", p.total));
                        }
                        if p.bytes >= 64 * 1024 * 1024 {
                            log.say(&format!(
                                "Checking a large file ({}): {}",
                                keevoro_engine::summary::bytes(p.bytes),
                                friendly_path(p.reading)
                            ));
                        }
                        let tell = every.due();
                        if tell {
                            job.note_line(&crate::words::checking(p.done, p.total));
                        }
                        ctx.jobs.heard(
                            job,
                            Said::Check(CheckEvent {
                                done: p.done,
                                total: p.total,
                                finished: false,
                                stopped: false,
                                left: 0,
                                said: Vec::new(),
                            }),
                            tell,
                        );
                    },
                    &keep_going,
                )
            });
        // Another check may start from here, while this one says how it went.
        ctx.jobs.release(job);
        let by_person = job.asked_to_stop();
        let done = match &result {
            Ok(report) => {
                if report.checked > 0 || !report.failed.is_empty() || report.stopped {
                    log.say(&format!(
                        "Background checking {} after {} seconds: {} checked, {} could not be read",
                        match (report.stopped, by_person) {
                            (true, true) => "stopped",
                            (true, false) => "paused",
                            (false, _) => "finished",
                        },
                        started.elapsed().as_secs(),
                        report.checked,
                        report.failed.len()
                    ));
                }
                for (path, why) in report.failed.iter().take(200) {
                    log.say(&format!("Could not check {}: {why}", friendly_path(path)));
                }
                report.checked
            }
            Err(e) => {
                log.say(&format!("Background checking failed: {e:#}"));
                0
            }
        };
        // Sources not plugged in were passed over whole, not tried file by
        // file; what waits for them is said, and the check ends.
        let waits = match &result {
            Ok(report) if !report.gone.is_empty() => Catalog::open_in_library(&home)
                .map(|catalog| gone_words(&catalog, &report.gone))
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        for said in &waits {
            log.say(said);
        }
        // Stopped by a person, it is noted with the backup, so it does not
        // start again by itself until the next search or backup, and what
        // that means is said.
        let mut left = 0;
        if by_person {
            if let Ok(catalog) = Catalog::open_in_library(&home) {
                let _ = catalog.set_setting(STOPPED, &super::seconds_now().to_string());
                left = catalog.unhashed_count().unwrap_or(0);
            }
        }
        let last = Said::Check(CheckEvent {
            done,
            total,
            finished: true,
            stopped: by_person,
            left,
            said: if by_person {
                crate::words::check_stopped(left).into_iter().chain(waits).collect()
            } else {
                waits
            },
        });
        ctx.jobs.heard(job, last.clone(), true);
        ending.with(Some(last));
    })
    .ok()?;
    Some(handle)
}
