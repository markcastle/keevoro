//! Jobs shared between real programs.
//!
//! The unit tests share jobs between two Keevoros in one process, which
//! proves the files and the rules but not what only separate processes
//! have: a process number of their own, a pause switch of their own, and
//! the chance to be ended without warning. So these tests start a second
//! program: this same test program, run again with `KEEVORO_HELPER` set,
//! which makes the `helper` test below do one job and report on its
//! standard output. Without that variable, `helper` does nothing.
//!
//! Each test holds the other program's job where it wants it with a pause,
//! never with a sleep, and waits for what it reports with a deadline.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use keevoro_api::{Folders, FrontEnd, JobId, Keevoro, Kind, Said, Which};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// Pausing is one switch for the whole of this test program, so these
/// tests take turns.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn one_at_a_time() -> MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Lifts the pause however a test ends.
struct CarryOnAfter;

impl Drop for CarryOnAfter {
    fn drop(&mut self) {
        keevoro_core::pause::set_paused(false);
    }
}

fn folders(dir: &Path) -> Folders {
    Folders {
        config: dir.join("config"),
        data: dir.join("data"),
        cache: dir.join("cache"),
        logs: dir.join("logs"),
    }
}

/// A memory card with a few photos on it.
fn card_in(dir: &Path) -> PathBuf {
    let card = dir.join("Card");
    for (n, rel) in [
        "DCIM/100CANON/IMG_0001.jpg",
        "DCIM/100CANON/IMG_0002.jpg",
        "DCIM/IMG_0003.jpg",
    ]
    .iter()
    .enumerate()
    {
        let path = card.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let shade = u8::try_from(n * 80).unwrap();
        let img = image::RgbImage::from_pixel(320, 240, image::Rgb([shade, 120, 60]));
        img.save(&path).unwrap();
    }
    card
}

/// The other program, and the lines it reports.
struct Helper {
    child: Child,
    said: Receiver<String>,
}

impl Helper {
    /// Starts the other program doing `what` with Keevoro in `dir`.
    fn start(what: &str, dir: &Path) -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["helper", "--exact", "--nocapture", "--test-threads=1"])
            .env("KEEVORO_HELPER", what)
            .env("KEEVORO_HELPER_DIR", dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let (tx, said) = channel();
        let out = BufReader::new(child.stdout.take().unwrap());
        std::thread::spawn(move || {
            // The test runner starts the line a test's output begins on
            // with the test's name, so a report may be part-way along one.
            for line in out.lines().map_while(Result::ok) {
                if let Some((_, report)) = line.split_once("HELPER ") {
                    let _ = tx.send(report.to_string());
                }
            }
        });
        Self { child, said }
    }

    /// The next thing it reports, which must begin with `expected`; the
    /// rest of the line is answered.
    fn expect(&self, expected: &str) -> String {
        let line = self
            .said
            .recv_timeout(Duration::from_secs(60))
            .unwrap_or_else(|_| panic!("the other program never said {expected}"));
        line.strip_prefix(expected)
            .unwrap_or_else(|| panic!("expected {expected}, heard {line}"))
            .trim()
            .to_string()
    }

    fn job(&self, expected: &str) -> JobId {
        JobId(self.expect(expected).parse().unwrap())
    }

    /// Waits for it to end, and says whether it ended well.
    fn ends(mut self) -> bool {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status.success();
            }
            assert!(Instant::now() < deadline, "the other program never ended");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// Waits, for a generous while, for something the other program does in
/// its own time.
fn eventually<T>(what: &str, found: impl Fn() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(it) = found() {
            return it;
        }
        assert!(Instant::now() < deadline, "{what} never happened");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Keevoro in `dir` with one backup, "Card photos", and the card.
fn app_with_a_backup(dir: &Path) -> (Keevoro, keevoro_api::Named, PathBuf) {
    let keevoro = Keevoro::open(&folders(dir));
    let named = keevoro.start_backup("Card photos").unwrap();
    let card = card_in(dir);
    (keevoro, named, card)
}

fn shared_files(dir: &Path, named: &keevoro_api::Named) -> PathBuf {
    dir.join("data").join("jobs").join(&named.id)
}

#[test]
fn another_programs_search_is_seen_refused_followed_and_stopped_and_it_tidies_up() {
    let _turn = one_at_a_time();
    let _after = CarryOnAfter;
    let dir = tempfile::tempdir().unwrap();
    let (app, named, card) = app_with_a_backup(dir.path());

    let other = Helper::start("hold", dir.path());
    let job = other.job("holding");

    assert!(app.busy() && !app.busy_here());
    let seen = eventually("its job seen", || {
        app.jobs(Some(&named.id)).into_iter().find(|s| s.handle.id == job)
    });
    assert!(seen.elsewhere && seen.paused);
    assert_eq!(seen.handle.started_by, FrontEnd::CommandLine);
    let refused = app.run_backup(&named, None).unwrap_err();
    assert_eq!(
        (refused.kind, refused.message.as_str()),
        (
            Kind::Busy,
            "Keevoro is already looking through a source for \"Card photos\" from the command line. Let that finish, or stop it, first."
        )
    );

    let heard: Arc<Mutex<Vec<Said>>> = Arc::default();
    {
        let heard = heard.clone();
        assert!(app.follow(job, Arc::new(move |e| heard.lock().unwrap().push(e.said.clone()))));
    }
    assert_eq!(app.stop(Which::Job(job)), [job]);
    let Some(Said::Scan(last)) = app.wait(job) else {
        panic!("waiting from another program hears how it ended")
    };
    assert!(last.finished && last.stopped, "{last:?}");
    eventually("the follower told the end", || {
        heard.lock().unwrap().contains(&Said::Scan(last.clone())).then_some(())
    });
    assert_eq!(other.expect("ended"), "stopped");
    assert!(other.ends(), "it ended normally");

    // It let go: its lock and requests are gone, and only its last words
    // are left, for a minute, for anyone still waiting on them.
    let left: Vec<String> = std::fs::read_dir(shared_files(dir.path(), &named))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, ["status.json"]);
    assert!(!app.busy());
    let mine = app.look(&named, &card, None).unwrap();
    assert!(matches!(app.wait(mine.id), Some(Said::Scan(e)) if !e.stopped));
    app.wait_for_jobs();
}

#[test]
fn a_program_ended_without_warning_does_not_shut_anyone_out_of_the_backup() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let (app, named, card) = app_with_a_backup(dir.path());

    let mut other = Helper::start("hold", dir.path());
    other.job("holding");
    assert!(app.busy());
    // Ended as a crash or a power cut would end it: no chance to let go.
    other.child.kill().unwrap();
    other.child.wait().unwrap();
    assert!(
        shared_files(dir.path(), &named).join("holder.json").exists(),
        "its lock is left behind"
    );

    assert!(!app.busy(), "a lock whose program has gone is nobody's");
    let mine = app.look(&named, &card, None).unwrap();
    assert!(matches!(app.wait(mine.id), Some(Said::Scan(e)) if !e.stopped));
    app.wait_for_jobs();
    let log = app.log_tail(40);
    assert!(
        log.contains("\"Card photos\" was left marked as busy by a program that had stopped"),
        "{log}"
    );
}

#[test]
fn pause_and_carry_on_reach_another_program() {
    let _turn = one_at_a_time();
    let _after = CarryOnAfter;
    let dir = tempfile::tempdir().unwrap();
    let (app, named, _card) = app_with_a_backup(dir.path());

    let other = Helper::start("wait for a pause", dir.path());
    let job = other.job("looking");
    app.pause();
    // The other program noticed, which only its own switch could tell it.
    // It says so either way, so that a request that never arrived is
    // told apart from a status that never said so.
    assert_eq!(other.expect("pause"), "heard", "the request to pause reached it");
    let log = app.log_tail(40);
    assert!(!log.contains("Couldn't pass a request"), "{log}");
    eventually("it is seen paused", || {
        app.jobs(Some(&named.id))
            .into_iter()
            .find(|s| s.handle.id == job && s.paused)
    });
    app.carry_on();
    let Some(Said::Scan(last)) = app.wait(job) else {
        panic!("it went on to the end")
    };
    assert!(last.finished && !last.stopped, "{last:?}");
    assert_eq!(other.expect("ended"), "finished");
    assert!(other.ends());
}

/// The other program. Does nothing unless started by a test above.
#[test]
fn helper() {
    let Ok(what) = std::env::var("KEEVORO_HELPER") else {
        return;
    };
    let dir = PathBuf::from(std::env::var("KEEVORO_HELPER_DIR").unwrap());
    let say = |line: &str| println!("HELPER {line}");
    let keevoro = Keevoro::open(&folders(&dir))
        .as_front_end(FrontEnd::CommandLine)
        .with_progress_every(Duration::ZERO);
    let named = keevoro.backups().find("Card photos").unwrap();
    let card = dir.join("Card");
    let job = match what.as_str() {
        // Holds the backup: its search waits before its first file.
        "hold" => {
            keevoro.pause();
            let job = keevoro.look(&named, &card, None).unwrap();
            say(&format!("holding {}", job.id.0));
            job
        }
        // Says it is looking, then waits until it is paused, which only a
        // request from another program can do.
        "wait for a pause" => {
            let told = Arc::new(Mutex::new(false));
            let listener: keevoro_api::Listener = Arc::new(move |event| {
                let mut told = told.lock().unwrap();
                if *told {
                    return;
                }
                *told = true;
                say(&format!("looking {}", event.job.0));
                let deadline = Instant::now() + Duration::from_secs(30);
                while !keevoro_core::pause::is_paused() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(10));
                }
                say(if keevoro_core::pause::is_paused() {
                    "pause heard"
                } else {
                    "pause never heard"
                });
            });
            keevoro.look(&named, &card, Some(listener)).unwrap()
        }
        other => panic!("no such helper: {other}"),
    };
    let ended = match keevoro.wait(job.id) {
        Some(Said::Scan(e)) if e.stopped => "stopped",
        Some(Said::Scan(e)) if e.finished => "finished",
        _ => "unknown",
    };
    keevoro.wait_for_jobs();
    say(&format!("ended {ended}"));
}
