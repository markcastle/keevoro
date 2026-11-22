//! The lock, the status and the requests, with the clock and the
//! computer's processes in the test's hands.
//!
//! Each `Across` here stands for a separate program: two in one test share
//! a folder exactly as two programs would. Nothing sleeps to let time
//! pass; the fake clock is moved instead. The keeper runs on its own
//! thread in real time, and is waited for with a deadline, never a fixed
//! sleep.

use super::lock::tests::{FakeClock, FakeProcesses, HERE};
use super::lock::ThisComputer;
use super::*;
use crate::jobs::{Counts, JobBackup, JobHandle, JobStatus};
use std::path::Path;
use std::sync::atomic::AtomicUsize;

const MINUTE: i64 = 60_000;
/// A plausible time to start the fake clock at.
const START: i64 = 1_800_000_000_000;

fn named(id: &str) -> Named {
    Named {
        id: id.into(),
        name: "Card photos".into(),
        home: PathBuf::from("home"),
    }
}

/// What a program's jobs would be doing, as far as the keeper needs to
/// know.
#[derive(Default)]
struct FakeWork {
    asked: Mutex<Vec<Request>>,
    lost: AtomicUsize,
    nothing_held: AtomicUsize,
    running: Mutex<Vec<status::Running>>,
}

impl Work for FakeWork {
    fn status(&self, _backup: &str, token: &str, beat: i64) -> Status {
        Status {
            token: token.into(),
            beat,
            paused: false,
            running: lock_of(&self.running).clone(),
            ended: vec![status::Ended {
                job: JobId(9),
                last: None,
                status: None,
            }],
        }
    }

    fn asked(&self, request: &Request) {
        lock_of(&self.asked).push(request.clone());
    }

    fn lost(&self, _backup: &str) {
        self.lost.fetch_add(1, Ordering::SeqCst);
    }

    fn nothing_held(&self) {
        self.nothing_held.fetch_add(1, Ordering::SeqCst);
    }
}

fn running(kind: JobKind) -> status::Running {
    status::Running {
        status: JobStatus {
            handle: JobHandle {
                id: JobId(5),
                job: kind,
                op: kind.op().map(crate::register::Op::name),
                backup: Some(JobBackup {
                    id: "backup-1".into(),
                    name: "Card photos".into(),
                }),
                started_at: 1,
                started_by: FrontEnd::CommandLine,
                by_itself: None,
            },
            step: "Backing up".into(),
            line: "Backing up".into(),
            counts: Counts::default(),
            running_seconds: 2,
            estimate: None,
            over_network: false,
            stopping: false,
            paused: false,
            paused_alone: false,
            last: None,
            elsewhere: false,
            ended: false,
        },
        told: 3,
    }
}

/// One program's side, in `root`, with its own clock and view of the
/// computer's processes.
struct Program {
    across: Across,
    clock: Arc<FakeClock>,
    processes: Arc<FakeProcesses>,
    work: Arc<FakeWork>,
    log: Arc<Log>,
}

fn program(root: &Path) -> Program {
    let clock = Arc::new(FakeClock::default());
    clock.now.store(START, Ordering::SeqCst);
    let processes = Arc::new(FakeProcesses::default());
    let log = Arc::new(Log::open(&root.join("logs")));
    Program {
        across: Across::new(root.join("jobs"), log.clone(), clock.clone(), processes.clone()),
        clock,
        processes,
        work: Arc::default(),
        log,
    }
}

impl Program {
    fn take(&self, backup: &str) -> Result<(), NotTaken> {
        let work: Arc<dyn Work> = self.work.clone();
        self.across
            .take(&named(backup), FrontEnd::CommandLine, Arc::downgrade(&work))
    }

    fn let_go(&self, backup: &str) {
        self.across.let_go(backup, &*self.work);
    }
}

/// A lock left on `backup` by process `pid` on `host`, whose heartbeat was
/// `beat_ago` milliseconds before the fake clock's start.
fn left_behind(root: &Path, backup: &str, pid: u32, host: &str, beat_ago: i64) -> Holder {
    let place = root.join("jobs").join(backup);
    std::fs::create_dir_all(&place).unwrap();
    let holder = Holder {
        token: format!("theirs-{pid}"),
        pid,
        started: 1_700_000_000,
        host: host.into(),
        by: FrontEnd::Agent,
        since: START - beat_ago,
    };
    std::fs::write(place.join("holder.json"), serde_json::to_vec(&holder).unwrap()).unwrap();
    holder
}

fn holder_on_disk(root: &Path, backup: &str) -> Option<Holder> {
    std::fs::read(root.join("jobs").join(backup).join("holder.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
}

/// Waits for something the keeper does in its own time, for a generous
/// while, looking every so often.
fn eventually(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "{what} never happened");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn refused(result: Result<(), NotTaken>) -> Option<Box<Other>> {
    match result {
        Err(NotTaken::Held(other)) => other,
        Err(NotTaken::Failed(e)) => panic!("it failed instead: {e}"),
        Ok(()) => panic!("it was taken"),
    }
}

#[test]
fn one_program_holds_a_backup_another_is_told_who_and_can_take_it_once_let_go() {
    let dir = tempfile::tempdir().unwrap();
    let app = program(dir.path());
    let command_line = program(dir.path());

    app.take("backup-1").unwrap();
    // The holder's own second job is counted under the same hold.
    app.take("backup-1").unwrap();
    let mine = holder_on_disk(dir.path(), "backup-1").unwrap();
    assert_eq!(
        (mine.pid, mine.host.as_str(), mine.by),
        (7, HERE, FrontEnd::CommandLine)
    );
    command_line.processes.alive.lock().unwrap().insert(7, 1_700_000_007);

    let other = refused(command_line.take("backup-1")).expect("it says who holds it");
    assert_eq!(other.holder.token, mine.token);
    assert!(command_line.across.elsewhere("backup-1").is_some());
    assert_eq!(command_line.across.others().len(), 1);
    assert!(
        app.across.elsewhere("backup-1").is_none(),
        "its own hold is not elsewhere"
    );
    assert!(command_line.take("backup-2").is_ok(), "another backup is free");

    app.let_go("backup-1");
    assert!(
        holder_on_disk(dir.path(), "backup-1").is_some(),
        "one job still needs it"
    );
    app.let_go("backup-1");
    assert!(
        holder_on_disk(dir.path(), "backup-1").is_none(),
        "let go when the last ends"
    );
    assert!(command_line.across.elsewhere("backup-1").is_none());
    command_line.take("backup-1").unwrap();
    command_line.let_go("backup-1");
    command_line.let_go("backup-2");
}

#[test]
fn the_jobs_are_told_when_the_last_hold_is_let_go_and_not_before() {
    let dir = tempfile::tempdir().unwrap();
    let app = program(dir.path());
    let told = || app.work.nothing_held.load(Ordering::SeqCst);

    app.take("backup-1").unwrap();
    app.take("backup-1").unwrap();
    app.take("backup-2").unwrap();
    app.let_go("backup-1");
    app.let_go("backup-1");
    assert_eq!(told(), 0, "another backup is still held, and seen");
    app.let_go("backup-2");
    assert_eq!(told(), 1, "nothing is held now");
    app.let_go("backup-2");
    assert_eq!(told(), 1, "letting go of what is not held says nothing");
}

#[test]
fn a_lock_left_by_a_process_that_has_gone_is_taken_at_once_and_said_in_the_log() {
    let dir = tempfile::tempdir().unwrap();
    let me = program(dir.path());
    // Its heartbeat is fresh, but its process is gone.
    left_behind(dir.path(), "backup-1", 4242, HERE, 0);
    assert!(me.across.elsewhere("backup-1").is_none(), "a stale lock is nobody's");
    me.take("backup-1").unwrap();
    assert_eq!(holder_on_disk(dir.path(), "backup-1").unwrap().pid, 7);
    assert_eq!(me.clock.now_ms(), START, "nothing was waited for");
    let log = me.log.tail(1);
    assert!(
        log.contains(
            "\"Card photos\" was left marked as busy by a program that had stopped (process 4242, an AI agent)"
        ),
        "{log}"
    );
    me.let_go("backup-1");
}

#[test]
fn a_live_holder_with_a_recent_heartbeat_keeps_its_lock() {
    let dir = tempfile::tempdir().unwrap();
    let me = program(dir.path());
    let theirs = left_behind(dir.path(), "backup-1", 4242, HERE, 59_000);
    me.processes.alive.lock().unwrap().insert(4242, theirs.started);
    let other = refused(me.take("backup-1")).unwrap();
    assert_eq!(other.holder, theirs);
    assert_eq!(
        other.refusal("Card photos"),
        "Keevoro is already working on \"Card photos\" for an AI agent. Let that finish, or stop it, first."
    );
    assert_eq!(holder_on_disk(dir.path(), "backup-1").unwrap(), theirs, "untouched");
}

#[test]
fn a_number_reused_by_another_process_does_not_keep_a_lock() {
    let dir = tempfile::tempdir().unwrap();
    let me = program(dir.path());
    left_behind(dir.path(), "backup-1", 4242, HERE, 0);
    // Process 4242 is running, but it started at another time.
    me.processes.alive.lock().unwrap().insert(4242, 1_700_000_999);
    me.take("backup-1").unwrap();
    me.let_go("backup-1");
}

#[test]
fn an_old_heartbeat_that_does_not_move_while_watched_gives_the_lock_up() {
    let dir = tempfile::tempdir().unwrap();
    let me = program(dir.path());
    let theirs = left_behind(dir.path(), "backup-1", 4242, HERE, 2 * MINUTE);
    // Alive, as far as the computer says, but stuck.
    me.processes.alive.lock().unwrap().insert(4242, theirs.started);
    me.take("backup-1").unwrap();
    assert_eq!(
        me.clock.now_ms(),
        START + i64::try_from(WATCH.as_millis()).unwrap(),
        "it watched before deciding"
    );
    me.let_go("backup-1");
}

#[test]
fn an_old_heartbeat_that_moves_while_watched_is_a_clock_that_jumped_and_the_lock_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let me = program(dir.path());
    let theirs = left_behind(dir.path(), "backup-1", 4242, HERE, 2 * MINUTE);
    me.processes.alive.lock().unwrap().insert(4242, theirs.started);
    // While watched, the holder beats, as a live program does.
    let status_path = dir.path().join("jobs").join("backup-1").join("status.json");
    let token = theirs.token.clone();
    *me.clock.on_sleep.lock().unwrap() = Some(Box::new(move || {
        let status = Status {
            token: token.clone(),
            beat: START - MINUTE,
            paused: false,
            running: vec![running(JobKind::BackUp)],
            ended: Vec::new(),
        };
        std::fs::write(&status_path, status.to_bytes()).unwrap();
    }));
    let other = refused(me.take("backup-1")).unwrap();
    assert_eq!(
        other.refusal("Card photos"),
        "Keevoro is already backing up \"Card photos\" for an AI agent. Let that finish, or stop it, first."
    );
}

#[test]
fn a_heartbeat_from_the_future_after_the_clock_went_back_keeps_the_lock() {
    let dir = tempfile::tempdir().unwrap();
    let me = program(dir.path());
    let theirs = left_behind(dir.path(), "backup-1", 4242, HERE, -60 * MINUTE);
    me.processes.alive.lock().unwrap().insert(4242, theirs.started);
    assert!(refused(me.take("backup-1")).is_some());
    assert_eq!(me.clock.now_ms(), START, "nothing to watch");
}

#[test]
fn a_lock_from_another_computer_is_judged_by_its_heartbeat_alone() {
    let dir = tempfile::tempdir().unwrap();
    let me = program(dir.path());
    // Process 4242 is not running here, which says nothing about there.
    left_behind(dir.path(), "backup-1", 4242, "another computer", 0);
    assert!(refused(me.take("backup-1")).is_some());
    left_behind(dir.path(), "backup-1", 4242, "another computer", 10 * MINUTE);
    me.take("backup-1").unwrap();
    me.let_go("backup-1");
}

#[test]
fn a_garbled_lock_is_left_while_new_and_passed_over_once_old() {
    let dir = tempfile::tempdir().unwrap();
    let me = program(dir.path());
    let place = dir.path().join("jobs").join("backup-1");
    std::fs::create_dir_all(&place).unwrap();
    std::fs::write(place.join("holder.json"), b"").unwrap();
    // Just made, in two steps on a drive that cannot link files.
    me.clock.now.store(ThisComputer.now_ms(), Ordering::SeqCst);
    assert!(refused(me.take("backup-1")).is_none(), "nobody to name");
    // Long after, it is nobody's.
    me.clock.fetch_add_ms(2 * MINUTE);
    me.take("backup-1").unwrap();
    me.let_go("backup-1");
}

#[test]
fn passing_over_a_stale_lock_puts_back_one_taken_in_the_meantime() {
    let dir = tempfile::tempdir().unwrap();
    let me = program(dir.path());
    let stale = left_behind(dir.path(), "backup-1", 4242, HERE, 0);
    // Between judging that lock stale and moving it aside, another program
    // passed over it first and took the backup.
    let taken = left_behind(dir.path(), "backup-1", 5353, HERE, 0);
    let place = me.across.place("backup-1");
    me.across.pass_over(&place, Some(&stale), &named("backup-1")).unwrap();
    assert_eq!(
        holder_on_disk(dir.path(), "backup-1").unwrap(),
        taken,
        "theirs is put back"
    );
    assert_eq!(
        std::fs::read_dir(&place.dir).unwrap().count(),
        1,
        "nothing is left aside"
    );
    // And the one judged stale is removed.
    me.across.pass_over(&place, Some(&taken), &named("backup-1")).unwrap();
    assert!(holder_on_disk(dir.path(), "backup-1").is_none());
}

#[test]
fn the_keeper_writes_the_status_beats_serves_requests_and_cleans_up() {
    let dir = tempfile::tempdir().unwrap();
    let holder = program(dir.path());
    let asker = program(dir.path());
    asker.processes.alive.lock().unwrap().insert(7, 1_700_000_007);
    lock_of(&holder.work.running).push(running(JobKind::BackUp));
    holder.take("backup-1").unwrap();

    // The status appears, readable by anyone, with the jobs marked as
    // elsewhere for the reader.
    eventually("a status", || {
        asker
            .across
            .elsewhere("backup-1")
            .is_some_and(|other| other.status.is_some())
    });
    let other = asker.across.elsewhere("backup-1").unwrap();
    let theirs = other.running();
    assert_eq!(theirs.len(), 1);
    assert!(theirs[0].0.elsewhere);
    assert_eq!(theirs[0].1, 3);
    assert_eq!(other.status.as_ref().unwrap().beat, START);
    assert_eq!(
        other.refusal("Card photos"),
        "Keevoro is already backing up \"Card photos\" from the command line. Let that finish, or stop it, first."
    );

    // It beats again even when nothing changes.
    holder.clock.fetch_add_ms(1_000);
    eventually("a heartbeat", || {
        asker
            .across
            .elsewhere("backup-1")
            .and_then(|o| o.status)
            .is_some_and(|s| s.beat == START + 1_000)
    });

    // A request is noticed within a tick or two, and only by its holder.
    asker.across.ask(&other, Ask::Stop, vec![JobId(5)], FrontEnd::Agent);
    eventually("the request served", || !lock_of(&holder.work.asked).is_empty());
    let asked = lock_of(&holder.work.asked).clone();
    assert_eq!(
        (asked[0].ask, asked[0].jobs.clone(), asked[0].by),
        (Ask::Stop, vec![JobId(5)], FrontEnd::Agent)
    );
    let requests = dir.path().join("jobs").join("backup-1").join("requests");
    assert_eq!(
        std::fs::read_dir(&requests).unwrap().count(),
        0,
        "taken requests are removed"
    );

    // Letting go writes the last status and removes the lock and requests.
    lock_of(&holder.work.running).clear();
    holder.let_go("backup-1");
    assert!(holder_on_disk(dir.path(), "backup-1").is_none());
    assert!(!requests.exists());
    assert_eq!(
        asker.across.ended(JobId(9)),
        Some(None),
        "a job's end is there for anyone waiting on it"
    );
    assert_eq!(asker.across.ended(JobId(10)), None);

    // Its last words outlive it for a while, for anyone waiting on them,
    // and are swept away once they are old.
    drop(holder.across);
    assert_eq!(asker.across.ended(JobId(9)), Some(None));
    asker.across.sweep();
    assert_eq!(asker.across.ended(JobId(9)), Some(None), "not yet");
    asker.clock.fetch_add_ms(2 * MINUTE);
    asker.across.sweep();
    assert_eq!(asker.across.ended(JobId(9)), None);
    assert!(!dir.path().join("jobs").join("backup-1").exists());
}

#[test]
fn a_status_still_held_is_never_swept_however_old() {
    let dir = tempfile::tempdir().unwrap();
    let sweeper = program(dir.path());
    let theirs = left_behind(dir.path(), "backup-1", 4242, HERE, 10 * MINUTE);
    let status = Status {
        token: theirs.token,
        beat: START - 10 * MINUTE,
        paused: false,
        running: Vec::new(),
        ended: Vec::new(),
    };
    let place = dir.path().join("jobs").join("backup-1");
    std::fs::write(place.join("status.json"), status.to_bytes()).unwrap();
    sweeper.across.sweep();
    assert!(place.join("status.json").exists());
    // Something that is not a status at all, with nobody holding it, goes.
    std::fs::remove_file(place.join("holder.json")).unwrap();
    std::fs::write(place.join("status.json"), b"{").unwrap();
    sweeper.across.sweep();
    assert!(!place.exists());
}

#[test]
fn a_holder_whose_lock_was_taken_from_it_stops_its_work() {
    let dir = tempfile::tempdir().unwrap();
    let holder = program(dir.path());
    holder.take("backup-1").unwrap();
    // Another program judged it stopped and took over.
    left_behind(dir.path(), "backup-1", 4242, HERE, 0);
    eventually("the loss noticed", || holder.work.lost.load(Ordering::SeqCst) == 1);
    holder.let_go("backup-1");
    assert_eq!(
        holder_on_disk(dir.path(), "backup-1").unwrap().pid,
        4242,
        "the new holder's lock is not removed"
    );
}

#[test]
fn a_backup_id_never_names_a_folder_outside_its_own() {
    assert_eq!(folder_name("backup-3"), "backup-3");
    assert_eq!(folder_name("library-1"), "library-1");
    assert_eq!(folder_name("../../etc"), "______etc");
    assert_eq!(folder_name(".hidden"), "_hidden");
    assert_eq!(folder_name(""), "_");
    assert_eq!(folder_name("C:\\Windows"), "C__Windows");
}

/// `holder.json` and `status.json` are read by older Keevoro programs
/// running beside a newer one. An older program that meets a word it does
/// not know in them cannot read the file at all, and a lock it cannot read
/// is taken over once it is a minute old, so a second backup would run on
/// the same catalog. So the words in these three never grow: mark anything
/// new with a new optional field instead, which older programs pass over.
/// A variant added here fails to build, so whoever adds one reads this
/// first (docs/developing.md, "Words other programs read").
#[test]
fn the_words_older_programs_read_in_shared_files_never_grow() {
    use crate::jobs::{FrontEnd, JobKind, Outcome};
    fn front_end(f: FrontEnd) -> &'static str {
        match f {
            FrontEnd::App => "app",
            FrontEnd::CommandLine => "commandLine",
            FrontEnd::Agent => "agent",
        }
    }
    fn job(k: JobKind) -> &'static str {
        match k {
            JobKind::Survey => "survey",
            JobKind::Look => "look",
            JobKind::LookAgain => "lookAgain",
            JobKind::BackUp => "backUp",
            JobKind::Check => "check",
            JobKind::Duplicates => "duplicates",
        }
    }
    fn outcome(o: Outcome) -> &'static str {
        match o {
            Outcome::Worked => "worked",
            Outcome::SomeFiles => "someFiles",
            Outcome::Unreachable => "unreachable",
            Outcome::Refused => "refused",
            Outcome::Stopped => "stopped",
        }
    }
    for f in [FrontEnd::App, FrontEnd::CommandLine, FrontEnd::Agent] {
        assert_eq!(serde_json::to_value(f).unwrap(), front_end(f));
    }
    for k in [
        JobKind::Survey,
        JobKind::Look,
        JobKind::LookAgain,
        JobKind::BackUp,
        JobKind::Check,
        JobKind::Duplicates,
    ] {
        assert_eq!(serde_json::to_value(k).unwrap(), job(k));
    }
    for o in [
        Outcome::Worked,
        Outcome::SomeFiles,
        Outcome::Unreachable,
        Outcome::Refused,
        Outcome::Stopped,
    ] {
        assert_eq!(serde_json::to_value(o).unwrap(), outcome(o));
    }
}

/// What a newer program writes about a backup that runs by itself is read
/// by an older one as an ordinary backup: the new fields are passed over.
#[test]
fn a_backup_running_by_itself_is_read_by_an_older_program_as_a_backup() {
    /// The handle as 0.41 read it.
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    #[allow(dead_code)]
    struct HandleV0 {
        id: JobId,
        job: JobKind,
        op: Option<String>,
        backup: Option<JobBackup>,
        started_at: i64,
        started_by: FrontEnd,
    }
    let mut by_itself = running(JobKind::BackUp);
    by_itself.status.handle.by_itself = Some(keevoro_core::unattended::Occasion::Schedule);
    by_itself.status.handle.op = Some("schedule.run");
    by_itself.status.last = Some(crate::jobs::Said::Send(crate::jobs::SendEvent {
        held_back: vec!["4 new pictures were set aside.".into()],
        not_now: Some("Waiting while this computer is on battery".into()),
        ..crate::jobs::SendEvent::finished(b"Done.", None, crate::jobs::Outcome::Worked)
    }));
    let written = serde_json::to_value(&by_itself.status).unwrap();
    assert_eq!(written["byItself"], "schedule");
    let old: HandleV0 = serde_json::from_value(written.clone()).unwrap();
    assert_eq!((old.job, old.started_by), (JobKind::BackUp, FrontEnd::CommandLine));
    let now: JobStatus = serde_json::from_value(written).unwrap();
    assert_eq!(now.handle.by_itself, Some(keevoro_core::unattended::Occasion::Schedule));
    assert_eq!(now.handle.op, Some("schedule.run"));
}
