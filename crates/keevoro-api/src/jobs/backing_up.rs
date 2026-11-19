//! `backup.run` and `backup.check`: backing up to every destination, and
//! confirming what is at each.
//!
//! A backup goes to each destination in turn, the originals first and the
//! previews after. How each destination's turn went is written into the
//! backup's history, and what it said when it ended is kept with the
//! backup until the next one, so it is still there after closing Keevoro.

use super::{seconds_now, spawn, Context, Every, JobHandle, JobKind, Listener, Said, SendEvent};
use crate::backup::Backup;
use crate::backups::Named;
use crate::error::Result;
use keevoro_catalog::Catalog;
use keevoro_engine::connect::{self, World};
use keevoro_engine::history::Sent;
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
                check(&home, &catalog, &world, &stop, &mut said, &mut progress)
            }
        })();
        // Another may start the moment this one is heard to end.
        ctx.jobs.release(job);
        // How it went, said to the person and written into the backup's
        // record, so "was that slow?" has an answer afterwards.
        if record && result.is_ok() {
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
        match &result {
            Ok(()) => ctx.log.say(&format!(
                "Backup job finished: {}",
                String::from_utf8_lossy(&said).trim()
            )),
            Err(e) => ctx.log.say(&format!("Backup job failed: {e:#}")),
        }
        let last = SendEvent::finished(&said, result.err().map(|e| format!("{e:#}")));
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
) -> anyhow::Result<()> {
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
/// the next destination; the one in hand is checked to the end.
fn check(
    home: &Path,
    catalog: &Catalog,
    world: &World<'_>,
    stop: &Arc<AtomicBool>,
    said: &mut Vec<u8>,
    progress: &mut dyn FnMut(&str),
) -> anyhow::Result<()> {
    let all = connect::destinations(catalog)?;
    if all.is_empty() {
        anyhow::bail!("Choose where to back up to first.");
    }
    let several = all.len() > 1;
    for to in &all {
        if stop.load(std::sync::atomic::Ordering::SeqCst) {
            break;
        }
        if several {
            said.extend_from_slice(format!("At {}:\n", to.describe()).as_bytes());
        }
        let destination = to.open(world)?;
        keevoro_engine::send::check(said, catalog, home, Some(destination), false, progress)?;
    }
    Ok(())
}
