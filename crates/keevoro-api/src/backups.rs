//! Named backups, and the settings file that lists them.
//!
//! A backup is one job the person comes back to: "Drone footage" or
//! "Family photos", each with its own sources, its own choices and its own
//! places to send things, none of which affect the others. Everything a
//! backup knows lives in its home folder. Keevoro's settings file, in its
//! config folder, only lists the backups, says which one the app has open,
//! and keeps the few settings that belong to Keevoro as a whole.
//!
//! The app, the command line and an agent all read the same file, so a
//! backup started in one is there in the others. Which backup is "open" is
//! the app's idea: the command line names a backup each time. So
//! everything here can be asked of a backup by its name, and the open one
//! is only what is used when no name is given.
//!
//! The file's format is kept exactly as earlier versions wrote it, so an
//! existing settings file loads unchanged and an older Keevoro can still
//! read one written by this one.

use crate::agents::AgentAccess;
use crate::backup::Backup;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

const FILE: &str = "settings.json";

/// What the settings file holds.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// From before backups: the one library folder. Read once, to turn it
    /// into the first backup, and kept so an older Keevoro still opens.
    #[serde(default)]
    pub library: Option<PathBuf>,
    /// Called `sessions` in the file for a few early versions, which is
    /// still read.
    #[serde(default, alias = "sessions")]
    pub backups: Vec<Named>,
    /// The id of the backup the app has open.
    #[serde(default)]
    pub current: Option<String>,
    /// True when the person has switched off the use of the computer's own
    /// picture codecs and graphics hardware. Off unless they say so, which
    /// is why an old file without it means "use them".
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub plain_decoding: bool,
    /// Which folders an AI agent may use, and whether it may make changes.
    /// Left out of the file until someone sets it, so a file written by
    /// someone who never has is exactly as it always was.
    #[serde(default, skip_serializing_if = "AgentAccess::is_default")]
    pub agents: AgentAccess,
}

/// One backup, as the settings file lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Named {
    /// Never changes, and never reused: the name of its folder.
    pub id: String,
    /// What the person called it. Can be changed.
    pub name: String,
    /// Its home folder, where its record and previews are kept. Working
    /// space, never a copy of anyone's photos.
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
                settings.backups.push(Named {
                    id: "first".to_string(),
                    name: "My photos".to_string(),
                    home: library,
                });
            }
        }
        settings.fall_back_to_the_first();
        settings
    }

    /// Writes the settings to `dir`, beside the real file and then moved
    /// over it, so a crash never leaves half a settings file.
    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        let temp = dir.join(format!("{FILE}.part"));
        std::fs::write(&temp, text)?;
        std::fs::rename(temp, dir.join(FILE))
    }

    /// The backup the app has open, if there is one.
    pub fn open_backup(&self) -> Option<&Named> {
        self.backups.iter().find(|s| Some(&s.id) == self.current.as_ref())
    }

    /// If the open backup is no longer listed, the first one is opened.
    fn fall_back_to_the_first(&mut self) {
        if self.open_backup().is_none() {
            self.current = self.backups.first().map(|s| s.id.clone());
        }
    }

    /// Adds a backup called `name`, kept under `backups_dir`. A blank name
    /// becomes "My photos"; a name already in use gets a number, so two
    /// backups can always be told apart. It is opened if nothing else is.
    fn start(&mut self, name: &str, backups_dir: &Path) -> Named {
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
        let started = Named {
            home: backups_dir.join(&id),
            id,
            name,
        };
        self.backups.push(started.clone());
        self.fall_back_to_the_first();
        started
    }

    /// The backup with this id or, failing that, this name, ignoring case.
    fn find(&self, id_or_name: &str) -> Option<&Named> {
        let wanted = id_or_name.trim();
        self.backups
            .iter()
            .find(|s| s.id == wanted)
            .or_else(|| self.backups.iter().find(|s| s.name.eq_ignore_ascii_case(wanted)))
    }
}

/// Keevoro's list of backups, kept in its settings file. Every change is
/// saved at once.
pub struct Backups {
    config: PathBuf,
    data: PathBuf,
    settings: Mutex<Settings>,
}

impl Backups {
    /// Reads the list from the settings file in `config`. New backups get
    /// their home folders under `data`.
    pub fn load(config: &Path, data: &Path) -> Self {
        Self {
            config: config.to_path_buf(),
            data: data.to_path_buf(),
            settings: Mutex::new(Settings::load(config)),
        }
    }

    /// The settings as they stand. A poisoned lock only means a panic
    /// elsewhere; what is inside is still the last thing saved.
    fn settings(&self) -> MutexGuard<'_, Settings> {
        self.settings.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Makes a change and saves it. If it cannot be saved, nothing changes.
    pub(crate) fn change<T>(&self, change: impl FnOnce(&mut Settings) -> Result<T>) -> Result<T> {
        let mut settings = self.settings();
        let mut changed = settings.clone();
        let answer = change(&mut changed)?;
        changed
            .save(&self.config)
            .map_err(|e| Error::failed(format!("Keevoro couldn't save its settings: {e}.")))?;
        *settings = changed;
        Ok(answer)
    }

    /// A copy of everything in the settings file.
    pub fn read(&self) -> Settings {
        self.settings().clone()
    }

    /// Keevoro's own data folder, under which new backups are kept.
    pub fn data_folder(&self) -> &Path {
        &self.data
    }

    /// Keevoro's config folder, where the settings file is.
    pub fn config_folder(&self) -> &Path {
        &self.config
    }

    /// Every backup, in the order they were started. Quick: nothing is
    /// opened or counted.
    pub fn all(&self) -> Vec<Named> {
        self.settings().backups.clone()
    }

    /// The backup the app has open, if any.
    pub fn current(&self) -> Option<Named> {
        self.settings().open_backup().cloned()
    }

    /// The backup with this id or name, ignoring case.
    pub fn find(&self, id_or_name: &str) -> Option<Named> {
        self.settings().find(id_or_name).cloned()
    }

    /// The backup named, or the open one when none is named.
    pub fn named_or_open(&self, id_or_name: Option<&str>) -> Result<Named> {
        match id_or_name {
            Some(wanted) => self.find(wanted).ok_or_else(|| Error::no_such_backup(wanted)),
            None => self.current().ok_or_else(Error::no_backup),
        }
    }

    /// Opens the backup named, or the open one when none is named.
    pub fn open(&self, id_or_name: Option<&str>) -> Result<Backup> {
        Backup::open(&self.named_or_open(id_or_name)?.home)
    }

    /// `backups.start`: starts a new, empty backup. Its folder is made at
    /// once, which proves it can be. It becomes the open one only if no
    /// other is.
    pub fn start(&self, name: &str) -> Result<Named> {
        self.change(|settings| {
            let started = settings.start(name, &self.data.join("backups"));
            // Opening its record proves the folder can be made and written to.
            Backup::open(&started.home)?;
            Ok(started)
        })
    }

    /// Makes this backup the open one. False if there is no such backup.
    pub fn switch_to(&self, id: &str) -> Result<bool> {
        if !self.settings().backups.iter().any(|s| s.id == id) {
            return Ok(false);
        }
        self.change(|settings| {
            settings.current = Some(id.to_string());
            Ok(true)
        })
    }

    /// `backups.rename`: gives a backup a new name. Its folder, its id and
    /// everything it knows stay as they were.
    pub fn rename(&self, id_or_name: &str, to: &str) -> Result<Named> {
        let to = to.trim();
        if to.is_empty() {
            return Err(Error::refused("Give the backup a name."));
        }
        self.change(|settings| {
            let id = settings
                .find(id_or_name)
                .ok_or_else(|| Error::no_such_backup(id_or_name))?
                .id
                .clone();
            if settings
                .backups
                .iter()
                .any(|s| s.id != id && s.name.eq_ignore_ascii_case(to))
            {
                return Err(Error::refused(format!(
                    "There is already a backup called \"{to}\". Choose another name."
                )));
            }
            let backup = settings
                .backups
                .iter_mut()
                .find(|s| s.id == id)
                .ok_or_else(|| Error::no_such_backup(id_or_name))?;
            to.clone_into(&mut backup.name);
            Ok(backup.clone())
        })
    }

    /// `backups.forget`: takes a backup off Keevoro's list.
    ///
    /// Nothing on disk is deleted: not a source, not anything at a
    /// destination, and not the backup's own home folder either, which is
    /// left where it is with its record and previews in it. Only the list
    /// changes. If it was the open one, the first left is opened instead.
    pub fn forget(&self, id_or_name: &str) -> Result<Named> {
        self.change(|settings| {
            let at = settings
                .find(id_or_name)
                .and_then(|found| settings.backups.iter().position(|s| s.id == found.id))
                .ok_or_else(|| Error::no_such_backup(id_or_name))?;
            let gone = settings.backups.remove(at);
            // A library from before backups would otherwise come back as
            // "My photos" the next time the file is read.
            if settings.library.as_ref() == Some(&gone.home) {
                settings.library = None;
            }
            settings.fall_back_to_the_first();
            Ok(gone)
        })
    }

    /// Kept for the first-run screen of older builds, which chose a library
    /// folder: that folder becomes a backup of its own, and is opened. It
    /// goes with the library model.
    pub fn adopt_library(&self, home: &Path) -> Result<Named> {
        Backup::open(home)?;
        self.change(|settings| {
            let adopted = Named {
                id: format!("library-{}", settings.backups.len() + 1),
                name: "My photos".to_string(),
                home: home.to_path_buf(),
            };
            settings.backups.push(adopted.clone());
            settings.current = Some(adopted.id.clone());
            settings.library = Some(home.to_path_buf());
            Ok(adopted)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_backups_are_remembered_between_runs() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config").join("keevoro");
        assert_eq!(
            Settings::load(&config),
            Settings::default(),
            "a first run has nothing remembered"
        );

        let backups = Backups::load(&config, &dir.path().join("data"));
        let started = backups.start("Drone footage").unwrap();
        assert_eq!(Settings::load(&config), backups.read());
        assert_eq!(Backups::load(&config, dir.path()).all(), [started]);
        assert_eq!(
            std::fs::read_dir(&config).unwrap().count(),
            1,
            "no temporary file is left"
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

    #[test]
    fn an_existing_settings_file_is_read_and_written_exactly_as_before() {
        // What 0.29 writes, with the codecs switched off.
        let before = r#"{
  "library": null,
  "backups": [
    {
      "id": "backup-1",
      "name": "Drone footage",
      "home": "/data/backups/backup-1"
    }
  ],
  "current": "backup-1",
  "plain_decoding": true
}"#;
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), before).unwrap();
        let settings = Settings::load(dir.path());
        assert!(settings.plain_decoding);
        assert_eq!(settings.agents, AgentAccess::default());
        settings.save(dir.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join(FILE)).unwrap(),
            before,
            "nothing new is written until someone sets it"
        );
    }

    #[test]
    fn backups_are_separate_named_and_can_be_switched_between() {
        let dir = tempfile::tempdir().unwrap();
        let backups = Backups::load(&dir.path().join("config"), dir.path());
        assert_eq!(backups.current(), None);
        let drone = backups.start("Drone footage").unwrap();
        let family = backups.start("  Family photos ").unwrap();
        assert_eq!(family.name, "Family photos");
        assert_ne!(drone.home, family.home, "each has its own folder");
        assert!(drone.home.join(".keevoro").join("catalog.sqlite").is_file());
        assert_eq!(
            backups.current(),
            Some(drone.clone()),
            "starting another does not change which is open"
        );

        assert!(backups.switch_to(&family.id).unwrap());
        assert_eq!(backups.current(), Some(family.clone()));
        assert!(!backups.switch_to("nonsense").unwrap());
        assert_eq!(backups.current(), Some(family));

        // Names are never confused, and a blank one gets a sensible default.
        assert_eq!(backups.start("drone footage").unwrap().name, "drone footage 2");
        assert_eq!(backups.start("").unwrap().name, "My photos");
    }

    #[test]
    fn a_backup_is_found_by_its_name_or_its_id() {
        let dir = tempfile::tempdir().unwrap();
        let backups = Backups::load(&dir.path().join("config"), dir.path());
        let drone = backups.start("Drone footage").unwrap();
        backups.start("Family").unwrap();
        assert_eq!(backups.find("DRONE FOOTAGE "), Some(drone.clone()));
        assert_eq!(backups.find(&drone.id), Some(drone.clone()));
        assert_eq!(backups.find("Holidays"), None);

        assert_eq!(backups.named_or_open(None).unwrap(), drone);
        assert_eq!(backups.named_or_open(Some("family")).unwrap().name, "Family");
        let e = backups.named_or_open(Some("Holidays")).unwrap_err();
        assert_eq!(e.kind, crate::Kind::NotFound);
        assert_eq!(e.message, "There is no backup called \"Holidays\".");
        assert_eq!(
            backups.open(Some("Family")).unwrap().home(),
            backups.find("Family").unwrap().home
        );

        let empty = Backups::load(&dir.path().join("elsewhere"), dir.path());
        assert_eq!(empty.named_or_open(None).unwrap_err().kind, crate::Kind::NoBackup);
    }

    #[test]
    fn a_backup_can_be_renamed_but_not_to_a_name_in_use() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        let backups = Backups::load(&config, dir.path());
        let drone = backups.start("Drone footage").unwrap();
        backups.start("Family").unwrap();

        let renamed = backups.rename("drone footage", "  Drone ").unwrap();
        assert_eq!(
            (renamed.id.as_str(), renamed.name.as_str()),
            (drone.id.as_str(), "Drone")
        );
        assert_eq!(renamed.home, drone.home, "its folder stays where it is");
        assert_eq!(Backups::load(&config, dir.path()).find("Drone"), Some(renamed));
        // Changing only the case of its own name is fine.
        assert_eq!(backups.rename("Drone", "DRONE").unwrap().name, "DRONE");

        let e = backups.rename("DRONE", "family").unwrap_err();
        assert_eq!(e.kind, crate::Kind::Refused);
        assert_eq!(
            e.message,
            "There is already a backup called \"family\". Choose another name."
        );
        assert_eq!(
            backups.rename("DRONE", "  ").unwrap_err().message,
            "Give the backup a name."
        );
        assert_eq!(backups.rename("Holidays", "x").unwrap_err().kind, crate::Kind::NotFound);
        assert_eq!(
            backups.find("DRONE").unwrap().name,
            "DRONE",
            "a refusal changes nothing"
        );
    }

    #[test]
    fn forgetting_a_backup_only_takes_it_off_the_list() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        let backups = Backups::load(&config, dir.path());
        let drone = backups.start("Drone footage").unwrap();
        let family = backups.start("Family").unwrap();
        std::fs::write(drone.home.join("a note.txt"), "kept").unwrap();

        assert_eq!(backups.forget("drone footage").unwrap(), drone);
        assert_eq!(backups.all(), std::slice::from_ref(&family));
        assert_eq!(backups.current(), Some(family), "the open one moves to what is left");
        assert_eq!(Backups::load(&config, dir.path()).all().len(), 1);
        assert!(
            drone.home.join(".keevoro").join("catalog.sqlite").is_file(),
            "its folder and record are left where they are"
        );
        assert!(drone.home.join("a note.txt").is_file());
        assert_eq!(backups.forget("Drone footage").unwrap_err().kind, crate::Kind::NotFound);
    }

    #[test]
    fn a_forgotten_library_from_before_backups_stays_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join(FILE), r#"{"library":"/photos/Keevoro Library"}"#).unwrap();
        let backups = Backups::load(&config, dir.path());
        backups.forget("My photos").unwrap();
        assert!(Backups::load(&config, dir.path()).all().is_empty());
    }

    #[test]
    fn the_open_backup_is_remembered_and_a_stale_one_falls_back() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = Settings::default();
        let first = settings.start("One", dir.path()).id;
        settings.start("Two", dir.path());
        settings.current = Some(first);
        settings.save(dir.path()).unwrap();
        assert_eq!(Settings::load(dir.path()).open_backup().unwrap().name, "One");

        settings.current = Some("gone".to_string());
        settings.save(dir.path()).unwrap();
        assert_eq!(Settings::load(dir.path()).open_backup().unwrap().name, "One");
    }

    #[test]
    fn a_settings_file_that_cannot_be_saved_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let in_the_way = dir.path().join("file");
        std::fs::write(&in_the_way, "x").unwrap();
        let backups = Backups::load(&in_the_way.join("config"), dir.path());
        let e = backups.start("Drone").unwrap_err();
        assert!(e.message.starts_with("Keevoro couldn't save its settings"), "{e}");
        assert!(backups.all().is_empty());
    }

    #[test]
    fn an_old_library_folder_can_still_be_adopted() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        let backups = Backups::load(&config, dir.path());
        let library = dir.path().join("Keevoro Library");
        let adopted = backups.adopt_library(&library).unwrap();
        assert_eq!(backups.current(), Some(adopted.clone()));
        assert_eq!(adopted.name, "My photos");
        assert_eq!(Settings::load(&config).library, Some(library));
    }
}
