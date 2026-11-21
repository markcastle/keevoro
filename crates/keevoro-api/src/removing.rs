//! Removing a backup from Keevoro, with a week to change one's mind.
//!
//! A person removing "Test 2" expects it gone, and the room it took on
//! this computer back, with nothing left lying about. A person who clicks
//! Remove on the wrong backup must not lose its record by one mis-click.
//! So removing does two things a week apart:
//!
//! - **At once.** The backup goes from the list. Its home folder, which
//!   holds only Keevoro's own record of it and the previews and working
//!   files made for it, is moved aside, into `removed/<id>/home` in
//!   Keevoro's data folder, with a note of what it was beside it. Nothing
//!   else moves. `backups.put_back` puts it back exactly as it was.
//! - **A week later** ([`KEPT_FOR`]), the next time Keevoro starts, or
//!   whenever the person asks with "Clear away now", the folder set aside
//!   is deleted, and with it the keys saved in the keychain for cloud
//!   storage that no other backup uses. Keeping the keys until then is
//!   what lets putting it back work without asking for them again.
//!
//! Nothing at a destination and nothing on a source is ever touched, by
//! either step: only Keevoro's own folder for the backup is moved and
//! deleted, and only when it lies inside Keevoro's own data folder (judged
//! by where it really leads, as a reset judges it). A home folder anywhere
//! else, such as a library from an early version kept in Pictures, is only
//! forgotten and left exactly where it is.

use crate::backups::Named;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// How long a removed backup can be put back.
pub const KEPT_FOR: i64 = 7 * 24 * 60 * 60;

/// The same, in days, for saying.
pub const KEPT_FOR_DAYS: i64 = KEPT_FOR / (24 * 60 * 60);

/// Where removed backups wait, inside Keevoro's data folder.
const FOLDER: &str = "removed";
/// The note beside each, saying what it was.
const NOTE: &str = "removed.json";
/// The backup's home folder, once moved aside.
const HOME: &str = "home";

/// What is remembered of a removed backup until it is cleared away.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    pub name: String,
    /// Where its home folder was, and goes back to.
    pub home: PathBuf,
    /// When it was removed, in seconds since 1970.
    pub removed_at: i64,
    /// True when its home folder was Keevoro's own and was moved aside;
    /// false when it was somewhere else and only forgotten.
    pub moved: bool,
    /// How much room its home folder took when it was removed.
    #[serde(default)]
    pub bytes: u64,
}

/// One removed backup, as the list of those that can be put back shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovedBackup {
    pub id: String,
    pub name: String,
    pub removed_at: i64,
    /// When it is cleared away for good, unless put back first.
    pub kept_until: i64,
    /// The room its folder takes on this computer until then.
    pub bytes: u64,
    /// What to say about it, in a sentence.
    pub said: String,
}

impl Note {
    pub(crate) fn view(&self) -> RemovedBackup {
        let kept_until = self.removed_at.saturating_add(KEPT_FOR);
        RemovedBackup {
            id: self.id.clone(),
            name: self.name.clone(),
            removed_at: self.removed_at,
            kept_until,
            bytes: self.bytes,
            said: format!(
                "Removed {}. It can be put back until {}, when {}.",
                keevoro_engine::summary::ago(self.removed_at, crate::jobs::seconds_now()),
                the_day(kept_until),
                if self.moved {
                    format!(
                        "its {} of previews and working files are cleared away",
                        keevoro_engine::summary::bytes(self.bytes)
                    )
                } else {
                    "Keevoro forgets it".to_string()
                }
            ),
        }
    }
}

/// A day, given in seconds since 1970, as a person writes it: "28 November
/// 2026".
pub fn the_day(seconds: i64) -> String {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    match keevoro_core::PartialDate::from_unix_seconds(seconds) {
        Some(d) => format!(
            "{} {} {:04}",
            d.day.unwrap_or(1),
            MONTHS[usize::from(d.month.unwrap_or(1).clamp(1, 12)) - 1],
            d.year
        ),
        None => "a week from now".to_string(),
    }
}

/// Is a removed backup due to be cleared away for good at `now`? Pure.
pub fn due(removed_at: i64, now: i64) -> bool {
    now.saturating_sub(removed_at) >= KEPT_FOR
}

pub(crate) fn folder(data: &Path) -> PathBuf {
    data.join(FOLDER)
}

/// True while a removed backup with this id waits to be cleared away, so
/// its id is not given to a new backup.
pub(crate) fn holds(data: &Path, id: &str) -> bool {
    folder(data).join(id).exists()
}

/// How much room a folder and everything in it takes. Links are not
/// followed, so nothing outside it is counted.
pub(crate) fn room_taken(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return if meta.is_file() { meta.len() } else { 0 };
    }
    std::fs::read_dir(path)
        .map(|entries| entries.filter_map(|e| e.ok()).map(|e| room_taken(&e.path())).sum())
        .unwrap_or(0)
}

/// Moves a backup's home folder aside, when it is Keevoro's own, and
/// writes the note. Nothing else is touched. On any problem nothing has
/// moved.
pub(crate) fn set_aside(data: &Path, named: &Named, now: i64) -> Result<Note> {
    let failed = |e: std::io::Error| {
        Error::failed(format!(
            "Keevoro couldn't set \"{}\" aside: {e}. Nothing was removed; try again in a moment.",
            named.name
        ))
    };
    let own = crate::reset::is_inside(&named.home, data);
    let place = folder(data).join(&named.id);
    if place.exists() {
        // An id is never reused, so this is one removed and put back by
        // hand, or half set aside before: leave it for the person.
        return Err(Error::failed(format!(
            "Keevoro couldn't set \"{}\" aside: something is already at {}.",
            named.name,
            keevoro_engine::friendly_path(&place)
        )));
    }
    std::fs::create_dir_all(&place).map_err(failed)?;
    let note = Note {
        id: named.id.clone(),
        name: named.name.clone(),
        home: named.home.clone(),
        removed_at: now,
        moved: own,
        bytes: if own { room_taken(&named.home) } else { 0 },
    };
    let written = serde_json::to_string_pretty(&note)
        .map_err(std::io::Error::other)
        .and_then(|text| std::fs::write(place.join(NOTE), text));
    if let Err(e) = written {
        let _ = std::fs::remove_dir_all(&place);
        return Err(failed(e));
    }
    if own {
        // Within Keevoro's own data folder, so on the same disk: a move is
        // one step, and on Windows refused while a file in it is open.
        if let Err(e) = std::fs::rename(&named.home, place.join(HOME)) {
            let _ = std::fs::remove_dir_all(&place);
            return Err(failed(e));
        }
    }
    Ok(note)
}

/// Every removed backup still waiting, oldest first. A note that cannot be
/// read is passed over and left as it is.
pub(crate) fn notes(data: &Path) -> Vec<Note> {
    let mut all: Vec<Note> = std::fs::read_dir(folder(data))
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| std::fs::read_to_string(e.path().join(NOTE)).ok())
        .filter_map(|text| serde_json::from_str(&text).ok())
        .collect();
    all.sort_by(|a: &Note, b: &Note| a.removed_at.cmp(&b.removed_at).then_with(|| a.id.cmp(&b.id)));
    all
}

/// The note of one removed backup, by its id or name, ignoring case.
pub(crate) fn find(data: &Path, id_or_name: &str) -> Option<Note> {
    let wanted = id_or_name.trim();
    let all = notes(data);
    all.iter()
        .find(|n| n.id == wanted)
        .or_else(|| all.iter().rev().find(|n| n.name.eq_ignore_ascii_case(wanted)))
        .cloned()
}

/// Moves a removed backup's home folder back where it was and lets go of
/// its note. Nothing else is touched.
pub(crate) fn bring_back(data: &Path, note: &Note) -> Result<()> {
    let place = folder(data).join(&note.id);
    if note.moved {
        if note.home.exists() {
            return Err(Error::refused(format!(
                "\"{}\" can't be put back: something is already at {}.",
                note.name,
                keevoro_engine::friendly_path(&note.home)
            )));
        }
        if let Some(parent) = note.home.parent() {
            std::fs::create_dir_all(parent).map_err(|e| couldnt_put_back(note, &e))?;
        }
        std::fs::rename(place.join(HOME), &note.home).map_err(|e| couldnt_put_back(note, &e))?;
    }
    std::fs::remove_dir_all(&place).map_err(|e| couldnt_put_back(note, &e))?;
    Ok(())
}

fn couldnt_put_back(note: &Note, e: &std::io::Error) -> Error {
    Error::failed(format!("Keevoro couldn't put \"{}\" back: {e}.", note.name))
}

/// The keychain entries a removed backup's destinations use, read from its
/// record while it waits.
pub(crate) fn keys_of(data: &Path, note: &Note) -> Vec<(String, String)> {
    let home = if note.moved {
        folder(data).join(&note.id).join(HOME)
    } else {
        note.home.clone()
    };
    crate::backup::existing_catalog(&home)
        .and_then(|catalog| keevoro_engine::connect::keys_kept_for(&catalog).ok())
        .unwrap_or_default()
}

/// Deletes what was set aside for a removed backup: its home folder when
/// it was Keevoro's own, and the note. Only ever inside Keevoro's own
/// `removed` folder.
pub(crate) fn clear_away(data: &Path, note: &Note) -> Result<()> {
    let place = folder(data).join(&note.id);
    // The id came from a note Keevoro wrote; checked all the same, so
    // nothing outside its own folder can ever be named.
    if !crate::reset::is_inside(&place, &folder(data)) {
        return Ok(());
    }
    std::fs::remove_dir_all(&place).map_err(|e| {
        Error::failed(format!(
            "Keevoro couldn't clear away \"{}\": {e}. It will try again next time it starts.",
            note.name
        ))
    })
}

/// `backups.forget`, asked first or done: what removing a backup does, in
/// words, before it is done and after.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Removal {
    pub id: String,
    pub name: String,
    /// True once it is removed; false when only asked what removing would do.
    pub removed: bool,
    /// Its sources, as they are shown. Never touched.
    pub sources: Vec<String>,
    /// Where it backs up to, in words. Never touched.
    pub destinations: Vec<String>,
    /// Its own folder on this computer, which holds Keevoro's record of it,
    /// its previews and its working files.
    pub own_folder: String,
    /// The room that folder takes.
    pub own_folder_bytes: u64,
    /// True when that folder is Keevoro's own and is cleared away; false
    /// for one somewhere else, which is only forgotten.
    pub clears_own_folder: bool,
    /// Cloud storage whose saved keys go from the keychain with it, being
    /// used by no other backup.
    pub keys_for: Vec<String>,
    /// Once removed: when it is cleared away for good unless put back.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kept_until: Option<i64>,
    /// All of that in sentences, the same in the app and at the command line.
    pub said: Vec<String>,
}

/// "A", "A and B", "A, B and C".
fn listed(things: &[String]) -> String {
    match things {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// What removing a backup does, a sentence each, before it is done
/// (`done` false) or after. Pure.
pub fn removal_words(removal: &Removal, done: bool) -> Vec<String> {
    let name = &removal.name;
    let mut said = vec![if done {
        format!("Removed \"{name}\" from Keevoro: its sources, what it found, your choices and its history.")
    } else {
        format!(
            "Keevoro's record of \"{name}\" on this computer goes: its sources, what it found, your choices and \
             its history."
        )
    }];
    said.push(match removal.destinations.as_slice() {
        [] => "It has not been set to back up anywhere, so there is nothing anywhere else to keep.".to_string(),
        all => format!(
            "Everything already backed up to {} stays where it is. Keevoro does not touch it.",
            listed(all)
        ),
    });
    said.push(match removal.sources.as_slice() {
        [] => "Nothing on any source is touched.".to_string(),
        all => format!("Nothing on its sources ({}) is touched.", listed(all)),
    });
    if removal.clears_own_folder {
        said.push(format!(
            "Its previews and working files on this computer ({}) are cleared away. You can put it back from \
             Manage backups for {KEPT_FOR_DAYS} days; after that the room is freed.",
            keevoro_engine::summary::bytes(removal.own_folder_bytes)
        ));
    } else {
        said.push(format!(
            "Its folder, {}, was not made by Keevoro, so it is left exactly where it is.",
            removal.own_folder
        ));
    }
    if !removal.keys_for.is_empty() {
        said.push(format!(
            "The storage keys saved on this computer for {} go then too. No other backup uses them.",
            listed(&removal.keys_for)
        ));
    }
    said
}

impl crate::Keevoro {
    /// `backups.forget`: removes a backup from Keevoro, or with `dry_run`
    /// only says what that would do. Refused while a search, a backup or a
    /// check is running on it, here or in another program. The background
    /// check for duplicates, which holds its record open, is stopped first.
    pub fn forget_backup(&self, backup: &str, dry_run: bool) -> Result<Removal> {
        let named = self.backups().named_or_open(Some(backup))?;
        if let Some(why) = self.jobs.working_on(&named) {
            return Err(Error::busy(why));
        }
        let mut removal = self.what_removing_does(&named);
        if dry_run {
            removal.said = removal_words(&removal, false);
            return Ok(removal);
        }
        self.jobs.stop_and_wait(
            crate::jobs::Which::KindsOn(&[crate::JobKind::Duplicates], &named.id),
            std::time::Duration::from_secs(30),
            self.log(),
        );
        let note = self.backups().remove(&named.id, crate::jobs::seconds_now())?;
        removal.removed = true;
        if note.moved {
            removal.own_folder_bytes = note.bytes;
        }
        removal.kept_until = Some(note.removed_at.saturating_add(KEPT_FOR));
        removal.said = removal_words(&removal, true);
        self.log().say(&format!(
            "Removed backup \"{}\". Nothing at its destinations or on its sources was touched; {}",
            named.name,
            if note.moved {
                format!(
                    "its own folder is set aside until {}, to put back if wanted",
                    the_day(note.removed_at.saturating_add(KEPT_FOR))
                )
            } else {
                format!(
                    "its folder was left where it is: {}",
                    keevoro_engine::friendly_path(&named.home)
                )
            }
        ));
        Ok(removal)
    }

    /// What removing `named` would do, without the sentences.
    fn what_removing_does(&self, named: &Named) -> Removal {
        let open = crate::backup::Backup::open_existing(&named.home);
        let found = open.as_ref().and_then(|b| b.discover().ok());
        let own = crate::reset::is_inside(&named.home, self.backups().data_folder());
        let keys = open
            .as_ref()
            .and_then(|b| keevoro_engine::connect::keys_kept_for(b.catalog()).ok())
            .unwrap_or_default();
        let shared = self.keys_used_elsewhere(&named.id);
        Removal {
            id: named.id.clone(),
            name: named.name.clone(),
            removed: false,
            sources: found
                .as_ref()
                .map(|d| d.sources.iter().map(|s| s.root.clone()).collect())
                .unwrap_or_default(),
            destinations: found
                .as_ref()
                .map(|d| d.destinations.iter().map(|d| d.name.clone()).collect())
                .unwrap_or_default(),
            own_folder: keevoro_engine::friendly_path(&named.home),
            own_folder_bytes: if own { room_taken(&named.home) } else { 0 },
            clears_own_folder: own,
            keys_for: keys
                .into_iter()
                .filter(|(key, _)| !shared.contains(key))
                .map(|(_, described)| described)
                .collect(),
            kept_until: None,
            said: Vec::new(),
        }
    }

    /// The keychain entries used by any backup but the one with this id:
    /// those on the list, and those removed and waiting, which could be
    /// put back.
    fn keys_used_elsewhere(&self, but: &str) -> std::collections::HashSet<String> {
        let data = self.backups().data_folder();
        let listed = self
            .backups()
            .all()
            .into_iter()
            .filter(|b| b.id != but)
            .filter_map(|b| crate::backup::existing_catalog(&b.home))
            .flat_map(|catalog| keevoro_engine::connect::keys_kept_for(&catalog).unwrap_or_default());
        let waiting = notes(data)
            .into_iter()
            .filter(|n| n.id != but)
            .flat_map(|n| keys_of(data, &n));
        listed.chain(waiting).map(|(key, _)| key).collect()
    }

    /// `backups.removed`: the backups removed in the last week, which can
    /// still be put back, oldest first. Any removed longer ago are cleared
    /// away first.
    pub fn removed_backups(&self) -> Vec<RemovedBackup> {
        self.clear_away_old_removed();
        notes(self.backups().data_folder()).iter().map(Note::view).collect()
    }

    /// `backups.put_back`: a backup removed in the last week, back exactly
    /// as it was.
    pub fn put_back_backup(&self, backup: &str) -> Result<Named> {
        let note = find(self.backups().data_folder(), backup).ok_or_else(|| {
            Error::not_found(format!(
                "There is no removed backup called \"{}\" to put back.",
                backup.trim()
            ))
        })?;
        let named = self.backups().put_back(&note)?;
        self.log()
            .say(&format!("Put back backup \"{}\", as it was", named.name));
        Ok(named)
    }

    /// `backups.clear_away`: clears away a removed backup now, rather than
    /// a week after it was removed: its own folder and the keys no other
    /// backup uses. It can no longer be put back.
    pub fn clear_away_backup(&self, backup: &str) -> Result<RemovedBackup> {
        let data = self.backups().data_folder();
        let note = find(data, backup).ok_or_else(|| {
            Error::not_found(format!(
                "There is no removed backup called \"{}\" to clear away.",
                backup.trim()
            ))
        })?;
        self.clear_away_note(&note)?;
        Ok(note.view())
    }

    /// Clears away, quietly, every removed backup older than a week. The
    /// app does this as it starts; listing the removed ones does it too.
    /// Answers how many.
    pub fn clear_away_old_removed(&self) -> usize {
        let now = crate::jobs::seconds_now();
        notes(self.backups().data_folder())
            .iter()
            .filter(|n| due(n.removed_at, now))
            .filter(|n| self.clear_away_note(n).is_ok())
            .count()
    }

    fn clear_away_note(&self, note: &Note) -> Result<()> {
        let data = self.backups().data_folder();
        let shared = self.keys_used_elsewhere(&note.id);
        let keys: Vec<String> = keys_of(data, note)
            .into_iter()
            .map(|(key, _)| key)
            .filter(|key| !shared.contains(key))
            .collect();
        keevoro_engine::connect::forget_keys(self.secrets(), &keys)
            .map_err(|e| Error::failed(format!("Keevoro couldn't remove the saved storage keys: {e:#}.")))?;
        clear_away(data, note)?;
        self.log().say(&format!(
            "Cleared away backup \"{}\", removed {}: {} freed",
            note.name,
            keevoro_engine::summary::ago(note.removed_at, crate::jobs::seconds_now()),
            keevoro_engine::summary::bytes(note.bytes)
        ));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(data: &Path, id: &str) -> Named {
        let home = data.join("backups").join(id);
        std::fs::create_dir_all(home.join(".keevoro")).unwrap();
        std::fs::write(home.join(".keevoro").join("catalog.sqlite"), "record").unwrap();
        std::fs::write(home.join("thumb.webp"), vec![0u8; 1000]).unwrap();
        Named {
            id: id.to_string(),
            name: "Test 2".to_string(),
            home,
        }
    }

    #[test]
    fn a_backup_set_aside_is_moved_whole_and_put_back_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let backup = named(&data, "backup-2");
        let note = set_aside(&data, &backup, 1_000).unwrap();
        assert!(note.moved);
        assert!(note.bytes >= 1000, "{}", note.bytes);
        assert!(!backup.home.exists(), "moved aside");
        assert!(holds(&data, "backup-2"), "its id is not given to another");
        assert_eq!(notes(&data), std::slice::from_ref(&note));
        assert_eq!(find(&data, "test 2"), Some(note.clone()));
        assert_eq!(find(&data, "backup-2"), Some(note.clone()));

        bring_back(&data, &note).unwrap();
        assert_eq!(
            std::fs::read_to_string(backup.home.join(".keevoro").join("catalog.sqlite")).unwrap(),
            "record"
        );
        assert!(backup.home.join("thumb.webp").is_file());
        assert!(notes(&data).is_empty());
        assert!(!holds(&data, "backup-2"));
    }

    #[test]
    fn clearing_away_deletes_only_what_was_set_aside() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let backup = named(&data, "backup-2");
        let neighbour = named(&data, "backup-3");
        let note = set_aside(&data, &backup, 1_000).unwrap();
        clear_away(&data, &note).unwrap();
        assert!(notes(&data).is_empty());
        assert!(!folder(&data).join("backup-2").exists());
        assert!(neighbour.home.join("thumb.webp").is_file(), "nothing else is touched");
        // A note naming somewhere outside cannot reach it.
        let outside = Note {
            id: "../backups/backup-3".to_string(),
            ..note
        };
        clear_away(&data, &outside).unwrap();
        assert!(neighbour.home.join("thumb.webp").is_file());
    }

    #[test]
    fn a_home_folder_that_is_not_keevoros_own_is_only_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let library = dir.path().join("Pictures").join("Keevoro Library");
        std::fs::create_dir_all(library.join("originals")).unwrap();
        std::fs::write(library.join("originals").join("IMG_1.jpg"), "a photo").unwrap();
        let old = Named {
            id: "first".into(),
            name: "My photos".into(),
            home: library.clone(),
        };
        let note = set_aside(&data, &old, 5).unwrap();
        assert!(!note.moved);
        assert_eq!(note.bytes, 0);
        assert!(
            library.join("originals").join("IMG_1.jpg").is_file(),
            "left where it is"
        );
        clear_away(&data, &note).unwrap();
        assert_eq!(
            std::fs::read_to_string(library.join("originals").join("IMG_1.jpg")).unwrap(),
            "a photo"
        );
    }

    #[test]
    fn a_removed_backup_is_kept_for_a_week() {
        assert!(!due(1_000, 1_000));
        assert!(!due(1_000, 1_000 + KEPT_FOR - 1));
        assert!(due(1_000, 1_000 + KEPT_FOR));
        assert_eq!(KEPT_FOR_DAYS, 7);
    }

    #[test]
    fn a_folder_that_cannot_be_moved_leaves_everything_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let backup = named(&data, "backup-2");
        // Something already where it would go.
        std::fs::create_dir_all(folder(&data).join("backup-2")).unwrap();
        let e = set_aside(&data, &backup, 1).unwrap_err();
        assert!(e.message.contains("couldn't set \"Test 2\" aside"), "{e}");
        assert!(backup.home.join("thumb.webp").is_file());
    }
}
