//! `backup.run` and `backup.check`: backing up to every destination, and
//! confirming what is at each.
//!
//! A backup goes to each destination in turn, the originals first and the
//! previews after. How each destination's turn went is written into the
//! backup's history, and what it said when it ended is kept with the
//! backup until the next one, so it is still there after closing Keevoro.

use super::{seconds_now, spawn, Context, Every, JobHandle, JobKind, Listener, Outcome, Said, SendEvent};
use crate::backup::Backup;
use crate::backups::Named;
use crate::error::Result;
use keevoro_catalog::Catalog;
use keevoro_engine::connect::{self, World};
use keevoro_engine::history::Sent;
use keevoro_engine::send::Went;
use keevoro_media::VideoTools;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// What a backup sets out to send: what every destination still lacks,
/// added up, so its speed can become a time left.
fn waiting_everywhere(home: &Path) -> u64 {
    Catalog::open_in_library(home)
        .ok()
        .and_then(|catalog| keevoro_engine::discover(home, &catalog).ok())
        .map_or(0, |found| {
            let everywhere: u64 = found.destinations.iter().map(|d| d.waiting_bytes).sum();
            everywhere.max(found.waiting_bytes)
        })
}

pub(super) fn start(ctx: &Context, backup: &Named, kind: JobKind, listener: Option<Listener>) -> Result<JobHandle> {
    let record = kind == JobKind::BackUp;
    let job = ctx.jobs.begin(kind, Some(backup), listener)?;
    let handle = job.handle.clone();
    let home = backup.home.clone();
    if record {
        job.note_bytes(waiting_everywhere(&home), keevoro_core::meter::totals().0);
    }
    // Said at once, before the first real step has anything to report.
    let opening = if record {
        "Getting ready: reading what Keevoro knows about this backup"
    } else {
        "Getting ready to check"
    };
    job.note_line(opening);
    ctx.jobs.heard(&job, Said::Send(SendEvent::progress(opening)), true);
    // What an earlier backup said when it ended is about that one. A new
    // one starts with a clean slate, so "Stopped" is never left showing
    // beside a backup that is running.
    if record {
        let _ = Backup::open(&home).and_then(|b| b.forget_last_summary());
    }
    let backup = backup.clone();
    spawn(ctx, &job, move |ctx, job, ending| {
        let mut said = Vec::new();
        let mut every = Every::now(ctx.jobs.progress_every());
        // How each destination's turn went, for the backup's record.
        let mut turns = Vec::new();
        let stop = job.stop_switch();
        let result = (|| {
            let catalog = Catalog::open_in_library(&home)?;
            let world = World {
                secrets: &*ctx.secrets,
                ask: &connect::testing::nobody_to_ask,
            };
            let mut progress = |line: &str| {
                // Remembered every time, said a few times a second.
                job.note_line(line);
                let tell = every.due();
                ctx.jobs.heard(job, Said::Send(SendEvent::progress(line)), tell);
            };
            if record {
                run(&home, &catalog, &world, &stop, &mut said, &mut progress, &mut turns)
            } else {
                check(&catalog, &world, &stop, &mut said, &mut progress)
            }
        })();
        // Another may start the moment this one is heard to end.
        ctx.jobs.release(job);
        // How it went, said to the person and written into the backup's
        // record, so "was that slow?" has an answer afterwards.
        if record && result.as_ref().is_ok_and(|went| went.refused.is_none()) {
            let several = turns.len() > 1;
            for sent in &turns {
                if let Some(sentence) = sent.sentence() {
                    // With more than one destination, each sentence says whose it is.
                    let line = if several {
                        format!("To {}: {}", sent.to, sentence.to_lowercase())
                    } else {
                        sentence
                    };
                    said.extend_from_slice(line.as_bytes());
                    said.push(b'\n');
                    if let Ok(catalog) = Catalog::open_in_library(&home) {
                        let _ = keevoro_engine::history::record(&catalog, sent);
                    }
                }
            }
        }
        let (problem, outcome) = match result {
            Ok(went) => (went.refused.as_ref().map(|e| format!("{e:#}")), outcome(&went)),
            // Nowhere to back up to, or Keevoro's own record could not be read.
            Err(e) => (Some(format!("{e:#}")), Outcome::Refused),
        };
        match &problem {
            None => ctx.log.say(&format!(
                "Backup job finished: {}",
                String::from_utf8_lossy(&said).trim()
            )),
            Some(e) => ctx.log.say(&format!("Backup job failed: {e}")),
        }
        let last = SendEvent::finished(&said, problem, outcome);
        // What a backup said when it ended is kept with the backup, so it
        // is still there after looking at another screen or closing
        // Keevoro, until the next backup replaces it.
        if record {
            let _ = Backup::open(&home).and_then(|b| b.remember_last_summary(&last));
        }
        let last = Said::Send(last);
        ctx.jobs.heard(job, last.clone(), true);
        // Whatever the backup did not fingerprint is picked up again now.
        super::duplicates::start(ctx, &backup);
        ending.with(Some(last));
    })?;
    Ok(handle)
}

/// How it went, in a word. A destination out of reach matters most, as it
/// may be the one place a person thinks their photos are safe; then one
/// without the room; then files that could not be sent.
fn outcome(went: &Went) -> Outcome {
    if went.unreachable > 0 {
        Outcome::Unreachable
    } else if went.no_room > 0 {
        Outcome::Refused
    } else if went.failed_files > 0 {
        Outcome::SomeFiles
    } else if went.stopped {
        Outcome::Stopped
    } else {
        Outcome::Worked
    }
}

/// To each destination in turn. The files themselves go first; previews
/// are made and sent after.
fn run(
    home: &Path,
    catalog: &Catalog,
    world: &World<'_>,
    stop: &Arc<AtomicBool>,
    said: &mut Vec<u8>,
    progress: &mut dyn FnMut(&str),
    turns: &mut Vec<Sent>,
) -> anyhow::Result<Went> {
    keevoro_engine::send::everywhere(
        said,
        catalog,
        home,
        world,
        VideoTools::find().as_ref(),
        stop,
        progress,
        &mut |turn| {
            turns.push(Sent {
                finished_at: seconds_now(),
                seconds: turn.seconds,
                bytes: turn.bytes,
                to: turn.to.describe(),
                stopped: turn.stopped,
            });
        },
    )
}

/// Confirms what is at each destination. Asked to stop, it stops before
/// the next destination; the one in hand is checked to the end. One that
/// cannot be reached is said and passed over, as a backup does.
fn check(
    catalog: &Catalog,
    world: &World<'_>,
    stop: &Arc<AtomicBool>,
    said: &mut Vec<u8>,
    progress: &mut dyn FnMut(&str),
) -> anyhow::Result<Went> {
    let all = connect::destinations(catalog)?;
    if all.is_empty() {
        anyhow::bail!("Choose where to back up to first.");
    }
    let several = all.len() > 1;
    let mut went = Went::default();
    for to in &all {
        if stop.load(std::sync::atomic::Ordering::SeqCst) {
            went.stopped = true;
            said.extend_from_slice(b"Stopped. Nothing was changed.\n");
            break;
        }
        if several {
            said.extend_from_slice(format!("At {}:\n", to.describe()).as_bytes());
        }
        let checked = to
            .open(world)
            .and_then(|destination| keevoro_engine::send::check(said, catalog, destination, false, progress));
        match checked {
            Ok(wrong) => {
                went.reached += 1;
                went.failed_files += wrong;
            }
            Err(e) => {
                said.extend_from_slice(format!("Couldn't check {}. {e:#}\n", to.describe()).as_bytes());
                went.unreachable += 1;
                went.refused = Some(e);
            }
        }
    }
    if went.reached > 0 {
        went.refused = None;
    }
    // Something that needs attention is a problem, as it always was.
    if went.failed_files > 0 && went.refused.is_none() {
        let wrong = went.failed_files;
        went.refused = Some(anyhow::anyhow!(
            "{} {} attention.",
            keevoro_engine::summary::count(wrong, "file"),
            if wrong == 1 { "needs" } else { "need" }
        ));
    }
    Ok(went)
}
