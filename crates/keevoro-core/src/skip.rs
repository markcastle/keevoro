//! Places that are never worth scanning.
//!
//! Skipping these keeps scans fast and keeps app icons, browser caches and
//! game textures out of the user's library. Every skip carries a plain
//! reason the UI can show.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    SystemFolder,
    AppFolder,
    RecycleBin,
    HiddenFolder,
    DeveloperFolder,
    SystemFile,
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
        }
    }
}

/// Folder and file names to skip. Names are compared without case.
#[derive(Debug, Clone)]
pub struct SkipRules {
    folders: Vec<(String, SkipReason)>,
    skip_hidden: bool,
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
        }
    }
}

impl SkipRules {
    /// Rules that skip nothing, for tests and power users.
    pub fn none() -> Self {
        Self {
            folders: Vec::new(),
            skip_hidden: false,
        }
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
        if name.starts_with("._") || matches!(lower.as_str(), "thumbs.db" | "desktop.ini" | ".ds_store") {
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
        ] {
            assert!(!r.describe().is_empty());
        }
    }
}
