//! Starting again from nothing.
//!
//! A reset removes what Keevoro itself made and remembers: its list of
//! backups, what each one found and chose, its previews, its saved keys.
//! Afterwards the app is as it was when first installed.
//!
//! It must never remove anything that is the person's. So the rule here is
//! strict: a backup's folder is deleted only if it lies inside Keevoro's own
//! data folder. Anything else it once pointed at (a "Keevoro Library" in
//! Pictures from an early version, say, which may hold gathered photos) is
//! forgotten and left exactly where it is. Sources and the places files
//! were sent to are never opened by a reset at all.

use crate::settings::{Backup, Settings};
use std::path::{Path, PathBuf};

/// What a reset did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Done {
    /// Backups whose own folders were removed.
    pub removed: Vec<String>,
    /// Folders that were only forgotten, because they are not Keevoro's own.
    pub left_alone: Vec<PathBuf>,
}

/// Is `folder` strictly inside `owned`? Compared as real, resolved paths,
/// so `..` or a link cannot make somewhere else look like Keevoro's.
fn is_inside(folder: &Path, owned: &Path) -> bool {
    match (std::fs::canonicalize(folder), std::fs::canonicalize(owned)) {
        (Ok(folder), Ok(owned)) => folder != owned && folder.starts_with(&owned),
        _ => false,
    }
}

/// Forgets every backup and removes what Keevoro made for them.
///
/// `data_dir` is Keevoro's own data folder; `cache_dir` holds previews of
/// files not yet sent. Problems removing a folder do not stop the reset:
/// the settings are cleared regardless, so the app starts fresh.
pub fn reset(settings: &mut Settings, config_dir: &Path, data_dir: &Path, cache_dir: &Path) -> std::io::Result<Done> {
    let mut done = Done::default();
    for Backup { name, home, .. } in std::mem::take(&mut settings.backups) {
        if is_inside(&home, data_dir) {
            let _ = std::fs::remove_dir_all(&home);
            done.removed.push(name);
        } else if home.exists() {
            done.left_alone.push(home);
        }
    }
    let _ = std::fs::remove_dir_all(cache_dir);
    *settings = Settings::default();
    settings.save(config_dir)?;
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reset_removes_what_keevoro_made_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let (config, data, cache) = (
            dir.path().join("config"),
            dir.path().join("data"),
            dir.path().join("cache"),
        );
        let mut settings = Settings::default();
        let home = settings
            .start_backup("Drone footage", &data.join("backups"))
            .home
            .clone();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join("catalog.sqlite"), "notes").unwrap();
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("thumb.webp"), "p").unwrap();

        // The person's own things: a source, somewhere files were sent, and
        // an old library folder with gathered photos in it.
        let source = dir.path().join("Card").join("DCIM");
        let sent = dir.path().join("Sent");
        let old_library = dir.path().join("Pictures").join("Keevoro Library");
        for folder in [&source, &sent, &old_library.join("originals")] {
            std::fs::create_dir_all(folder).unwrap();
            std::fs::write(folder.join("IMG_0001.jpg"), "a photo").unwrap();
        }
        settings.backups.push(Backup {
            id: "first".into(),
            name: "My photos".into(),
            home: old_library.clone(),
        });
        settings.save(&config).unwrap();

        let done = reset(&mut settings, &config, &data, &cache).unwrap();
        assert_eq!(done.removed, ["Drone footage"]);
        assert_eq!(done.left_alone, std::slice::from_ref(&old_library));

        assert!(!home.exists(), "Keevoro's own notes are gone");
        assert!(!cache.exists(), "and its previews");
        assert_eq!(settings, Settings::default());
        assert_eq!(
            Settings::load(&config),
            Settings::default(),
            "and it starts fresh next time"
        );

        for file in [
            source.join("IMG_0001.jpg"),
            sent.join("IMG_0001.jpg"),
            old_library.join("originals").join("IMG_0001.jpg"),
        ] {
            assert_eq!(
                std::fs::read_to_string(&file).unwrap(),
                "a photo",
                "{} was touched",
                file.display()
            );
        }
    }

    #[test]
    fn a_folder_is_only_keevoros_if_it_really_is_inside_its_data() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let inside = data.join("backups").join("backup-1");
        let outside = dir.path().join("elsewhere");
        std::fs::create_dir_all(&inside).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        assert!(is_inside(&inside, &data));
        assert!(!is_inside(&outside, &data));
        assert!(!is_inside(&data, &data), "never the data folder itself");
        // A path that climbs back out is judged by where it really leads.
        assert!(!is_inside(
            &data.join("backups").join("..").join("..").join("elsewhere"),
            &data
        ));
        assert!(!is_inside(&dir.path().join("not there"), &data));
    }

    #[test]
    fn resetting_twice_or_with_nothing_there_is_harmless() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = Settings::default();
        let nowhere = dir.path().join("none");
        assert_eq!(
            reset(&mut settings, &nowhere, &nowhere, &nowhere).unwrap(),
            Done::default()
        );
        assert_eq!(
            reset(&mut settings, &nowhere, &nowhere, &nowhere).unwrap(),
            Done::default()
        );
    }
}
