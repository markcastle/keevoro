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
//! Several programs may have the file open at once, so it is read afresh
//! every time it is asked about, and changed only while holding a lock on
//! it: the change is made to the file as it is at that moment, never to an
//! older copy, so one program's change never undoes another's. It is
//! written whole beside itself and moved into place, so nobody ever reads
//! half of it.

use crate::agents::AgentAccess;
use crate::backup::Backup;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

const FILE: &str = "settings.json";
/// Held, by whichever program is changing the settings, for the moment it
/// takes. The operating system lets go of it if that program stops.
const LOCK: &str = "settings.lock";

/// What the settings file holds.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
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
    /// The person's changes to the kinds of file Keevoro looks for: the
    /// endings they added and the built-in ones they changed. Left out
    /// while the list is as it came.
    #[serde(default, skip_serializing_if = "keevoro_core::FileKinds::is_built_in")]
    pub file_kinds: keevoro_core::FileKinds,
    /// Whatever a later version wrote that this one does not know, kept as
    /// it was and written back, so changing a setting here never loses it.
    /// (An older version than this one still drops such parts, which is why
    /// nothing new goes in this file lightly: see `docs/developing.md`.)
    #[serde(flatten)]
    pub later: serde_json::Map<String, serde_json::Value>,
}

/// The parts of the settings file this version knows, including those it
/// reads from earlier versions and does not write again.
const KNOWN_PARTS: &[&str] = &[
    "backups",
    "sessions",
    "library",
    "current",
    "plain_decoding",
    "agents",
    "file_kinds",
];

/// Keeps a settings file that could not all be understood, beside it, the
/// first time, before anything is written over it: whatever a person set
/// there can then still be found. Only the first is kept, so asking again
/// and again makes no more copies.
fn keep_aside(dir: &Path, text: &str) {
    let aside = dir.join(KEPT_ASIDE);
    if !aside.exists() {
        let _ = std::fs::write(aside, text);
    }
}

/// Where a settings file that could not all be understood is kept.
pub(crate) const KEPT_ASIDE: &str = "settings that could not be read.json";

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
        Self::read(dir).ok().flatten().unwrap_or_default()
    }

    /// The settings in `dir`: `None` when there is no file, or one that
    /// cannot be understood; an error only when the file is there and
    /// cannot be read at all, which another program replacing it can cause
    /// for a moment on some computers.
    fn read(dir: &Path) -> std::io::Result<Option<Self>> {
        let text = match std::fs::read_to_string(dir.join(FILE)) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            keep_aside(dir, &text);
            return Ok(None);
        };
        let (mut settings, whole) = Self::from_value(&value);
        if !whole {
            keep_aside(dir, &text);
        }
        settings.fall_back_to_the_first();
        Ok(Some(settings))
    }

    /// The settings in a file's JSON, each part read on its own, so a part
    /// that cannot be understood (written by a newer Keevoro, or damaged)
    /// costs that part alone, never the list of backups. Also answers
    /// whether every part was understood.
    fn from_value(value: &serde_json::Value) -> (Self, bool) {
        let part = |name: &str| value.get(name).filter(|v| !v.is_null()).cloned();
        let mut whole = true;
        let mut settings = Self::default();
        match part("backups").or_else(|| part("sessions")) {
            Some(serde_json::Value::Array(each)) => {
                for one in each {
                    match serde_json::from_value::<Named>(one) {
                        Ok(named) => settings.backups.push(named),
                        Err(_) => whole = false,
                    }
                }
            }
            Some(_) => whole = false,
            None => {}
        }
        // From before there were backups: the one library folder, which
        // 0.29 and earlier made the first backup as they read it, without
        // always writing that down. Read the same way still.
        if settings.backups.is_empty() {
            if let Some(library) = part("library").and_then(|l| l.as_str().map(PathBuf::from)) {
                settings.backups.push(Named {
                    id: "first".to_string(),
                    name: "My photos".to_string(),
                    home: library,
                });
            }
        }
        fn one<T: serde::de::DeserializeOwned + Default>(found: Option<serde_json::Value>, whole: &mut bool) -> T {
            match found {
                Some(v) => serde_json::from_value(v).unwrap_or_else(|_| {
                    *whole = false;
                    T::default()
                }),
                None => T::default(),
            }
        }
        settings.current = one(part("current"), &mut whole);
        settings.plain_decoding = one(part("plain_decoding"), &mut whole);
        settings.agents = one(part("agents"), &mut whole);
        settings.file_kinds = one(part("file_kinds"), &mut whole);
        if let Some(all) = value.as_object() {
            settings.later = all
                .iter()
                .filter(|(key, _)| !KNOWN_PARTS.contains(&key.as_str()))
                .map(|(key, v)| (key.clone(), v.clone()))
                .collect();
        }
        (settings, whole)
    }

    /// Writes the settings to `dir`, beside the real file under a name of
    /// its own and then moved over it, so a crash never leaves half a
    /// settings file and two programs writing at once never share one.
    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        std::fs::create_dir_all(dir)?;
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let temp = dir.join(format!(".{FILE}.{}.{n}.part", std::process::id()));
        std::fs::write(&temp, text)?;
        // Windows can refuse for a moment while another program reads it.
        let mut tries = 0;
        loop {
            match std::fs::rename(&temp, dir.join(FILE)) {
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && tries < 20 => {
                    tries += 1;
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&temp);
                    return Err(e);
                }
                Ok(()) => return Ok(()),
            }
        }
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
    /// `taken` says whether an id is held by a backup removed and waiting
    /// to be cleared away, which could still be put back.
    fn start(&mut self, name: &str, backups_dir: &Path, taken: &dyn Fn(&str) -> bool) -> Named {
        let wanted = match name.trim() {
            "" => "My photos",
            given => given,
        };
        let name = self.free_name(wanted);
        // A folder name that is safe everywhere, and never reused.
        let mut id = 1;
        while self.backups.iter().any(|s| s.id == format!("backup-{id}"))
            || backups_dir.join(format!("backup-{id}")).exists()
            || taken(&format!("backup-{id}"))
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

    /// `wanted`, or, when another backup is called that, `wanted 2`, `wanted
    /// 3` and so on: the first name no backup has, ignoring case.
    fn free_name(&self, wanted: &str) -> String {
        let mut name = wanted.to_string();
        let mut n = 2;
        while self.backups.iter().any(|s| s.name.eq_ignore_ascii_case(&name)) {
            name = format!("{wanted} {n}");
            n += 1;
        }
        name
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

/// Removes settings files half-written by a program that stopped between
/// writing one and moving it into place, once an hour old, so a crash
/// leaves nothing behind for good. A younger one may be another program's
/// at this moment.
fn clear_half_written(config: &Path) {
    let Ok(entries) = std::fs::read_dir(config) else { return };
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let stale = entry
            .metadata()
            .ok()
            .filter(std::fs::Metadata::is_file)
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age >= std::time::Duration::from_secs(60 * 60));
        if name.starts_with(&format!(".{FILE}.")) && name.ends_with(".part") && stale {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Keevoro's list of backups, kept in its settings file. Every change is
/// saved at once, and every question is answered from the file as it is
/// now, whichever program last changed it.
pub struct Backups {
    config: PathBuf,
    data: PathBuf,
    /// The settings as last read, for the rare moment the file cannot be
    /// read because another program is replacing it.
    last: Mutex<Settings>,
}

impl Backups {
    /// Keevoro's list, in the settings file in `config`. New backups get
    /// their home folders under `data`.
    pub fn load(config: &Path, data: &Path) -> Self {
        clear_half_written(config);
        Self {
            config: config.to_path_buf(),
            data: data.to_path_buf(),
            last: Mutex::new(Settings::load(config)),
        }
    }

    fn last(&self) -> MutexGuard<'_, Settings> {
        // A poisoned lock only means a panic elsewhere; what is inside is
        // still the last thing read.
        self.last.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The settings as they are in the file now. It is a small file, so
    /// reading it each time costs nothing worth saving.
    fn settings(&self) -> Settings {
        for _ in 0..20 {
            match Settings::read(&self.config) {
                Ok(read) => {
                    let now = read.unwrap_or_default();
                    now.clone_into(&mut self.last());
                    return now;
                }
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(10)),
            }
        }
        self.last().clone()
    }

    /// Makes a change and saves it. The change is made to the file as it
    /// is now, under a lock that other Keevoro programs wait for, so a
    /// change another program made a moment ago is kept. If it cannot be
    /// saved, nothing changes.
    pub(crate) fn change<T>(&self, change: impl FnOnce(&mut Settings) -> Result<T>) -> Result<T> {
        let failed = |e: std::io::Error| Error::failed(format!("Keevoro couldn't save its settings: {e}."));
        std::fs::create_dir_all(&self.config).map_err(failed)?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.config.join(LOCK))
            .map_err(failed)?;
        fs4::FileExt::lock(&lock).map_err(failed)?;
        let mut changed = Settings::read(&self.config).map_err(failed)?.unwrap_or_default();
        let answer = change(&mut changed)?;
        changed.save(&self.config).map_err(failed)?;
        *self.last() = changed;
        // Let go as soon as it is saved; closing the file would too.
        let _ = fs4::FileExt::unlock(&lock);
        Ok(answer)
    }

    /// A copy of everything in the settings file.
    pub fn read(&self) -> Settings {
        self.settings()
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
        self.settings().backups
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
        let name = match name.trim() {
            "" => String::new(),
            given => checked_name(given)?,
        };
        self.change(|settings| {
            let started = settings.start(&name, &self.data.join("backups"), &|id| {
                crate::removing::holds(&self.data, id)
            });
            // Opening its record proves the folder can be made and written to.
            Backup::open(&started.home)?;
            Ok(started)
        })
    }

    /// Makes this backup the open one. False if there is no such backup.
    pub fn switch_to(&self, id: &str) -> Result<bool> {
        self.change(|settings| {
            let there = settings.backups.iter().any(|s| s.id == id);
            if there {
                settings.current = Some(id.to_string());
            }
            Ok(there)
        })
    }

    /// `backups.rename`: gives a backup a new name. Its folder, its id and
    /// everything it knows stay as they were.
    pub fn rename(&self, id_or_name: &str, to: &str) -> Result<Named> {
        let to = &checked_name(to)?;
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

    /// `backups.forget`: takes a backup off Keevoro's list and sets its
    /// home folder aside for [`crate::removing::KEPT_FOR`], so it can be
    /// put back (see `removing.rs`). Nothing at a destination or on a
    /// source is touched. If it was the open one, the first left is opened
    /// instead; if it was the last, Keevoro is as it was at first.
    ///
    /// The folder is moved while the settings are held, so no other
    /// Keevoro program opens it part-way; if the list cannot then be saved,
    /// it is moved back, and nothing has changed.
    pub(crate) fn remove(&self, id_or_name: &str, now: i64) -> Result<crate::removing::Note> {
        let set_aside = std::cell::RefCell::new(None);
        let removed = self.change(|settings| {
            let found = settings
                .find(id_or_name)
                .cloned()
                .ok_or_else(|| Error::no_such_backup(id_or_name))?;
            let note = crate::removing::set_aside(&self.data, &found, now)?;
            *set_aside.borrow_mut() = Some(note.clone());
            settings.backups.retain(|s| s.id != found.id);
            settings.fall_back_to_the_first();
            Ok(note)
        });
        if removed.is_err() {
            if let Some(note) = set_aside.borrow().as_ref() {
                let _ = crate::removing::bring_back(&self.data, note);
            }
        }
        removed
    }

    /// `backups.put_back`: a removed backup, back on the list exactly as it
    /// was, under its own name or, if another backup has that name now,
    /// the name with a number.
    pub(crate) fn put_back(&self, note: &crate::removing::Note) -> Result<Named> {
        self.change(|settings| {
            if settings.backups.iter().any(|s| s.id == note.id) {
                return Err(Error::refused(format!("\"{}\" is already on the list.", note.name)));
            }
            crate::removing::bring_back(&self.data, note)?;
            let named = Named {
                id: note.id.clone(),
                name: settings.free_name(&note.name),
                home: note.home.clone(),
            };
            settings.backups.push(named.clone());
            settings.fall_back_to_the_first();
            Ok(named)
        })
    }
}

/// The longest name a backup can have. Long enough for "Grandad's slides
/// from the loft, 1960s to 1980s"; short enough to fit the switcher.
pub const LONGEST_NAME: usize = 60;

/// A name a backup can have, trimmed, or why not. A blank name, one too
/// long to show, or one with a line break or other control character in
/// it is refused, in words.
pub fn checked_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::refused("Give the backup a name."));
    }
    if name.chars().count() > LONGEST_NAME {
        return Err(Error::refused(format!(
            "Keep the name to {LONGEST_NAME} letters or fewer."
        )));
    }
    if name.chars().any(char::is_control) {
        return Err(Error::refused("A name can't have a line break or a tab in it."));
    }
    Ok(name.to_string())
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
        let mut left: Vec<String> = std::fs::read_dir(&config)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        left.sort();
        assert_eq!(left, [FILE, LOCK], "no temporary file is left");
    }

    #[test]
    fn a_settings_file_half_written_by_a_program_that_stopped_is_cleared_once_old() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join(format!(".{FILE}.123.0.part"));
        let young = dir.path().join(format!(".{FILE}.124.0.part"));
        for path in [&old, &young] {
            std::fs::write(path, "{").unwrap();
        }
        let file = std::fs::File::options().write(true).open(&old).unwrap();
        file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(7200))
            .unwrap();
        std::fs::write(dir.path().join("notes.part"), "mine").unwrap();
        Backups::load(dir.path(), dir.path());
        assert!(!old.exists());
        assert!(young.exists(), "perhaps another program's, this moment");
        assert!(dir.path().join("notes.part").exists());
    }

    /// An older Keevoro (an older `keevoro` command beside a newer app, or
    /// a release rolled back) reads this file part by part and writes back
    /// only the parts it knows, so anything new here is lost the first time
    /// it changes a setting. Nothing for backing up by itself (a schedule,
    /// what happened, the computer-wide choices) belongs here: it goes in
    /// each backup's catalog, or in a file of its own that no older version
    /// ever writes. This fails to build, or fails, when a part is added, so
    /// whoever adds one reads why first (docs/developing.md, "When what
    /// Keevoro keeps changes").
    #[test]
    fn nothing_new_goes_in_the_settings_file_without_knowing_older_versions_drop_it() {
        let Settings {
            backups: _,
            current: _,
            plain_decoding: _,
            agents: _,
            file_kinds: _,
            later: _,
        } = Settings::default();
        assert_eq!(
            KNOWN_PARTS,
            [
                "backups",
                "sessions",
                "library",
                "current",
                "plain_decoding",
                "agents",
                "file_kinds"
            ],
            "the parts 0.41 knows"
        );
        for word in ["schedule", "background", "unattended", "away", "tray"] {
            assert!(KNOWN_PARTS.iter().all(|part| !part.contains(word)), "{word}");
        }
    }

    #[test]
    fn parts_a_later_version_wrote_are_kept_when_a_setting_is_changed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(FILE),
            r#"{"backups":[],"theme":"dark","later":{"a":[1,2]},"library":null}"#,
        )
        .unwrap();
        let backups = Backups::load(dir.path(), &dir.path().join("data"));
        backups.start("Family").unwrap();
        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.path().join(FILE)).unwrap()).unwrap();
        assert_eq!(written["theme"], "dark");
        assert_eq!(written["later"]["a"][1], 2);
        assert!(
            written.get("library").is_none(),
            "an old part read is not written again"
        );
        assert_eq!(written["backups"][0]["name"], "Family");
    }

    #[test]
    fn a_damaged_settings_file_means_ask_again_not_crash() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), "{ not json").unwrap();
        assert_eq!(Settings::load(dir.path()), Settings::default());
        // A file from a future version with extra fields still loads.
        std::fs::write(dir.path().join(FILE), r#"{"plain_decoding":true,"theme":"dark"}"#).unwrap();
        assert!(Settings::load(dir.path()).plain_decoding);
    }

    #[test]
    fn a_part_of_the_settings_that_cannot_be_understood_costs_that_part_alone_and_the_file_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        // A newer Keevoro's idea of agents, and one backup listed oddly.
        let text = r#"{"backups":[{"id":"backup-1","name":"Family","home":"/data/backups/backup-1"},{"id":7}],"current":"backup-1","plain_decoding":true,"agents":{"lookIn":"everywhere"},"file_kinds":[{"ending":"xyz","kind":"video"}]}"#;
        std::fs::write(dir.path().join(FILE), text).unwrap();
        let settings = Settings::load(dir.path());
        assert_eq!(settings.backups.len(), 1);
        assert_eq!(settings.open_backup().unwrap().name, "Family");
        assert!(settings.plain_decoding);
        assert_eq!(settings.agents, AgentAccess::default());
        assert!(
            !settings.file_kinds.is_built_in(),
            "the kinds a person changed are kept"
        );
        // What could not be understood is kept, once, before anything is
        // written over it.
        assert_eq!(std::fs::read_to_string(dir.path().join(KEPT_ASIDE)).unwrap(), text);
        settings.save(dir.path()).unwrap();
        Settings::load(dir.path());
        assert_eq!(std::fs::read_to_string(dir.path().join(KEPT_ASIDE)).unwrap(), text);
        // A file that is not JSON at all is kept too.
        let other = tempfile::tempdir().unwrap();
        std::fs::write(other.path().join(FILE), "{ not json").unwrap();
        assert_eq!(Settings::load(other.path()), Settings::default());
        assert_eq!(
            std::fs::read_to_string(other.path().join(KEPT_ASIDE)).unwrap(),
            "{ not json"
        );
        // And one that is understood whole makes no copy.
        let fine = tempfile::tempdir().unwrap();
        Settings::default().save(fine.path()).unwrap();
        Settings::load(fine.path());
        assert!(!fine.path().join(KEPT_ASIDE).exists());
    }

    #[test]
    fn a_settings_file_from_before_backups_still_opens_its_library_as_the_first_backup() {
        // As 0.29 and earlier could leave it, if nothing was changed since.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), r#"{"library":"/data/Keevoro Library"}"#).unwrap();
        let settings = Settings::load(dir.path());
        let open = settings.open_backup().unwrap();
        assert_eq!(
            (open.id.as_str(), open.name.as_str(), open.home.as_path()),
            ("first", "My photos", Path::new("/data/Keevoro Library"))
        );
    }

    #[test]
    fn a_settings_file_from_when_backups_were_called_sessions_still_loads() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(FILE),
            r#"{"sessions":[{"id":"session-1","name":"Drone footage","home":"/data/sessions/session-1"}],"current":"session-1"}"#,
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
        // What is written, with the codecs switched off.
        let before = r#"{
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
    fn removing_a_backup_takes_it_off_the_list_and_sets_its_folder_aside_to_put_back() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        let data = dir.path().join("data");
        let backups = Backups::load(&config, &data);
        let drone = backups.start("Drone footage").unwrap();
        let family = backups.start("Family").unwrap();
        std::fs::write(drone.home.join("a note.txt"), "kept").unwrap();

        let note = backups.remove("drone footage", 100).unwrap();
        assert_eq!(
            (note.id.as_str(), note.name.as_str()),
            (drone.id.as_str(), "Drone footage")
        );
        assert_eq!(backups.all(), std::slice::from_ref(&family));
        assert_eq!(
            backups.current(),
            Some(family.clone()),
            "the open one moves to what is left"
        );
        assert_eq!(Backups::load(&config, &data).all().len(), 1);
        assert!(!drone.home.exists(), "its folder is set aside");
        assert_eq!(
            backups.remove("Drone footage", 100).unwrap_err().kind,
            crate::Kind::NotFound
        );
        // Its id is not given to a new backup while it could be put back.
        let next = backups.start("Holidays").unwrap();
        assert_ne!(next.id, drone.id);

        let back = backups.put_back(&note).unwrap();
        assert_eq!(back, drone, "exactly as it was");
        assert_eq!(std::fs::read_to_string(drone.home.join("a note.txt")).unwrap(), "kept");
        assert!(drone.home.join(".keevoro").join("catalog.sqlite").is_file());
    }

    #[test]
    fn the_last_backup_can_be_removed_leaving_keevoro_as_it_was_at_first() {
        let dir = tempfile::tempdir().unwrap();
        let backups = Backups::load(&dir.path().join("config"), &dir.path().join("data"));
        backups.start("Test 2").unwrap();
        backups.remove("Test 2", 1).unwrap();
        assert!(backups.all().is_empty());
        assert_eq!(backups.current(), None, "the first-run state");
    }

    #[test]
    fn a_backup_put_back_whose_name_was_taken_since_gets_a_number() {
        let dir = tempfile::tempdir().unwrap();
        let backups = Backups::load(&dir.path().join("config"), &dir.path().join("data"));
        backups.start("Test 2").unwrap();
        let note = backups.remove("Test 2", 1).unwrap();
        backups.start("Test 2").unwrap();
        assert_eq!(backups.put_back(&note).unwrap().name, "Test 2 2");
    }

    #[test]
    fn a_name_is_never_empty_too_long_or_on_two_lines() {
        let dir = tempfile::tempdir().unwrap();
        let backups = Backups::load(&dir.path().join("config"), &dir.path().join("data"));
        backups.start("Drone").unwrap();
        let long = "x".repeat(LONGEST_NAME + 1);
        for (bad, why) in [
            ("   ", "Give the backup a name."),
            (long.as_str(), "Keep the name to 60 letters or fewer."),
            ("Family\nphotos", "A name can't have a line break or a tab in it."),
        ] {
            let e = backups.rename("Drone", bad).unwrap_err();
            assert_eq!((e.kind, e.message.as_str()), (crate::Kind::Refused, why));
        }
        assert_eq!(
            backups.start(&long).unwrap_err().message,
            "Keep the name to 60 letters or fewer."
        );
        // Sixty letters in any language is fine.
        let sixty = "é".repeat(LONGEST_NAME);
        assert_eq!(backups.rename("Drone", &sixty).unwrap().name, sixty);
        assert_eq!(backups.all().len(), 1, "nothing refused was started");
    }

    #[test]
    fn the_open_backup_is_remembered_and_a_stale_one_falls_back() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = Settings::default();
        let first = settings.start("One", dir.path(), &|_| false).id;
        settings.start("Two", dir.path(), &|_| false);
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
    fn two_programs_on_one_folder_see_each_others_backups_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        let app = Backups::load(&config, dir.path());
        let command_line = Backups::load(&config, dir.path());

        let drone = app.start("Drone footage").unwrap();
        assert_eq!(
            command_line.all(),
            std::slice::from_ref(&drone),
            "seen without opening again"
        );
        let family = command_line.start("Family").unwrap();
        assert_eq!(app.find("family"), Some(family.clone()));
        assert_ne!(family.id, drone.id, "an id is never given twice");

        command_line.rename("Drone footage", "Drone").unwrap();
        assert_eq!(app.find(&drone.id).unwrap().name, "Drone");
        app.remove("Family", 1).unwrap();
        assert_eq!(command_line.all().len(), 1);
        assert_eq!(command_line.current().unwrap().id, drone.id);
    }

    #[test]
    fn a_change_never_undoes_another_programs_change() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        let programs: Vec<Backups> = (0..4).map(|_| Backups::load(&config, dir.path())).collect();
        // Each was opened before any of the others changed anything, and
        // they all change it at once.
        std::thread::scope(|threads| {
            for (n, program) in programs.iter().enumerate() {
                threads.spawn(move || {
                    for m in 0..5 {
                        program.start(&format!("Backup {n}.{m}")).unwrap();
                    }
                });
            }
        });
        let all = programs[0].all();
        assert_eq!(all.len(), 20, "{all:?}");
        let mut ids: Vec<&str> = all.iter().map(|b| b.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 20, "every backup has its own folder");
        assert_eq!(Settings::load(&config).backups, all);
    }

    #[test]
    fn a_settings_file_that_disappears_means_no_backups_not_old_ones() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        let backups = Backups::load(&config, dir.path());
        backups.start("Drone").unwrap();
        std::fs::remove_file(config.join(FILE)).unwrap();
        assert!(backups.all().is_empty(), "reset by another program");
    }
}
