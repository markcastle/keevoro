//! The background check for duplicates: reading in full the files a quick
//! search only sampled, so that true duplicates are known and a backup has
//! less to do.
//!
//! Only one runs at a time. It gives way to a search and to a backup: a
//! backup reads the same files, and fingerprints them as it goes, so
//! checking alongside it would read them twice over. Each of those starts
//! it again when it ends, and it carries on from where it was.

use super::{spawn, CheckEvent, Context, Every, JobHandle, JobKind, Said};
use crate::backups::Named;
use keevoro_catalog::Catalog;
use keevoro_engine::friendly_path;
use std::time::Instant;

/// Starts the check on this backup, unless one is running already.
pub(crate) fn start(ctx: &Context, backup: &Named) -> Option<JobHandle> {
    let job = ctx.jobs.begin_only_one(JobKind::Duplicates, backup)?;
    let handle = job.handle.clone();
    let home = backup.home.clone();
    job.note_line("Checking for duplicates");
    spawn(ctx, &job, move |ctx, job, ending| {
        let log = &ctx.log;
        let started = Instant::now();
        let mut every = Every::now(ctx.jobs.progress_every());
        let mut total = 0;
        let result = Catalog::open_in_library(&home)
            .map_err(anyhow::Error::from)
            .and_then(|catalog| {
                keevoro_engine::deep::deep_look(
                    &catalog,
                    None,
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
                    &|| !job.stopped() && !ctx.jobs.should_give_way(),
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
