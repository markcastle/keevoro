//! The few things the app remembers about itself between runs.
//!
//! Only one so far: where the library is. It is kept in a small JSON file
//! in the app's own settings folder, not in the library, because the app
//! has to know where the library is before it can open it.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const FILE: &str = "settings.json";

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// The library folder, once the user has chosen one.
    #[serde(default)]
    pub library: Option<PathBuf>,
}

impl Settings {
    /// Reads the settings from `dir`. Anything missing or unreadable means
    /// "nothing chosen yet": the app then simply asks again, which is
    /// better than refusing to start over a damaged settings file.
    pub fn load(dir: &Path) -> Self {
        std::fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
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

        let chosen = Settings {
            library: Some(PathBuf::from("D:/Keevoro Library")),
        };
        chosen.save(&home).unwrap();
        assert_eq!(Settings::load(&home), chosen);
        assert_eq!(
            std::fs::read_dir(&home).unwrap().count(),
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
}
