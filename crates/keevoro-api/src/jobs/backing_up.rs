//! `backup.run` and `backup.check`: backing up to every destination, and
//! confirming what is at each.
//!
//! A backup goes to each destination in turn, the originals first and the
//! previews after (made in the background while the originals go). One
//! estimate follows the whole of it, from the moment it starts. How each destination's turn went is written into the
//! backup's history, and what it said when it ended is kept with the
//! backup until the next one, so it is still there after closing Keevoro.

use super::{seconds_now, spawn, Context, Every, JobHandle, JobKind, Listener, Outcome, Said, SendEvent};
use crate::backup::Backup;
use crate::backups::Named;
use crate::error::Result;
use keevoro_catalog::Catalog;
use keevoro_core::estimate::Tracker;
use keevoro_engine::connect::{self, World};
use keevoro_engine::history::{Ended, RunDetail, Sent};
use keevoro_engine::send::Went;
use keevoro_media::VideoTools;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub(super) fn start(ctx: &Context, backup: &Named, kind: JobKind, listener: Option<Listener>) -> Result<JobHandle> {
    let record = kind == JobKind::BackUp;
    let job = ctx.jobs.begin(kind, Some(backup), listener)?;
    let handle = job.handle.clone();
    let home = backup.home.clone();
    // One estimate of the time left for the whole backup, which starts
    // from what earlier backups on this computer measured.
    let tracker = Tracker::new(crate::rates::remembered(&ctx.rates));
    if record {
        job.track(tracker.clone());
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
                run(
                    &home,
                    &catalog,
                    &world,
                    &stop,
                    &tracker,
                    &mut said,
                    &mut progress,
                    &mut turns,
                )
            } else {
                check(&catalog, &world, &stop, &mut said, &mut progress)
            }
        })();
        // Another may start the moment this one is heard to end.
        ctx.jobs.release(job);
        // What it measured makes the next backup's first estimate better.
        if record {
            if let Err(e) = crate::rates::remember(&ctx.rates, &tracker.measured()) {
                ctx.log
                    .say(&format!("Couldn't remember how fast this backup went: {e}"));
            }
        }
        // How it went, said to the person, so "was that slow?" has an
        // answer at once.
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
                }
            }
        }
        let (problem, outcome) = match &result {
            Ok(went) => (went.refused.as_ref().map(|e| format!("{e:#}")), outcome(went)),
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
        // Written into the backup's record, with everything measured, so a
        // person can look back at it in full.
        if let (true, Ok(went)) = (record, &result) {
            let entry = entry(&home, &job.handle, &tracker, went, &last, outcome);
            if entry.bytes > 0 || outcome != Outcome::Worked {
                let recorded = Catalog::open_in_library(&home)
                    .map_err(anyhow::Error::from)
                    .and_then(|c| keevoro_engine::history::record(&c, &entry));
                if let Err(e) = recorded {
                    ctx.log.say(&format!("Couldn't write down how this backup went: {e:#}"));
                }
            }
        }
        // What a backup said when it ended is kept with the backup, so it
        // is still there after looking at another screen or closing
        // Keevoro, until the next backup replaces it.
        if record {
            let _ = Backup::open(&home).and_then(|b| b.remember_last_summary(&last));
        }
        let last = Said::Send(last);
        ctx.jobs.heard(job, last.clone(), true);
        // Whatever the backup did not fingerprint is picked up again now.
        super::duplicates::start(ctx, &backup, None);
        ending.with(Some(last));
    })?;
    Ok(handle)
}

/// One backup, for its history: the line in the list, and everything
/// measured beside it.
fn entry(home: &Path, handle: &JobHandle, tracker: &Tracker, went: &Went, last: &SendEvent, outcome: Outcome) -> Sent {
    let found = Backup::open(home).and_then(|b| b.found()).ok();
    let ended = match outcome {
        Outcome::Worked | Outcome::SomeFiles => Ended::Finished,
        Outcome::Stopped => Ended::Stopped,
        Outcome::Unreachable | Outcome::Refused => Ended::Problem,
    };
    let to: Vec<&str> = went.destinations.iter().map(|d| d.to.as_str()).collect();
    Sent {
        finished_at: seconds_now(),
        seconds: tracker.running_seconds(),
        bytes: went.destinations.iter().map(|d| d.bytes).sum(),
        to: to.join(" and "),
        stopped: went.stopped,
        detail: Some(Box::new(RunDetail {
            started_at: handle.started_at,
            paused_seconds: tracker.paused_seconds(),
            ended,
            problem: last.problem.clone(),
            started_by: handle.started_by.name().replace("the Keevoro app", "the app"),
            version: env!("CARGO_PKG_VERSION").to_string(),
            destinations: went.destinations.clone(),
            stages: tracker.record(),
            duplicates: found.as_ref().map_or(0, |f| f.duplicates),
            duplicate_bytes: found.as_ref().map_or(0, |f| f.duplicate_bytes),
            previews: went.previews.clone(),
            summary: last.summary.clone(),
        })),
    }
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
/// are made beside them and sent after.
#[allow(clippy::too_many_arguments)]
fn run(
    home: &Path,
    catalog: &Catalog,
    world: &World<'_>,
    stop: &Arc<AtomicBool>,
    tracker: &Tracker,
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
        tracker,
        progress,
        &mut |turn| {
            turns.push(Sent {
                finished_at: seconds_now(),
                seconds: turn.seconds,
                bytes: turn.bytes,
                to: turn.to.describe(),
                stopped: turn.stopped,
                detail: None,
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
