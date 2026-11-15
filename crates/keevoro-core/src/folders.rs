//! Reading a user's existing folder structure.
//!
//! People have often already organised their photos into folders. Keevoro
//! keeps that work: the meaningful part of each path becomes a nested album,
//! and folder names that contain dates become date hints. Noise added by
//! operating systems, cameras and backup tools is trimmed away.
//!
//! `D:\My Pictures\Holidays\Spain 2009\Day 2` becomes the album
//! `Holidays / Spain 2009 / Day 2` with a date hint of 2009.

use crate::date::{date_from_folder, PartialDate};
use regex::Regex;
use serde::Serialize;
use std::sync::LazyLock;

/// What a file's folder path says about it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct FolderHints {
    /// Meaningful folder names from the outermost to the innermost.
    /// Empty when the folders said nothing useful.
    pub album: Vec<String>,
    /// The most specific date found in any folder name, innermost wins.
    pub date: Option<PartialDate>,
}

impl FolderHints {
    /// The album as the user will see it, for example `Holidays / Spain 2009`.
    pub fn album_label(&self) -> Option<String> {
        (!self.album.is_empty()).then(|| self.album.join(" / "))
    }
}

/// Folder names that never mean anything to the user. Compared without case.
const NOISE: &[&str] = &[
    "dcim",
    "pictures",
    "my pictures",
    "photos",
    "my photos",
    "images",
    "videos",
    "my videos",
    "movies",
    "camera",
    "camera roll",
    "camera uploads",
    "new folder",
    "untitled folder",
    "documents",
    "my documents",
    "desktop",
    "downloads",
    "onedrive",
    "dropbox",
    "google drive",
    "icloud drive",
    "import",
    "imports",
    "imported",
    "sd card",
    "removable disk",
    "mobile",
    "phone",
];

/// Folders whose own name and the next folder (a user account name) are noise.
const ACCOUNT_ROOTS: &[&str] = &["users", "documents and settings", "home"];

/// Works out the album path and date hint from the folders between a source's
/// root and a file. `segments` excludes the file name.
pub fn folder_hints<S: AsRef<str>>(segments: &[S]) -> FolderHints {
    let mut hints = FolderHints::default();
    let mut skip_next = false;
    for raw in segments {
        let raw = raw.as_ref().trim();
        if skip_next {
            skip_next = false;
            continue;
        }
        if raw.is_empty() {
            continue;
        }
        let lower = raw.to_lowercase();
        if ACCOUNT_ROOTS.contains(&lower.as_str()) {
            skip_next = true;
            continue;
        }
        let Some(name) = clean_name(raw) else {
            continue;
        };
        if let Some(fd) = date_from_folder(&name) {
            hints.date = Some(fd.date);
            if fd.date_only {
                continue;
            }
        }
        hints.album.push(name);
    }
    hints
}

/// Removes noise from one folder name. Returns `None` when nothing
/// meaningful is left.
fn clean_name(raw: &str) -> Option<String> {
    static COPY_SUFFIX: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\s+-\s+copy(?:\s*\(\d+\))?$"));
    static COPY_PREFIX: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:copy of|backup of)\s+"));
    static NUMBERED_NOISE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:new folder|untitled folder)\s*\(?\d*\)?$"));
    // Camera folders like 100CANON, 101NIKON, 100APPLE, 100_FUJI, 101MSDCF.
    static CAMERA: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^\d{3}[a-z0-9_]{5}$"));
    // Drive-only remainders like "C", "C:", "C drive", "drive D".
    static DRIVE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:[a-z]:?|[a-z]:?\s+drive|drive\s+[a-z]:?)$"));

    let mut name = COPY_SUFFIX.replace(raw, "").trim().to_string();
    name = COPY_PREFIX.replace(&name, "").trim().to_string();
    let lower = name.to_lowercase();
    if name.is_empty()
        || NOISE.contains(&lower.as_str())
        || NUMBERED_NOISE.is_match(&name)
        || CAMERA.is_match(&name)
        || DRIVE.is_match(&name)
    {
        return None;
    }
    Some(name)
}

fn re(pattern: &str) -> Regex {
    // Patterns are compile-time constants covered by tests.
    #[allow(clippy::expect_used)]
    Regex::new(pattern).expect("built-in folder pattern must compile")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hints(path: &str) -> FolderHints {
        let segs: Vec<&str> = path.split(['/', '\\']).collect();
        folder_hints(&segs)
    }

    #[test]
    fn keeps_meaningful_nested_folders() {
        let h = hints(r"My Pictures\Holidays\Spain 2009\Day 2");
        assert_eq!(h.album_label().as_deref(), Some("Holidays / Spain 2009 / Day 2"));
        assert_eq!(h.date.unwrap().to_string(), "2009");
    }

    #[test]
    fn trims_user_profile_paths() {
        let h = hints("Users/mark/Pictures/Family/Christmas 2011");
        assert_eq!(h.album, vec!["Family", "Christmas 2011"]);
        let h = hints(r"Documents and Settings\Dad\My Documents\My Pictures\Garden");
        assert_eq!(h.album, vec!["Garden"]);
    }

    #[test]
    fn camera_folders_are_noise() {
        for path in [
            "DCIM/100CANON",
            "DCIM/101NIKON",
            "DCIM/100APPLE",
            "DCIM/100_FUJI",
            "DCIM/101MSDCF",
        ] {
            assert!(hints(path).album.is_empty(), "{path}");
        }
    }

    #[test]
    fn date_only_folders_are_hints_not_albums() {
        let h = hints("Photos/2009/2009_07_14");
        assert!(h.album.is_empty());
        assert_eq!(h.date.unwrap().to_string(), "2009-07-14");
    }

    #[test]
    fn innermost_date_wins_but_names_are_kept() {
        let h = hints("2009/2009-07 Spain/Beach");
        assert_eq!(h.album, vec!["2009-07 Spain", "Beach"]);
        assert_eq!(h.date.unwrap().to_string(), "2009-07");
    }

    #[test]
    fn strips_copy_and_backup_wrappers() {
        assert_eq!(hints("Holidays - Copy/Spain").album, vec!["Holidays", "Spain"]);
        assert_eq!(hints("Holidays - Copy (2)").album, vec!["Holidays"]);
        assert_eq!(hints("Copy of Wedding").album, vec!["Wedding"]);
        assert!(hints("Backup of C/New Folder (3)").album.is_empty());
        assert!(hints("Backup of C drive").album.is_empty());
    }

    #[test]
    fn same_folder_on_two_drives_gives_the_same_album() {
        let a = hints(r"Users\mark\Pictures\Spain 2009");
        let b = hints(r"Backup of C\Users\mark\Pictures\Spain 2009");
        assert_eq!(a.album, b.album);
    }

    #[test]
    fn nothing_useful_gives_no_album() {
        let h = hints("DCIM/100CANON");
        assert_eq!(h.album_label(), None);
        assert_eq!(h.date, None);
        assert_eq!(folder_hints::<&str>(&[]), FolderHints::default());
    }

    #[test]
    fn empty_segments_are_ignored() {
        assert_eq!(hints("/Holidays//Spain/").album, vec!["Holidays", "Spain"]);
    }
}
