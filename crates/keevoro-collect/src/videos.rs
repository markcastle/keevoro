//! Posters for the videos a backup keeps.
//!
//! Like photo previews, these are made from each video where it was found,
//! only reading it. Every video gets a poster, a still picture for a
//! gallery to show, kept in the backup's own folder until it is sent.

use crate::{in_library, Failed};
use keevoro_catalog::{Catalog, CatalogError, VideoState};
use keevoro_core::MediaKind;
use keevoro_index::{rendition_path, Rendition};
use keevoro_media::video::{probe, still};
use keevoro_media::{write_rendition, MediaError, VideoTools};
use keevoro_scan::FoundItem;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Default, Serialize)]
pub struct VideoReport {
    /// Posters made in this run.
    pub posters: usize,
    /// Videos that needed nothing more.
    pub already_there: usize,
    /// Videos with something missing, each with a plain reason. Includes
    /// ones remembered from earlier runs.
    pub problems: Vec<Failed>,
}

/// Makes a poster for every video in `items` that has its place and lacks
/// one. `on_progress` is called with (done, total) after each video that
/// needed work.
pub fn make_video_previews(
    library: &Path,
    catalog: &Catalog,
    items: &[&FoundItem],
    tools: Option<&VideoTools>,
    on_progress: &mut dyn FnMut(usize, usize),
) -> Result<VideoReport, CatalogError> {
    make_video_previews_while(library, catalog, items, tools, on_progress, &|| true)
}

/// As [`make_video_previews`], asking `keep_going` before each video so the
/// work can be stopped.
pub fn make_video_previews_while(
    library: &Path,
    catalog: &Catalog,
    items: &[&FoundItem],
    tools: Option<&VideoTools>,
    on_progress: &mut dyn FnMut(usize, usize),
    keep_going: &dyn Fn() -> bool,
) -> Result<VideoReport, CatalogError> {
    make_video_previews_paced(
        library,
        catalog,
        items,
        tools,
        on_progress,
        keep_going,
        crate::Pace::full(),
    )
}

/// How many videos in `items` still need a poster or their details.
pub fn video_previews_to_make(library: &Path, catalog: &Catalog, items: &[&FoundItem]) -> Result<usize, CatalogError> {
    Ok(todo_list(library, catalog, items, &mut VideoReport::default())?.len())
}

type Todo<'a> = Vec<(&'a str, std::path::PathBuf, std::path::PathBuf)>;

fn todo_list<'a>(
    library: &Path,
    catalog: &Catalog,
    items: &[&'a FoundItem],
    report: &mut VideoReport,
) -> Result<Todo<'a>, CatalogError> {
    let mut todo = Vec::new();
    for item in items.iter().filter(|i| i.kind == MediaKind::Video) {
        let Some(hash) = item.blake3.as_deref() else { continue };
        if catalog.collected_path(hash)?.is_none() {
            continue;
        }
        let original = item.path.clone();
        let id = &hash[..hash.len().min(16)];
        let poster_file = in_library(library, &rendition_path(Rendition::Poster, id));
        match catalog.video(hash)? {
            Some(VideoState::Unreadable { reason }) => report.problems.push(Failed { path: original, reason }),
            Some(VideoState::Known { poster: true, .. }) if poster_file.is_file() => report.already_there += 1,
            _ => todo.push((hash, original, poster_file)),
        }
    }
    Ok(todo)
}

/// As [`make_video_previews_while`], one at a time and only while `pace`
/// allows any, on the calling thread.
pub fn make_video_previews_paced(
    library: &Path,
    catalog: &Catalog,
    items: &[&FoundItem],
    tools: Option<&VideoTools>,
    on_progress: &mut dyn FnMut(usize, usize),
    keep_going: &dyn Fn() -> bool,
    pace: crate::Pace<'_>,
) -> Result<VideoReport, CatalogError> {
    let _busy = keevoro_core::doing::doing(keevoro_core::doing::Work::MakingVideoPreviews);
    let mut report = VideoReport::default();
    let todo = todo_list(library, catalog, items, &mut report)?;
    let total = todo.len();
    for (done, (hash, original, poster_file)) in todo.into_iter().enumerate() {
        keevoro_core::pause::wait_while_paused();
        if !pace.wait_for_a_turn(0, &|| !keep_going()) {
            break;
        }
        match work_on(tools, &original, &poster_file, &mut report) {
            Ok(state) => catalog.set_video(hash, &state)?,
            Err(e) => {
                // A disk hiccup may pass, so only lasting reasons are
                // remembered; the rest are tried again next time.
                if !matches!(e, MediaError::Io(_)) {
                    catalog.set_video(hash, &VideoState::Unreadable { reason: e.to_string() })?;
                }
                report.problems.push(Failed {
                    path: original,
                    reason: e.to_string(),
                });
            }
        }
        on_progress(done + 1, total);
    }
    Ok(report)
}

/// Probes one video and makes whatever is missing. An error means the
/// video could not be read at all.
fn work_on(
    tools: Option<&VideoTools>,
    original: &Path,
    poster_file: &Path,
    report: &mut VideoReport,
) -> Result<VideoState, MediaError> {
    // ffprobe, when it is there, knows the most about a video. Without it
    // the computer's own decoder still gives a poster, a length and a size,
    // which is all a gallery needs.
    let info = match tools {
        Some(tools) => Some(probe(tools, original)?),
        None => None,
    };

    let mut has_poster = poster_file.is_file();
    let mut learnt = None;
    if !has_poster || info.is_none() {
        match still(tools, original) {
            Ok(made) => {
                if !has_poster {
                    write_rendition(poster_file, &made.webp)?;
                    has_poster = true;
                    report.posters += 1;
                }
                learnt = Some((made.duration_secs.unwrap_or(0.0), made.width, made.height));
            }
            // With nothing else known about the video, this is all there is.
            Err(e) if info.is_none() => return Err(e),
            Err(e) => report.problems.push(Failed {
                path: original.to_path_buf(),
                reason: format!("No poster could be made. {e}"),
            }),
        }
    }

    Ok(VideoState::Known {
        duration_secs: info
            .as_ref()
            .map(|i| i.duration_secs)
            .or(learnt.map(|l| l.0))
            .unwrap_or(0.0),
        width: info.as_ref().map(|i| i.width).or(learnt.map(|l| l.1)).unwrap_or(0),
        height: info.as_ref().map(|i| i.height).or(learnt.map(|l| l.2)).unwrap_or(0),
        poster: has_poster,
        mp4: false,
        conversion_problem: None,
    })
}
