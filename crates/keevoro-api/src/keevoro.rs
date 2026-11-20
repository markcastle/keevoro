//! Keevoro as a whole: its folders, its backups, its settings, its log and
//! the keychain it keeps storage keys in.
//!
//! The app makes one of these when it starts and keeps it; the command line
//! will make one for each command. Everything is reached through `&self`,
//! and nothing slow is done while the settings are held, so the app can ask
//! for one thing while another is under way.

use crate::activity::{self, ActivityView};
use crate::agents::{AgentAccess, AgentsAccessRequest};
use crate::backup::Backup;
use crate::backups::{Backups, Named};
use crate::destinations::{AddDestinationRequest, DestinationAdded};
use crate::error::{Error, Result};
use crate::jobs::across::Across;
use crate::jobs::{FrontEnd, Jobs};
use crate::log::Log;
use crate::picture::PictureSize;
use crate::reset;
use keevoro_engine::connect::{Keychain, SecretStore};
use keevoro_update::{Check, GitHub, Web};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The folders Keevoro keeps its own things in. None of them holds anyone's
/// photos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folders {
    /// Where the settings file is.
    pub config: PathBuf,
    /// Keevoro's own data: new backups' home folders go under `backups`.
    pub data: PathBuf,
    /// Pictures made for looking at. Safe to delete.
    pub cache: PathBuf,
    /// The log.
    pub logs: PathBuf,
}

/// The name Keevoro's folders are kept under on this computer. It is the
/// app's own identifier (`tauri.conf.json`), so that the app and the
/// command line share their backups, settings and log.
pub const IDENTIFIER: &str = "com.markcastle.keevoro";

impl Folders {
    /// The folders the app keeps its things in on this computer, which
    /// every Keevoro program shares: `None` only on a computer that will
    /// not say where its usual folders are.
    pub fn for_this_computer() -> Option<Self> {
        let logs = if cfg!(target_os = "macos") {
            dirs::home_dir()?.join("Library").join("Logs").join(IDENTIFIER)
        } else {
            dirs::data_local_dir()?.join(IDENTIFIER).join("logs")
        };
        Some(Self {
            config: dirs::config_dir()?.join(IDENTIFIER),
            data: dirs::data_dir()?.join(IDENTIFIER),
            // Previews of photos not yet backed up. Safe to delete.
            cache: dirs::cache_dir()?.join(IDENTIFIER).join("previews"),
            logs,
        })
    }

    /// Every folder under one: for trying Keevoro out, or a test, without
    /// touching the real ones.
    pub fn under(home: &Path) -> Self {
        Self {
            config: home.join("config"),
            data: home.join("data"),
            cache: home.join("cache"),
            logs: home.join("logs"),
        }
    }
}

/// One backup in `backups.list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupListing {
    pub id: String,
    pub name: String,
    /// Its home folder, which holds Keevoro's record of it.
    pub home: String,
    /// True for the one the app has open.
    pub open: bool,
    /// What it holds, or `None` when its home folder is not there (a drive
    /// that is unplugged, say).
    pub found: Option<BackupCounts>,
}

/// The numbers `backups.list` gives for each backup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupCounts {
    pub sources: usize,
    pub destinations: usize,
    pub photos: usize,
    pub videos: usize,
    /// Files being kept that are not yet safely everywhere they should be,
    /// and the room they need.
    pub waiting: usize,
    pub waiting_bytes: u64,
}

/// `backups.start`
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartBackupRequest {
    pub name: String,
}

/// `backups.rename`
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenameBackupRequest {
    /// The backup's name or id.
    pub backup: String,
    pub to: String,
}

/// `backups.forget`
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ForgetBackupRequest {
    /// The backup's name or id.
    pub backup: String,
}

/// Whether Keevoro uses the computer's own picture codecs, and what that
/// means on this computer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Codecs {
    /// The person's choice. On unless they switched it off.
    pub on: bool,
    /// False on a computer where Keevoro has no system codecs to ask, so
    /// the switch would change nothing.
    pub available: bool,
    /// The ways a photo will be decoded, in the order they are tried.
    pub ways: Vec<&'static str>,
    /// How many previews are made at once.
    pub cores: usize,
}

/// `keevoro.settings`
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub own_codecs: Codecs,
}

/// `keevoro.settings`: without anything in it, it only reads.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsRequest {
    #[serde(default)]
    pub own_codecs: Option<bool>,
}

/// `keevoro.log`
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LogRequest {
    /// How many of the last lines; 400 when not said.
    #[serde(default)]
    pub lines: Option<usize>,
}

/// A newer Keevoro that this computer can install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Offer {
    pub version: String,
    /// The release's page, where "What's new" is.
    pub page: String,
}

/// Keevoro, open.
pub struct Keevoro {
    backups: Backups,
    cache: PathBuf,
    /// Where rates measured in backups are remembered (`rates.rs`).
    rates: PathBuf,
    log: Arc<Log>,
    secrets: Arc<dyn SecretStore + Send + Sync>,
    web: Box<dyn Web + Send + Sync>,
    /// The jobs running, and everyone listening to them.
    pub(crate) jobs: Arc<Jobs>,
}

impl Keevoro {
    /// Opens Keevoro's settings and log in these folders, with the
    /// computer's own keychain for storage keys, and GitHub for updates.
    /// The saved choice about picture codecs takes effect at once.
    pub fn open(folders: &Folders) -> Self {
        let log = Arc::new(Log::open(&folders.logs));
        let jobs = Arc::<Jobs>::default();
        // Jobs are shared with Keevoro's other programs through a folder for
        // each backup in Keevoro's own data, never in a source or a
        // destination.
        let computer = Arc::new(crate::jobs::across::lock::ThisComputer);
        jobs.share(
            Across::new(folders.data.join("jobs"), log.clone(), computer.clone(), computer),
            log.clone(),
        );
        let keevoro = Self {
            backups: Backups::load(&folders.config, &folders.data),
            cache: folders.cache.clone(),
            rates: folders.data.join(crate::rates::FILE),
            log,
            secrets: Arc::new(Keychain),
            web: Box::new(GitHub::from_env()),
            jobs,
        };
        keevoro.apply_saved_choices();
        keevoro
    }

    /// Keeps storage keys somewhere else: for tests, a keychain in memory.
    #[must_use]
    pub fn with_secrets(mut self, secrets: impl SecretStore + Send + Sync + 'static) -> Self {
        self.secrets = Arc::new(secrets);
        self
    }

    /// Says which front end this is, so a job can say who started it. The
    /// app's, unless told otherwise.
    #[must_use]
    pub fn as_front_end(self, front_end: FrontEnd) -> Self {
        self.jobs.set_front_end(front_end);
        self
    }

    /// How often a running job says how it is getting on; a few times a
    /// second unless told otherwise. Tests hear every step with zero.
    #[must_use]
    pub fn with_progress_every(self, every: std::time::Duration) -> Self {
        self.jobs.set_progress_every(every);
        self
    }

    /// Asks somewhere else about updates: for tests, a stand-in for GitHub.
    #[must_use]
    pub fn with_web(mut self, web: impl Web + Send + Sync + 'static) -> Self {
        self.web = Box::new(web);
        self
    }

    pub fn backups(&self) -> &Backups {
        &self.backups
    }

    pub fn log(&self) -> &Log {
        &self.log
    }

    pub(crate) fn log_shared(&self) -> Arc<Log> {
        self.log.clone()
    }

    pub(crate) fn rates_file(&self) -> PathBuf {
        self.rates.clone()
    }

    pub(crate) fn secrets_shared(&self) -> Arc<dyn SecretStore + Send + Sync> {
        self.secrets.clone()
    }

    /// Where storage keys are kept.
    pub fn secrets(&self) -> &dyn SecretStore {
        &*self.secrets
    }

    fn apply_saved_choices(&self) {
        keevoro_media::decode::set_use_system(!self.backups.read().plain_decoding);
    }

    /// `backups.list`: every backup, with what each holds. Opens and counts
    /// each one, so it takes a moment with many; [`Backups::all`] is the
    /// quick list of names.
    pub fn list_backups(&self) -> Vec<BackupListing> {
        let open = self.backups.current().map(|b| b.id);
        self.backups
            .all()
            .into_iter()
            .map(|named| BackupListing {
                found: Backup::open_existing(&named.home)
                    .and_then(|backup| backup.discover().ok())
                    .map(|d| BackupCounts {
                        sources: d.sources.len(),
                        destinations: d.destinations.len(),
                        photos: d.photos,
                        videos: d.videos,
                        waiting: d.waiting_to_gather,
                        waiting_bytes: d.waiting_bytes,
                    }),
                open: open.as_ref() == Some(&named.id),
                home: named.home.display().to_string(),
                id: named.id,
                name: named.name,
            })
            .collect()
    }

    /// `backups.start`
    pub fn start_backup(&self, name: &str) -> Result<Named> {
        let started = self.backups.start(name)?;
        self.log.say(&format!("Started backup \"{}\"", started.name));
        Ok(started)
    }

    /// `backups.rename`
    pub fn rename_backup(&self, backup: &str, to: &str) -> Result<Named> {
        let before = self.backups.named_or_open(Some(backup))?;
        let renamed = self.backups.rename(&before.id, to)?;
        self.log
            .say(&format!("Renamed backup \"{}\" to \"{}\"", before.name, renamed.name));
        Ok(renamed)
    }

    /// `backups.forget`: takes it off the list. Nothing on disk is deleted,
    /// not even its own home folder.
    pub fn forget_backup(&self, backup: &str) -> Result<Named> {
        let gone = self.backups.forget(backup)?;
        self.log.say(&format!(
            "Forgot backup \"{}\". Its folder was left where it is: {}",
            gone.name,
            keevoro_engine::friendly_path(&gone.home)
        ));
        Ok(gone)
    }

    /// Makes another backup the open one. False if there is none with that
    /// id.
    pub fn switch_to(&self, id: &str) -> Result<bool> {
        let switched = self.backups.switch_to(id)?;
        if let Some(open) = self.backups.current().filter(|_| switched) {
            self.log.say(&format!("Switched to backup \"{}\"", open.name));
        }
        Ok(switched)
    }

    /// `destinations.add`. The keys go to the keychain and nowhere else.
    pub fn add_destination(&self, backup: &Backup, request: &AddDestinationRequest) -> Result<DestinationAdded> {
        backup.add_destination(request, self.secrets())
    }

    /// `destinations.remove`: one by its place in the list, or every one.
    pub fn remove_destination(&self, backup: &Backup, at: Option<usize>) -> Result<Vec<String>> {
        let gone = backup.remove_destination(at, self.secrets())?;
        if at.is_some() {
            for name in &gone {
                self.log.say(&format!(
                    "No longer backing up to {name}. Nothing stored there was changed"
                ));
            }
        }
        Ok(gone)
    }

    /// `photos.picture`, as WebP bytes.
    pub fn picture(&self, backup: &Backup, photo: &str, size: PictureSize) -> Result<Vec<u8>> {
        crate::picture::picture(backup, &self.cache, photo, size)
    }

    /// `keevoro.settings`, read.
    pub fn settings(&self) -> SettingsView {
        let on = keevoro_media::decode::use_system();
        let cores = std::thread::available_parallelism().map_or(2, std::num::NonZero::get);
        SettingsView {
            own_codecs: Codecs {
                on,
                available: keevoro_media::decode::system_codecs_available(),
                ways: keevoro_media::Decoders::for_this_computer(on).names(),
                cores: (cores / 2).clamp(1, 4),
            },
        }
    }

    /// `keevoro.settings`, set: switches the use of the computer's own
    /// codecs on or off, and remembers.
    pub fn set_own_codecs(&self, on: bool) -> Result<SettingsView> {
        self.backups.change(|settings| {
            settings.plain_decoding = !on;
            Ok(())
        })?;
        keevoro_media::decode::set_use_system(on);
        self.log.say(if on {
            "Using this computer's own picture codecs again"
        } else {
            "Switched to the built-in picture decoder only"
        });
        Ok(self.settings())
    }

    /// `keevoro.log`: the last `lines` lines, oldest first.
    pub fn log_tail(&self, lines: usize) -> String {
        self.log.tail(lines)
    }

    /// What a running backup or check last said it was doing, or empty.
    pub fn backup_step(&self) -> String {
        self.jobs.backup_step()
    }

    /// `keevoro.activity`
    pub fn activity(&self) -> ActivityView {
        activity::now(Some(&self.backup_step()))
    }

    /// `keevoro.reset`: puts Keevoro back as it was when first installed.
    /// Every backup is forgotten, along with what it found and chose, its
    /// previews and any storage keys it saved. Nothing on a source, nothing
    /// that was sent anywhere, and no folder outside Keevoro's own data is
    /// touched.
    pub fn reset(&self) -> Result<reset::Done> {
        if self.busy() {
            return Err(Error::busy(
                "Keevoro is in the middle of something. Let that finish, or stop it, before starting again.",
            ));
        }
        let done = reset::reset(&self.backups, self.secrets(), &self.cache)?;
        // What was learnt about this computer's speed goes too.
        let _ = std::fs::remove_file(&self.rates);
        self.log.say(&format!(
            "Reset to how Keevoro was when first installed: {} backups removed, {} folders left where they are",
            done.removed.len(),
            done.left_alone.len()
        ));
        for folder in &done.left_alone {
            self.log.say(&format!(
                "Left alone, because it is not Keevoro's own: {}",
                keevoro_engine::friendly_path(folder)
            ));
        }
        Ok(done)
    }

    /// `keevoro.update`: a newer Keevoro with an installer for this kind of
    /// computer, if there is one. Being offline, or GitHub being
    /// unreachable, is not worth bothering anyone about: it simply means no
    /// offer today. Installing is for the app.
    pub fn update_offer(&self) -> Option<Offer> {
        offer(&*self.web, env!("CARGO_PKG_VERSION"), std::env::consts::OS)
    }

    /// `agents.access`, read.
    pub fn agent_access(&self) -> AgentAccess {
        self.backups.read().agents
    }

    /// `agents.access`, set. Each part given replaces what was there.
    pub fn set_agent_access(&self, request: AgentsAccessRequest) -> Result<AgentAccess> {
        let access = self.backups.change(|settings| {
            settings.agents.apply(request)?;
            Ok(settings.agents.clone())
        })?;
        self.log.say(&format!(
            "Agents may now look in {} folders and back up to {}, and {}",
            access.look_in.len(),
            access.back_up_to.len(),
            if access.may_change {
                "may make changes"
            } else {
                "may only look"
            }
        ));
        Ok(access)
    }
}

/// Is there a newer Keevoro than `current` with an installer for `os`?
pub(crate) fn offer(web: &dyn Web, current: &str, os: &str) -> Option<Offer> {
    let release = match keevoro_update::check(web, current) {
        Ok(Check::Available(release)) => release,
        Ok(Check::UpToDate) | Err(_) => return None,
    };
    let installer = keevoro_update::installer_for(os)?;
    release.assets.iter().any(|a| a.name == installer).then_some(Offer {
        version: release.version,
        page: release.page,
    })
}

impl Backup {
    /// Opens the backup in `home` only if its record is there already.
    pub(crate) fn open_existing(home: &Path) -> Option<Backup> {
        crate::backup::existing_catalog(home).map(|catalog| Backup::with_catalog(home, catalog))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use keevoro_engine::connect::testing::MemorySecrets;
    use keevoro_update::UpdateError;
    use std::sync::Mutex;

    /// A keychain in memory that can be shared between threads.
    #[derive(Default)]
    pub(crate) struct SharedSecrets(pub Mutex<MemorySecrets>);

    impl SecretStore for SharedSecrets {
        fn get(&self, name: &str) -> anyhow::Result<Option<String>> {
            self.0.lock().unwrap().get(name)
        }
        fn set(&self, name: &str, value: &str) -> anyhow::Result<()> {
            self.0.lock().unwrap().set(name, value)
        }
        fn delete(&self, name: &str) -> anyhow::Result<()> {
            self.0.lock().unwrap().delete(name)
        }
    }

    /// GitHub, answering with one release.
    pub(crate) struct FakeGitHub(pub Option<&'static str>);

    impl Web for FakeGitHub {
        fn text(&self, _url: &str) -> std::result::Result<String, UpdateError> {
            self.0
                .map(str::to_string)
                .ok_or_else(|| UpdateError::Network("offline".into()))
        }
        fn bytes(&self, _url: &str) -> std::result::Result<Vec<u8>, UpdateError> {
            Err(UpdateError::Network("not in a test".into()))
        }
    }

    pub(crate) fn folders(dir: &Path) -> Folders {
        Folders::under(dir)
    }

    #[test]
    fn keevoros_folders_on_this_computer_are_its_own_and_hold_no_photos() {
        // Wherever the computer keeps such things, each is named for
        // Keevoro, and none is inside another's.
        if let Some(folders) = Folders::for_this_computer() {
            for folder in [&folders.config, &folders.data, &folders.cache, &folders.logs] {
                assert!(
                    folder.components().any(|c| c.as_os_str() == IDENTIFIER),
                    "{}",
                    folder.display()
                );
            }
            assert!(folders.cache.ends_with("previews"));
        }
        let dir = Path::new("try");
        assert_eq!(Folders::under(dir).data, dir.join("data"));
    }

    /// Keevoro in a temporary folder, with a keychain in memory and no
    /// network.
    pub(crate) fn keevoro(dir: &Path) -> Keevoro {
        Keevoro::open(&folders(dir))
            .with_secrets(SharedSecrets::default())
            .with_web(FakeGitHub(None))
    }

    #[test]
    fn backups_are_started_renamed_and_forgotten_with_a_line_in_the_log_each() {
        let dir = tempfile::tempdir().unwrap();
        let keevoro = keevoro(dir.path());
        let drone = keevoro.start_backup("Drone footage").unwrap();
        assert!(drone.home.starts_with(dir.path().join("data").join("backups")));
        keevoro.start_backup("Family").unwrap();
        let renamed = keevoro.rename_backup("drone footage", "Drone").unwrap();
        assert_eq!(renamed.id, drone.id);
        assert!(keevoro
            .switch_to(&keevoro.backups().find("Family").unwrap().id)
            .unwrap());
        assert!(!keevoro.switch_to("nonsense").unwrap());
        let gone = keevoro.forget_backup("Drone").unwrap();
        assert!(gone.home.is_dir(), "its folder is left where it is");

        let log = keevoro.log_tail(10);
        for line in [
            "Started backup \"Drone footage\"",
            "Started backup \"Family\"",
            "Renamed backup \"Drone footage\" to \"Drone\"",
            "Switched to backup \"Family\"",
            "Forgot backup \"Drone\". Its folder was left where it is",
        ] {
            assert!(log.contains(line), "{line} is missing from:\n{log}");
        }
        assert_eq!(log.matches("Switched").count(), 1);
    }

    #[test]
    fn the_list_of_backups_says_what_each_holds_without_making_folders() {
        let dir = tempfile::tempdir().unwrap();
        let keevoro = keevoro(dir.path());
        let drone = keevoro.start_backup("Drone").unwrap();
        let unplugged = keevoro.start_backup("Unplugged").unwrap();
        keevoro.switch_to(&unplugged.id).unwrap();
        // Its home folder is on a drive that is not plugged in.
        std::fs::remove_dir_all(&unplugged.home).unwrap();

        let list = keevoro.list_backups();
        assert_eq!(list.len(), 2);
        assert_eq!((list[0].name.as_str(), list[0].open), ("Drone", false));
        assert_eq!(list[0].home, drone.home.display().to_string());
        let counts = list[0].found.clone().unwrap();
        assert_eq!((counts.sources, counts.photos, counts.waiting), (0, 0, 0));
        assert!(list[1].open);
        assert_eq!(list[1].found, None, "a backup that is not there has no counts");
        assert!(!unplugged.home.exists(), "and listing it does not make its folder");
    }

    #[test]
    fn two_keevoros_on_one_folder_see_the_same_backups_as_they_change() {
        let dir = tempfile::tempdir().unwrap();
        let app = keevoro(dir.path());
        let command_line = keevoro(dir.path());
        let drone = app.start_backup("Drone footage").unwrap();
        // Started after the command line opened, and seen at once.
        assert_eq!(command_line.list_backups()[0].id, drone.id);
        assert_eq!(
            command_line.backups().named_or_open(Some("drone footage")).unwrap(),
            drone
        );
        command_line.start_backup("Family").unwrap();
        command_line.rename_backup("Drone footage", "Drone").unwrap();
        let names: Vec<String> = app.list_backups().into_iter().map(|b| b.name).collect();
        assert_eq!(names, ["Drone", "Family"]);
        app.set_own_codecs(false).unwrap();
        assert!(command_line.backups().read().plain_decoding);
        app.set_own_codecs(true).unwrap();
    }

    #[test]
    fn the_codecs_switch_is_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let keevoro = keevoro(dir.path());
        let view = keevoro.set_own_codecs(false).unwrap();
        assert!(!view.own_codecs.on);
        assert!(!view.own_codecs.ways.is_empty());
        assert!(keevoro.backups().read().plain_decoding);
        assert!(keevoro
            .log_tail(5)
            .ends_with("Switched to the built-in picture decoder only"));
        let view = keevoro.set_own_codecs(true).unwrap();
        assert!(view.own_codecs.on);
        assert!(!keevoro.backups().read().plain_decoding);
        assert!((1..=4).contains(&view.own_codecs.cores));
        // What the app has always received.
        let json = serde_json::to_value(&view.own_codecs).unwrap();
        for field in ["on", "available", "ways", "cores"] {
            assert!(json.get(field).is_some(), "{field}");
        }
    }

    #[test]
    fn a_reset_is_logged_and_starts_from_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let keevoro = keevoro(dir.path());
        let home = keevoro.start_backup("Drone").unwrap().home;
        let done = keevoro.reset().unwrap();
        assert_eq!(done.removed, ["Drone"]);
        assert!(!home.exists());
        assert!(keevoro.backups().all().is_empty());
        assert!(keevoro
            .log_tail(5)
            .contains("Reset to how Keevoro was when first installed: 1 backups removed"));
        assert_eq!(
            serde_json::to_value(&done).unwrap(),
            serde_json::json!({ "removed": ["Drone"], "leftAlone": [] })
        );
    }

    #[test]
    fn an_update_is_offered_only_when_there_is_a_newer_one_for_this_computer() {
        const NEWER: &str = r#"{"tag_name":"v99.0.0","html_url":"https://github.com/markcastle/keevoro/releases/tag/v99.0.0",
            "assets":[{"name":"Keevoro-windows-x64-setup.exe","url":"u"}]}"#;
        assert_eq!(
            offer(&FakeGitHub(Some(NEWER)), "0.29.2", "windows"),
            Some(Offer {
                version: "99.0.0".into(),
                page: "https://github.com/markcastle/keevoro/releases/tag/v99.0.0".into()
            })
        );
        assert_eq!(
            offer(&FakeGitHub(Some(NEWER)), "0.29.2", "macos"),
            None,
            "no installer for a Mac"
        );
        assert_eq!(offer(&FakeGitHub(Some(NEWER)), "0.29.2", "linux"), None);
        assert_eq!(offer(&FakeGitHub(Some(NEWER)), "99.0.0", "windows"), None, "up to date");
        assert_eq!(
            offer(&FakeGitHub(None), "0.29.2", "windows"),
            None,
            "offline is no offer"
        );
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(keevoro(dir.path()).update_offer(), None);
    }

    #[test]
    fn agents_are_allowed_what_a_person_says_and_it_is_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let keevoro = keevoro(dir.path());
        assert_eq!(keevoro.agent_access(), AgentAccess::default());
        let photos = dir.path().join("Photos");
        let access = keevoro
            .set_agent_access(AgentsAccessRequest {
                look_in: Some(vec![photos.display().to_string()]),
                ..AgentsAccessRequest::default()
            })
            .unwrap();
        assert!(access.may_look_in(&photos.join("2004")));
        assert!(!access.may_back_up_to(&photos));
        assert_eq!(super::tests::keevoro(dir.path()).agent_access(), access, "remembered");
        assert!(keevoro
            .log_tail(1)
            .ends_with("Agents may now look in 1 folders and back up to 0, and may only look"));
    }
}
