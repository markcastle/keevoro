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

fn is_none_of_them(n: &usize) -> bool {
    *n == 0
}

/// Reads one variable of the environment, as storage keys set for scripts
/// are read.
pub type Environment = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

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
    /// Why it cannot be renamed or removed right now, when a search, a
    /// backup or a check is running on it, here or in another program.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub busy: Option<String>,
}

/// The numbers `backups.list` gives for each backup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupCounts {
    pub sources: usize,
    pub destinations: usize,
    pub photos: usize,
    pub videos: usize,
    /// Other files carried along. Never said when there are none.
    #[serde(skip_serializing_if = "is_none_of_them")]
    pub others: usize,
    /// Files being kept that are not yet safely everywhere they should be,
    /// and the room they need.
    pub waiting: usize,
    pub waiting_bytes: u64,
    /// The size of everything it found.
    pub bytes: u64,
    /// Its sources, where each is, and where it backs up to, in words:
    /// what tells two backups apart.
    pub source_places: Vec<String>,
    pub destination_names: Vec<String>,
    /// When it last backed up, in seconds since 1970, if it ever has.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_ran: Option<i64>,
}

/// `backups.start`
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartBackupRequest {
    pub name: String,
    /// What it is for: one or more of `photos`, `videos` and `others`.
    /// Left out, everything Keevoro looks for.
    #[serde(default)]
    pub wants: Option<Vec<crate::wants::WantsWord>>,
    /// How it arranges files where it sends them: `byDate` or `folders`.
    /// Left out, it is asked the first time, and goes by date until then.
    #[serde(default)]
    pub arrange: Option<crate::arrange::ArrangeWord>,
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
    /// Only say what removing it would do, and change nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// `backups.put_back`, `backups.clear_away`
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemovedBackupRequest {
    /// The removed backup's name or id, as `backups.removed` gives it.
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

/// `destinations.tidy`
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TidyRequest {
    /// A backup folder, whether or not a backup still goes there.
    pub place: String,
    /// Only say what would be cleared, and clear nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// What `destinations.tidy` cleared, or, asked first, would clear.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tidied {
    pub folder: String,
    /// True when it was cleared; false when it was only asked what would be.
    pub cleared: bool,
    /// Abandoned working files removed (or that would be), and their size.
    pub files: u64,
    pub bytes: u64,
    /// Left because they were written to in the last hour, so a backup may
    /// be using them right now.
    pub in_use: u64,
    /// Left because Keevoro does not recognise them as its own.
    pub not_ours: u64,
    /// All of that in sentences, the same in the app and at the command line.
    pub said: Vec<String>,
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
    /// Where storage keys set for scripts are read, when none are given.
    environment: Environment,
    web: Box<dyn Web + Send + Sync>,
    /// The jobs running, and everyone listening to them.
    pub(crate) jobs: Arc<Jobs>,
    /// The last few folders looked inside (`places.files`).
    pub(crate) listings: crate::inside::Listings,
    /// Keevoro's own data folder, where the one-at-a-time lock for
    /// backups that run by themselves is kept.
    pub(crate) data: PathBuf,
    /// What the computer says about itself, for a backup that runs by
    /// itself to decide whether to start: the real computer, or a test's.
    pub(crate) signals: Arc<dyn crate::jobs::Signals>,
    /// When Keevoro's own window was last used, in seconds since 1970;
    /// zero when it has not been.
    pub(crate) window_used: Arc<std::sync::atomic::AtomicI64>,
    /// What the scheduler knows of each backup's schedule between looks.
    pub(crate) schedules: Arc<crate::schedule::Known>,
    /// The time and time zone, for schedules: the computer's, or a test's.
    pub(crate) clock: Arc<dyn crate::schedule::Clock>,
    /// When a pause for a while ends by itself, in seconds since 1970;
    /// zero for a pause until someone carries on, or none.
    pub(crate) paused_until: Arc<std::sync::atomic::AtomicI64>,
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
            environment: Arc::new(keevoro_engine::connect::the_environment),
            web: Box::new(GitHub::from_env()),
            jobs,
            listings: crate::inside::Listings::default(),
            data: folders.data.clone(),
            signals: Arc::new(crate::jobs::ThisComputer::default()),
            window_used: Arc::default(),
            schedules: Arc::default(),
            clock: Arc::new(crate::schedule::SystemClock),
            paused_until: Arc::default(),
        };
        keevoro.apply_saved_choices();
        // Pictures kept for quick showing are kept within bounds, quietly,
        // off the thread that opened Keevoro.
        keevoro_engine::look::trim_cache_quietly(&keevoro.cache);
        keevoro
    }

    /// Keeps storage keys somewhere else: for tests, a keychain in memory.
    #[must_use]
    pub fn with_secrets(mut self, secrets: impl SecretStore + Send + Sync + 'static) -> Self {
        self.secrets = Arc::new(secrets);
        self
    }

    /// Reads storage keys set for scripts somewhere else than this
    /// program's environment: for tests, one made up.
    #[must_use]
    pub fn with_environment(mut self, environment: impl Fn(&str) -> Option<String> + Send + Sync + 'static) -> Self {
        self.environment = Arc::new(environment);
        self
    }

    /// Reads what the computer says about itself (battery, how busy it
    /// is) somewhere else: for tests, made up.
    #[must_use]
    pub fn with_signals(mut self, signals: impl crate::jobs::Signals + 'static) -> Self {
        self.signals = Arc::new(signals);
        self
    }

    /// Reads the time and time zone somewhere else, for schedules: for
    /// tests, a clock the test moves.
    #[must_use]
    pub fn with_clock(mut self, clock: impl crate::schedule::Clock + 'static) -> Self {
        self.clock = Arc::new(clock);
        self
    }

    /// The time now, as schedules read it.
    pub fn now(&self) -> jiff::Timestamp {
        self.clock.now()
    }

    /// Notes that the person is using Keevoro's own window now, so a
    /// backup that runs by itself waits until they have finished.
    pub fn note_window_used(&self) {
        self.window_used
            .store(crate::jobs::seconds_now(), std::sync::atomic::Ordering::SeqCst);
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

    /// Where pictures made for showing are kept.
    pub(crate) fn cache_folder(&self) -> PathBuf {
        self.cache.clone()
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

    /// One variable of the environment, as this Keevoro reads it.
    pub(crate) fn environment_variable(&self, name: &str) -> Option<String> {
        (self.environment)(name)
    }

    pub(crate) fn environment_shared(&self) -> Environment {
        self.environment.clone()
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
            .map(|named| self.backup_seen(named, open.as_deref()))
            .collect()
    }

    /// One backup as `backups.list` gives it, for a screen that fills in
    /// each backup as it is counted rather than waiting for them all.
    pub fn backup_listing(&self, id_or_name: &str) -> Result<BackupListing> {
        let named = self.backups.named_or_open(Some(id_or_name))?;
        let open = self.backups.current().map(|b| b.id);
        Ok(self.backup_seen(named, open.as_deref()))
    }

    fn backup_seen(&self, named: Named, open: Option<&str>) -> BackupListing {
        let backup = Backup::open_existing(&named.home);
        let last_ran = backup
            .as_ref()
            .and_then(|b| b.history().ok())
            .and_then(|h| h.first().map(|sent| sent.finished_at));
        BackupListing {
            found: backup.and_then(|backup| backup.discover().ok()).map(|d| BackupCounts {
                sources: d.sources.len(),
                destinations: d.destinations.len(),
                photos: d.photos,
                videos: d.videos,
                others: d.others,
                waiting: d.waiting_to_gather,
                waiting_bytes: d.waiting_bytes,
                bytes: d.bytes,
                source_places: d.sources.iter().map(|s| s.root.clone()).collect(),
                destination_names: d.destinations.iter().map(|d| d.name.clone()).collect(),
                last_ran,
            }),
            busy: self.jobs.working_on(&named),
            open: open == Some(named.id.as_str()),
            home: named.home.display().to_string(),
            id: named.id,
            name: named.name,
        }
    }

    /// `backups.start`
    pub fn start_backup(&self, name: &str) -> Result<Named> {
        let started = self.backups.start(name)?;
        self.log.say(&format!("Started backup \"{}\"", started.name));
        Ok(started)
    }

    /// Is some kind of other file on the list? Only then are other files
    /// offered as a choice, or spoken of.
    pub fn lists_other_files(&self) -> bool {
        self.backups.read().file_kinds.has_other()
    }

    /// `backup.wants`, set, and said in the log.
    pub fn set_backup_wants(&self, backup: &Backup, wants: keevoro_core::Wants) -> Result<crate::wants::WantsView> {
        let others = self.lists_other_files();
        let view = backup.set_wants(wants, others)?;
        self.log.say(&format!(
            "This backup is now for {}. Nothing already backed up was changed",
            wants.words(others)
        ));
        Ok(view)
    }

    /// `backup.arrange`, set, and said in the log.
    pub fn set_arrangement(
        &self,
        backup: &Backup,
        arrange: keevoro_core::Arrange,
    ) -> Result<crate::arrange::ArrangeView> {
        let view = backup.set_arrangement(arrange)?;
        self.log.say(&format!(
            "This backup now arranges new files {}. Nothing already backed up was moved",
            arrange.words()
        ));
        Ok(view)
    }

    /// `sources.arrange`, set, and said in the log.
    pub fn set_source_arrangement(
        &self,
        backup: &Backup,
        source: i64,
        own: Option<keevoro_core::Arrange>,
    ) -> Result<crate::arrange::SourceArrangeView> {
        let view = backup.set_source_arrangement(source, own)?;
        self.log.say(&format!(
            "{} Nothing already backed up was moved",
            view.said.split(" From now on").next().unwrap_or_default()
        ));
        Ok(view)
    }

    /// `sources.wants`, set, and said in the log.
    pub fn set_source_wants(
        &self,
        backup: &Backup,
        source: i64,
        wants: Option<keevoro_core::Wants>,
    ) -> Result<crate::wants::WantsView> {
        let view = backup.set_source_wants(source, wants, self.lists_other_files())?;
        self.log.say(&format!(
            "{} Nothing already backed up was changed",
            view.said
                .split(". Anything")
                .next()
                .unwrap_or_default()
                .trim_end_matches('.')
        ));
        Ok(view)
    }

    /// `backups.rename`. Refused while a search, a backup or a check is
    /// running on it, here or in another program.
    pub fn rename_backup(&self, backup: &str, to: &str) -> Result<Named> {
        let before = self.backups.named_or_open(Some(backup))?;
        if let Some(why) = self.jobs.working_on(&before) {
            return Err(Error::busy(why));
        }
        let renamed = self.backups.rename(&before.id, to)?;
        self.log
            .say(&format!("Renamed backup \"{}\" to \"{}\"", before.name, renamed.name));
        Ok(renamed)
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
    /// When none were given and the environment's were used, the log says
    /// so, naming the variables and never the keys.
    pub fn add_destination(&self, backup: &Backup, request: &AddDestinationRequest) -> Result<DestinationAdded> {
        let added = backup.add_destination(request, self.secrets(), &*self.environment)?;
        if let Some(keys) = &added.keys {
            self.log.say(keys);
        }
        Ok(added)
    }

    /// `sources.add_bucket`, with the keychain and environment, saying in
    /// the log when the environment's keys were used (naming the
    /// variables, never the keys).
    pub fn add_bucket_source(
        &self,
        backup: &Backup,
        request: &crate::bucket_sources::AddBucketRequest,
    ) -> Result<crate::bucket_sources::BucketAdded> {
        let added = backup.add_bucket_source(request, self.secrets(), &*self.environment)?;
        self.log.say(&format!("Added {} as a source", added.place));
        if let Some(keys) = &added.keys {
            self.log.say(keys);
        }
        Ok(added)
    }

    /// `sources.forget`: a bucket source's keys go from the keychain too.
    pub fn forget_source(&self, backup: &mut Backup, source: i64) -> Result<bool> {
        let secret = backup.bucket_secret(source)?;
        let forgotten = backup.forget_source(source)?;
        if let (true, Some(secret)) = (forgotten, secret) {
            self.secrets().delete(&secret)?;
        }
        Ok(forgotten)
    }

    /// The outside world a destination is reached through: the keychain,
    /// and the environment for scripts' keys.
    pub(crate) fn world(&self) -> keevoro_engine::connect::World<'_> {
        keevoro_engine::connect::World {
            secrets: self.secrets(),
            environment: &*self.environment,
        }
    }

    /// `destinations.room`, looking at each destination first unless it
    /// was looked at a moment ago.
    pub fn room(&self, backup: &Backup) -> Result<crate::destinations::Room> {
        backup.room_looking(&self.world())
    }

    /// `backup.rehearse`, looking at each destination first.
    pub fn rehearse(&self, backup: &Backup) -> Result<crate::destinations::Rehearsal> {
        backup.rehearse(&self.world())
    }

    /// `destinations.remove`: one by its place in the list, or every one.
    /// A folder taken off has Keevoro's own abandoned working files cleared
    /// from it, as a backup there would have; nothing backed up is touched.
    pub fn remove_destination(&self, backup: &Backup, at: Option<usize>) -> Result<Vec<String>> {
        let folders: Vec<(String, String)> = backup
            .destinations()?
            .into_iter()
            .filter_map(|d| d.folder.map(|folder| (d.name, folder)))
            .collect();
        let gone = backup.remove_destination(at, self.secrets())?;
        if at.is_some() {
            for name in &gone {
                self.log.say(&format!(
                    "No longer backing up to {name}. Nothing that was backed up there was changed"
                ));
            }
        }
        // Not there (a drive unplugged) or not tidied: no reason not to take it off.
        for (_, folder) in folders.iter().filter(|(name, _)| gone.contains(name)) {
            if Path::new(folder).is_dir() {
                let _ = self.tidy_folder(Path::new(folder), false);
            }
        }
        Ok(gone)
    }

    /// `destinations.tidy`: clears Keevoro's own abandoned working files
    /// (`.keevoro-uploads`) from a backup folder, whether or not any backup
    /// still goes there. Only that working folder is ever touched; what was
    /// backed up, and anything else in the folder, stays as it is.
    pub fn tidy_folder(&self, folder: &Path, dry_run: bool) -> Result<Tidied> {
        if !folder.is_dir() {
            return Err(Error::not_found(format!(
                "There is no folder at {}.",
                keevoro_engine::friendly_path(folder)
            )));
        }
        let place = keevoro_engine::friendly_path(folder);
        let sweep = if dry_run {
            keevoro_engine::working_files_to_clear
        } else {
            keevoro_engine::sweep_working_files
        };
        let swept = sweep(folder, keevoro_engine::ABANDONED_AFTER).map_err(|e| {
            Error::failed(format!(
                "Keevoro couldn't {} its unfinished copies in {place}: {e}.",
                if dry_run { "look at" } else { "clear" }
            ))
        })?;
        let said = crate::words::tidied(&place, &swept, !dry_run);
        if !dry_run && (swept.files > 0 || swept.left() > 0) {
            for line in &said {
                self.log.say(line);
            }
        }
        Ok(Tidied {
            folder: place,
            cleared: !dry_run,
            files: swept.files,
            bytes: swept.bytes,
            in_use: swept.in_use,
            not_ours: swept.not_ours,
            said,
        })
    }

    /// `photos.picture` of any photo or video by its path, as WebP bytes:
    /// any kind on the list, even one left out, since a look inside shows
    /// it to say so. `backup`, when given, may know the file from a search.
    pub fn picture_of_file(&self, file: &Path, size: PictureSize, backup: Option<&crate::Named>) -> Result<Vec<u8>> {
        let kinds = self.kinds_to_look_for();
        let open = backup.and_then(|b| Backup::open_existing(&b.home));
        crate::picture::picture_of_file(&self.cache, file, &kinds, size, open.as_ref())
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
        let (now, zone) = (self.clock.now(), self.clock.zone());
        ActivityView {
            check: self.jobs.check_now(),
            paused_until: self
                .paused_until()
                .map(|until| crate::glance::until_words(until, now, &zone)),
            ..activity::now(Some(&self.backup_step()))
        }
    }

    /// How the check for duplicates stands on a backup when none is
    /// running: how many files it has still to read, and whether a person
    /// stopped it, with what that means.
    pub fn duplicates_left(&self, backup: &Named) -> crate::jobs::DuplicatesLeft {
        Backup::open_existing(&backup.home)
            .map(|b| crate::jobs::DuplicatesLeft::of(b.catalog()))
            .unwrap_or_default()
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
        // What was learnt about this computer's speed goes too, and how
        // the app's window behaves. The app takes away its own entry for
        // starting with the computer.
        let _ = std::fs::remove_file(&self.rates);
        let _ = std::fs::remove_file(self.backups.config_folder().join(crate::window::FILE));
        // The list of reports sent from this computer goes too. The reports
        // themselves were sent, and stay with Keevoro's maker.
        let _ = std::fs::remove_file(self.backups.config_folder().join(crate::reports::FILE));
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

    /// `keevoro.update`: a newer Keevoro that this program could install on
    /// this kind of computer, if there is one: for the app, a newer release
    /// with an installer for it; for the command line (and an agent, which
    /// is served by the command line's program), one with a newer `keevoro`
    /// for it, which is what `keevoro update` installs. Being offline, or
    /// GitHub being unreachable, is not worth bothering anyone about: it
    /// simply means no offer today.
    pub fn update_offer(&self) -> Option<Offer> {
        let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
        offer(
            &*self.web,
            env!("CARGO_PKG_VERSION"),
            file_to_update(self.jobs.by(), os, arch),
        )
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

/// The release file a front end would install on this kind of computer:
/// the app's installer for the app, the `keevoro` program otherwise. The
/// app has no installer for Linux yet, so asking with the app's question
/// there always found nothing newer.
pub(crate) fn file_to_update(by: FrontEnd, os: &str, arch: &str) -> Option<&'static str> {
    match by {
        FrontEnd::App => keevoro_update::installer_for(os),
        FrontEnd::CommandLine | FrontEnd::Agent => keevoro_update::asset_for(os, arch),
    }
}

/// Is there a newer Keevoro than `current` with `file` in its release?
pub(crate) fn offer(web: &dyn Web, current: &str, file: Option<&str>) -> Option<Offer> {
    let release = match keevoro_update::check(web, current) {
        Ok(Check::Available(release)) => release,
        Ok(Check::UpToDate) | Err(_) => return None,
    };
    let file = file?;
    release.assets.iter().any(|a| a.name == file).then_some(Offer {
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

    /// Keevoro in a temporary folder, with a keychain in memory, nothing
    /// set in its environment and no network.
    pub(crate) fn keevoro(dir: &Path) -> Keevoro {
        Keevoro::open(&folders(dir))
            .with_secrets(SharedSecrets::default())
            .with_environment(keevoro_engine::connect::testing::no_environment)
            .with_web(FakeGitHub(None))
    }

    /// Makes a file `age` old, as an earlier backup would have left it.
    fn left_behind(path: &Path, bytes: usize, age: std::time::Duration) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![1u8; bytes]).unwrap();
        let file = std::fs::File::options().write(true).open(path).unwrap();
        file.set_modified(std::time::SystemTime::now() - age).unwrap();
    }

    #[test]
    fn unfinished_copies_are_cleared_from_a_folder_taken_off_a_backup_or_any_folder_asked() {
        use crate::{call, call_for_agent, Op};
        use serde_json::json;
        let dir = tempfile::tempdir().unwrap();
        let k = keevoro(dir.path());
        let named = k.start_backup("Card").unwrap();
        let backup = Backup::open(&named.home).unwrap();
        let usb = dir.path().join("USB");
        k.add_destination(
            &backup,
            &AddDestinationRequest {
                kind: crate::destinations::DestinationKind::Folder,
                place: usb.display().to_string(),
                bucket: None,
                access_key_id: None,
                secret_access_key: None,
            },
        )
        .unwrap();
        let day = std::time::Duration::from_secs(86_400);
        left_behind(&usb.join("originals/IMG_1.jpg"), 10, day);
        left_behind(&usb.join(".keevoro-uploads/tmp-abc"), 5000, day);
        k.remove_destination(&backup, Some(0)).unwrap();
        assert!(!usb.join(".keevoro-uploads").exists(), "cleared when taken off");
        assert!(
            usb.join("originals/IMG_1.jpg").is_file(),
            "nothing backed up is touched"
        );
        let log = k.log_tail(10);
        assert!(
            log.contains("Cleared 4.9 KB of unfinished copies (1 file) from"),
            "{log}"
        );
        assert!(log.contains("Nothing that was backed up there was changed"), "{log}");
        // One whose drive is unplugged is still taken off.
        k.add_destination(
            &backup,
            &AddDestinationRequest {
                kind: crate::destinations::DestinationKind::Folder,
                place: usb.display().to_string(),
                bucket: None,
                access_key_id: None,
                secret_access_key: None,
            },
        )
        .unwrap();
        let unplugged = dir.path().join("USB unplugged");
        std::fs::rename(&usb, &unplugged).unwrap();
        assert_eq!(k.remove_destination(&backup, Some(0)).unwrap().len(), 1);
        std::fs::rename(&unplugged, &usb).unwrap();

        // The same for a folder no backup goes to any more, by name.
        left_behind(&usb.join(".keevoro-uploads/18e8/part-00531"), 7000, day);
        left_behind(&usb.join(".keevoro-uploads/tmp-live"), 10, std::time::Duration::ZERO);
        let tidied = call(
            &k,
            Op::DestinationsTidy,
            None,
            json!({ "place": usb.display().to_string() }),
        )
        .unwrap();
        assert_eq!(
            (
                tidied["files"].as_u64(),
                tidied["bytes"].as_u64(),
                tidied["inUse"].as_u64()
            ),
            (Some(1), Some(7000), Some(1))
        );
        assert!(usb.join(".keevoro-uploads/tmp-live").is_file());
        let e = call(
            &k,
            Op::DestinationsTidy,
            None,
            json!({ "place": dir.path().join("nowhere").display().to_string() }),
        )
        .unwrap_err();
        assert_eq!(e.kind, crate::Kind::NotFound);
        // An AI agent cannot.
        let e = call_for_agent(
            &k,
            Op::DestinationsTidy,
            None,
            json!({ "place": usb.display().to_string() }),
        )
        .unwrap_err();
        assert_eq!(e.kind, crate::Kind::Refused);
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
        let gone = keevoro.forget_backup("Drone", false).unwrap();
        assert!(gone.removed);
        assert!(!drone.home.exists(), "its folder is set aside");

        let log = keevoro.log_tail(10);
        for line in [
            "Started backup \"Drone footage\"",
            "Started backup \"Family\"",
            "Renamed backup \"Drone footage\" to \"Drone\"",
            "Switched to backup \"Family\"",
            "Removed backup \"Drone\". Nothing at its destinations or on its sources was touched",
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
        keevoro.start_backup("Test 2").unwrap();
        keevoro.forget_backup("Test 2", false).unwrap();
        let done = keevoro.reset().unwrap();
        assert_eq!(done.removed, ["Drone"]);
        assert!(!home.exists());
        assert!(keevoro.removed_backups().is_empty(), "nor one removed and waiting");
        assert!(!dir.path().join("data").join("removed").exists());
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
        let app = |os| file_to_update(FrontEnd::App, os, "x86_64");
        assert_eq!(
            offer(&FakeGitHub(Some(NEWER)), "0.29.2", app("windows")),
            Some(Offer {
                version: "99.0.0".into(),
                page: "https://github.com/markcastle/keevoro/releases/tag/v99.0.0".into()
            })
        );
        assert_eq!(
            offer(&FakeGitHub(Some(NEWER)), "0.29.2", app("macos")),
            None,
            "no installer for a Mac"
        );
        assert_eq!(offer(&FakeGitHub(Some(NEWER)), "0.29.2", app("linux")), None);
        assert_eq!(
            offer(&FakeGitHub(Some(NEWER)), "99.0.0", app("windows")),
            None,
            "up to date"
        );
        assert_eq!(
            offer(&FakeGitHub(None), "0.29.2", app("windows")),
            None,
            "offline is no offer"
        );
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(keevoro(dir.path()).update_offer(), None);
    }

    #[test]
    fn the_command_line_is_offered_a_newer_command_line_on_linux_too() {
        const NEWER: &str = r#"{"tag_name":"v99.0.0","html_url":"https://example.org/v99.0.0",
            "assets":[{"name":"Keevoro-windows-x64-setup.exe","url":"u"},
                      {"name":"keevoro-windows-x64.exe","url":"u"},
                      {"name":"keevoro-linux-x64","url":"u"}]}"#;
        const WINDOWS_ONLY: &str = r#"{"tag_name":"v99.0.0","html_url":"https://example.org/v99.0.0",
            "assets":[{"name":"keevoro-windows-x64.exe","url":"u"}]}"#;
        for by in [FrontEnd::CommandLine, FrontEnd::Agent] {
            let linux = file_to_update(by, "linux", "x86_64");
            assert_eq!(linux, Some("keevoro-linux-x64"));
            assert_eq!(
                offer(&FakeGitHub(Some(NEWER)), "0.37.1", linux).map(|o| o.version),
                Some("99.0.0".to_string())
            );
            assert_eq!(
                offer(&FakeGitHub(Some(WINDOWS_ONLY)), "0.37.1", linux),
                None,
                "a release built for Windows only has nothing for Linux"
            );
            let mac = file_to_update(by, "macos", "aarch64");
            assert_eq!(offer(&FakeGitHub(Some(NEWER)), "0.37.1", mac), None);
        }
        assert_eq!(
            file_to_update(FrontEnd::App, "linux", "x86_64"),
            None,
            "no installer yet"
        );
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
