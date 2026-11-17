//! Making thumbnails and display copies for everything in the library.
//!
//! Previews are made from the library's own verified copies, never from the
//! old drive, so this step works after the drive has gone back in the
//! drawer. Each preview is named after the file's content id, which means
//! renaming or moving an album never breaks a thumbnail.
//!
//! Some files cannot be previewed yet (HEIC, camera RAW, damaged pictures).
//! That is remembered with its reason, so the same file is not attempted on
//! every run, and the original stays safely in the library regardless.

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

/// How many previews to make at once: half the processor's cores, and never
/// more than four. Enough to be several times quicker than one at a time,
/// while leaving the rest of the computer to its owner.
fn workers_for(photos: usize) -> usize {
    let cores = std::thread::available_parallelism().map_or(2, std::num::NonZero::get);
    (cores / 2).clamp(1, 4).min(photos.max(1))
}

/// Makes previews for every gathered photo in `items` that lacks them.
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
    let mut report = PreviewReport::default();
    let mut todo = Vec::new();
    for item in items.iter().filter(|i| i.kind == MediaKind::Photo) {
        let Some(hash) = item.blake3.as_deref() else { continue };
        let Some(relative) = catalog.collected_path(hash)? else {
            continue;
        };
        let id = &hash[..hash.len().min(16)];
        let thumb = in_library(library, &rendition_path(Rendition::Thumb, id));
        let display = in_library(library, &rendition_path(Rendition::Display, id));
        match catalog.rendition(hash)? {
            Some(RenditionState::Made { .. }) if thumb.is_file() && display.is_file() => report.already_there += 1,
            Some(RenditionState::NoPreview { reason }) => report.no_preview.push(Failed {
                path: in_library(library, &relative),
                reason,
            }),
            _ => todo.push((hash, crate::bytes_of(library, catalog, item)?, thumb, display)),
        }
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
        for _ in 0..workers_for(total) {
            let (finished, todo, next, stop) = (finished.clone(), &todo, &next, &stop);
            workers.spawn(move || loop {
                keevoro_core::pause::wait_while_paused();
                if stop.load(Ordering::SeqCst) {
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
            });
        }
        drop(finished);

        let mut done = 0;
        for (at, made) in results {
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
