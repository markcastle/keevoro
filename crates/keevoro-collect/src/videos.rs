//! Posters and MP4 copies for the videos in the library.
//!
//! Like photo previews, these are made from the library's own verified
//! copies, so the old drive is not needed. Every video gets a poster.
//! Conversion to MP4 only happens when the user has asked for it, and only
//! for videos that need it: a modern MP4 is left exactly as it is.
//!
//! The original is always kept. The MP4 is an extra file under
//! `renditions/video/`, named by the original's content id.

use crate::{in_library, Failed};
use keevoro_catalog::{Catalog, CatalogError, VideoState};
use keevoro_core::MediaKind;
use keevoro_index::{rendition_path, Rendition};
use keevoro_media::video::{convert, plan, probe, still};
use keevoro_media::{write_rendition, Conversion, MediaError, VideoTools};
use keevoro_scan::FoundItem;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Default, Serialize)]
pub struct VideoReport {
    /// Posters made in this run.
    pub posters: usize,
    /// Videos re-encoded to MP4 in this run.
    pub converted: usize,
    /// Videos repackaged as MP4 without re-encoding in this run.
    pub repackaged: usize,
    /// Videos that needed nothing more.
    pub already_there: usize,
    /// Videos with something missing, each with a plain reason. Includes
    /// ones remembered from earlier runs.
    pub problems: Vec<Failed>,
}

/// Makes posters, and MP4 copies when `convert_old` is on, for every
/// gathered video in `items`. `on_progress` is called with (done, total)
/// after each video that needed work.
pub fn make_video_previews(
    library: &Path,
    catalog: &Catalog,
    items: &[&FoundItem],
    tools: Option<&VideoTools>,
    convert_old: bool,
    on_progress: &mut dyn FnMut(usize, usize),
) -> Result<VideoReport, CatalogError> {
    let mut report = VideoReport::default();
    let mut todo = Vec::new();
    for item in items.iter().filter(|i| i.kind == MediaKind::Video) {
        let Some(hash) = item.blake3.as_deref() else { continue };
        let Some(relative) = catalog.collected_path(hash)? else {
            continue;
        };
        let _ = relative;
        let original = crate::bytes_of(library, catalog, item)?;
        let id = &hash[..hash.len().min(16)];
        let poster_file = in_library(library, &rendition_path(Rendition::Poster, id));
        let mp4_file = in_library(library, &rendition_path(Rendition::Video, id));
        match catalog.video(hash)? {
            Some(VideoState::Unreadable { reason }) => report.problems.push(Failed { path: original, reason }),
            Some(VideoState::Known {
                poster: true,
                mp4,
                conversion_problem,
                ..
            }) if poster_file.is_file()
                // Conversion is settled when it is off, done, or was tried
                // and given up on.
                && (!convert_old || (mp4 && mp4_file.is_file()) || conversion_problem.is_some() || !needs_mp4(&original)) =>
            {
                if let (true, Some(reason)) = (convert_old, conversion_problem) {
                    report.problems.push(Failed { path: original, reason });
                } else {
                    report.already_there += 1;
                }
            }
            _ => todo.push((hash, original, poster_file, mp4_file)),
        }
    }

    let total = todo.len();
    for (done, (hash, original, poster_file, mp4_file)) in todo.into_iter().enumerate() {
        match work_on(tools, &original, &poster_file, &mp4_file, convert_old, &mut report) {
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

/// True unless the file is plainly a modern MP4, judged by name alone.
/// Used only to avoid re-probing files that were already settled.
fn needs_mp4(original: &Path) -> bool {
    !matches!(extension(original).as_str(), "mp4" | "m4v")
}

fn extension(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Probes one video and makes whatever is missing. An error means the
/// video could not be read at all.
fn work_on(
    tools: Option<&VideoTools>,
    original: &Path,
    poster_file: &Path,
    mp4_file: &Path,
    convert_old: bool,
    report: &mut VideoReport,
) -> Result<VideoState, MediaError> {
    // ffprobe, when it is there, knows the most about a video and is what
    // conversion is planned from. Without it the computer's own decoder
    // still gives a poster, a length and a size, which is all a gallery
    // needs.
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

    let mut has_mp4 = mp4_file.is_file();
    let mut conversion_problem = None;
    // Converting old videos is ffmpeg's job alone, so it only happens
    // where ffmpeg is.
    if let (Some(tools), Some(info)) = (tools, &info) {
        let action = plan(info, &extension(original));
        if convert_old && !has_mp4 && action != Conversion::Leave {
            match convert(tools, original, info, action, mp4_file) {
                Ok(()) => {
                    has_mp4 = true;
                    match action {
                        Conversion::Remux => report.repackaged += 1,
                        _ => report.converted += 1,
                    }
                }
                Err(MediaError::Io(e)) => return Err(MediaError::Io(e)),
                Err(_) => {
                    let reason = "It couldn't be converted to MP4. The original is safe in your library".to_string();
                    report.problems.push(Failed {
                        path: original.to_path_buf(),
                        reason: reason.clone(),
                    });
                    conversion_problem = Some(reason);
                }
            }
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
        mp4: has_mp4,
        conversion_problem,
    })
}
