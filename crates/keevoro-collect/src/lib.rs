//! Getting kept files ready to be backed up, without copying them.
//!
//! Every file being kept is given its place: the folders and name it will
//! have wherever it is backed up to, so that every copy of a file, at every
//! destination, has the same folder and name. Places are agreed once and
//! remembered in the catalog. Two different files never share one: the
//! second `IMG_1234.jpg` in a folder becomes `IMG_1234 (2).jpg`.
//!
//! This crate also makes the previews and video posters a backup carries
//! for a gallery. Nothing here writes to a source: the files stay where
//! they were found and are read from there.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod previews;
mod videos;

pub use previews::{
    foreground_workers, make_previews, make_previews_paced, make_previews_while, previews_to_make, Pace, PreviewReport,
};
pub use videos::{
    make_video_previews, make_video_previews_paced, make_video_previews_while, video_previews_to_make, VideoReport,
};

use keevoro_catalog::{Catalog, CatalogError};
use keevoro_core::{library_folder, numbered_name, safe_name};
use keevoro_scan::FoundItem;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct Failed {
    pub path: PathBuf,
    pub reason: String,
}

/// Free space on the disk holding `path`, if the system will say.
pub fn free_space(path: &Path) -> Option<u64> {
    // The folder may not exist yet; ask about the nearest folder that does.
    path.ancestors()
        .find(|p| p.is_dir())
        .and_then(|p| fs4::available_space(p).ok())
}

/// Turns a path relative to a backup's own folder (`a/b/c.jpg`) into a
/// real path.
pub(crate) fn in_library(library: &Path, relative: &str) -> PathBuf {
    relative.split('/').fold(library.to_path_buf(), |p, s| p.join(s))
}

/// The folders and name a file belongs under, from its album and date.
fn place_for(item: &FoundItem) -> (Vec<String>, String) {
    let name = item
        .path
        .file_name()
        .map_or_else(|| "unnamed".to_string(), |n| safe_name(&n.to_string_lossy()));
    (library_folder(item.kind, &item.folder.album, item.date.as_ref()), name)
}

/// Agrees a place for each of `items` without copying anything: the files
/// go straight from their source to each destination. A file that already
/// has a place keeps it. Returns how many were given one now.
///
/// A file a search only sampled has no whole fingerprint yet. With
/// `sampled_too` it is given a place all the same, under its quick
/// fingerprint, so that it can be fingerprinted as it is sent instead of
/// being read once beforehand. The caller must only pass such files when
/// no other file shares their quick fingerprint: a place is one file's.
pub fn agree_places(
    catalog: &Catalog,
    items: &[&FoundItem],
    sampled_too: bool,
    on_progress: &mut dyn FnMut(usize, usize),
) -> Result<usize, CatalogError> {
    // One change to the catalog, not one per file: with thousands of files
    // the difference is seconds against minutes.
    catalog.in_one_go(|| agree_each(catalog, items, sampled_too, on_progress))
}

fn agree_each(
    catalog: &Catalog,
    items: &[&FoundItem],
    sampled_too: bool,
    on_progress: &mut dyn FnMut(usize, usize),
) -> Result<usize, CatalogError> {
    let mut placed = 0;
    for (done, item) in items.iter().enumerate() {
        if done % 250 == 0 {
            on_progress(done, items.len());
        }
        let Some(hash) = item.blake3.as_deref() else { continue };
        if !item.hashed && !sampled_too {
            continue;
        }
        if catalog.collected_path(hash)?.is_some() {
            continue;
        }
        let (folders, name) = place_for(item);
        let folder = folders.join("/");
        // Two different files with the same name in the same folder: the
        // second becomes "IMG_1234 (2).jpg", as it would on a disk.
        let mut place = format!("{folder}/{name}");
        let mut n = 2;
        while catalog.place_taken(&place)? {
            place = format!("{folder}/{}", numbered_name(&name, n));
            n += 1;
        }
        catalog.mark_placed(hash, &place, item.bytes)?;
        placed += 1;
    }
    Ok(placed)
}

#[cfg(test)]
mod tests;
