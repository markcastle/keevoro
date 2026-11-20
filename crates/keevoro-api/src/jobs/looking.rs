//! `sources.look` and `sources.look_again`: looking through a drive or a
//! folder quickly, and adding it to a backup as a source.
//!
//! A quick search reads only a little of each large file. What it only
//! sampled is read in full afterwards, in the background, by the check
//! for duplicates, which this starts again when it ends.

use super::{spawn, Context, Every, Job, JobHandle, JobKind, Listener, Said, ScanEvent};
use crate::backups::Named;
use crate::error::{Error, Result};
use keevoro_catalog::Catalog;
use keevoro_core::Kinds;
use keevoro_engine::friendly_path;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

/// The places `sources.look_again` looks through: the one source asked
/// for, or every one, in the order they were added.
pub(super) fn sources_to_look_again(backup: &Named, source: Option<i64>) -> Result<Vec<PathBuf>> {
    let catalog = Catalog::open_in_library(&backup.home)?;
    let sources = catalog.sources()?;
    let places: Vec<PathBuf> = sources
        .iter()
        .filter(|s| source.is_none_or(|id| s.id == id))
        .map(|s| s.root.clone())
        .collect();
    match (places.is_empty(), source) {
        (true, Some(_)) => Err(Error::not_found("That source isn't part of this backup.")),
        (true, None) => Err(Error::refused(
            "This backup has no sources to look through again. Add one first.",
        )),
        (false, _) => Ok(places),
    }
}

/// Starts looking through `places`, one after another, for the `kinds`
/// of file the settings named as it started.
pub(super) fn start(
    ctx: &Context,
    backup: &Named,
    places: Vec<PathBuf>,
    kind: JobKind,
    kinds: Kinds,
    listener: Option<Listener>,
) -> Result<JobHandle> {
    let job = ctx.jobs.begin(kind, Some(backup), listener)?;
    let handle = job.handle.clone();
    // Said at once, so anyone who asks straight away hears it.
    if let Some(first) = places.first() {
        job.note_line(&format!("Looking through {}", name_of(first)));
    }
    let backup = backup.clone();
    spawn(ctx, &job, move |ctx, job, ending| {
        let mut last = None;
        for (n, place) in places.iter().enumerate() {
            let said = look_through(ctx, job, &backup.home, place, &kinds);
            // A search that was stopped stops the rest too.
            let more = n + 1 < places.len() && !said.stopped && !job.stopped();
            if !more {
                last = Some(said);
                break;
            }
            ctx.jobs.heard(job, Said::Scan(said), true);
        }
        // Another search may start the moment this one is heard to end.
        ctx.jobs.release(job);
        let last = last.map(Said::Scan);
        if let Some(said) = &last {
            ctx.jobs.heard(job, said.clone(), true);
        }
        super::duplicates::start(ctx, &backup);
        ending.with(last);
    })?;
    Ok(handle)
}

/// A source's friendly name: its folder's name, or the drive.
fn name_of(source: &Path) -> String {
    source
        .file_name()
        .map_or_else(|| source.display().to_string(), |n| n.to_string_lossy().to_string())
}

/// Looks through one place, saying how it goes, and gives the last event.
fn look_through(ctx: &Context, job: &Arc<Job>, home: &Path, source: &Path, kinds: &Kinds) -> ScanEvent {
    let name = name_of(source);
    let started = Instant::now();
    let log = &ctx.log;
    log.say(&format!("Looking through {}", friendly_path(source)));
    job.note_line(&format!("Looking through {name}"));
    let mut every = Every::after(ctx.jobs.progress_every());
    let mut latest = (0, 0, 0);
    let mut announced: Option<PathBuf> = None;
    let event = |p: &keevoro_scan::ScanProgress, saving: bool| ScanEvent {
        name: name.clone(),
        photos: p.photos,
        videos: p.videos,
        files: p.files_seen,
        folder: friendly_path(&p.folder),
        reading: p.reading.as_ref().map(|(path, bytes)| (friendly_path(path), *bytes)),
        saving,
        stopped: false,
        finished: false,
        problem: None,
        unreadable: 0,
        unreadable_names: Vec::new(),
    };
    let mut last_progress = keevoro_scan::ScanProgress::default();
    let result = Catalog::open_in_library(home)
        .map_err(anyhow::Error::from)
        .and_then(|mut catalog| {
            let last_progress = std::cell::RefCell::new(&mut last_progress);
            keevoro_engine::scan_into_with(
                &mut catalog,
                source,
                Some(&name),
                &keevoro_scan::ScanOptions {
                    quick: true,
                    stop: Some(job.stop_switch()),
                    kinds: kinds.clone(),
                    ..Default::default()
                },
                &mut |p| {
                    latest = (p.photos, p.videos, p.files_seen);
                    **last_progress.borrow_mut() = p.clone();
                    // A large file starting is always said at once: it may
                    // be the last thing to change for minutes.
                    let big = p.reading.as_ref().map(|(path, _)| path.clone());
                    let news = big.is_some() && big != announced;
                    if news {
                        if let Some((path, bytes)) = &p.reading {
                            log.say(&format!(
                                "Reading a large file ({}): {}",
                                keevoro_engine::summary::bytes(*bytes),
                                friendly_path(path)
                            ));
                        }
                        announced = big;
                    }
                    let tell = every.due_or(news);
                    ctx.jobs.heard(job, Said::Scan(event(p, false)), tell);
                },
                &mut || {
                    let p = last_progress.borrow();
                    log.say(&format!(
                        "Finished looking after {} files; saving what was found",
                        p.files_seen
                    ));
                    job.note_line(&format!("Saving what was found in {name}"));
                    ctx.jobs.heard(job, Said::Scan(event(&p, true)), true);
                },
            )
        });
    let was_stopped = result.as_ref().is_ok_and(|report| report.stopped);
    match &result {
        Ok(report) if report.stopped => log.say(&format!(
            "Stopped looking through {} after {} seconds and {} files. Nothing from it was kept.",
            friendly_path(source),
            started.elapsed().as_secs(),
            latest.2
        )),
        Ok(report) => {
            log.say(&format!(
                "Looked through {} in {} seconds: {} photos and videos, {} other files, {} folders skipped, {} unreadable",
                friendly_path(source),
                started.elapsed().as_secs(),
                report.items.len(),
                report.other_files,
                report.skipped.len(),
                report.unreadable.len()
            ));
            for unreadable in report.unreadable.iter().take(200) {
                log.say(&format!(
                    "Could not read {}: {}",
                    friendly_path(&unreadable.path),
                    unreadable.error
                ));
            }
        }
        Err(e) => log.say(&format!("Looking through {} failed: {e:#}", source.display())),
    }
    ScanEvent {
        name,
        photos: latest.0,
        videos: latest.1,
        files: latest.2,
        folder: String::new(),
        reading: None,
        saving: false,
        stopped: was_stopped,
        finished: true,
        // What could not be read is said on the screen, not only in the log.
        unreadable: result.as_ref().map_or(0, |r| r.unreadable.len()),
        unreadable_names: result.as_ref().map_or_else(
            |_| Vec::new(),
            |r| r.unreadable.iter().take(5).map(|u| friendly_path(&u.path)).collect(),
        ),
        problem: result.err().map(|e| format!("{e:#}")),
    }
}
