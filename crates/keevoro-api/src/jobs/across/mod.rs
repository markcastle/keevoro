//! Jobs across programs: how the app, the command line and an agent share
//! a backup without a background service.
//!
//! Each backup has a folder of its own under `jobs` in Keevoro's data
//! folder, named after the backup's id. It is never in a source and never
//! in a destination, and it holds three things:
//!
//! - `holder.json`, the lock (`lock.rs`): which program is doing long work
//!   on the backup. Only one program at a time may, and a program that
//!   stopped without letting go is noticed and passed over.
//! - `status.json` (`status.rs`): what that program's jobs are doing now,
//!   written by it a few times a second and read by anyone, which is also
//!   its heartbeat.
//! - `requests/` (`requests.rs`): where another program leaves "stop",
//!   "pause" or "carry on" for the holder, which looks a few times a
//!   second.
//!
//! The program holding a lock runs a keeper thread for it, which wakes
//! every [`TICK`] to serve requests, write the status when something has
//! changed, beat the heartbeat, and check the lock is still its own. It
//! sleeps between, so an idle hold costs next to nothing, and it ends with
//! the hold.
//!
//! When the work ends the lock and any requests are removed at once. The
//! status is left behind on purpose, with the last words of the jobs: a
//! program waiting for one of them may not look until after the program
//! that ran it has closed, which the command line does the moment its job
//! ends. Statuses nobody holds are swept away once they are more than
//! [`SWEEP_AFTER`] old, by any Keevoro program as it lets go of a backup
//! or closes, so at most one small file per backup is left for a minute.
//!
//! This module knows files and timing, not jobs: it asks the jobs what to
//! write, and tells them what was asked, through [`Work`].

mod files;
pub(crate) mod lock;
pub(crate) mod requests;
pub(crate) mod status;

use crate::backups::Named;
use crate::jobs::{FrontEnd, JobId, JobKind, JobStatus, Said};
use crate::log::Log;
use lock::{judge, Clock, Holder, Life, Processes, Verdict, STALE_AFTER, WATCH};
pub(crate) use requests::Ask;
use requests::Request;
use status::Status;
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{Duration, Instant};

/// How often a holder's keeper wakes: how quickly a request is noticed,
/// and the most often a status is written.
pub(crate) const TICK: Duration = Duration::from_millis(250);

/// How long the last words of a backup's jobs are kept for anyone waiting
/// on them, once nobody holds it.
pub(crate) const SWEEP_AFTER: Duration = Duration::from_secs(60);

/// How often someone following or waiting for another program's job
/// looks at its status.
pub(crate) const LOOK_EVERY: Duration = Duration::from_millis(100);

fn lock_of<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What the jobs do for the keeper.
pub(crate) trait Work: Send + Sync {
    /// The status to write for a backup, under this token and heartbeat.
    fn status(&self, backup: &str, token: &str, beat: i64) -> Status;
    /// Another program asked for something.
    fn asked(&self, request: &Request);
    /// The lock on a backup was taken by another program, which judged
    /// this one to have stopped: every job on it must stop.
    fn lost(&self, backup: &str);
    /// This program has let go of its last hold, so no other program can
    /// see its work or ask anything of it.
    fn nothing_held(&self);
}

/// One backup's folder of shared files.
#[derive(Debug, Clone)]
pub(crate) struct Place {
    /// The backup's id, as the folder is named.
    pub backup: String,
    dir: PathBuf,
}

/// What reading `holder.json` found.
enum Found {
    Nothing,
    /// Something that is not a lock: a drive that cannot link files makes
    /// one in two steps, so it may be half-made. Its age in milliseconds.
    Garbled(i64),
    Lock(Holder),
}

impl Place {
    fn holder_path(&self) -> PathBuf {
        self.dir.join("holder.json")
    }

    fn status_path(&self) -> PathBuf {
        self.dir.join("status.json")
    }

    fn requests(&self) -> PathBuf {
        self.dir.join("requests")
    }

    fn read_holder(&self, now: i64) -> io::Result<Found> {
        let path = self.holder_path();
        let Some(bytes) = files::read(&path)? else {
            return Ok(Found::Nothing);
        };
        match serde_json::from_slice(&bytes) {
            Ok(holder) => Ok(Found::Lock(holder)),
            Err(_) => {
                let changed = std::fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .and_then(|d| i64::try_from(d.as_millis()).ok())
                    .unwrap_or(now);
                Ok(Found::Garbled(now.saturating_sub(changed)))
            }
        }
    }

    pub(crate) fn read_status(&self) -> Option<Status> {
        files::read(&self.status_path())
            .ok()
            .flatten()
            .and_then(|bytes| Status::from_bytes(&bytes))
    }

    /// The latest heartbeat of this hold: its status's, or when it was
    /// taken if it has not written one yet.
    fn beat_of(&self, holder: &Holder) -> i64 {
        self.read_status()
            .filter(|s| s.token == holder.token)
            .map_or(holder.since, |s| s.beat.max(holder.since))
    }
}

/// A backup another program holds, as found on disk.
#[derive(Debug, Clone)]
pub(crate) struct Other {
    pub place: Place,
    pub holder: Holder,
    /// Its status, if it has written one for this hold.
    pub status: Option<Status>,
}

impl Other {
    /// Its jobs that are running, each marked as running elsewhere.
    pub(crate) fn running(&self) -> Vec<(JobStatus, u64)> {
        self.status
            .iter()
            .flat_map(|s| s.running.iter())
            .map(|r| {
                let mut status = r.status.clone();
                status.elsewhere = true;
                status.paused = self.status.as_ref().is_some_and(|s| s.paused);
                (status, r.told)
            })
            .collect()
    }

    /// What starting a job on this backup is told: what is running, on
    /// which backup, and where.
    pub(crate) fn refusal(&self, backup: &str) -> String {
        let kind = self
            .status
            .iter()
            .flat_map(|s| s.running.iter())
            .map(|r| r.status.handle.job)
            .find(|kind| kind.slot().is_some());
        let what = match kind {
            Some(JobKind::BackUp) => format!("backing up \"{backup}\""),
            Some(JobKind::Check) => format!("checking what has reached the destinations of \"{backup}\""),
            Some(JobKind::Look) => format!("looking through a source for \"{backup}\""),
            Some(JobKind::LookAgain) => format!("looking through the sources of \"{backup}\" again"),
            _ => format!("working on \"{backup}\""),
        };
        format!(
            "Keevoro is already {what} {}. Let that finish, or stop it, first.",
            self.holder.by.words()
        )
    }
}

/// The keeper of one hold: what it needs to know between ticks.
struct Keeper {
    /// Something worth writing has changed since the last status.
    dirty: AtomicBool,
    ending: Mutex<bool>,
    wake: Condvar,
}

/// One backup this program holds.
struct Held {
    token: String,
    /// How many of this program's jobs need the hold. It is let go when
    /// the last one ends.
    jobs: usize,
    keeper: Arc<Keeper>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// This program's side of sharing jobs with other programs.
pub(crate) struct Across {
    root: PathBuf,
    clock: Arc<dyn Clock>,
    processes: Arc<dyn Processes>,
    log: Arc<Log>,
    me: (u32, u64, String),
    /// Every token this Keevoro has held under, to know its own leftovers.
    tokens: Mutex<Vec<String>>,
    held: Mutex<HashMap<String, Held>>,
    /// Taking and letting go are one at a time, so a job starting just as
    /// another on the same backup ends never finds the lock half let go.
    changing: Mutex<()>,
}

/// Why a lock could not be taken.
#[derive(Debug)]
pub(crate) enum NotTaken {
    /// Another program holds it; who, if that could be read.
    Held(Option<Box<Other>>),
    Failed(io::Error),
}

impl From<io::Error> for NotTaken {
    fn from(e: io::Error) -> Self {
        NotTaken::Failed(e)
    }
}

/// A backup id as a folder name: ids are plain already, but nothing from
/// a settings file is trusted to name a folder.
fn folder_name(id: &str) -> String {
    let plain: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if plain.is_empty() || plain.starts_with('.') {
        format!("_{plain}")
    } else {
        plain
    }
}

impl Across {
    pub(crate) fn new(root: PathBuf, log: Arc<Log>, clock: Arc<dyn Clock>, processes: Arc<dyn Processes>) -> Self {
        let (pid, started) = processes.me();
        let host = processes.host();
        Self {
            root,
            clock,
            processes,
            log,
            me: (pid, started, host),
            tokens: Mutex::default(),
            held: Mutex::default(),
            changing: Mutex::default(),
        }
    }

    fn place(&self, backup: &str) -> Place {
        Place {
            backup: backup.to_string(),
            dir: self.root.join(folder_name(backup)),
        }
    }

    /// A token no hold has had before: this process, when it started, the
    /// time, and a count shared by every Keevoro in this process.
    fn new_token(&self) -> String {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let (pid, started, _) = &self.me;
        let token = format!(
            "{pid}-{started}-{}-{}",
            self.clock.now_ms(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        );
        lock_of(&self.tokens).push(token.clone());
        token
    }

    fn is_mine(&self, token: &str) -> bool {
        lock_of(&self.tokens).iter().any(|t| t == token)
    }

    fn life_of(&self, holder: &Holder) -> Life {
        if holder.host == self.me.2 {
            self.processes.life(holder.pid, holder.started)
        } else {
            Life::Unknown
        }
    }

    fn verdict(&self, place: &Place, holder: &Holder) -> Verdict {
        judge(
            holder,
            place.beat_of(holder),
            self.clock.now_ms(),
            &self.me.2,
            self.life_of(holder),
        )
    }

    /// Takes the lock on `backup` for `by`, or counts one more job under
    /// the hold this program already has. On the first, a keeper starts.
    pub(crate) fn take(&self, backup: &Named, by: FrontEnd, work: Weak<dyn Work>) -> Result<(), NotTaken> {
        let _changing = lock_of(&self.changing);
        if let Some(held) = lock_of(&self.held).get_mut(&backup.id) {
            held.jobs += 1;
            return Ok(());
        }
        let place = self.place(&backup.id);
        let token = self.acquire(&place, backup, by)?;
        let keeper = Arc::new(Keeper {
            dirty: AtomicBool::new(true),
            ending: Mutex::new(false),
            wake: Condvar::new(),
        });
        let spawned = {
            let keeper = keeper.clone();
            let place = place.clone();
            let token = token.clone();
            let clock = self.clock.clone();
            std::thread::Builder::new()
                .name(format!("keevoro keeper {}", backup.id))
                .spawn(move || keep(&place, &token, &keeper, &*clock, &work))
        };
        // Without a keeper there is no heartbeat, so the lock is not kept:
        // left, it would shut everyone else out for as long as this
        // program runs.
        let thread = spawned.map_err(|e| {
            let _ = files::remove(&place.holder_path());
            NotTaken::Failed(e)
        })?;
        lock_of(&self.held).insert(
            backup.id.clone(),
            Held {
                token,
                jobs: 1,
                keeper,
                thread: Some(thread),
            },
        );
        Ok(())
    }

    /// Makes the lock file, passing over one left by a program that has
    /// stopped. Answers the token it was made under.
    fn acquire(&self, place: &Place, backup: &Named, by: FrontEnd) -> Result<String, NotTaken> {
        std::fs::create_dir_all(&place.dir)?;
        // A few tries: each lost race means another program took it, or let
        // go of it, in between, and is worth looking at again.
        for _ in 0..4 {
            let (pid, started, host) = &self.me;
            let mine = Holder {
                token: self.new_token(),
                pid: *pid,
                started: *started,
                host: host.clone(),
                by,
                since: self.clock.now_ms(),
            };
            let bytes = serde_json::to_vec(&mine).map_err(io::Error::other)?;
            if files::create_whole(&place.holder_path(), &bytes)? {
                // Requests left for an earlier holder are not for this one.
                let _ = requests::take(&place.requests(), &mine.token);
                return Ok(mine.token);
            }
            match place.read_holder(self.clock.now_ms())? {
                Found::Nothing => {}
                Found::Garbled(age) if age > i64::try_from(STALE_AFTER.as_millis()).unwrap_or(i64::MAX) => {
                    self.pass_over(place, None, backup)?;
                }
                Found::Garbled(_) => return Err(NotTaken::Held(None)),
                Found::Lock(theirs) => match self.verdict(place, &theirs) {
                    Verdict::Held => return Err(NotTaken::Held(Some(Box::new(self.other(place, theirs))))),
                    Verdict::Stale => self.pass_over(place, Some(&theirs), backup)?,
                    Verdict::Doubtful => {
                        // Old by the clock, but perhaps alive: does its
                        // heartbeat move while we watch?
                        let before = place.beat_of(&theirs);
                        self.clock.sleep(WATCH);
                        match place.read_holder(self.clock.now_ms())? {
                            Found::Lock(now) if now.token == theirs.token => {
                                if place.beat_of(&now) != before {
                                    return Err(NotTaken::Held(Some(Box::new(self.other(place, now)))));
                                }
                                self.pass_over(place, Some(&theirs), backup)?;
                            }
                            // It changed hands meanwhile: look again.
                            _ => {}
                        }
                    }
                },
            }
        }
        Err(NotTaken::Held(None))
    }

    /// Removes a stale lock, but only the one judged stale. It is moved
    /// aside in one step, then read: if another program took the lock
    /// between our reading it and moving it, theirs is put back.
    fn pass_over(&self, place: &Place, stale: Option<&Holder>, backup: &Named) -> io::Result<()> {
        let aside = place.dir.join(format!(".holder.json.passed-over.{}", self.new_token()));
        match std::fs::rename(place.holder_path(), &aside) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            other => other?,
        }
        let moved = files::read(&aside)?.unwrap_or_default();
        let moved_token = serde_json::from_slice::<Holder>(&moved).ok().map(|h| h.token);
        let expected = stale.map(|h| h.token.clone());
        if moved_token == expected {
            files::remove(&aside)?;
            match stale {
                Some(stale) => self.log.say(&format!(
                    "\"{}\" was left marked as busy by a program that had stopped (process {}, {}); carrying on",
                    backup.name,
                    stale.pid,
                    stale.by.name()
                )),
                None => self.log.say(&format!(
                    "\"{}\" was left marked as busy by a program that had stopped; carrying on",
                    backup.name
                )),
            }
        } else {
            // Not the one judged stale: put it back. If that fails, the
            // place has been taken again in between, and the holder that
            // lost out notices at its next check and stops.
            let _ = files::create_whole(&place.holder_path(), &moved);
            files::remove(&aside)?;
        }
        Ok(())
    }

    fn other(&self, place: &Place, holder: Holder) -> Other {
        let status = place.read_status().filter(|s| s.token == holder.token);
        Other {
            place: place.clone(),
            holder,
            status,
        }
    }

    /// One fewer job needs the hold on `backup`. When none does, the
    /// keeper stops, the last status is written, and the lock and any
    /// requests are removed.
    pub(crate) fn let_go(&self, backup: &str, work: &dyn Work) {
        let _changing = lock_of(&self.changing);
        let held = {
            let mut all = lock_of(&self.held);
            let Some(held) = all.get_mut(backup) else { return };
            held.jobs = held.jobs.saturating_sub(1);
            if held.jobs > 0 {
                return;
            }
            all.remove(backup)
        };
        let Some(mut held) = held else { return };
        let nothing_left = lock_of(&self.held).is_empty();
        *lock_of(&held.keeper.ending) = true;
        held.keeper.wake.notify_all();
        if let Some(thread) = held.thread.take() {
            let _ = thread.join();
        }
        let place = self.place(backup);
        let last = work.status(backup, &held.token, self.clock.now_ms());
        let _ = files::write_whole(&place.status_path(), &last.to_bytes());
        if let Ok(Found::Lock(holder)) = place.read_holder(self.clock.now_ms()) {
            if holder.token == held.token {
                let _ = files::remove(&place.holder_path());
            }
        }
        let _ = requests::take(&place.requests(), &held.token);
        let _ = std::fs::remove_dir(place.requests());
        // Under `changing`, so no new hold can begin meanwhile.
        if nothing_left {
            work.nothing_held();
        }
        self.sweep();
    }

    /// Removes what is left of holds that have ended: a status nobody holds
    /// that is more than [`SWEEP_AFTER`] old, the backup's empty requests
    /// folder, and the backup's folder once it is empty.
    pub(crate) fn sweep(&self) {
        let now = self.clock.now_ms();
        let after = i64::try_from(SWEEP_AFTER.as_millis()).unwrap_or(i64::MAX);
        for place in self.places() {
            if !matches!(place.read_holder(now), Ok(Found::Nothing)) {
                continue;
            }
            let old = match place.read_status() {
                Some(status) => now.saturating_sub(status.beat) > after,
                // Not a status at all: nothing anyone could be waiting for.
                None => true,
            };
            if old {
                let _ = files::remove(&place.status_path());
            }
            let _ = std::fs::remove_dir(place.requests());
            let _ = std::fs::remove_dir(&place.dir);
        }
    }

    /// Something on `backup` has changed that is worth writing.
    pub(crate) fn touched(&self, backup: &str) {
        if let Some(held) = lock_of(&self.held).get(backup) {
            held.keeper.dirty.store(true, Ordering::SeqCst);
        }
    }

    /// The hold another program has on `backup`, if it is alive.
    pub(crate) fn elsewhere(&self, backup: &str) -> Option<Other> {
        self.live_other(self.place(backup))
    }

    fn live_other(&self, place: Place) -> Option<Other> {
        let Ok(Found::Lock(holder)) = place.read_holder(self.clock.now_ms()) else {
            return None;
        };
        if self.is_mine(&holder.token) || self.verdict(&place, &holder) == Verdict::Stale {
            return None;
        }
        Some(self.other(&place, holder))
    }

    fn places(&self) -> Vec<Place> {
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return Vec::new();
        };
        entries
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .map(|e| Place {
                backup: e.file_name().to_string_lossy().into_owned(),
                dir: e.path(),
            })
            .collect()
    }

    /// Every backup another live program holds.
    pub(crate) fn others(&self) -> Vec<Other> {
        self.places()
            .into_iter()
            .filter_map(|place| self.live_other(place))
            .collect()
    }

    /// The last word of a job another program ran, if a status remembers
    /// it: `Some(None)` for one that ended saying nothing.
    pub(crate) fn ended(&self, job: JobId) -> Option<Option<Said>> {
        self.places().into_iter().find_map(|place| {
            place
                .read_status()
                .filter(|s| !self.is_mine(&s.token))
                .and_then(|s| s.ended.into_iter().find(|e| e.job == job))
                .map(|e| e.last)
        })
    }

    /// How a job another program ran stood when it ended, if that
    /// program shared it and still remembers it.
    pub(crate) fn ended_status(&self, job: JobId) -> Option<JobStatus> {
        self.places().into_iter().find_map(|place| {
            place
                .read_status()
                .filter(|s| !self.is_mine(&s.token))
                .and_then(|s| s.ended.into_iter().find(|e| e.job == job))
                .and_then(|e| e.status)
                .map(|status| JobStatus {
                    elsewhere: true,
                    ..status
                })
        })
    }

    /// Leaves a request for another program.
    pub(crate) fn ask(&self, other: &Other, ask: Ask, jobs: Vec<JobId>, by: FrontEnd) {
        let request = Request {
            to: other.holder.token.clone(),
            ask,
            jobs,
            by,
            at: self.clock.now_ms(),
        };
        if let Err(e) = requests::leave(&other.place.requests(), &request) {
            self.log
                .say(&format!("Couldn't pass a request on to another Keevoro program: {e}"));
        }
    }

    /// Waits a little before looking again at another program's status.
    pub(crate) fn pause_before_looking_again(&self) {
        self.clock.sleep(LOOK_EVERY);
    }
}

impl Drop for Across {
    /// Closing normally tidies away what old holds left behind.
    fn drop(&mut self) {
        self.sweep();
    }
}

/// The keeper's loop: serve requests, write the status when it has changed
/// or a heartbeat is due, and check the lock is still ours. Wakes every
/// [`TICK`]; ends when the hold is let go or lost.
fn keep(place: &Place, token: &str, keeper: &Keeper, clock: &dyn Clock, work: &Weak<dyn Work>) {
    let mut written: Option<Instant> = None;
    let mut checked = Instant::now();
    loop {
        {
            let ending = lock_of(&keeper.ending);
            if *ending {
                return;
            }
            let (ending, _) = keeper
                .wake
                .wait_timeout(ending, TICK)
                .unwrap_or_else(PoisonError::into_inner);
            if *ending {
                return;
            }
        }
        let Some(work) = work.upgrade() else { return };
        for request in requests::take(&place.requests(), token) {
            work.asked(&request);
            // Whoever asked is watching for it to be done: a pause, say.
            keeper.dirty.store(true, Ordering::SeqCst);
        }
        let due = written.is_none_or(|at| at.elapsed() >= lock::BEAT);
        if keeper.dirty.swap(false, Ordering::SeqCst) || due {
            let status = work.status(&place.backup, token, clock.now_ms());
            if files::write_whole(&place.status_path(), &status.to_bytes()).is_ok() {
                written = Some(Instant::now());
            } else {
                // Not written, so still worth writing at the next tick.
                keeper.dirty.store(true, Ordering::SeqCst);
            }
        }
        if checked.elapsed() >= lock::BEAT {
            checked = Instant::now();
            if !still_ours(place, token) {
                work.lost(&place.backup);
                return;
            }
        }
    }
}

/// True if the lock is still under `token`. A lock briefly missing, while
/// another program checks it in passing, is looked at again before giving
/// up on it.
fn still_ours(place: &Place, token: &str) -> bool {
    for attempt in 0..3 {
        if let Ok(Found::Lock(holder)) = place.read_holder(0) {
            return holder.token == token;
        }
        if attempt < 2 {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    false
}

#[cfg(test)]
mod tests;
