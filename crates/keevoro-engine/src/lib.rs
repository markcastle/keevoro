//! What Keevoro does, as plain function calls.
//!
//! The crates below this one each do one job: scan a drive, copy a file,
//! make a thumbnail. This crate strings those jobs into the things a person
//! actually asks for ("look through this drive", "show me what you found",
//! "gather my photos"), and returns plain data describing the result.
//!
//! Two front ends sit on top of it and neither contains any of this logic:
//!
//! - the `keevoro` command line, which prints the results as sentences;
//! - the desktop app, which shows them as screens.
//!
//! Keeping the logic here is what stops the two drifting apart. A rule such
//! as "a picture the user chose to keep is not junk" is written once, in
//! [`wanted`], and both front ends get it.
//!
//! Nothing here prints, prompts or knows about windows. Progress is reported
//! through callbacks so each front end can show it in its own way.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod backup;
pub mod connect;
pub mod discover;
pub mod preview;
pub mod summary;

pub use discover::{
    collection_photos, discover, set_aside, set_collection_kept, set_photo_kept, CollectionView, Discover, PhotoChoice,
    PhotoView, SourceView,
};
pub use discover::{forget_source, friendly_path};
pub use preview::preview;

use anyhow::{bail, Context, Result};
use keevoro_catalog::Catalog;
use keevoro_collect::{
    free_space, gather, make_previews, make_video_previews, GatherError, GatherReport, PreviewReport, VideoReport,
};
use keevoro_core::MediaKind;
use keevoro_media::VideoTools;
use keevoro_scan::{scan, FoundItem, ScanOptions, ScanProgress, ScanReport};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Does the user want this file in their library?
///
/// An explicit choice always wins. Without one, everything is kept except
/// what looks like junk, which is how "junk is pre-unticked" works.
pub fn wanted(catalog: &Catalog, item: &FoundItem) -> Result<bool> {
    let choice = match &item.blake3 {
        Some(hash) => catalog.choice(hash)?,
        None => None,
    };
    Ok(choice.unwrap_or(item.junk.is_none()))
}

/// Looks through one drive or folder and remembers what was found in the
/// library's catalog under a friendly name. Nothing on the drive is
/// changed. Scanning the same place again replaces what was known about it.
pub fn scan_into(
    catalog: &mut Catalog,
    source: &Path,
    name: Option<&str>,
    on_progress: &mut dyn FnMut(&ScanProgress),
) -> Result<ScanReport> {
    // A remembered drive is recognised by its full path, so `D:` and
    // `D:\.` must come out the same.
    let root = std::fs::canonicalize(source).unwrap_or_else(|_| source.to_path_buf());
    let report = scan(&root, &ScanOptions::default(), on_progress);
    let fallback = root
        .file_name()
        .map_or_else(|| root.display().to_string(), |n| n.to_string_lossy().to_string());
    catalog.save_scan(name.unwrap_or(&fallback), &report, now())?;
    Ok(report)
}

/// Which part of gathering is under way, for a progress display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatherStep {
    Copying { done: usize, total: usize },
    MakingPreviews { done: usize, total: usize },
    WorkingOnVideos { done: usize, total: usize },
}

/// Everything that happened in one gather.
#[derive(Debug)]
pub struct GatherOutcome {
    pub copied: GatherReport,
    pub previews: PreviewReport,
    /// `None` when there were no videos, or no ffmpeg to work on them.
    pub videos: Option<VideoReport>,
    /// True when there were videos but ffmpeg is not installed, so they
    /// have no posters or MP4 copies. They are still safely gathered.
    pub videos_need_ffmpeg: bool,
}

impl GatherOutcome {
    /// What happened, as sentences to show the user when gathering ends.
    /// Good news first; anything needing attention last.
    pub fn sentences(&self) -> Vec<String> {
        let mut said = Vec::new();
        let c = &self.copied;
        if c.copied > 0 {
            said.push(format!(
                "Safely copied {} ({}) into your library.",
                summary::count(c.copied, "file"),
                summary::bytes(c.copied_bytes)
            ));
        } else if c.failed.is_empty() {
            said.push("Everything you're keeping was already safely in your library.".to_string());
        }
        let converted = self.videos.as_ref().map_or(0, |v| v.converted + v.repackaged);
        if converted > 0 {
            said.push(format!(
                "Made MP4 copies of {} so they play everywhere. The originals are kept.",
                summary::count(converted, "old video")
            ));
        }
        if !c.failed.is_empty() {
            said.push(format!(
                "{} couldn't be copied. Nothing was lost: plug the source in and gather again to retry.",
                summary::count(c.failed.len(), "file")
            ));
        }
        if !self.previews.no_preview.is_empty() {
            said.push(format!(
                "{} can't be previewed yet. The originals are safe in your library.",
                summary::count(self.previews.no_preview.len(), "photo")
            ));
        }
        if self.videos_need_ffmpeg {
            said.push("Your videos are safe, but have no preview pictures because ffmpeg isn't installed.".to_string());
        }
        said
    }
}

/// Copies everything the user is keeping into the library, verifies each
/// copy, then makes previews. `Ok(None)` means there was nothing to gather.
///
/// Sources are only read. Safe to stop and run again.
pub fn gather_everything(
    library: &Path,
    catalog: &Catalog,
    convert_videos: bool,
    video_tools: Option<&VideoTools>,
    on_step: &mut dyn FnMut(GatherStep),
) -> Result<Option<GatherOutcome>> {
    let reports = catalog.load_reports()?;
    // Rule one: never write to a source. A library inside a scanned folder
    // would do exactly that.
    let home = std::fs::canonicalize(library).unwrap_or_else(|_| library.to_path_buf());
    if let Some(r) = reports.iter().find(|r| home.starts_with(&r.root)) {
        bail!(
            "The library is inside {}, which is one of the places being gathered from. \
             Keevoro never writes to those. Choose a library folder somewhere else.",
            r.root.display()
        );
    }

    let (unique, _) = summary::unique_items(&reports);
    let mut chosen = Vec::new();
    for item in unique {
        if wanted(catalog, item)? {
            chosen.push(item);
        }
    }
    if chosen.is_empty() {
        return Ok(None);
    }

    let copied = match gather(library, catalog, &chosen, free_space(library), &mut |p| {
        on_step(GatherStep::Copying {
            done: p.done,
            total: p.total,
        });
    }) {
        Ok(report) => report,
        Err(GatherError::NotEnoughRoom { needed, free }) => bail!(
            "There isn't enough room for your library. It needs {} and the disk has {} free.",
            summary::bytes(needed),
            summary::bytes(free)
        ),
        Err(e) => return Err(e).context("Couldn't gather your photos"),
    };

    // Previews come from the library's own copies, so they are made for
    // everything gathered so far, including on earlier runs.
    let previews = make_previews(library, catalog, &chosen, &mut |done, total| {
        on_step(GatherStep::MakingPreviews { done, total });
    })?;

    let has_videos = chosen.iter().any(|i| i.kind == MediaKind::Video);
    let videos = match video_tools {
        Some(tools) if has_videos => Some(make_video_previews(
            library,
            catalog,
            &chosen,
            tools,
            convert_videos,
            &mut |done, total| on_step(GatherStep::WorkingOnVideos { done, total }),
        )?),
        _ => None,
    };

    Ok(Some(GatherOutcome {
        copied,
        previews,
        videos_need_ffmpeg: has_videos && video_tools.is_none(),
        videos,
    }))
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(0)
}
