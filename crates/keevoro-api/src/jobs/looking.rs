//! `sources.look` and `sources.look_again`: looking through a drive or a
//! folder quickly, and adding it to a backup as a source.
//!
//! A quick search reads only a little of each large file. What it only
//! sampled is read in full afterwards, in the background, by the check
//! for duplicates, which this starts again when it ends.

use super::{spawn, Context, Every, Job, JobHandle, JobKind, Listener, Said, ScanEvent};
use crate::agents::AgentAccess;
use crate::backups::Named;
use crate::error::{Error, Result};
use keevoro_catalog::Catalog;
use keevoro_core::Kinds;
use keevoro_engine::friendly_path;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

/// The places `sources.look_again` looks through: the one source asked
/// for, or every one, in the order they were added. A source the person
/// left out (or one inside a folder left out) is passed by, so what was
/// found there is kept for when it is put back.
pub(super) fn sources_to_look_again(backup: &Named, source: Option<i64>) -> Result<Vec<PathBuf>> {
    let catalog = Catalog::open_in_library(&backup.home)?;
    let sources = catalog.sources()?;
    let choices = catalog.choices()?;
    let asked: Vec<PathBuf> = sources
        .iter()
        .filter(|s| source.is_none_or(|id| s.id == id))
        .map(|s| s.root.clone())
        .collect();
    let left_out = asked
        .iter()
        .find_map(|root| choices.left_out_folder(root).map(friendly_path));
    let places: Vec<PathBuf> = asked
        .into_iter()
        .filter(|root| choices.left_out_folder(root).is_none())
        .collect();
    match (places.is_empty(), source, left_out) {
        (true, _, Some(folder)) => Err(Error::refused(left_out_said(&folder))),
        (true, Some(_), None) => Err(Error::not_found("That source isn't part of this backup.")),
        (true, None, None) => Err(Error::refused(
            "This backup has no sources to look through again. Add one first.",
        )),
        (false, _, _) => Ok(places),
    }
}

/// Why a place is not looked through: the person left it out.
pub(super) fn left_out_said(folder: &str) -> String {
    format!("You left {folder} out of this backup, so it isn't looked through. Put it back first to include it.")
}

/// Refuses to look through a place the person left out of `backup`, or
/// one inside such a folder, rather than finding nothing there.
pub(super) fn not_left_out(backup: &Named, place: &Path) -> Result<()> {
    let real = std::fs::canonicalize(place).unwrap_or_else(|_| place.to_path_buf());
    let folders = Catalog::open_in_library(&backup.home)?.left_out_folders()?;
    match folders.iter().find(|f| keevoro_core::places::within(&real, f)) {
        Some(folder) => Err(Error::refused(left_out_said(&friendly_path(folder)))),
        None => Ok(()),
    }
}

/// What a search is to look for, and how: the `kinds` of file the
/// settings named as it started, the folders to pass by, and, for a search
/// an AI agent asked for, what that agent may look in, which the check for
/// duplicates that follows is kept to.
pub(super) struct HowToLook {
    pub(super) kinds: Kinds,
    pub(super) skip: keevoro_core::SkipRules,
    pub(super) for_agent: Option<AgentAccess>,
}

/// Starts looking through `places`, one after another.
pub(super) fn start(
    ctx: &Context,
    backup: &Named,
    places: Vec<PathBuf>,
    kind: JobKind,
    how: HowToLook,
    listener: Option<Listener>,
) -> Result<JobHandle> {
    let HowToLook { kinds, skip, for_agent } = how;
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
            let said = look_through(ctx, job, &backup.home, place, &kinds, &skip);
            // A remembered map of anywhere this search covers may now be
            // out of date, so it is surveyed again the next time it opens.
            let real = std::fs::canonicalize(place).unwrap_or_else(|_| place.clone());
            keevoro_engine::surveys::mark_out_of_date(&ctx.cache, &real);
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
        super::duplicates::start(ctx, &backup, for_agent, super::duplicates::Why::AfterWork);
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
fn look_through(
    ctx: &Context,
    job: &Arc<Job>,
    home: &Path,
    source: &Path,
    kinds: &Kinds,
    skip: &keevoro_core::SkipRules,
) -> ScanEvent {
    let name = name_of(source);
    // What to call it if it is unplugged part-way: the drive's own name
    // when the place is a whole drive. From the system's quick list, so it
    // costs next to nothing and never waits on a network share.
    let drive = crate::places::drive_name(source);
    let started = Instant::now();
    let log = &ctx.log;
    log.say(&format!("Looking through {}", friendly_path(source)));
    job.note_line(&format!("Looking through {name}"));
    let mut every = Every::after(ctx.jobs.progress_every());
    let mut latest = (0, 0, 0, 0);
    let mut announced: Option<PathBuf> = None;
    let event = |p: &keevoro_scan::ScanProgress, saving: bool| ScanEvent {
        name: name.clone(),
        photos: p.photos,
        videos: p.videos,
        others: p.others,
        files: p.files_seen,
        folder: friendly_path(&p.folder),
        reading: p.reading.as_ref().map(|(path, bytes)| (friendly_path(path), *bytes)),
        saving,
        ..ScanEvent::default()
    };
    let mut last_progress = keevoro_scan::ScanProgress::default();
    // What an earlier look through this place found, to say which files
    // are no longer offered if Keevoro's own folders are left out now.
    let root = std::fs::canonicalize(source).unwrap_or_else(|_| source.to_path_buf());
    let found_before: Vec<PathBuf> = Catalog::open_in_library(home)
        .and_then(|c| c.load_reports())
        .map(|reports| {
            reports
                .into_iter()
                .filter(|r| r.root == root)
                .flat_map(|r| r.items.into_iter().map(|i| i.path))
                .collect()
        })
        .unwrap_or_default();
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
                    skip: skip.clone(),
                    ..Default::default()
                },
                &mut |p| {
                    latest = (p.photos, p.videos, p.files_seen, p.others);
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
    let unplugged = result
        .as_ref()
        .is_ok_and(|report| report.gone)
        .then(|| crate::words::unplugged_while_looking(drive.as_deref().unwrap_or(&name), drive.is_some()));
    if let Some(said) = &unplugged {
        log.say(&format!(
            "{} went part-way through looking, after {} files: {said}",
            friendly_path(source),
            latest.2
        ));
    }
    match &result {
        Ok(report) if report.stopped => log.say(&format!(
            "Stopped looking through {} after {} seconds and {} files. Nothing from it was kept.",
            friendly_path(source),
            started.elapsed().as_secs(),
            latest.2
        )),
        Ok(report) => {
            let of = |kind| report.items.iter().filter(|i| i.kind == kind).count();
            let carried = of(keevoro_core::MediaKind::Other);
            // Said as ever, unless other files were carried along too.
            let (found, rest) = if carried == 0 {
                (format!("{} photos and videos", report.items.len()), "other files")
            } else {
                (
                    keevoro_core::words::files(
                        of(keevoro_core::MediaKind::Photo),
                        of(keevoro_core::MediaKind::Video),
                        carried,
                    ),
                    "files Keevoro does not look for",
                )
            };
            log.say(&format!(
                "Looked through {} in {} seconds: {found}, {} {rest}, {} folders skipped, {} unreadable",
                friendly_path(source),
                started.elapsed().as_secs(),
                report.other_files,
                report.skipped.len(),
                report.unreadable.len()
            ));
            if let Some(said) = online_only_said(report) {
                log.say(&format!("In {}: {said}", friendly_path(source)));
            }
            for left in &left_out_of(report, &found_before) {
                log.say(&left.said);
            }
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
        others: latest.3,
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
        left_out: result
            .as_ref()
            .map_or_else(|_| Vec::new(), |r| left_out_of(r, &found_before)),
        // What could not be looked at without downloading it is said on
        // the screen too, so no photo is left out silently.
        online_only: result.as_ref().map_or(0, |r| r.online_only.len()),
        online_only_said: result.as_ref().ok().and_then(online_only_said),
        problem: result.err().map(|e| format!("{e:#}")),
        unplugged,
    }
}

/// What a look through said of the files it found only in online storage.
fn online_only_said(report: &keevoro_scan::ScanReport) -> Option<String> {
    let bytes =
        (report.online_only_bytes.len() == report.online_only.len()).then(|| report.online_only_bytes.iter().sum());
    keevoro_engine::summary::online_only(report.online_only.len(), bytes, &report.root)
}

/// The folders Keevoro made that a look through left out, said.
fn left_out_of(report: &keevoro_scan::ScanReport, found_before: &[PathBuf]) -> Vec<super::own::LeftOut> {
    let skipped: Vec<(PathBuf, keevoro_core::SkipReason)> =
        report.skipped.iter().map(|s| (s.path.clone(), s.reason)).collect();
    super::own::left_out(&skipped, found_before)
}
