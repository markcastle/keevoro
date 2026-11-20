//! Keevoro's own folders, met inside a place being looked through or
//! surveyed: which they are on this computer, and what is said about
//! leaving them out.
//!
//! A backup, a backup's own folder, a library an older version made, and
//! Keevoro's data, cache and log folders are never a source: backing a
//! backup up into another is never what a person means. The search and the
//! survey recognise most of them by what is in them (`keevoro_core::skip`);
//! the places this computer knows about (every backup's destinations and
//! home folder, and Keevoro's own folders) are left out by their full path
//! too, even before a backup has written anything there. Nothing is left
//! out silently: each one is said, with how to include a backup's photos
//! on purpose, and an old library is described so the person can decide
//! about it themselves. Keevoro never removes one.

use crate::keevoro::Keevoro;
use keevoro_core::{SkipReason, SkipRules};
use keevoro_engine::friendly_path;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// One folder Keevoro made, left out of a search or a survey.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftOut {
    /// As a person would write it, and as it is handed back to show it.
    pub folder: String,
    /// `keevoroBackup`, `keevoroOwn` or `oldLibrary`.
    pub what: String,
    /// What is said about it, in a sentence or two.
    pub said: String,
}

impl Keevoro {
    /// The rules for a search or a survey: the usual ones, and Keevoro's
    /// own places on this computer, each by its full path.
    pub(crate) fn skip_rules(&self) -> SkipRules {
        let mut own = Vec::new();
        let mut add = |place: &Path, reason: SkipReason| {
            if let Ok(real) = std::fs::canonicalize(place) {
                own.push((real, reason));
            }
        };
        for named in self.backups().all() {
            add(&named.home, SkipReason::KeevoroOwn);
            let Some(backup) = crate::backup::Backup::open_existing(&named.home) else {
                continue;
            };
            for saved in keevoro_engine::connect::destinations(backup.catalog()).unwrap_or_default() {
                if saved.kind == "folder" {
                    add(Path::new(&saved.place), SkipReason::KeevoroBackup);
                }
            }
        }
        let folders = [
            self.backups().data_folder().to_path_buf(),
            self.backups().config_folder().to_path_buf(),
            self.cache_folder(),
            self.log().path().parent().map(Path::to_path_buf).unwrap_or_default(),
        ];
        for folder in folders {
            add(&folder, SkipReason::KeevoroOwn);
        }
        SkipRules::default().with_own_places(own)
    }
}

/// What to say about each folder Keevoro made that was left out. `found
/// before` is what an earlier look through the same place had found, so
/// that files no longer offered are counted.
pub(crate) fn left_out(skipped: &[(PathBuf, SkipReason)], found_before: &[PathBuf]) -> Vec<LeftOut> {
    skipped
        .iter()
        .filter(|(_, reason)| reason.keevoro_made())
        .map(|(folder, reason)| {
            let at = friendly_path(folder);
            let mut said = match reason {
                SkipReason::OldLibrary => old_library(folder, &at),
                other => other.left_out(&at).unwrap_or_default(),
            };
            let before = found_before.iter().filter(|p| p.starts_with(folder)).count();
            if before > 0 {
                said.push_str(&format!(
                    " The {} found there before {} no longer offered; anything already backed up stays backed up.",
                    keevoro_engine::summary::count(before, "file"),
                    if before == 1 { "is" } else { "are" }
                ));
            }
            LeftOut {
                folder: at,
                what: match reason {
                    SkipReason::KeevoroBackup => "keevoroBackup",
                    SkipReason::OldLibrary => "oldLibrary",
                    _ => "keevoroOwn",
                }
                .to_string(),
                said,
            }
        })
        .collect()
}

/// An old library, described: what it is, where, how big, and how much of
/// it is copies of photos. Working that out reads the folder listings only.
fn old_library(folder: &Path, at: &str) -> String {
    let (mut copies, mut copies_bytes, mut other_bytes) = (0usize, 0u64, 0u64);
    let originals = folder.join("originals");
    // Listings only, and links never followed.
    let mut folders = vec![folder.to_path_buf()];
    while let Some(next) = folders.pop() {
        for entry in std::fs::read_dir(&next).into_iter().flatten().flatten() {
            let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if meta.is_dir() {
                folders.push(entry.path());
            } else if meta.is_file() && entry.path().starts_with(&originals) {
                copies += 1;
                copies_bytes += meta.len();
            } else if meta.is_file() {
                other_bytes += meta.len();
            }
        }
    }
    let bytes = keevoro_engine::summary::bytes;
    format!(
        "Left out {at}: a folder an older version of Keevoro made, which it no longer uses. It takes {}: {} of \
         copies of your photos it gathered there ({}), and {} of previews and working files it made. Keevoro has \
         not checked that every copy is also in one of your backups, so it leaves the folder for you to decide about \
         and never removes it.",
        bytes(copies_bytes + other_bytes),
        bytes(copies_bytes),
        keevoro_engine::summary::count(copies, "file"),
        bytes(other_bytes)
    )
}
