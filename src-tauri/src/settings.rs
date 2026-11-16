//! The few things the app remembers about itself between runs.
//!
//! Chiefly its sessions. A session is one job the person comes back to:
//! "Drone footage" or "Family photos", each with its own sources, its own
//! choices and its own place to send things, none of which affect the
//! others. Everything a session knows lives in its home folder; this file
//! only lists the sessions and says which one is open.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const FILE: &str = "settings.json";

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// From before sessions: the one library folder. Read once, to turn it
    /// into the first session, and kept so an older Keevoro still opens.
    #[serde(default)]
    pub library: Option<PathBuf>,
    #[serde(default)]
    pub sessions: Vec<Session>,
    /// The id of the session that is open.
    #[serde(default)]
    pub current: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
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
        // A library from before sessions becomes the first session, with
        // everything it had found and chosen.
        if settings.sessions.is_empty() {
            if let Some(library) = settings.library.clone() {
                settings.sessions.push(Session {
                    id: "first".to_string(),
                    name: "My photos".to_string(),
                    home: library,
                });
            }
        }
        if !settings
            .sessions
            .iter()
            .any(|s| Some(&s.id) == settings.current.as_ref())
        {
            settings.current = settings.sessions.first().map(|s| s.id.clone());
        }
        settings
    }

    /// The session that is open, if there is one.
    pub fn open_session(&self) -> Option<&Session> {
        self.sessions.iter().find(|s| Some(&s.id) == self.current.as_ref())
    }

    /// Adds a session called `name`, kept under `sessions_dir`, and opens
    /// it. A blank name becomes "My photos"; a name already in use gets a
    /// number, so two sessions can always be told apart.
    pub fn start_session(&mut self, name: &str, sessions_dir: &Path) -> &Session {
        let wanted = match name.trim() {
            "" => "My photos",
            given => given,
        };
        let mut name = wanted.to_string();
        let mut n = 2;
        while self.sessions.iter().any(|s| s.name.eq_ignore_ascii_case(&name)) {
            name = format!("{wanted} {n}");
            n += 1;
        }
        // A folder name that is safe everywhere, and never reused.
        let mut id = 1;
        while self.sessions.iter().any(|s| s.id == format!("session-{id}"))
            || sessions_dir.join(format!("session-{id}")).exists()
        {
            id += 1;
        }
        let id = format!("session-{id}");
        self.sessions.push(Session {
            home: sessions_dir.join(&id),
            id: id.clone(),
            name,
        });
        self.current = Some(id);
        &self.sessions[self.sessions.len() - 1]
    }

    /// Opens another session. False if there is none with that id.
    pub fn switch_to(&mut self, id: &str) -> bool {
        let found = self.sessions.iter().any(|s| s.id == id);
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
        chosen.start_session("Drone footage", &home.join("sessions"));
        chosen.save(&home).unwrap();
        assert_eq!(Settings::load(&home), chosen);
        assert_eq!(
            std::fs::read_dir(&home).unwrap().count(),
            1,
            "no temporary file is left, and a session's folder is not made until it is used"
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
    fn a_library_from_before_sessions_becomes_the_first_session() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), r#"{"library":"/photos/Keevoro Library"}"#).unwrap();
        let settings = Settings::load(dir.path());
        let open = settings.open_session().unwrap();
        assert_eq!(
            (open.name.as_str(), &open.home),
            ("My photos", &PathBuf::from("/photos/Keevoro Library"))
        );
    }

    #[test]
    fn sessions_are_separate_named_and_can_be_switched_between() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = Settings::default();
        assert_eq!(settings.open_session(), None);
        let drone = settings.start_session("Drone footage", dir.path()).clone();
        let family = settings.start_session("  Family photos ", dir.path()).clone();
        assert_eq!(family.name, "Family photos");
        assert_ne!(drone.home, family.home, "each has its own folder");
        assert_eq!(
            settings.open_session(),
            Some(&family),
            "a new session opens straight away"
        );

        assert!(settings.switch_to(&drone.id));
        assert_eq!(settings.open_session(), Some(&drone));
        assert!(!settings.switch_to("nonsense"));
        assert_eq!(settings.open_session(), Some(&drone));

        // Names are never confused, and a blank one gets a sensible default.
        assert_eq!(
            settings.start_session("drone footage", dir.path()).name,
            "drone footage 2"
        );
        assert_eq!(settings.start_session("", dir.path()).name, "My photos");
    }

    #[test]
    fn the_open_session_is_remembered_and_a_stale_one_falls_back() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = Settings::default();
        let first = settings.start_session("One", dir.path()).id.clone();
        settings.start_session("Two", dir.path());
        settings.switch_to(&first);
        settings.save(dir.path()).unwrap();
        assert_eq!(Settings::load(dir.path()).open_session().unwrap().name, "One");

        settings.current = Some("gone".to_string());
        settings.save(dir.path()).unwrap();
        assert_eq!(Settings::load(dir.path()).open_session().unwrap().name, "One");
    }
}
