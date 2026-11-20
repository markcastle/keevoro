//! Making thumbnails and display copies of the photos a backup keeps.
//!
//! Previews are made from each photo where it was found, only reading it,
//! and kept in the backup's own folder until they are sent. Each preview
//! is named after the file's content id, which means renaming or moving an
//! album never breaks a thumbnail.
//!
//! Some files cannot be previewed yet (HEIC, camera RAW, damaged pictures).
//! That is remembered with its reason, so the same file is not attempted on
//! every run, and the original is backed up regardless.

use crate::{in_library, Failed};
use keevoro_catalog::{Catalog, CatalogError, RenditionState};
use keevoro_core::MediaKind;
use keevoro_index::{rendition_path, Rendition};
use keevoro_media::{render_photo, write_rendition, MediaError};
use keevoro_scan::FoundItem;
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;

#[derive(Debug, Default, Serialize)]
pub struct PreviewReport {
    /// Previews made in this run.
    pub made: usize,
    pub already_there: usize,
    /// Photos with no preview, each with a plain reason. Includes ones
    /// remembered from earlier runs.
    pub no_preview: Vec<Failed>,
}

/// How many previews to make at once in the foreground: half the
/// processor's cores, and never more than four. Enough to be several times
/// quicker than one at a time, while leaving the rest of the computer to
/// its owner. Work in the background uses fewer (see [`Pace`]).
pub fn foreground_workers() -> usize {
    let cores = std::thread::available_parallelism().map_or(2, std::num::NonZero::get);
    (cores / 2).clamp(1, 4)
}

/// How fast previews may be made, decided afresh as they are. Work done in
/// the background, beside a backup, is given one that slows or waits when
/// the computer is wanted for something else.
#[derive(Clone, Copy)]
pub struct Pace<'a> {
    /// How many may be made at once now. Zero means wait.
    pub allowed: &'a (dyn Fn() -> usize + Sync),
    /// Called on each worker thread as it starts: to lower its priority,
    /// say.
    pub on_worker: &'a (dyn Fn() + Sync),
}

impl Pace<'_> {
    /// As fast as the foreground allows, as previews were always made.
    pub fn full() -> Pace<'static> {
        Pace {
            allowed: &|| usize::MAX,
            on_worker: &|| {},
        }
    }

    /// Waits, a little at a time, while no work is allowed. False if asked
    /// to stop meanwhile.
    pub(crate) fn wait_for_a_turn(&self, worker: usize, stop: &dyn Fn() -> bool) -> bool {
        while worker >= (self.allowed)() {
            if stop() {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        !stop()
    }
}

/// The photos in `items` that have their place and lack previews.
fn todo_list<'a>(
    library: &Path,
    catalog: &Catalog,
    items: &[&'a FoundItem],
    report: &mut PreviewReport,
) -> Result<Vec<(&'a str, std::path::PathBuf, std::path::PathBuf, std::path::PathBuf)>, CatalogError> {
    let mut todo = Vec::new();
    for item in items.iter().filter(|i| i.kind == MediaKind::Photo) {
        let Some(hash) = item.blake3.as_deref() else { continue };
        if catalog.collected_path(hash)?.is_none() {
            continue;
        }
        let id = &hash[..hash.len().min(16)];
        let thumb = in_library(library, &rendition_path(Rendition::Thumb, id));
        let display = in_library(library, &rendition_path(Rendition::Display, id));
        match catalog.rendition(hash)? {
            Some(RenditionState::Made { .. }) if thumb.is_file() && display.is_file() => report.already_there += 1,
            Some(RenditionState::NoPreview { reason }) => report.no_preview.push(Failed {
                path: item.path.clone(),
                reason,
            }),
            _ => todo.push((hash, item.path.clone(), thumb, display)),
        }
    }
    Ok(todo)
}

/// How many photos in `items` still need previews made.
pub fn previews_to_make(library: &Path, catalog: &Catalog, items: &[&FoundItem]) -> Result<usize, CatalogError> {
    Ok(todo_list(library, catalog, items, &mut PreviewReport::default())?.len())
}

/// Makes previews for every photo in `items` that has its place and lacks
/// them.
/// `on_progress` is called with (done, total) after each photo.
pub fn make_previews(
    library: &Path,
    catalog: &Catalog,
    items: &[&FoundItem],
    on_progress: &mut dyn FnMut(usize, usize),
) -> Result<PreviewReport, CatalogError> {
    make_previews_while(library, catalog, items, on_progress, &|| true)
}

/// As [`make_previews`], asking `keep_going` before each photo so the work
/// can be stopped. Whatever was made stays made.
pub fn make_previews_while(
    library: &Path,
    catalog: &Catalog,
    items: &[&FoundItem],
    on_progress: &mut dyn FnMut(usize, usize),
    keep_going: &dyn Fn() -> bool,
) -> Result<PreviewReport, CatalogError> {
    make_previews_paced(library, catalog, items, on_progress, keep_going, Pace::full())
}

/// As [`make_previews_while`], at the pace `pace` allows from moment to
/// moment. `keep_going` is asked between photos, and also while waiting
/// for a turn, so a stop is heard at once.
pub fn make_previews_paced(
    library: &Path,
    catalog: &Catalog,
    items: &[&FoundItem],
    on_progress: &mut dyn FnMut(usize, usize),
    keep_going: &dyn Fn() -> bool,
    pace: Pace<'_>,
) -> Result<PreviewReport, CatalogError> {
    let _busy = keevoro_core::doing::doing(keevoro_core::doing::Work::MakingPreviews);
    let mut report = PreviewReport::default();
    let todo = todo_list(library, catalog, items, &mut report)?;
    if !keep_going() {
        return Ok(report);
    }

    // Making a preview is all thinking and no waiting, so several are
    // made at once, one per worker. The catalog is only ever written from
    // this thread, as each finished preview is handed back.
    let total = todo.len();
    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let (finished, results) = mpsc::channel::<(usize, Result<RenditionState, MediaError>)>();
    let mut failure = None;
    std::thread::scope(|workers| {
        for worker in 0..foreground_workers().min(total.max(1)) {
            let (finished, todo, next, stop) = (finished.clone(), &todo, &next, &stop);
            workers.spawn(move || {
                (pace.on_worker)();
                loop {
                    keevoro_core::pause::wait_while_paused();
                    // Nothing left to take is as good as a stop for a worker
                    // still waiting its turn.
                    let stopped = || stop.load(Ordering::SeqCst) || next.load(Ordering::SeqCst) >= todo.len();
                    if !pace.wait_for_a_turn(worker, &stopped) {
                        break;
                    }
                    let at = next.fetch_add(1, Ordering::SeqCst);
                    let Some((_, original, thumb, display)) = todo.get(at) else {
                        break;
                    };
                    let made = render_photo(original).and_then(|r| {
                        write_rendition(thumb, &r.thumb)?;
                        write_rendition(display, &r.display)?;
                        Ok(RenditionState::Made {
                            width: r.width,
                            height: r.height,
                            blurhash: r.blurhash,
                        })
                    });
                    if finished.send((at, made)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(finished);

        let mut done = 0;
        loop {
            // Asked now and then even when nothing is finishing, since the
            // workers may all be waiting for their turn.
            let (at, made) = match results.recv_timeout(std::time::Duration::from_millis(250)) {
                Ok(finished) => finished,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if !keep_going() {
                        stop.store(true, Ordering::SeqCst);
                    }
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };
            let (hash, original, _, _) = &todo[at];
            let recorded = match made {
                Ok(state) => {
                    report.made += 1;
                    catalog.set_rendition(hash, &state)
                }
                Err(e) => {
                    report.no_preview.push(Failed {
                        path: original.clone(),
                        reason: e.to_string(),
                    });
                    // A disk hiccup may pass, so only lasting reasons are
                    // remembered; the rest are tried again next time.
                    if matches!(e, MediaError::Io(_)) {
                        Ok(())
                    } else {
                        catalog.set_rendition(hash, &RenditionState::NoPreview { reason: e.to_string() })
                    }
                }
            };
            if let Err(e) = recorded {
                failure = Some(e);
                stop.store(true, Ordering::SeqCst);
            }
            done += 1;
            on_progress(done, total);
            if !keep_going() {
                stop.store(true, Ordering::SeqCst);
            }
        }
    });
    if let Some(e) = failure {
        return Err(e);
    }
    // Finished in whatever order the workers got to them; reported in the
    // order they were asked for, so the same run always reads the same.
    report.no_preview.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(report)
}
