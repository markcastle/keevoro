//! The few things the app remembers about itself between runs.
//!
//! Chiefly its backups. A backup is one job the person comes back to:
//! "Drone footage" or "Family photos", each with its own sources, its own
//! choices and its own place to send things, none of which affect the
//! others. Everything a backup knows lives in its home folder; this file
//! only lists the backups and says which one is open.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const FILE: &str = "settings.json";

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// From before backups: the one library folder. Read once, to turn it
    /// into the first backup, and kept so an older Keevoro still opens.
    #[serde(default)]
    pub library: Option<PathBuf>,
    /// Called `sessions` in the file for a few early versions, which is
    /// still read.
    #[serde(default, alias = "sessions")]
    pub backups: Vec<Backup>,
    /// The id of the backup that is open.
    #[serde(default)]
    pub current: Option<String>,
    /// False when the person has switched off the use of the computer's
    /// own picture codecs and graphics hardware. On unless they say so.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub plain_decoding: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Backup {
    pub id: String,
    /// What the person called it.
    pub name: String,
    /// Where its catalog and previews are kept.
    pub home: PathBuf,
}

impl Settings {
    /// Reads the settings from `dir`. Anything missing or unreadable means
    /// "nothing chosen yet": the app then simply asks again, which is
    /// better than refusing to start over a damaged settings file.
    pub fn load(dir: &Path) -> Self {
        let mut settings: Self = std::fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        // A library from before backups becomes the first backup, with
        // everything it had found and chosen.
        if settings.backups.is_empty() {
            if let Some(library) = settings.library.clone() {
                settings.backups.push(Backup {
                    id: "first".to_string(),
                    name: "My photos".to_string(),
                    home: library,
                });
            }
        }
        if !settings
            .backups
            .iter()
            .any(|s| Some(&s.id) == settings.current.as_ref())
        {
            settings.current = settings.backups.first().map(|s| s.id.clone());
        }
        settings
    }

    /// The backup that is open, if there is one.
    pub fn open_backup(&self) -> Option<&Backup> {
        self.backups.iter().find(|s| Some(&s.id) == self.current.as_ref())
    }

    /// Adds a backup called `name`, kept under `backups_dir`, and opens
    /// it. A blank name becomes "My photos"; a name already in use gets a
    /// number, so two backups can always be told apart.
    pub fn start_backup(&mut self, name: &str, backups_dir: &Path) -> &Backup {
        let wanted = match name.trim() {
            "" => "My photos",
            given => given,
        };
        let mut name = wanted.to_string();
        let mut n = 2;
        while self.backups.iter().any(|s| s.name.eq_ignore_ascii_case(&name)) {
            name = format!("{wanted} {n}");
            n += 1;
        }
        // A folder name that is safe everywhere, and never reused.
        let mut id = 1;
        while self.backups.iter().any(|s| s.id == format!("backup-{id}"))
            || backups_dir.join(format!("backup-{id}")).exists()
        {
            id += 1;
        }
        let id = format!("backup-{id}");
        self.backups.push(Backup {
            home: backups_dir.join(&id),
            id: id.clone(),
            name,
        });
        self.current = Some(id);
        &self.backups[self.backups.len() - 1]
    }

    /// Opens another backup. False if there is none with that id.
    pub fn switch_to(&mut self, id: &str) -> bool {
        let found = self.backups.iter().any(|s| s.id == id);
        if found {
            self.current = Some(id.to_string());
        }
        found
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        // Written beside the real file and moved over it, so a crash never
        // leaves half a settings file.
        let temp = dir.join(format!("{FILE}.part"));
        std::fs::write(&temp, text)?;
        std::fs::rename(temp, dir.join(FILE))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_library_is_remembered_between_runs() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("config").join("keevoro");
        assert_eq!(
            Settings::load(&home),
            Settings::default(),
            "a first run has nothing remembered"
        );

        let mut chosen = Settings::default();
        chosen.start_backup("Drone footage", &home.join("backups"));
        chosen.save(&home).unwrap();
        assert_eq!(Settings::load(&home), chosen);
        assert_eq!(
            std::fs::read_dir(&home).unwrap().count(),
            1,
            "no temporary file is left, and a backup's folder is not made until it is used"
        );
    }

    #[test]
    fn a_damaged_settings_file_means_ask_again_not_crash() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), "{ not json").unwrap();
        assert_eq!(Settings::load(dir.path()), Settings::default());
        // A file from a future version with extra fields still loads.
        std::fs::write(dir.path().join(FILE), r#"{"library":"/photos","theme":"dark"}"#).unwrap();
        assert_eq!(Settings::load(dir.path()).library, Some(PathBuf::from("/photos")));
    }

    #[test]
    fn a_library_from_before_backups_becomes_the_first_backup() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), r#"{"library":"/photos/Keevoro Library"}"#).unwrap();
        let settings = Settings::load(dir.path());
        let open = settings.open_backup().unwrap();
        assert_eq!(
            (open.name.as_str(), &open.home),
            ("My photos", &PathBuf::from("/photos/Keevoro Library"))
        );
    }

    #[test]
    fn backups_are_separate_named_and_can_be_switched_between() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = Settings::default();
        assert_eq!(settings.open_backup(), None);
        let drone = settings.start_backup("Drone footage", dir.path()).clone();
        let family = settings.start_backup("  Family photos ", dir.path()).clone();
        assert_eq!(family.name, "Family photos");
        assert_ne!(drone.home, family.home, "each has its own folder");
        assert_eq!(
            settings.open_backup(),
            Some(&family),
            "a new backup opens straight away"
        );

        assert!(settings.switch_to(&drone.id));
        assert_eq!(settings.open_backup(), Some(&drone));
        assert!(!settings.switch_to("nonsense"));
        assert_eq!(settings.open_backup(), Some(&drone));

        // Names are never confused, and a blank one gets a sensible default.
        assert_eq!(
            settings.start_backup("drone footage", dir.path()).name,
            "drone footage 2"
        );
        assert_eq!(settings.start_backup("", dir.path()).name, "My photos");
    }

    #[test]
    fn the_open_backup_is_remembered_and_a_stale_one_falls_back() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = Settings::default();
        let first = settings.start_backup("One", dir.path()).id.clone();
        settings.start_backup("Two", dir.path());
        settings.switch_to(&first);
        settings.save(dir.path()).unwrap();
        assert_eq!(Settings::load(dir.path()).open_backup().unwrap().name, "One");

        settings.current = Some("gone".to_string());
        settings.save(dir.path()).unwrap();
        assert_eq!(Settings::load(dir.path()).open_backup().unwrap().name, "One");
    }

    #[test]
    fn a_settings_file_from_when_backups_were_called_sessions_still_loads() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(FILE),
            r#"{"library":null,"sessions":[{"id":"session-1","name":"Drone footage","home":"/data/sessions/session-1"}],"current":"session-1"}"#,
        )
        .unwrap();
        let settings = Settings::load(dir.path());
        let open = settings.open_backup().unwrap();
        assert_eq!((open.name.as_str(), open.id.as_str()), ("Drone footage", "session-1"));
        // Saved again, it is written the new way and reads back the same.
        settings.save(dir.path()).unwrap();
        let text = std::fs::read_to_string(dir.path().join(FILE)).unwrap();
        assert!(text.contains("\"backups\"") && !text.contains("\"sessions\""), "{text}");
        assert_eq!(Settings::load(dir.path()), settings);
    }
}
