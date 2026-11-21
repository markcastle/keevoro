//! Places that are never worth scanning.
//!
//! Skipping these keeps scans fast and keeps app icons, browser caches and
//! game textures out of the user's library. Every skip carries a plain
//! reason the UI can show.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The note Keevoro writes into every backup, explaining its folders.
pub const BACKUP_GUIDE: &str = "What is in this backup.txt";

/// Where Keevoro keeps its record of a backup, inside the backup's own
/// folder; also where the library of versions before 0.30 kept its record.
pub const RECORD: &str = ".keevoro/catalog.sqlite";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    SystemFolder,
    AppFolder,
    RecycleBin,
    HiddenFolder,
    DeveloperFolder,
    SystemFile,
    /// A backup Keevoro made: a backup already, not a source.
    KeevoroBackup,
    /// Keevoro's own working space: a backup's record and previews, or
    /// Keevoro's own data, cache or log.
    KeevoroOwn,
    /// A library made by a version of Keevoro before 0.30, which gathered
    /// copies of photos into it. Keevoro no longer uses it.
    OldLibrary,
    /// A folder the person left out of this backup.
    LeftOut,
}

impl SkipReason {
    /// Wording for the UI.
    pub fn describe(self) -> &'static str {
        match self {
            Self::SystemFolder => "System folder",
            Self::AppFolder => "App or cache folder",
            Self::RecycleBin => "Recycle bin or trash",
            Self::HiddenFolder => "Hidden folder",
            Self::DeveloperFolder => "Developer folder",
            Self::SystemFile => "System file",
            Self::KeevoroBackup => "A Keevoro backup",
            Self::KeevoroOwn => "Keevoro's own files",
            Self::OldLibrary => "A library from an older Keevoro",
            Self::LeftOut => "Left out by you",
        }
    }

    /// True for a folder left out because Keevoro made it.
    pub fn keevoro_made(self) -> bool {
        matches!(self, Self::KeevoroBackup | Self::KeevoroOwn | Self::OldLibrary)
    }

    /// What is said when a search or survey leaves out a folder Keevoro
    /// made, `at` being where it is as a person would write it.
    pub fn left_out(self, at: &str) -> Option<String> {
        match self {
            Self::KeevoroBackup => Some(format!(
                "Left out a Keevoro backup at {at}: it is a backup already, not a source. To back up its photos \
                 somewhere else too, add its originals folder as a source."
            )),
            Self::KeevoroOwn => Some(format!(
                "Left out Keevoro's own files at {at}: its records and previews, not your photos."
            )),
            Self::OldLibrary => Some(format!(
                "Left out {at}: a folder an older version of Keevoro made, which it no longer uses. It holds copies \
                 of photos it gathered there and previews it made; Keevoro leaves it to you to remove."
            )),
            Self::LeftOut => Some(format!(
                "Left out {at}, as you chose: it is not looked through for this backup. Put it back to include it again."
            )),
            _ => None,
        }
    }
}

/// Did Keevoro make this folder, and so it is no source? Judged by what is
/// in it, never by its name, since a person can call a backup anything:
///
/// - Keevoro's record (`.keevoro/catalog.sqlite`): a backup's own folder,
///   or a library from before 0.30 with its `originals` and `renditions`.
/// - The note every backup has (`What is in this backup.txt`) beside an
///   `index` folder: a backup, wherever it was sent. Both together are
///   very unlikely by chance.
///
/// `has_file` and `has_folder` answer for names inside the folder.
pub fn made_by_keevoro(has_file: &dyn Fn(&str) -> bool, has_folder: &dyn Fn(&str) -> bool) -> Option<SkipReason> {
    if has_file(RECORD) && has_folder("originals") {
        Some(SkipReason::OldLibrary)
    } else if has_file(RECORD) {
        Some(SkipReason::KeevoroOwn)
    } else if has_file(BACKUP_GUIDE) && has_folder("index") {
        Some(SkipReason::KeevoroBackup)
    } else {
        None
    }
}

/// [`made_by_keevoro`], for a folder on disk. Looks at three names in it,
/// never at what is in them.
pub fn folder_made_by_keevoro(folder: &Path) -> Option<SkipReason> {
    made_by_keevoro(&|name| folder.join(name).is_file(), &|name| folder.join(name).is_dir())
}

/// Folder and file names to skip. Names are compared without case.
#[derive(Debug, Clone)]
pub struct SkipRules {
    folders: Vec<(String, SkipReason)>,
    skip_hidden: bool,
    /// Keevoro's own places on this computer, each as the computer gives
    /// its full path: every backup's destinations and home folders, and
    /// Keevoro's data, cache and log folders.
    own: Vec<(PathBuf, SkipReason)>,
    /// Folders the person left out of the backup in hand, by full path:
    /// each is left out with everything beneath it.
    left_out: Vec<PathBuf>,
}

impl Default for SkipRules {
    fn default() -> Self {
        use SkipReason::*;
        let folders: &[(&str, SkipReason)] = &[
            // Windows
            ("windows", SystemFolder),
            ("program files", SystemFolder),
            ("program files (x86)", SystemFolder),
            ("programdata", SystemFolder),
            ("system volume information", SystemFolder),
            ("$windows.~bt", SystemFolder),
            ("$windows.~ws", SystemFolder),
            ("msocache", SystemFolder),
            ("appdata", AppFolder),
            ("application data", AppFolder),
            ("local settings", AppFolder),
            ("temporary internet files", AppFolder),
            ("$recycle.bin", RecycleBin),
            ("recycler", RecycleBin),
            ("recycled", RecycleBin),
            // macOS
            ("system", SystemFolder),
            ("library", AppFolder),
            ("applications", AppFolder),
            ("caches", AppFolder),
            ("backups.backupdb", SystemFolder),
            (".trashes", RecycleBin),
            (".trash", RecycleBin),
            // Linux
            ("proc", SystemFolder),
            ("sys", SystemFolder),
            ("lost+found", SystemFolder),
            // Developer
            ("node_modules", DeveloperFolder),
            (".git", DeveloperFolder),
            ("__pycache__", DeveloperFolder),
        ];
        Self {
            folders: folders.iter().map(|(n, r)| ((*n).to_string(), *r)).collect(),
            skip_hidden: true,
            own: Vec::new(),
            left_out: Vec::new(),
        }
    }
}

impl SkipRules {
    /// Rules that skip nothing, for tests and power users.
    pub fn none() -> Self {
        Self {
            folders: Vec::new(),
            skip_hidden: false,
            own: Vec::new(),
            left_out: Vec::new(),
        }
    }

    /// Also leaves out these places, Keevoro's own, each given as the
    /// computer gives its full path: a backup's destination folder is
    /// [`SkipReason::KeevoroBackup`] even before its first backup has
    /// written anything there.
    #[must_use]
    pub fn with_own_places(mut self, own: Vec<(PathBuf, SkipReason)>) -> Self {
        self.own = own;
        self
    }

    /// Also leaves out these folders, which the person left out of the
    /// backup in hand, each given as the computer gives its full path,
    /// with everything beneath them ([`SkipReason::LeftOut`]).
    #[must_use]
    pub fn with_left_out(mut self, folders: Vec<PathBuf>) -> Self {
        self.left_out = folders;
        self
    }

    /// Should this folder, by its full path as the computer gives it, be
    /// left out by its place: as one of Keevoro's own, or because the
    /// person left it, or a folder it is in, out? A folder a search or a
    /// survey begins in is asked too, so one inside a left-out folder is
    /// never looked through.
    pub fn own_place(&self, folder: &Path) -> Option<SkipReason> {
        self.own
            .iter()
            .find(|(place, _)| crate::places::same_place(place, folder))
            .map(|(_, r)| *r)
            .or_else(|| {
                self.left_out
                    .iter()
                    .any(|out| crate::places::within(folder, out))
                    .then_some(SkipReason::LeftOut)
            })
    }

    /// Should a folder with this name be skipped, and why?
    pub fn skip_folder(&self, name: &str) -> Option<SkipReason> {
        let lower = name.to_lowercase();
        if let Some((_, r)) = self.folders.iter().find(|(n, _)| *n == lower) {
            return Some(*r);
        }
        if self.skip_hidden && name.starts_with('.') {
            return Some(SkipReason::HiddenFolder);
        }
        None
    }

    /// Should a file with this name be skipped before reading it?
    pub fn skip_file(&self, name: &str) -> Option<SkipReason> {
        let lower = name.to_lowercase();
        // macOS AppleDouble resource files carry media extensions but no media.
        // The rest are the system's own: Windows' page, hibernation and
        // swap files and the Mac's sleep image are the size of the
        // computer's memory, locked while it runs, and never anyone's
        // file, so nobody adding .sys as an other kind means them.
        if name.starts_with("._")
            || matches!(
                lower.as_str(),
                "thumbs.db"
                    | "desktop.ini"
                    | ".ds_store"
                    | "pagefile.sys"
                    | "hiberfil.sys"
                    | "swapfile.sys"
                    | "sleepimage"
            )
        {
            return Some(SkipReason::SystemFile);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_system_and_app_folders_on_both_platforms() {
        let r = SkipRules::default();
        assert_eq!(r.skip_folder("Windows"), Some(SkipReason::SystemFolder));
        assert_eq!(r.skip_folder("Program Files (x86)"), Some(SkipReason::SystemFolder));
        assert_eq!(r.skip_folder("AppData"), Some(SkipReason::AppFolder));
        assert_eq!(r.skip_folder("$RECYCLE.BIN"), Some(SkipReason::RecycleBin));
        assert_eq!(r.skip_folder("Library"), Some(SkipReason::AppFolder));
        assert_eq!(r.skip_folder("Backups.backupdb"), Some(SkipReason::SystemFolder));
        assert_eq!(r.skip_folder(".Trashes"), Some(SkipReason::RecycleBin));
        assert_eq!(r.skip_folder("node_modules"), Some(SkipReason::DeveloperFolder));
    }

    #[test]
    fn a_folder_left_out_is_left_out_with_all_beneath_it_and_nothing_beside_it() {
        let photos = PathBuf::from("/home/mark/Photos");
        let rules = SkipRules::default().with_left_out(vec![photos.clone()]);
        assert_eq!(rules.own_place(&photos), Some(SkipReason::LeftOut));
        assert_eq!(rules.own_place(&photos.join("2019")), Some(SkipReason::LeftOut));
        assert_eq!(rules.own_place(Path::new("/home/mark/Photos 2")), None);
        assert_eq!(rules.own_place(Path::new("/home/mark")), None);
        assert!(!SkipReason::LeftOut.keevoro_made());
        assert_eq!(SkipReason::LeftOut.describe(), "Left out by you");
        // Keevoro's own place is said first: it can't be put back.
        let both = rules.with_own_places(vec![(photos.clone(), SkipReason::KeevoroBackup)]);
        assert_eq!(both.own_place(&photos), Some(SkipReason::KeevoroBackup));
    }

    #[test]
    fn hidden_folders_skipped_by_default_only() {
        assert_eq!(
            SkipRules::default().skip_folder(".thumbnails"),
            Some(SkipReason::HiddenFolder)
        );
        assert_eq!(SkipRules::none().skip_folder(".thumbnails"), None);
        assert_eq!(SkipRules::none().skip_folder("Windows"), None);
    }

    #[test]
    fn keeps_ordinary_folders() {
        let r = SkipRules::default();
        for name in ["Holidays", "Photo Library", "My Pictures", "Windows Photos 2009"] {
            assert_eq!(r.skip_folder(name), None, "{name}");
        }
    }

    #[test]
    fn skips_system_files() {
        let r = SkipRules::default();
        assert_eq!(r.skip_file("._IMG_0001.JPG"), Some(SkipReason::SystemFile));
        assert_eq!(r.skip_file("Thumbs.db"), Some(SkipReason::SystemFile));
        assert_eq!(r.skip_file(".DS_Store"), Some(SkipReason::SystemFile));
        assert_eq!(r.skip_file("IMG_0001.JPG"), None);
        for system in ["pagefile.sys", "HIBERFIL.SYS", "swapfile.sys", "sleepimage"] {
            assert_eq!(r.skip_file(system), Some(SkipReason::SystemFile), "{system}");
        }
        assert_eq!(r.skip_file("driver.sys"), None, "only the system's own, by name");
    }

    #[test]
    fn keevoros_own_folders_are_known_by_what_is_in_them_not_their_names() {
        let with = |files: &'static [&'static str], folders: &'static [&'static str]| {
            made_by_keevoro(&|n| files.contains(&n), &|n| folders.contains(&n))
        };
        assert_eq!(
            with(&[RECORD], &["originals", "renditions"]),
            Some(SkipReason::OldLibrary),
            "an old library"
        );
        assert_eq!(
            with(&[RECORD], &[]),
            Some(SkipReason::KeevoroOwn),
            "a backup's own folder"
        );
        assert_eq!(
            with(&[BACKUP_GUIDE], &["index", "originals", "renditions"]),
            Some(SkipReason::KeevoroBackup)
        );
        assert_eq!(
            with(&[BACKUP_GUIDE], &["originals"]),
            None,
            "the note alone could be a copy of it"
        );
        assert_eq!(
            with(&[], &["index", "originals", "renditions"]),
            None,
            "ordinary names alone are not enough"
        );
    }

    #[test]
    fn keevoros_own_places_are_left_out_by_their_full_path() {
        let rules = SkipRules::default()
            .with_own_places(vec![(PathBuf::from("/media/usb/Backups"), SkipReason::KeevoroBackup)]);
        assert_eq!(
            rules.own_place(Path::new("/media/usb/Backups")),
            Some(SkipReason::KeevoroBackup)
        );
        assert_eq!(rules.own_place(Path::new("/media/usb/Backups/originals")), None);
        assert_eq!(rules.own_place(Path::new("/media/usb")), None);
    }

    #[test]
    fn leaving_out_keevoros_own_folders_is_said_and_says_how_to_include_them() {
        let said = SkipReason::KeevoroBackup.left_out("F:\\Backup").unwrap();
        assert!(said.starts_with("Left out a Keevoro backup at F:\\Backup: it is a backup already, not a source."));
        assert!(said.contains("add its originals folder as a source"));
        assert!(SkipReason::KeevoroOwn
            .left_out("x")
            .unwrap()
            .contains("not your photos"));
        assert_eq!(SkipReason::HiddenFolder.left_out("x"), None);
        assert!(SkipReason::KeevoroOwn.keevoro_made() && !SkipReason::AppFolder.keevoro_made());
    }

    #[test]
    fn every_reason_has_wording() {
        use SkipReason::*;
        for r in [
            SystemFolder,
            AppFolder,
            RecycleBin,
            HiddenFolder,
            DeveloperFolder,
            SystemFile,
            KeevoroBackup,
            KeevoroOwn,
            OldLibrary,
        ] {
            assert!(!r.describe().is_empty());
        }
    }
}
