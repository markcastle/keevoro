//! A backup's lock: which program is doing long work on it, and how to
//! tell a program that has stopped from one that is busy.
//!
//! The lock is a small file, `holder.json`, made by the program that takes
//! it and removed when its work ends. It says who made it: the process
//! number, when that process started, the computer's name, the front end,
//! and a token no other holder ever has. While it holds the lock, the
//! program also proves it is alive every few seconds by writing the time
//! into the backup's status (see `status.rs`); that is the heartbeat.
//!
//! A program can stop without removing its lock: it crashed, the computer
//! lost power, someone ended it. Nobody must be shut out of their own
//! backup by that, so a lock is stale, and may be taken, when:
//!
//! - its process is gone, or the number now belongs to a different process
//!   (they are reused, quickly on Windows; the start time tells them
//!   apart), which is known at once on the same computer; or
//! - its heartbeat is more than [`STALE_AFTER`] old by the clock **and**
//!   does not move while it is watched for [`WATCH`]. This catches a
//!   program that is stuck, and a lock made on another computer, where
//!   the process cannot be looked up.
//!
//! The watching is there because clocks jump. A computer waking from sleep,
//! or setting its clock, can make a live heartbeat look an hour old for a
//! moment; one that moves while watched is alive whatever the clock says.
//! A heartbeat that seems to be in the future is taken as fresh, for the
//! same reason. Nothing here compares a time written by one program with
//! a time read by another to decide anything except "is this old enough
//! to be worth watching".

use crate::jobs::FrontEnd;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// How often a program holding a lock proves it is alive.
pub(crate) const BEAT: Duration = Duration::from_secs(2);

/// How old, by the clock, a heartbeat must be before its lock is doubted.
pub(crate) const STALE_AFTER: Duration = Duration::from_secs(60);

/// How long a doubted heartbeat is watched for a sign of life: two beats
/// and a margin.
pub(crate) const WATCH: Duration = Duration::from_secs(5);

/// What `holder.json` says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Holder {
    /// Unique to this hold: no other program, and no later hold by the same
    /// program, has it.
    pub token: String,
    pub pid: u32,
    /// When the process started, in seconds since 1970 as the operating
    /// system tells it, or 0 if it would not say.
    pub started: u64,
    /// The computer's name, since a process can only be looked up on the
    /// computer it runs on.
    pub host: String,
    pub by: FrontEnd,
    /// When the lock was taken, in milliseconds since 1970. Its heartbeat
    /// until the first status is written.
    pub since: i64,
}

/// Whether a process is still the one that took a lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Life {
    Running,
    /// Not running, a zombie, or the number now belongs to another process.
    Gone,
    /// It could not be looked up.
    Unknown,
}

/// The computer's processes, behind a seam so tests can say what is alive.
pub(crate) trait Processes: Send + Sync {
    /// This process: its number and when it started.
    fn me(&self) -> (u32, u64);
    /// This computer's name.
    fn host(&self) -> String;
    /// Whether process `pid`, started at `started`, is still running.
    fn life(&self, pid: u32, started: u64) -> Life;
}

/// The computer's clock and the passing of time, behind a seam so tests
/// can move them.
pub(crate) trait Clock: Send + Sync {
    /// Milliseconds since 1970, by the computer's clock, which may jump.
    fn now_ms(&self) -> i64;
    fn sleep(&self, how_long: Duration);
}

/// The real computer.
pub(crate) struct ThisComputer;

impl Processes for ThisComputer {
    fn me(&self) -> (u32, u64) {
        let pid = std::process::id();
        (pid, started_at(pid).unwrap_or(0))
    }

    fn host(&self) -> String {
        sysinfo::System::host_name().unwrap_or_default()
    }

    fn life(&self, pid: u32, started: u64) -> Life {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System};
        let pid_ = Pid::from_u32(pid);
        let mut system = System::new();
        system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid_]), true, ProcessRefreshKind::nothing());
        match system.process(pid_) {
            None => Life::Gone,
            // Ended, but not yet tidied away by whoever started it.
            Some(process) if process.status() == ProcessStatus::Zombie => Life::Gone,
            // The number has been given to another process since. A second
            // either way allows for how the time is worked out.
            Some(process) if started != 0 && process.start_time().abs_diff(started) > 1 => Life::Gone,
            Some(_) => Life::Running,
        }
    }
}

impl Clock for ThisComputer {
    fn now_ms(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|d| i64::try_from(d.as_millis()).ok())
            .unwrap_or(0)
    }

    fn sleep(&self, how_long: Duration) {
        std::thread::sleep(how_long);
    }
}

/// When process `pid` started, as the operating system tells it.
fn started_at(pid: u32) -> Option<u64> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let pid = Pid::from_u32(pid);
    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, ProcessRefreshKind::nothing());
    system.process(pid).map(sysinfo::Process::start_time)
}

/// What a lock found on disk amounts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Its holder is alive: leave it be.
    Held,
    /// Its holder is gone: it may be taken.
    Stale,
    /// Old by the clock, but its holder may be alive: watch the heartbeat
    /// for [`WATCH`] before deciding.
    Doubtful,
}

/// Judges a lock from what is known now, without waiting. `beat` is its
/// latest heartbeat, `now` the clock, `here` this computer's name and
/// `life` what looking up its process found.
pub(crate) fn judge(holder: &Holder, beat: i64, now: i64, here: &str, life: Life) -> Verdict {
    let same_computer = holder.host == here;
    if same_computer && life == Life::Gone {
        return Verdict::Stale;
    }
    let stale_after = i64::try_from(STALE_AFTER.as_millis()).unwrap_or(i64::MAX);
    // A beat from the future is a clock that has jumped: fresh, not stale.
    if now.saturating_sub(beat) < stale_after {
        Verdict::Held
    } else {
        Verdict::Doubtful
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::sync::Mutex;

    pub(crate) const HERE: &str = "this computer";
    const MINUTE: i64 = 60_000;

    fn holder(host: &str) -> Holder {
        Holder {
            token: "t".into(),
            pid: 42,
            started: 1_700_000_000,
            host: host.into(),
            by: FrontEnd::CommandLine,
            since: 0,
        }
    }

    /// A clock that only moves when told, or when slept on.
    #[derive(Default)]
    pub(crate) struct FakeClock {
        pub now: AtomicI64,
        /// Called whenever someone sleeps, after the clock has moved on:
        /// how a test makes something happen "while watching".
        pub on_sleep: Mutex<Option<Box<dyn FnMut() + Send>>>,
    }

    impl FakeClock {
        /// The clock jumps, or time passes.
        pub fn fetch_add_ms(&self, ms: i64) {
            self.now.fetch_add(ms, Ordering::SeqCst);
        }
    }

    impl Clock for FakeClock {
        fn now_ms(&self) -> i64 {
            self.now.load(Ordering::SeqCst)
        }

        fn sleep(&self, how_long: Duration) {
            let ms = i64::try_from(how_long.as_millis()).unwrap();
            self.now.fetch_add(ms, Ordering::SeqCst);
            if let Some(during) = self.on_sleep.lock().unwrap().as_mut() {
                during();
            }
        }
    }

    /// Processes that are alive because a test says so.
    #[derive(Default)]
    pub(crate) struct FakeProcesses {
        pub alive: Mutex<BTreeMap<u32, u64>>,
    }

    impl Processes for FakeProcesses {
        fn me(&self) -> (u32, u64) {
            (7, 1_700_000_007)
        }

        fn host(&self) -> String {
            HERE.into()
        }

        fn life(&self, pid: u32, started: u64) -> Life {
            match self.alive.lock().unwrap().get(&pid) {
                Some(&when) if when == started => Life::Running,
                _ => Life::Gone,
            }
        }
    }

    #[test]
    fn a_lock_whose_process_has_gone_is_stale_at_once_however_fresh_its_heartbeat() {
        assert_eq!(judge(&holder(HERE), 1_000, 1_000, HERE, Life::Gone), Verdict::Stale);
    }

    #[test]
    fn a_live_holder_with_a_recent_heartbeat_holds_its_lock() {
        let now = 10 * MINUTE;
        assert_eq!(
            judge(&holder(HERE), now - 2_000, now, HERE, Life::Running),
            Verdict::Held
        );
        assert_eq!(
            judge(&holder(HERE), now - MINUTE + 1, now, HERE, Life::Running),
            Verdict::Held,
            "just under a minute"
        );
    }

    #[test]
    fn a_heartbeat_from_the_future_is_a_clock_that_jumped_and_counts_as_fresh() {
        let now = 10 * MINUTE;
        assert_eq!(
            judge(&holder(HERE), now + 60 * MINUTE, now, HERE, Life::Running),
            Verdict::Held
        );
        assert_eq!(
            judge(&holder(HERE), i64::MAX, i64::MIN, HERE, Life::Unknown),
            Verdict::Held
        );
    }

    #[test]
    fn an_old_heartbeat_of_a_live_or_unknown_process_is_only_doubted() {
        let now = 10 * MINUTE;
        for life in [Life::Running, Life::Unknown] {
            assert_eq!(judge(&holder(HERE), now - MINUTE, now, HERE, life), Verdict::Doubtful);
        }
    }

    #[test]
    fn on_another_computer_the_process_cannot_be_trusted_so_only_the_heartbeat_counts() {
        let now = 10 * MINUTE;
        // "Gone" here means nothing: that number is about this computer.
        assert_eq!(judge(&holder("elsewhere"), now, now, HERE, Life::Gone), Verdict::Held);
        assert_eq!(
            judge(&holder("elsewhere"), now - 2 * MINUTE, now, HERE, Life::Gone),
            Verdict::Doubtful
        );
    }

    #[test]
    fn this_process_is_alive_and_a_process_that_ended_is_gone() {
        let computer = ThisComputer;
        let (pid, started) = computer.me();
        assert_eq!(pid, std::process::id());
        assert_eq!(computer.life(pid, started), Life::Running);
        assert_eq!(
            computer.life(pid, started.saturating_sub(3600)),
            Life::Gone,
            "the same number, started at another time, is another process"
        );
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--list")
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let child_pid = child.id();
        child.wait().unwrap();
        assert_eq!(computer.life(child_pid, 0), Life::Gone);
        assert!(!computer.host().is_empty());
        let clock = ThisComputer;
        let before = clock.now_ms();
        clock.sleep(Duration::from_millis(5));
        assert!(clock.now_ms() >= before);
    }
}
