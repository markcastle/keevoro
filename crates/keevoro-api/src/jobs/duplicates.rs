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

/// Starts the check on this backup, unless one is running already. With
/// `within`, the folders an AI agent may look in, only files inside them
/// are read.
pub(crate) fn start(ctx: &Context, backup: &Named, within: Option<AgentAccess>) -> Option<JobHandle> {
    let job = ctx.jobs.begin_only_one(JobKind::Duplicates, backup)?;
    let handle = job.handle.clone();
    let home = backup.home.clone();
    let backup_id = backup.id.clone();
    job.note_line("Checking for duplicates");
    spawn(ctx, &job, move |ctx, job, ending| {
        let log = &ctx.log;
        let started = Instant::now();
        let mut every = Every::now(ctx.jobs.progress_every());
        let mut total = 0;
        let give_way = GiveWayWhen::new(ctx, &backup_id);
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
                        ctx.jobs.heard(
                            job,
                            Said::Check(CheckEvent {
                                done: p.done,
                                total: p.total,
                                finished: false,
                            }),
                            tell,
                        );
                    },
                    &|| !job.stopped() && !give_way.now(),
                )
            });
        // Another check may start from here, while this one says how it went.
        ctx.jobs.release(job);
        let done = match &result {
            Ok(report) => {
                if report.checked > 0 || !report.failed.is_empty() || report.stopped {
                    log.say(&format!(
                        "Background checking {} after {} seconds: {} checked, {} could not be read",
                        if report.stopped { "paused" } else { "finished" },
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
        let last = Said::Check(CheckEvent {
            done,
            total,
            finished: true,
        });
        ctx.jobs.heard(job, last.clone(), true);
        ending.with(Some(last));
    })
    .ok()?;
    Some(handle)
}
