//! Jobs: the long things Keevoro does, which are started, watched, paused
//! and stopped rather than waited on.
//!
//! Looking through a source, surveying a drive, backing up and checking
//! can each take from seconds to a day. Starting one answers at once with
//! a [`JobHandle`]: which job it is, what it is doing, on which backup,
//! when it began and who started it. The job runs on its own thread, and
//! as it goes it says things ([`Said`]) in the same words the app has
//! always shown.
//!
//! Anyone can hear them. A listener given when a job is started hears
//! that job; one given to [`Keevoro::listen`] hears every job, including
//! the background check for duplicates that Keevoro starts by itself; and
//! [`Keevoro::follow`] attaches one to a job already running. Someone who
//! was not listening asks [`Keevoro::jobs`] instead, which says what each
//! running job is doing now.
//!
//! The rules about what may run together are the app's rules, kept
//! exactly, and kept here so the command line and an agent are held to
//! them too:
//!
//! - one search of a source at a time on a backup, and one backup or check
//!   of one at a time, each refused with a plain sentence;
//! - a search and a backup may run side by side, as they always could;
//! - the background check for duplicates runs one at a time, gives way to
//!   a search and to a backup, and starts again when they end;
//! - stopping a job lets it finish the file in hand. A stopped search keeps
//!   nothing, a stopped backup keeps what it sent;
//! - pausing makes every heavy job wait between one file and the next.
//!
//! Jobs are shared between programs: the app, the command line and an
//! agent's tools may each be running, and each sees and steers the jobs
//! of the others (`across`). Only one program at a time does long work on
//! a backup: a search, a backup or a check in one program refuses those
//! in another, naming what is running and where. Within one program the
//! rules above hold as they always have, so a search and a backup still
//! run side by side, but not split across two programs: the backup's
//! status has one writer and its requests one reader, and asking the
//! person to wait a moment is a small price for that. `busy`, `jobs`,
//! `follow`, `wait`, `stop`, `pause` and `carry_on` answer and act the
//! same whichever program runs the job; a job in another program is
//! marked `elsewhere` in its status.

pub(crate) mod across;
mod backing_up;
mod duplicates;
mod looking;
pub mod own;
pub mod said;
mod surveying;
#[cfg(test)]
pub(crate) mod tests;

use crate::backups::Named;
use crate::error::{Error, Result};
use crate::keevoro::Keevoro;
use crate::log::Log;
use crate::register::Op;
use across::{Across, Ask, NotTaken, Work};
use keevoro_core::pause::PausedBy;
use keevoro_engine::connect::SecretStore;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError, Weak};
use std::time::{Duration, Instant};

pub use said::{CheckEvent, HintView, Outcome, Said, ScanEvent, SendEvent, SurveyEvent, SurveyView, TopView};

/// How often a running job says how it is getting on. Often enough to feel
/// alive, not so often that drawing it slows the job down. What it last
/// said is always in [`Keevoro::jobs`], however recent.
pub const PROGRESS_EVERY: Duration = Duration::from_millis(150);

/// How many finished jobs' last words are kept, for anyone who asks after
/// a job once it has ended.
const ENDED_KEPT: usize = 32;

/// Which front end a Keevoro belongs to, so a job can say who started it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FrontEnd {
    /// The desktop app.
    #[default]
    App,
    /// The `keevoro` command.
    CommandLine,
    /// An AI agent, through its tools.
    Agent,
}

impl FrontEnd {
    /// Who it is, in a few words, for the log.
    pub fn name(self) -> &'static str {
        match self {
            FrontEnd::App => "the Keevoro app",
            FrontEnd::CommandLine => "the command line",
            FrontEnd::Agent => "an AI agent",
        }
    }

    /// Where a job it runs is, to end a sentence such as "Keevoro is
    /// already backing up "Family"".
    pub fn words(self) -> &'static str {
        match self {
            FrontEnd::App => "in the Keevoro app",
            FrontEnd::CommandLine => "from the command line",
            FrontEnd::Agent => "for an AI agent",
        }
    }
}

/// What kind of job it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobKind {
    /// `places.survey`
    Survey,
    /// `sources.look`
    Look,
    /// `sources.look_again`
    LookAgain,
    /// `backup.run`
    BackUp,
    /// `backup.check`
    Check,
    /// Reading in full, in the background, the files a quick search only
    /// sampled, so that true duplicates are known. Keevoro starts it
    /// itself after a search or a backup.
    Duplicates,
}

/// The two things only one of which may run on a backup at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Looking,
    BackingUp,
}

impl Slot {
    fn refusal(self) -> &'static str {
        match self {
            Slot::Looking => "Keevoro is already looking through a source. Let that finish first.",
            Slot::BackingUp => "Keevoro is already working on your backup. Let that finish first.",
        }
    }
}

impl JobKind {
    /// The register's operation that starts it, if there is one.
    pub fn op(self) -> Option<Op> {
        match self {
            JobKind::Survey => Some(Op::PlacesSurvey),
            JobKind::Look => Some(Op::SourcesLook),
            JobKind::LookAgain => Some(Op::SourcesLookAgain),
            JobKind::BackUp => Some(Op::BackupRun),
            JobKind::Check => Some(Op::BackupCheck),
            JobKind::Duplicates => None,
        }
    }

    fn slot(self) -> Option<Slot> {
        match self {
            JobKind::Look | JobKind::LookAgain => Some(Slot::Looking),
            JobKind::BackUp | JobKind::Check => Some(Slot::BackingUp),
            JobKind::Survey | JobKind::Duplicates => None,
        }
    }

    /// True for the jobs that wait while Keevoro is paused, and so must be
    /// woken to see that they have been asked to stop.
    fn waits_while_paused(self) -> bool {
        matches!(
            self,
            JobKind::Look | JobKind::LookAgain | JobKind::BackUp | JobKind::Duplicates
        )
    }

    /// What asking it to stop writes in the log, if anything.
    fn asked_to_stop(self) -> Option<&'static str> {
        match self {
            JobKind::Look | JobKind::LookAgain => Some("Asked to stop the search"),
            JobKind::BackUp => Some("Asked to stop sending after the file in hand"),
            JobKind::Check => Some("Asked to stop checking after the destination in hand"),
            JobKind::Survey | JobKind::Duplicates => None,
        }
    }
}

/// A job's number, for asking after it and stopping it. Never reused while
/// Keevoro is open, and not used by another program running at the same
/// time: each program's numbers begin at its process number times a
/// million, which also keeps them small enough for JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(transparent)]
pub struct JobId(pub u64);

impl JobId {
    /// The next number for a job started in this program.
    fn next() -> Self {
        static NEXT: OnceLock<AtomicU64> = OnceLock::new();
        let next = NEXT.get_or_init(|| AtomicU64::new(u64::from(std::process::id()) * 1_000_000 + 1));
        JobId(next.fetch_add(1, Ordering::SeqCst))
    }
}

/// The backup a job works on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobBackup {
    pub id: String,
    pub name: String,
}

/// What starting a job answers, at once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobHandle {
    pub id: JobId,
    pub job: JobKind,
    /// The register's name for it, such as `backup.run`, if it has one.
    pub op: Option<&'static str>,
    /// The backup it works on. A survey may have none.
    pub backup: Option<JobBackup>,
    /// Seconds since 1970.
    pub started_at: i64,
    pub started_by: FrontEnd,
}

/// A [`JobHandle`] as another program wrote it down.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HandleRead {
    id: JobId,
    job: JobKind,
    op: Option<String>,
    backup: Option<JobBackup>,
    started_at: i64,
    started_by: FrontEnd,
}

impl<'de> Deserialize<'de> for JobHandle {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let read = HandleRead::deserialize(d)?;
        Ok(Self {
            id: read.id,
            job: read.job,
            // The register's own name, or none if this program does not
            // know the operation.
            op: read.op.and_then(|name| Op::named(&name)).map(Op::name),
            backup: read.backup,
            started_at: read.started_at,
            started_by: read.started_by,
        })
    }
}

/// Numbers a job has to show, where it has them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub photos: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub videos: Option<u64>,
    /// Files looked at so far.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folders: Option<u64>,
    /// So many done of so many.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
}

/// `jobs.status`: how one running job is getting on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobStatus {
    #[serde(flatten)]
    pub handle: JobHandle,
    /// The step it is on, in a few words, such as "Working out where each
    /// file goes: 2,500 of 11,245".
    pub step: String,
    /// The last line it said, in full. For a backup, the file in hand
    /// follows the step after a tab.
    pub line: String,
    pub counts: Counts,
    /// Seconds since it started, by the clock: paused time included (a
    /// backup's estimate says how much of it was paused).
    #[serde(default)]
    pub running_seconds: u64,
    /// For a backup: how long until the whole of it is done, every stage
    /// it has and the one it is on, worked out by one estimator
    /// (`keevoro_core::estimate`). Its `headline` says how long it has run
    /// and how long is left, in the words every front end shows.
    #[serde(default)]
    pub estimate: Option<keevoro_core::estimate::Estimate>,
    /// True when what is arriving goes over the network, to cloud storage.
    pub over_network: bool,
    /// True once it has been asked to stop and is finishing what is in hand.
    pub stopping: bool,
    /// True while Keevoro is paused.
    pub paused: bool,
    /// The last thing it said, in the app's own words.
    pub last: Option<Said>,
    /// True when another program runs it, and this is what that program
    /// last shared about it (a few times a second).
    #[serde(default)]
    pub elsewhere: bool,
    /// True once it has ended: `last` is then its last word, such as a
    /// survey's findings or how a backup went.
    #[serde(default)]
    pub ended: bool,
}

/// One thing a job said, with which job said it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub job: JobId,
    /// What sort of thing was said, which is also the name of the window
    /// event the app sends it as: `scan`, `survey`, `surveyed`, `send` or
    /// `check`.
    pub event: &'static str,
    pub said: Said,
}

/// Something that hears what jobs say. It is called on the job's own
/// thread, so it should be quick: hand the event on and return.
pub type Listener = Arc<dyn Fn(&Event) + Send + Sync>;

/// Which jobs to stop.
#[derive(Debug, Clone, Copy)]
pub enum Which<'a> {
    /// That one.
    Job(JobId),
    /// Every running job of these kinds.
    Kinds(&'a [JobKind]),
    /// Every running job on the backup with this id.
    OnBackup(&'a str),
    /// Every running job of these kinds on the backup with this id.
    KindsOn(&'a [JobKind], &'a str),
    /// Every running job.
    All,
}

impl Which<'_> {
    fn includes(&self, id: JobId, kind: JobKind, backup: Option<&str>) -> bool {
        match *self {
            Which::Job(wanted) => id == wanted,
            Which::Kinds(kinds) => kinds.contains(&kind),
            Which::OnBackup(on) => backup == Some(on),
            Which::KindsOn(kinds, on) => kinds.contains(&kind) && backup == Some(on),
            Which::All => true,
        }
    }
}

/// What a job is doing now.
#[derive(Default)]
struct Now {
    line: String,
    last: Option<Said>,
    /// For a backup: its estimate of the time left.
    tracker: Option<keevoro_core::estimate::Tracker>,
}

/// One running job.
pub(crate) struct Job {
    handle: JobHandle,
    stop: Arc<AtomicBool>,
    /// True while it holds its backup's slot. It lets go just before it
    /// says its last word, so the next one can start the moment it is
    /// heard.
    holding: AtomicBool,
    /// True once its work is done and it is only saying how it went.
    finishing: AtomicBool,
    listeners: Mutex<Vec<Listener>>,
    now: Mutex<Now>,
    /// How many times it has told its listeners something, so a program
    /// following it from outside can tell when there is something new.
    told: AtomicU64,
    /// True once it has ended and is only letting go of its backup. It is
    /// still registered until it has, so nobody waiting for it hears that
    /// it ended while other programs still see the backup as busy.
    gone: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Job {
    pub(crate) fn id(&self) -> JobId {
        self.handle.id
    }

    pub(crate) fn kind(&self) -> JobKind {
        self.handle.job
    }

    /// The switch that stops it, for the engine to look at between files.
    pub(crate) fn stop_switch(&self) -> Arc<AtomicBool> {
        self.stop.clone()
    }

    pub(crate) fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    pub(crate) fn note_line(&self, line: &str) {
        line.clone_into(&mut lock(&self.now).line);
    }

    pub(crate) fn track(&self, tracker: keevoro_core::estimate::Tracker) {
        lock(&self.now).tracker = Some(tracker);
    }

    fn on_backup(&self, id: &str) -> bool {
        self.backup_id() == Some(id)
    }

    fn backup_id(&self) -> Option<&str> {
        self.handle.backup.as_ref().map(|b| b.id.as_str())
    }

    /// True for a search, a backup or a check: the jobs only one program
    /// at a time may run on a backup.
    fn needs_the_backup(&self) -> bool {
        self.kind().slot().is_some() && self.handle.backup.is_some()
    }
}

/// When a job is said to be due to report again.
pub(crate) struct Every {
    every: Duration,
    last: Option<Instant>,
}

impl Every {
    /// Due at once.
    pub(crate) fn now(every: Duration) -> Self {
        Self { every, last: None }
    }

    /// Due once `every` has passed from now.
    pub(crate) fn after(every: Duration) -> Self {
        Self {
            every,
            last: Some(Instant::now()),
        }
    }

    pub(crate) fn due(&mut self) -> bool {
        self.due_or(false)
    }

    /// Due, or made due by something worth saying at once.
    pub(crate) fn due_or(&mut self, now: bool) -> bool {
        let due = now || self.last.is_none_or(|last| last.elapsed() >= self.every);
        if due {
            self.last = Some(Instant::now());
        }
        due
    }
}

/// A job that has ended: which, on which backup, and its last word.
struct Ended {
    job: JobId,
    backup: Option<String>,
    last: Option<Said>,
    /// How it stood when it ended, with its last word.
    status: JobStatus,
}

#[derive(Default)]
struct Registry {
    running: Vec<Arc<Job>>,
    /// The last words of jobs that have ended, newest last.
    ended: VecDeque<Ended>,
}

/// Every job Keevoro has running, and everyone listening to them.
pub(crate) struct Jobs {
    registry: Mutex<Registry>,
    changed: Condvar,
    everyone: Mutex<Vec<Listener>>,
    every_ms: AtomicU64,
    by: Mutex<FrontEnd>,
    /// Starting a job is one thing at a time, from the rules about what may
    /// run together to taking the backup's lock.
    starting: Mutex<()>,
    /// Sharing jobs with other programs, once Keevoro knows its folders.
    across: OnceLock<Arc<Across>>,
    log: OnceLock<Arc<Log>>,
}

impl Default for Jobs {
    fn default() -> Self {
        Self {
            registry: Mutex::default(),
            changed: Condvar::new(),
            everyone: Mutex::default(),
            every_ms: AtomicU64::new(u64::try_from(PROGRESS_EVERY.as_millis()).unwrap_or(150)),
            by: Mutex::default(),
            starting: Mutex::default(),
            across: OnceLock::new(),
            log: OnceLock::new(),
        }
    }
}

impl Jobs {
    /// Shares this program's jobs with others through `across`, and says
    /// what is asked of them in `log`.
    pub(crate) fn share(&self, across: Across, log: Arc<Log>) {
        let _ = self.across.set(Arc::new(across));
        let _ = self.log.set(log);
    }

    fn across(&self) -> Option<&Arc<Across>> {
        self.across.get()
    }

    fn by(&self) -> FrontEnd {
        *lock(&self.by)
    }

    pub(crate) fn set_front_end(&self, by: FrontEnd) {
        *lock(&self.by) = by;
    }

    pub(crate) fn set_progress_every(&self, every: Duration) {
        self.every_ms
            .store(u64::try_from(every.as_millis()).unwrap_or(u64::MAX), Ordering::SeqCst);
    }

    pub(crate) fn progress_every(&self) -> Duration {
        Duration::from_millis(self.every_ms.load(Ordering::SeqCst))
    }

    /// Registers a new job, unless another already holds the slot it needs
    /// on that backup, here or in another program.
    fn begin(self: &Arc<Self>, kind: JobKind, backup: Option<&Named>, listener: Option<Listener>) -> Result<Arc<Job>> {
        self.begin_unless(kind, backup, listener, false)
    }

    /// As [`Jobs::begin`]; with `only_one`, refused too while another job
    /// of the same kind is running anywhere in this program.
    fn begin_unless(
        self: &Arc<Self>,
        kind: JobKind,
        backup: Option<&Named>,
        listener: Option<Listener>,
        only_one: bool,
    ) -> Result<Arc<Job>> {
        let _starting = lock(&self.starting);
        {
            let registry = lock(&self.registry);
            if only_one
                && registry
                    .running
                    .iter()
                    .any(|job| job.kind() == kind && !job.finishing.load(Ordering::SeqCst))
            {
                return Err(Error::busy("That is running already."));
            }
            if let (Some(slot), Some(backup)) = (kind.slot(), backup) {
                let taken = registry.running.iter().any(|job| {
                    job.holding.load(Ordering::SeqCst) && job.on_backup(&backup.id) && job.kind().slot() == Some(slot)
                });
                if taken {
                    return Err(Error::busy(slot.refusal()));
                }
            }
        }
        if let (Some(backup), Some(across)) = (backup, self.across()) {
            if kind.slot().is_some() {
                let work: Arc<dyn Work> = self.clone();
                let work: Weak<dyn Work> = Arc::downgrade(&work);
                across.take(backup, self.by(), work).map_err(|not| match not {
                    NotTaken::Held(Some(other)) => Error::busy(other.refusal(&backup.name)),
                    NotTaken::Held(None) => Error::busy(format!(
                        "Keevoro is already working on \"{}\" in another program. Let that finish, or stop it, first.",
                        backup.name
                    )),
                    NotTaken::Failed(e) => Error::failed(format!(
                        "Keevoro couldn't note that it is working on \"{}\": {e}.",
                        backup.name
                    )),
                })?;
            } else if kind == JobKind::Duplicates && across.elsewhere(&backup.id).is_some() {
                // Another program is working on it, and checks for
                // duplicates itself when it is done.
                return Err(Error::busy("Another program is working on that backup."));
            }
        }
        let job = Arc::new(Job {
            handle: JobHandle {
                id: JobId::next(),
                job: kind,
                op: kind.op().map(Op::name),
                backup: backup.map(|b| JobBackup {
                    id: b.id.clone(),
                    name: b.name.clone(),
                }),
                started_at: seconds_now(),
                started_by: self.by(),
            },
            stop: Arc::new(AtomicBool::new(false)),
            holding: AtomicBool::new(kind.slot().is_some()),
            finishing: AtomicBool::new(false),
            listeners: Mutex::new(listener.into_iter().collect()),
            now: Mutex::default(),
            told: AtomicU64::new(0),
            gone: AtomicBool::new(false),
        });
        lock(&self.registry).running.push(job.clone());
        self.changed.notify_all();
        self.touched(&job);
        Ok(job)
    }

    /// Registers the background check, unless one is running already, or
    /// another program is working on that backup.
    fn begin_only_one(self: &Arc<Self>, kind: JobKind, backup: &Named) -> Option<Arc<Job>> {
        self.begin_unless(kind, Some(backup), None, true).ok()
    }

    /// Something about a job has changed that another program would want
    /// to know.
    fn touched(&self, job: &Job) {
        if let (Some(across), Some(backup)) = (self.across(), job.backup_id()) {
            across.touched(backup);
        }
    }

    /// Records what a job said and, when `tell` is true, tells everyone
    /// listening. Nothing is held while they are told, so a listener may
    /// ask anything of Keevoro, even to stop the job.
    pub(crate) fn heard(&self, job: &Job, said: Said, tell: bool) {
        if tell {
            let event = Event {
                job: job.id(),
                event: said.name(),
                said: said.clone(),
            };
            lock(&job.now).last = Some(said);
            job.told.fetch_add(1, Ordering::SeqCst);
            self.touched(job);
            let mine: Vec<Listener> = lock(&job.listeners).clone();
            let all: Vec<Listener> = lock(&self.everyone).clone();
            for listener in mine.iter().chain(&all) {
                listener(&event);
            }
        } else {
            lock(&job.now).last = Some(said);
        }
    }

    /// The job's work is done and it lets go of its slot: another may
    /// start in this program, while it says how it went. Other programs
    /// wait until it has ended.
    pub(crate) fn release(&self, job: &Job) {
        job.finishing.store(true, Ordering::SeqCst);
        job.holding.store(false, Ordering::SeqCst);
        self.touched(job);
        self.changed.notify_all();
    }

    /// The job has ended, with these last words. Its last word is
    /// recorded, then its backup let go of (which writes the last status
    /// for other programs), and only then is it no longer running.
    fn end(&self, job: &Job, last: Option<Said>) {
        job.holding.store(false, Ordering::SeqCst);
        job.gone.store(true, Ordering::SeqCst);
        {
            let status = JobStatus {
                ended: true,
                stopping: false,
                paused: false,
                estimate: None,
                last: last.clone(),
                ..Self::status_of(job)
            };
            let mut registry = lock(&self.registry);
            registry.ended.push_back(Ended {
                job: job.id(),
                backup: job.backup_id().map(str::to_string),
                last,
                status,
            });
            while registry.ended.len() > ENDED_KEPT {
                registry.ended.pop_front();
            }
        }
        match (self.across(), job.backup_id()) {
            (Some(across), Some(backup)) if job.needs_the_backup() => across.let_go(backup, self),
            _ => self.touched(job),
        }
        lock(&self.registry).running.retain(|j| j.id() != job.id());
        self.changed.notify_all();
    }

    /// The jobs running, leaving out any that have ended and are only
    /// letting go.
    fn running(&self) -> Vec<Arc<Job>> {
        lock(&self.registry)
            .running
            .iter()
            .filter(|job| !job.gone.load(Ordering::SeqCst))
            .cloned()
            .collect()
    }

    /// True while a search, a backup or a check is running in this
    /// program, on any backup.
    pub(crate) fn busy_here(&self) -> bool {
        self.running().iter().any(|job| job.holding.load(Ordering::SeqCst))
    }

    /// True while a search, a backup or a check is running on any backup,
    /// in this program or another.
    pub(crate) fn busy(&self) -> bool {
        self.busy_here() || self.across().is_some_and(|across| !across.others().is_empty())
    }

    /// True while another program is working on `backup`.
    pub(crate) fn held_elsewhere(&self, backup: &str) -> bool {
        self.across().is_some_and(|across| across.elsewhere(backup).is_some())
    }

    /// What a running backup or check last said, or empty.
    pub(crate) fn backup_step(&self) -> String {
        self.running()
            .iter()
            .find(|job| matches!(job.kind(), JobKind::BackUp | JobKind::Check) && job.holding.load(Ordering::SeqCst))
            .map(|job| lock(&job.now).line.clone())
            .unwrap_or_default()
    }

    fn status_of(job: &Job) -> JobStatus {
        let now = lock(&job.now);
        let mut counts = Counts::default();
        match &now.last {
            Some(Said::Scan(e)) => {
                counts.photos = Some(e.photos);
                counts.videos = Some(e.videos);
                counts.files = Some(e.files);
            }
            Some(Said::Survey(e)) => {
                counts.folders = Some(e.folders as u64);
                counts.files = Some(e.files as u64);
                counts.photos = Some(e.photos as u64);
                counts.videos = Some(e.videos as u64);
            }
            Some(Said::Check(e)) => {
                counts.done = Some(e.done as u64);
                counts.total = Some(e.total as u64);
            }
            Some(Said::Send(_) | Said::Surveyed(_)) | None => {}
        }
        let sending = matches!(job.kind(), JobKind::BackUp | JobKind::Check);
        let (samples, _) = crate::activity::recent();
        let running_seconds = u64::try_from(seconds_now().saturating_sub(job.handle.started_at)).unwrap_or(0);
        let estimate = now.tracker.as_ref().map(|tracker| {
            let mut estimate = tracker.estimate();
            estimate.headline = keevoro_core::estimate::headline(running_seconds, &estimate);
            estimate
        });
        JobStatus {
            handle: job.handle.clone(),
            step: now.line.split('\t').next().unwrap_or_default().to_string(),
            line: now.line.clone(),
            counts,
            running_seconds,
            estimate,
            over_network: sending && samples.iter().rev().take(10).any(|s| s.network > 0.0),
            stopping: job.stopped(),
            paused: keevoro_core::pause::is_paused(),
            last: now.last.clone(),
            elsewhere: false,
            ended: false,
        }
    }

    /// Every job running on the backup with this id, or on any: this
    /// program's, then other programs'.
    fn status(&self, backup: Option<&str>) -> Vec<JobStatus> {
        let mut all: Vec<JobStatus> = self
            .running()
            .iter()
            .filter(|job| backup.is_none_or(|id| job.on_backup(id)))
            .map(|job| Self::status_of(job))
            .collect();
        for other in self.across().map(|a| a.others()).unwrap_or_default() {
            all.extend(
                other
                    .running()
                    .into_iter()
                    .map(|(status, _)| freshened(status))
                    .filter(|status| backup.is_none_or(|id| status.handle.backup.as_ref().is_some_and(|b| b.id == id))),
            );
        }
        all
    }

    /// How a job ended, here or in another program, if it has ended and
    /// is still remembered.
    fn ended_status(&self, id: JobId) -> Option<JobStatus> {
        let here = lock(&self.registry)
            .ended
            .iter()
            .rev()
            .find(|ended| ended.job == id)
            .map(|ended| ended.status.clone());
        here.or_else(|| self.across()?.ended_status(id))
    }

    fn matching(&self, which: Which<'_>) -> Vec<Arc<Job>> {
        self.running()
            .into_iter()
            .filter(|job| which.includes(job.id(), job.kind(), job.backup_id()))
            .collect()
    }

    /// Asks the jobs to stop, here and in other programs, and says so in
    /// the log (another program says so itself).
    pub(crate) fn stop(&self, which: Which<'_>, log: &Log) -> Vec<JobId> {
        let asked = self.matching(which);
        for job in &asked {
            job.stop.store(true, Ordering::SeqCst);
            if let Some(line) = job.kind().asked_to_stop() {
                log.say(line);
            }
        }
        // A paused job is waiting, and must wake to see it has been asked
        // to stop.
        if asked.iter().any(|job| job.kind().waits_while_paused()) {
            keevoro_core::pause::carry_on();
        }
        let mut ids: Vec<JobId> = asked.iter().map(|job| job.id()).collect();
        // One job, found here, is not looked for elsewhere.
        let found_it = matches!(which, Which::Job(_)) && !ids.is_empty();
        if let (Some(across), false) = (self.across(), found_it) {
            for other in across.others() {
                let theirs: Vec<JobId> = other
                    .running()
                    .iter()
                    .map(|(status, _)| &status.handle)
                    .filter(|h| which.includes(h.id, h.job, h.backup.as_ref().map(|b| b.id.as_str())))
                    .map(|h| h.id)
                    .collect();
                if !theirs.is_empty() {
                    across.ask(&other, Ask::Stop, theirs.clone(), self.by());
                    ids.extend(theirs);
                }
            }
        }
        ids
    }

    /// Asks every other program working on a backup to pause, or to carry on.
    fn ask_everyone_else(&self, ask: Ask) {
        if let Some(across) = self.across() {
            for other in across.others() {
                across.ask(&other, ask, Vec::new(), self.by());
            }
        }
    }

    /// The last words of a job, once it has ended. Waits for it if it is
    /// running, here or in another program. `None` for a job never heard
    /// of, one that said nothing, or one whose program stopped without
    /// saying how it ended.
    fn wait(&self, id: JobId) -> Option<Said> {
        let mut registry = lock(&self.registry);
        let here = |r: &Registry| r.running.iter().any(|job| job.id() == id) || r.ended.iter().any(|e| e.job == id);
        if !here(&registry) {
            drop(registry);
            return self.wait_elsewhere(id);
        }
        while registry.running.iter().any(|job| job.id() == id) {
            registry = self.changed.wait(registry).unwrap_or_else(PoisonError::into_inner);
        }
        registry
            .ended
            .iter()
            .rev()
            .find(|ended| ended.job == id)
            .and_then(|ended| ended.last.clone())
    }

    /// Waits for a job another program is running, looking at its status
    /// every so often (see `across::LOOK_EVERY`).
    fn wait_elsewhere(&self, id: JobId) -> Option<Said> {
        let across = self.across()?;
        loop {
            if let Some(last) = across.ended(id) {
                return last;
            }
            let running = across
                .others()
                .iter()
                .any(|other| other.running().iter().any(|(status, _)| status.handle.id == id));
            if !running {
                // It may have ended between the two looks.
                return across.ended(id).flatten();
            }
            across.pause_before_looking_again();
        }
    }

    /// Follows a job another program is running: each time it has said
    /// something new, the listener hears the latest, and at the end its
    /// last word. False if no other program is running it.
    fn follow_elsewhere(&self, id: JobId, listener: Listener) -> bool {
        let Some(across) = self.across().cloned() else {
            return false;
        };
        let find = |across: &Across| {
            across.others().into_iter().find_map(|other| {
                other
                    .running()
                    .into_iter()
                    .find(|(status, _)| status.handle.id == id)
                    .map(|(_, told)| (other.place.backup.clone(), told))
            })
        };
        let Some((backup, mut told)) = find(&across) else {
            return false;
        };
        let event = move |said: Said| Event {
            job: id,
            event: said.name(),
            said,
        };
        std::thread::spawn(move || {
            let mut heard: Option<Said> = None;
            loop {
                across.pause_before_looking_again();
                let now = across
                    .elsewhere(&backup)
                    .and_then(|other| other.running().into_iter().find(|(status, _)| status.handle.id == id));
                match now {
                    Some((status, now_told)) => {
                        if now_told != told {
                            told = now_told;
                            if let Some(said) = status.last {
                                listener(&event(said.clone()));
                                heard = Some(said);
                            }
                        }
                    }
                    None => {
                        if let Some(Some(last)) = across.ended(id) {
                            if heard.as_ref() != Some(&last) {
                                listener(&event(last));
                            }
                        }
                        return;
                    }
                }
            }
        });
        true
    }

    /// Asks every job running in this program to stop, and waits for them
    /// all to end, for at most `at_most`. A job that starts meanwhile, such
    /// as the check for duplicates that follows a search, is asked too.
    /// True if they all ended in time.
    fn stop_here_and_wait(&self, at_most: Duration, log: &Log) -> bool {
        let until = Instant::now() + at_most;
        let mut registry = lock(&self.registry);
        loop {
            let asked: Vec<Arc<Job>> = registry.running.iter().filter(|job| !job.stopped()).cloned().collect();
            if !asked.is_empty() {
                drop(registry);
                for job in &asked {
                    job.stop.store(true, Ordering::SeqCst);
                    if let Some(line) = job.kind().asked_to_stop() {
                        log.say(line);
                    }
                }
                // A paused job must wake to see it has been asked to stop.
                keevoro_core::pause::carry_on();
                registry = lock(&self.registry);
                continue;
            }
            if registry.running.is_empty() {
                return true;
            }
            let Some(left) = until.checked_duration_since(Instant::now()) else {
                return false;
            };
            registry = self
                .changed
                .wait_timeout(registry, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    fn wait_until_idle(&self) {
        let mut registry = lock(&self.registry);
        while !registry.running.is_empty() {
            registry = self.changed.wait(registry).unwrap_or_else(PoisonError::into_inner);
        }
    }
}

impl Work for Jobs {
    fn status(&self, backup: &str, token: &str, beat: i64) -> across::status::Status {
        let (running, ended) = {
            let registry = lock(&self.registry);
            let running: Vec<Arc<Job>> = registry
                .running
                .iter()
                .filter(|j| j.on_backup(backup) && !j.gone.load(Ordering::SeqCst))
                .cloned()
                .collect();
            let mut ended: Vec<across::status::Ended> = registry
                .ended
                .iter()
                .rev()
                .filter(|e| e.backup.as_deref() == Some(backup))
                .take(across::status::ENDED_KEPT)
                .map(|e| across::status::Ended {
                    job: e.job,
                    last: e.last.clone(),
                    status: Some(e.status.clone()),
                })
                .collect();
            ended.reverse();
            (running, ended)
        };
        across::status::Status {
            token: token.to_string(),
            beat,
            paused: keevoro_core::pause::is_paused(),
            running: running
                .iter()
                .map(|job| across::status::Running {
                    status: Self::status_of(job),
                    told: job.told.load(Ordering::SeqCst),
                })
                .collect(),
            ended,
        }
    }

    fn asked(&self, request: &across::requests::Request) {
        let Some(log) = self.log.get() else { return };
        match request.ask {
            Ask::Stop => {
                for id in &request.jobs {
                    self.stop(Which::Job(*id), log);
                }
            }
            Ask::Pause => keevoro_core::pause::pause(PausedBy::AnotherProgram),
            Ask::CarryOn => keevoro_core::pause::carry_on(),
        }
    }

    fn nothing_held(&self) {
        // Another program can no longer see or reach anything here, so a
        // pause it asked for would hold what is left (such as the check for
        // duplicates) where nobody could tell it to carry on.
        if keevoro_core::pause::lift_pause_asked_for_elsewhere() {
            if let Some(log) = self.log.get() {
                log.say("Carrying on: the work another Keevoro program paused here has ended, so its pause is lifted");
            }
        }
    }

    fn lost(&self, backup: &str) {
        let Some(log) = self.log.get() else { return };
        let name = self
            .matching(Which::OnBackup(backup))
            .first()
            .and_then(|job| job.handle.backup.as_ref().map(|b| b.name.clone()))
            .unwrap_or_else(|| backup.to_string());
        log.say(&format!(
            "Another Keevoro program took over \"{name}\", having judged this one to have stopped; stopping work on it here"
        ));
        self.stop(Which::OnBackup(backup), log);
    }
}

/// A drive or folder to look through must be there to be looked through.
/// Another program's status, as it shared it a moment ago, with how long
/// the job has run brought up to now, so every program says the same.
fn freshened(mut status: JobStatus) -> JobStatus {
    status.running_seconds = u64::try_from(seconds_now().saturating_sub(status.handle.started_at)).unwrap_or(0);
    if let Some(estimate) = status.estimate.as_mut() {
        estimate.headline = keevoro_core::estimate::headline(status.running_seconds, estimate);
    }
    status
}

fn must_be_there(place: &Path) -> Result<()> {
    if place.is_dir() {
        Ok(())
    } else {
        Err(Error::not_found(format!(
            "Keevoro can't find the drive or folder {}. Is it plugged in?",
            place.display()
        )))
    }
}

/// Seconds since 1970, as the backup's history records them.
pub(crate) fn seconds_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(0)
}

/// What a job's thread needs of Keevoro. Cheap to copy into a thread.
#[derive(Clone)]
pub(crate) struct Context {
    pub(crate) jobs: Arc<Jobs>,
    pub(crate) log: Arc<Log>,
    pub(crate) secrets: Arc<dyn SecretStore + Send + Sync>,
    /// Where the rates measured in backups are remembered.
    pub(crate) rates: std::path::PathBuf,
    /// Keevoro's cache folder, where surveys are remembered.
    pub(crate) cache: std::path::PathBuf,
}

/// Ends a job however its thread ends, even by a panic, so its slot is
/// never left taken.
pub(crate) struct Ending {
    jobs: Arc<Jobs>,
    job: Arc<Job>,
    done: bool,
}

impl Ending {
    pub(crate) fn new(jobs: &Arc<Jobs>, job: &Arc<Job>) -> Self {
        Self {
            jobs: jobs.clone(),
            job: job.clone(),
            done: false,
        }
    }

    /// The job has said its last word.
    pub(crate) fn with(mut self, last: Option<Said>) {
        self.done = true;
        self.jobs.end(&self.job, last);
    }
}

impl Drop for Ending {
    fn drop(&mut self) {
        if !self.done {
            self.jobs.end(&self.job, None);
        }
    }
}

/// Runs a job's work on its own thread. The work ends the job with its
/// last word; if it cannot, because it panicked, the job is ended without
/// one. If the thread cannot be started at all, the job is ended and that
/// is said.
pub(crate) fn spawn(
    ctx: &Context,
    job: &Arc<Job>,
    work: impl FnOnce(&Context, &Arc<Job>, Ending) + Send + 'static,
) -> Result<()> {
    let ctx = ctx.clone();
    let job = job.clone();
    let ending = Ending::new(&ctx.jobs, &job);
    std::thread::Builder::new()
        .name(format!("keevoro job {}", job.id().0))
        .spawn(move || work(&ctx, &job, ending))
        .map(drop)
        // The ending went with the work that never ran, so the job is not
        // left looking as if it runs.
        .map_err(|e| Error::failed(format!("Keevoro couldn't start that: {e}.")))
}

impl Keevoro {
    fn context(&self) -> Context {
        Context {
            jobs: self.jobs.clone(),
            log: self.log_shared(),
            secrets: self.secrets_shared(),
            rates: self.rates_file(),
            cache: self.cache_folder(),
        }
    }

    /// Hears every job: those started here, and the background check that
    /// Keevoro starts by itself. The app hands each event to its window.
    pub fn listen(&self, listener: impl Fn(&Event) + Send + Sync + 'static) {
        lock(&self.jobs.everyone).push(Arc::new(listener));
    }

    /// `jobs.follow`: hears a job already running, from now until it ends.
    /// A job in another program is heard as often as that program shares
    /// what it is doing (a few times a second): the latest of what it said
    /// each time, and always its last word. False if there is no such job
    /// running anywhere.
    pub fn follow(&self, job: JobId, listener: Listener) -> bool {
        match self.jobs.matching(Which::Job(job)).first() {
            Some(running) => {
                lock(&running.listeners).push(listener);
                true
            }
            None => self.jobs.follow_elsewhere(job, listener),
        }
    }

    /// `places.survey`: finds the folders in a place that hold photos and
    /// videos, reading listings only. `backup`, when given, is used to say
    /// which of them are already its sources; nothing is changed.
    pub fn survey(&self, place: &Path, backup: Option<&Named>, listener: Option<Listener>) -> Result<JobHandle> {
        must_be_there(place)?;
        // A backup for photos only, or videos only, is offered only those.
        let mut kinds = self.kinds_to_look_for();
        let only = backup
            .and_then(|b| crate::backup::Backup::open_existing(&b.home))
            .and_then(|b| b.catalog().wants().ok())
            .and_then(keevoro_core::Wants::only);
        if let Some(kind) = only {
            kinds = kinds.only(kind);
        }
        surveying::start(
            &self.context(),
            place,
            backup,
            kinds,
            self.skip_rules_for(backup),
            listener,
        )
    }

    /// `sources.look`: looks through a drive or folder quickly and adds it
    /// to the backup as a source.
    pub fn look(&self, backup: &Named, place: &Path, listener: Option<Listener>) -> Result<JobHandle> {
        must_be_there(place)?;
        looking::not_left_out(backup, place)?;
        looking::start(
            &self.context(),
            backup,
            vec![place.to_path_buf()],
            JobKind::Look,
            self.kinds_to_look_for(),
            self.skip_rules_for(Some(backup)),
            listener,
        )
    }

    /// `sources.look_again`: looks through one source, or every one in
    /// turn, for what is new.
    pub fn look_again(&self, backup: &Named, source: Option<i64>, listener: Option<Listener>) -> Result<JobHandle> {
        let places = looking::sources_to_look_again(backup, source)?;
        looking::start(
            &self.context(),
            backup,
            places,
            JobKind::LookAgain,
            self.kinds_to_look_for(),
            self.skip_rules_for(Some(backup)),
            listener,
        )
    }

    /// `backup.run`: backs up to every destination, originals first, and
    /// records how it went.
    pub fn run_backup(&self, backup: &Named, listener: Option<Listener>) -> Result<JobHandle> {
        backing_up::start(&self.context(), backup, JobKind::BackUp, listener)
    }

    /// `backup.check`: confirms what is at each destination.
    pub fn check_backup(&self, backup: &Named, listener: Option<Listener>) -> Result<JobHandle> {
        backing_up::start(&self.context(), backup, JobKind::Check, listener)
    }

    /// Carries on, in the background, reading in full the files a quick
    /// search only sampled. Nothing is started if a check is running
    /// already. Keevoro starts this itself after a search and a backup.
    pub fn keep_checking(&self, backup: &Named) -> Option<JobHandle> {
        duplicates::start(&self.context(), backup)
    }

    /// `jobs.status`: every job running on the backup with this id, or on
    /// any backup, in this program and others. Empty when nothing is
    /// running.
    pub fn jobs(&self, backup: Option<&str>) -> Vec<JobStatus> {
        self.jobs.status(backup)
    }

    /// How a job that has ended stood when it ended, with its last word,
    /// whichever program ran it. `None` while it runs, and for a job not
    /// heard of or ended too long ago to be remembered.
    pub fn ended_job(&self, job: JobId) -> Option<JobStatus> {
        self.jobs.ended_status(job)
    }

    /// `jobs.stop`: asks jobs to stop once the file in hand is done,
    /// whichever program runs them. Nothing is lost: a stopped search keeps
    /// nothing of its own, and a stopped backup keeps what it sent. Answers
    /// the jobs asked.
    pub fn stop(&self, which: Which<'_>) -> Vec<JobId> {
        self.jobs.stop(which, self.log())
    }

    /// `jobs.pause`: every heavy job waits once the file in hand is done,
    /// in this program and any other working on a backup.
    pub fn pause(&self) {
        keevoro_core::pause::pause(PausedBy::ThisProgram);
        self.jobs.ask_everyone_else(Ask::Pause);
        self.log().say("Paused: all heavy work is waiting");
    }

    /// `jobs.carry_on`: paused jobs go on from where they were, in this
    /// program and any other working on a backup.
    pub fn carry_on(&self) {
        keevoro_core::pause::carry_on();
        self.jobs.ask_everyone_else(Ask::CarryOn);
        self.log().say("Resumed: work is carrying on from where it paused");
    }

    /// Waits for a job to end, and gives its last word: a survey's
    /// findings, or how a search or a backup ended. A job in another
    /// program is waited for too.
    pub fn wait(&self, job: JobId) -> Option<Said> {
        self.jobs.wait(job)
    }

    /// Waits until no job is running in this program, including the
    /// background check that a search or a backup starts when it ends.
    pub fn wait_for_jobs(&self) {
        self.jobs.wait_until_idle();
    }

    /// For a front end that is closing: asks every job running in this
    /// program to stop once the file in hand is done, and waits for them,
    /// for at most `at_most`. Jobs in Keevoro's other programs are left
    /// alone. Nothing is lost: a stopped search keeps nothing of its own, a
    /// stopped backup keeps what it sent, and each carries on next time.
    /// True if they all ended in time.
    pub fn stop_here_and_wait(&self, at_most: Duration) -> bool {
        self.jobs.stop_here_and_wait(at_most, self.log())
    }

    /// True while a search, a backup or a check is running, in this
    /// program or another.
    pub fn busy(&self) -> bool {
        self.jobs.busy()
    }

    /// True while a search, a backup or a check is running in this
    /// program.
    pub fn busy_here(&self) -> bool {
        self.jobs.busy_here()
    }
}
