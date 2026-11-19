//! Starting again from nothing: `keevoro.reset`.
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

use crate::backups::{Backups, Named, Settings};
use crate::error::Result;
use keevoro_engine::connect::{self, SecretStore, World};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// What a reset did.
#[derive(Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
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

/// Forgets every backup and removes what Keevoro made for them, along
/// with any storage keys they saved in the keychain.
///
/// `cache_dir` holds previews of files not yet sent. Problems removing a
/// folder or a key do not stop the reset: the settings are cleared
/// regardless, so Keevoro starts fresh.
pub fn reset(backups: &Backups, secrets: &dyn SecretStore, cache_dir: &Path) -> Result<Done> {
    let data_dir = backups.data_folder().to_path_buf();
    backups.change(|settings| {
        let mut done = Done::default();
        for Named { name, home, .. } in std::mem::take(&mut settings.backups) {
            // Keys first, while the backup's notes can still be read.
            if let Some(catalog) = crate::backup::existing_catalog(&home) {
                let world = World {
                    secrets,
                    ask: &connect::testing::nobody_to_ask,
                };
                let _ = connect::disconnect(&mut Vec::new(), &catalog, &world);
            }
            if is_inside(&home, &data_dir) {
                let _ = std::fs::remove_dir_all(&home);
                done.removed.push(name);
            } else if home.exists() {
                done.left_alone.push(home);
            }
        }
        let _ = std::fs::remove_dir_all(cache_dir);
        *settings = Settings::default();
        Ok(done)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::agents::AgentsAccessRequest;
    use keevoro_catalog::Catalog;
    use keevoro_engine::connect::testing::MemorySecrets;

    #[test]
    fn a_reset_removes_what_keevoro_made_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let (config, data, cache) = (
            dir.path().join("config"),
            dir.path().join("data"),
            dir.path().join("cache"),
        );
        let backups = Backups::load(&config, &data);
        let home = backups.start("Drone footage").unwrap().home;
        std::fs::write(home.join("notes.txt"), "notes").unwrap();
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
        backups
            .change(|settings| {
                settings.backups.push(Named {
                    id: "first".into(),
                    name: "My photos".into(),
                    home: old_library.clone(),
                });
                settings.plain_decoding = true;
                settings.agents.apply(AgentsAccessRequest {
                    may_change: Some(true),
                    ..AgentsAccessRequest::default()
                })
            })
            .unwrap();

        let done = reset(&backups, &MemorySecrets::default(), &cache).unwrap();
        assert_eq!(done.removed, ["Drone footage"]);
        assert_eq!(done.left_alone, std::slice::from_ref(&old_library));

        assert!(!home.exists(), "Keevoro's own notes are gone");
        assert!(!cache.exists(), "and its previews");
        assert_eq!(
            backups.read(),
            Settings::default(),
            "every setting is as it was at first"
        );
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
        let nowhere = dir.path().join("none");
        let backups = Backups::load(&nowhere, &nowhere);
        let secrets = MemorySecrets::default();
        assert_eq!(reset(&backups, &secrets, &nowhere).unwrap(), Done::default());
        assert_eq!(reset(&backups, &secrets, &nowhere).unwrap(), Done::default());
    }

    #[test]
    fn a_reset_removes_the_keys_a_backup_saved() {
        let dir = tempfile::tempdir().unwrap();
        let backups = Backups::load(&dir.path().join("config"), &dir.path().join("data"));
        let home = backups.start("Cloud").unwrap().home;
        let catalog = Catalog::open_in_library(&home).unwrap();
        // A bucket destination saved as it would be once connected.
        catalog
            .set_setting(
                "destinations",
                r#"[{"kind":"r2","place":"0123456789abcdef0123456789abcdef","bucket":"family"}]"#,
            )
            .unwrap();
        drop(catalog);
        let secrets = MemorySecrets::default();
        secrets
            .set("r2:0123456789abcdef0123456789abcdef/family", "access\nsecret")
            .unwrap();
        reset(&backups, &secrets, &dir.path().join("cache")).unwrap();
        assert!(secrets.0.borrow().is_empty(), "the keys went with it");
    }
}
