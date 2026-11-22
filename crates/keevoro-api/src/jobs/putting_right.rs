//! `backup.put_right`: putting a backup right, with one button
//! (`keevoro_engine::put_right`), at each of its destinations in turn.
//!
//! To every other program it is a check (`JobKind::Check`, so it takes the
//! backup's one slot for backing up, and older programs read it), started
//! by its own operation. It runs on its own thread at low priority, so the
//! person can use the rest of Keevoro meanwhile, and it can be stopped and
//! pressed again: it carries on from where it was.

use super::{spawn, Context, Every, JobHandle, JobKind, Listener, Outcome, Said, SendEvent};
use crate::backups::Named;
use crate::error::Result;
use crate::register::Op;
use keevoro_catalog::Catalog;
use keevoro_engine::connect::{self, World};
use keevoro_engine::put_right::HowToPutRight;
use std::sync::atomic::Ordering;

pub(super) fn start(
    ctx: &Context,
    backup: &Named,
    thorough: bool,
    today: String,
    listener: Option<Listener>,
) -> Result<JobHandle> {
    let job = ctx
        .jobs
        .begin_as(JobKind::Check, Op::BackupPutRight, backup, listener)?;
    let handle = job.handle.clone();
    let opening = "Getting ready to put this backup right";
    job.note_line(opening);
    ctx.jobs.heard(&job, Said::Send(SendEvent::progress(opening)), true);
    let backup = backup.clone();
    spawn(ctx, &job, move |ctx, job, ending| {
        keevoro_frame::quiet::lower_this_threads_priority();
        let mut said = Vec::new();
        let mut every = Every::now(ctx.jobs.progress_every());
        let stop = job.stop_switch();
        let result = (|| -> anyhow::Result<(usize, usize, bool)> {
            let catalog = Catalog::open_in_library(&backup.home)?;
            let world = World {
                secrets: &*ctx.secrets,
                environment: &*ctx.environment,
            };
            let all = connect::destinations(&catalog)?;
            if all.is_empty() {
                anyhow::bail!("Choose where to back up to first.");
            }
            let several = all.len() > 1;
            let (mut wrong, mut unreachable, mut stopped) = (0, 0, false);
            for to in &all {
                if stop.load(Ordering::SeqCst) {
                    stopped = true;
                    break;
                }
                if several {
                    said.extend_from_slice(format!("At {}:\n", to.describe()).as_bytes());
                }
                let mut progress = |line: &str| {
                    job.note_line(line);
                    let tell = every.due();
                    ctx.jobs.heard(job, Said::Send(SendEvent::progress(line)), tell);
                };
                let done = keevoro_engine::put_right::put_right(
                    &mut said,
                    &catalog,
                    &backup.home,
                    to,
                    &world,
                    HowToPutRight { thorough },
                    &today,
                    &super::seconds_now,
                    &stop,
                    &mut progress,
                );
                match done {
                    Ok(outcome) => {
                        wrong += outcome.attention.len();
                        stopped |= outcome.stopped;
                        if outcome.lost_connection {
                            unreachable += 1;
                        }
                    }
                    Err(e) => {
                        said.extend_from_slice(format!("Couldn't put {} right. {e:#}\n", to.describe()).as_bytes());
                        unreachable += 1;
                    }
                }
            }
            Ok((wrong, unreachable, stopped))
        })();
        ctx.jobs.release(job);
        let (problem, outcome) = match &result {
            Ok((_, unreachable, _)) if *unreachable > 0 => (None, Outcome::Unreachable),
            Ok((wrong, _, _)) if *wrong > 0 => (
                Some(format!(
                    "{} still {} attention.",
                    keevoro_engine::summary::count(*wrong, "file"),
                    if *wrong == 1 { "needs" } else { "need" }
                )),
                Outcome::SomeFiles,
            ),
            Ok((_, _, true)) => (None, Outcome::Stopped),
            Ok(_) => (None, Outcome::Worked),
            Err(e) => (Some(format!("{e:#}")), Outcome::Refused),
        };
        ctx.log.say(&format!(
            "Putting the backup right finished: {}",
            String::from_utf8_lossy(&said).trim()
        ));
        let last = Said::Send(SendEvent::finished(&said, problem, outcome));
        ctx.jobs.heard(job, last.clone(), true);
        ending.with(Some(last));
    })?;
    Ok(handle)
}
