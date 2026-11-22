//! How Keevoro's window and its icon by the clock behave: whether closing
//! the window keeps Keevoro running, whether minimising hides it too,
//! whether the person has been told what closing does, whether a backup
//! by itself may tell them when it finishes, and when they last looked.
//!
//! These are the app's own choices, kept in a file of their own,
//! `window.json` in Keevoro's config folder, never in `settings.json`:
//! older versions write `settings.json` back without parts they do not
//! know, and no older version knows this file, so none ever rewrites it.
//! The command line and an AI agent never read it.
//!
//! It is read a part at a time, so a part that cannot be understood costs
//! that part alone; a file that could not all be understood is kept beside
//! it before anything is written over it; parts a later version adds are
//! kept and written back. A missing file means every choice as it comes.

use crate::error::{Error, Result};
use crate::keevoro::Keevoro;
use std::path::Path;

/// The file, in Keevoro's config folder.
pub const FILE: &str = "window.json";

/// Where a file that could not all be understood is kept, as it was.
pub const KEPT_ASIDE: &str = "window settings that could not be read.json";

/// The parts this version knows.
const KNOWN: &[&str] = &[
    "v",
    "keepRunning",
    "minimiseToTray",
    "explainedClose",
    "notify",
    "seenAt",
];

/// The app's choices about its window, its icon by the clock and telling
/// the person things.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WindowSettings {
    /// Whether closing the window keeps Keevoro running by the clock.
    /// `None` until the person chooses: then it is on wherever this
    /// computer shows icons by the clock (see `keep_running_or`).
    pub keep_running: Option<bool>,
    /// Whether minimising hides the window to the icon by the clock too.
    pub minimise_to_tray: bool,
    /// True once the person has been told, the first time the window was
    /// hidden by closing it, that Keevoro keeps running.
    pub explained_close: bool,
    /// Whether a backup by itself may tell the person when it finishes
    /// with something worth saying. On unless they switched it off.
    pub notify: Option<bool>,
    /// When the person last had the window open, in seconds since 1970:
    /// what a backup by itself held back since then still needs their say.
    pub seen_at: Option<i64>,
    /// Parts a later version wrote, kept as they were and written back.
    later: serde_json::Map<String, serde_json::Value>,
}

impl WindowSettings {
    /// Whether closing the window keeps Keevoro running: the person's
    /// choice, or else `where_there_is_a_tray`.
    pub fn keep_running_or(&self, where_there_is_a_tray: bool) -> bool {
        self.keep_running.unwrap_or(where_there_is_a_tray)
    }

    /// Whether to tell the person when a backup by itself finishes.
    pub fn notifies(&self) -> bool {
        self.notify.unwrap_or(true)
    }

    /// Reads the choices from `config`: as they come when there is no file
    /// or it cannot be read at all, each part on its own otherwise.
    pub fn read(config: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(config.join(FILE)) else {
            return Self::default();
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            keep_aside(config, &text);
            return Self::default();
        };
        let (settings, whole) = Self::from_value(&value);
        if !whole {
            keep_aside(config, &text);
        }
        settings
    }

    /// Each part read on its own. Also answers whether every part was
    /// understood.
    fn from_value(value: &serde_json::Value) -> (Self, bool) {
        let Some(all) = value.as_object() else {
            return (Self::default(), false);
        };
        let mut whole = true;
        fn one<T: serde::de::DeserializeOwned>(
            all: &serde_json::Map<String, serde_json::Value>,
            name: &str,
            whole: &mut bool,
        ) -> Option<T> {
            let found = all.get(name).filter(|v| !v.is_null())?;
            let read = serde_json::from_value(found.clone()).ok();
            if read.is_none() {
                *whole = false;
            }
            read
        }
        let settings = Self {
            keep_running: one(all, "keepRunning", &mut whole),
            minimise_to_tray: one(all, "minimiseToTray", &mut whole).unwrap_or(false),
            explained_close: one(all, "explainedClose", &mut whole).unwrap_or(false),
            notify: one(all, "notify", &mut whole),
            seen_at: one(all, "seenAt", &mut whole),
            later: all
                .iter()
                .filter(|(key, _)| !KNOWN.contains(&key.as_str()))
                .map(|(key, v)| (key.clone(), v.clone()))
                .collect(),
        };
        (settings, whole)
    }

    fn to_value(&self) -> serde_json::Value {
        let mut all = self.later.clone();
        all.insert("v".into(), 1.into());
        if let Some(on) = self.keep_running {
            all.insert("keepRunning".into(), on.into());
        }
        all.insert("minimiseToTray".into(), self.minimise_to_tray.into());
        all.insert("explainedClose".into(), self.explained_close.into());
        if let Some(on) = self.notify {
            all.insert("notify".into(), on.into());
        }
        if let Some(at) = self.seen_at {
            all.insert("seenAt".into(), at.into());
        }
        serde_json::Value::Object(all)
    }

    /// Writes the choices to `config`, beside the real file and then moved
    /// over it, so a crash never leaves half a file.
    pub fn save(&self, config: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(config)?;
        let text = serde_json::to_string_pretty(&self.to_value()).map_err(std::io::Error::other)?;
        let temp = config.join(format!(".{FILE}.{}.part", std::process::id()));
        std::fs::write(&temp, text)?;
        std::fs::rename(&temp, config.join(FILE)).inspect_err(|_| {
            let _ = std::fs::remove_file(&temp);
        })
    }
}

/// Keeps a file that could not all be understood beside it, once, before
/// anything is written over it.
fn keep_aside(config: &Path, text: &str) {
    let aside = config.join(KEPT_ASIDE);
    if !aside.exists() {
        let _ = std::fs::write(aside, text);
    }
}

impl Keevoro {
    /// The app's choices about its window, as they are now.
    pub fn window_settings(&self) -> WindowSettings {
        WindowSettings::read(self.backups().config_folder())
    }

    /// Changes the app's choices about its window, and answers them as
    /// they are then.
    pub fn change_window_settings(&self, change: impl FnOnce(&mut WindowSettings)) -> Result<WindowSettings> {
        let config = self.backups().config_folder();
        let mut settings = WindowSettings::read(config);
        change(&mut settings);
        settings
            .save(config)
            .map_err(|e| Error::failed(format!("Keevoro couldn't save how its window behaves: {e}.")))?;
        Ok(settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_no_file_every_choice_is_as_it_comes() {
        let dir = tempfile::tempdir().unwrap();
        let settings = WindowSettings::read(dir.path());
        assert_eq!(settings, WindowSettings::default());
        assert!(settings.keep_running_or(true), "on where there is a tray");
        assert!(!settings.keep_running_or(false), "off where there may be none");
        assert!(settings.notifies());
        assert!(!settings.minimise_to_tray && !settings.explained_close);
        assert!(!dir.path().join(KEPT_ASIDE).exists());
    }

    #[test]
    fn choices_are_saved_and_read_back_and_a_choice_made_wins_over_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let settings = WindowSettings {
            keep_running: Some(false),
            minimise_to_tray: true,
            explained_close: true,
            notify: Some(false),
            seen_at: Some(1_763_700_000),
            ..WindowSettings::default()
        };
        settings.save(dir.path()).unwrap();
        let read = WindowSettings::read(dir.path());
        assert_eq!(read, settings);
        assert!(!read.keep_running_or(true));
        assert!(!read.notifies());
        let text = std::fs::read_to_string(dir.path().join(FILE)).unwrap();
        assert!(text.contains("\"keepRunning\": false"), "{text}");
        assert!(text.contains("\"v\": 1"), "{text}");
    }

    #[test]
    fn a_part_that_cannot_be_understood_costs_that_part_alone_and_the_file_is_kept_aside() {
        let dir = tempfile::tempdir().unwrap();
        let written = r#"{ "v": 1, "keepRunning": "yes please", "explainedClose": true, "seenAt": 5 }"#;
        std::fs::write(dir.path().join(FILE), written).unwrap();
        let read = WindowSettings::read(dir.path());
        assert_eq!(read.keep_running, None);
        assert!(read.explained_close);
        assert_eq!(read.seen_at, Some(5));
        assert_eq!(std::fs::read_to_string(dir.path().join(KEPT_ASIDE)).unwrap(), written);
    }

    #[test]
    fn a_file_that_is_not_json_at_all_counts_as_none_and_is_kept_aside() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), "{ half a fi").unwrap();
        assert_eq!(WindowSettings::read(dir.path()), WindowSettings::default());
        assert_eq!(
            std::fs::read_to_string(dir.path().join(KEPT_ASIDE)).unwrap(),
            "{ half a fi"
        );
        std::fs::write(dir.path().join(FILE), "[1, 2]").unwrap();
        assert_eq!(WindowSettings::read(dir.path()), WindowSettings::default());
    }

    #[test]
    fn parts_a_later_version_wrote_are_kept_and_written_back() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(FILE),
            r#"{ "v": 2, "keepRunning": true, "tell": "problemsOnly", "awaySeenAt": 7 }"#,
        )
        .unwrap();
        let mut read = WindowSettings::read(dir.path());
        assert_eq!(read.keep_running, Some(true));
        assert!(!dir.path().join(KEPT_ASIDE).exists(), "unknown parts are not damage");
        read.minimise_to_tray = true;
        read.save(dir.path()).unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.path().join(FILE)).unwrap()).unwrap();
        assert_eq!(value["tell"], "problemsOnly");
        assert_eq!(value["awaySeenAt"], 7);
        assert_eq!(value["minimiseToTray"], true);
    }

    #[test]
    fn keevoro_changes_them_in_its_config_folder_and_never_touches_the_settings_file() {
        let dir = tempfile::tempdir().unwrap();
        let keevoro = crate::keevoro::tests::keevoro(dir.path());
        keevoro.start_backup("Family").unwrap();
        let settings_file = keevoro.backups().config_folder().join("settings.json");
        let before = std::fs::read_to_string(&settings_file).unwrap();
        let changed = keevoro.change_window_settings(|s| s.keep_running = Some(true)).unwrap();
        assert_eq!(changed.keep_running, Some(true));
        assert_eq!(keevoro.window_settings().keep_running, Some(true));
        assert!(keevoro.backups().config_folder().join(FILE).is_file());
        assert_eq!(std::fs::read_to_string(&settings_file).unwrap(), before);
        // Starting again from nothing forgets them too.
        keevoro.reset().unwrap();
        assert!(!keevoro.backups().config_folder().join(FILE).exists());
        assert_eq!(keevoro.window_settings(), WindowSettings::default());
    }
}
