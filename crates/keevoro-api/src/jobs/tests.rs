//! Jobs, run for real against backups and folders in a temporary folder.
//!
//! Nothing here sleeps to let a job get somewhere. A job is held where a
//! test wants it by pausing Keevoro before it starts, or stopped from its
//! own listener at the moment it says the thing the test is waiting for.
//! Every test waits for every job, including the background check a
//! search or a backup starts, before it returns.

use super::*;
use crate::backup::tests::{card_in, card_looked_through_into, large_photo};
use crate::backup::Backup;
use crate::destinations::{AddDestinationRequest, DestinationKind};
use crate::{call, Kind};
use keevoro_catalog::Catalog;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Instant;

/// Pausing is one switch for the whole program, and stopping a search
/// lifts it, so tests that run jobs take turns.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

pub(crate) fn one_at_a_time() -> MutexGuard<'static, ()> {
    lock(&ONE_AT_A_TIME)
}

/// Lifts the pause however a test ends, so one that fails does not hold
/// up the rest.
pub(crate) struct CarryOnAfter;

impl Drop for CarryOnAfter {
    fn drop(&mut self) {
        keevoro_core::pause::carry_on();
    }
}

/// Keevoro in a temporary folder, where every job says every step.
fn keevoro(dir: &Path) -> Keevoro {
    crate::keevoro::tests::keevoro(dir).with_progress_every(Duration::ZERO)
}

/// Everything a listener heard.
#[derive(Clone, Default)]
struct Heard(Arc<Mutex<Vec<Event>>>);

impl Heard {
    fn listener(&self) -> Option<Listener> {
        let heard = self.0.clone();
        Some(Arc::new(move |event: &Event| lock(&heard).push(event.clone())))
    }

    fn said(&self) -> Vec<Said> {
        lock(&self.0).iter().map(|e| e.said.clone()).collect()
    }

    fn lines(&self) -> Vec<String> {
        self.said()
            .into_iter()
            .filter_map(|said| match said {
                Said::Send(e) => e.line,
                _ => None,
            })
            .collect()
    }
}

fn folder(place: &Path) -> AddDestinationRequest {
    AddDestinationRequest {
        kind: DestinationKind::Folder,
        place: place.display().to_string(),
        bucket: None,
        access_key_id: None,
        secret_access_key: None,
    }
}

/// A backup called "Card photos" that has looked through the card, and
/// goes to a folder called USB.
fn card_going_to_usb(dir: &Path, keevoro: &Keevoro) -> (Named, PathBuf, PathBuf) {
    let named = keevoro.start_backup("Card photos").unwrap();
    let card = card_looked_through_into(dir, &named.home);
    let usb = dir.join("USB");
    keevoro
        .add_destination(&Backup::open(&named.home).unwrap(), &folder(&usb))
        .unwrap();
    (named, card, usb)
}

fn there(named: &Named) -> usize {
    Backup::open(&named.home).unwrap().destinations().unwrap()[0].there
}

#[test]
fn a_search_says_how_it_goes_and_adds_a_source() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro(dir.path());
    let named = keevoro.start_backup("Card photos").unwrap();
    let card = card_in(dir.path());
    let heard = Heard::default();

    let job = keevoro.look(&named, &card, heard.listener()).unwrap();
    assert_eq!(job.job, JobKind::Look);
    assert_eq!(job.op, Some("sources.look"));
    assert_eq!(job.started_by, FrontEnd::App);
    assert_eq!(job.backup.as_ref().unwrap().name, "Card photos");
    assert!(job.started_at > 1_700_000_000);

    let Some(Said::Scan(last)) = keevoro.wait(job.id) else {
        panic!("a search ends with how it went")
    };
    assert!(last.finished && !last.stopped, "{last:?}");
    assert_eq!((last.name.as_str(), last.problem.as_deref()), ("Card", None));
    assert_eq!(last.files, 5);
    keevoro.wait_for_jobs();

    let said = heard.said();
    assert!(
        said.iter()
            .any(|s| matches!(s, Said::Scan(e) if !e.finished && !e.saving && e.files > 0)),
        "it said how far it had got: {said:?}"
    );
    assert!(said.iter().any(|s| matches!(s, Said::Scan(e) if e.saving)));
    assert_eq!(said.last(), Some(&Said::Scan(last)));
    assert!(lock(&heard.0).iter().all(|e| e.job == job.id && e.event == "scan"));

    let backup = Backup::open(&named.home).unwrap();
    let sources = backup.sources().unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].name, "Card");
    assert_eq!(backup.found().unwrap().photos, 3);
    let log = keevoro.log_tail(20);
    for line in ["Looking through", "Finished looking after 5 files", "Looked through"] {
        assert!(log.contains(line), "{line} is missing from:\n{log}");
    }
    assert!(keevoro.jobs(None).is_empty());
}

#[test]
fn a_stopped_search_keeps_nothing() {
    let _turn = one_at_a_time();
    let _after = CarryOnAfter;
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro(dir.path());
    let named = keevoro.start_backup("Card photos").unwrap();
    let card = card_in(dir.path());

    // Paused, it waits before the first file.
    keevoro.pause();
    let job = keevoro.look(&named, &card, None).unwrap();
    assert_eq!(keevoro.stop(Which::Job(job.id)), [job.id]);
    assert!(
        !keevoro_core::pause::is_paused(),
        "a paused search is woken to see it has been asked to stop"
    );
    let Some(Said::Scan(last)) = keevoro.wait(job.id) else {
        panic!("a stopped search still says how it ended")
    };
    assert!(last.finished && last.stopped, "{last:?}");
    keevoro.wait_for_jobs();

    assert!(Backup::open(&named.home).unwrap().sources().unwrap().is_empty());
    let log = keevoro.log_tail(20);
    assert!(log.contains("Asked to stop the search"), "{log}");
    assert!(log.contains("Nothing from it was kept."), "{log}");
    assert!(keevoro.stop(Which::Job(job.id)).is_empty(), "it has ended");
}

#[test]
fn status_says_what_is_running_and_is_empty_afterwards() {
    let _turn = one_at_a_time();
    let _after = CarryOnAfter;
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro(dir.path());
    let named = keevoro.start_backup("Card photos").unwrap();
    let other = keevoro.start_backup("Drone").unwrap();
    let card = card_in(dir.path());
    assert!(keevoro.jobs(None).is_empty());

    keevoro.pause();
    let job = keevoro.look(&named, &card, None).unwrap();
    let all = keevoro.jobs(None);
    assert_eq!(all.len(), 1);
    let status = &all[0];
    assert_eq!(status.handle, job);
    assert_eq!(status.step, "Looking through Card");
    assert!(status.paused && !status.stopping);
    assert_eq!(keevoro.jobs(Some(&named.id)).len(), 1);
    assert!(keevoro.jobs(Some(&other.id)).is_empty());

    let json = serde_json::to_value(status).unwrap();
    assert_eq!(json["op"], "sources.look");
    assert_eq!(json["job"], "look");
    assert_eq!(json["startedBy"], "app");
    assert_eq!(json["backup"], json!({ "id": named.id, "name": "Card photos" }));
    for field in [
        "id",
        "startedAt",
        "step",
        "line",
        "counts",
        "runningSeconds",
        "estimate",
        "overNetwork",
        "stopping",
        "paused",
        "last",
    ] {
        assert!(json.get(field).is_some(), "{field} is missing from {json}");
    }

    keevoro.carry_on();
    keevoro.wait_for_jobs();
    assert!(keevoro.jobs(None).is_empty());
    assert_eq!(call(&keevoro, Op::JobsStatus, None, Value::Null).unwrap(), json!([]));
}

#[test]
fn pause_holds_a_job_and_carry_on_lets_it_go() {
    let _turn = one_at_a_time();
    let _after = CarryOnAfter;
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro(dir.path());
    let named = keevoro.start_backup("Card photos").unwrap();
    let card = card_in(dir.path());

    keevoro.pause();
    let job = keevoro.look(&named, &card, None).unwrap();
    // Something that does not happen cannot be waited for, only given
    // time to happen in.
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(keevoro.jobs(None).len(), 1, "still running");
    assert!(keevoro.jobs(None)[0].paused);
    assert!(Backup::open(&named.home).unwrap().sources().unwrap().is_empty());

    keevoro.carry_on();
    assert!(matches!(keevoro.wait(job.id), Some(Said::Scan(e)) if !e.stopped));
    keevoro.wait_for_jobs();
    assert_eq!(Backup::open(&named.home).unwrap().sources().unwrap().len(), 1);
    let log = keevoro.log_tail(20);
    assert!(log.contains("Paused: all heavy work is waiting"), "{log}");
    assert!(
        log.contains("Resumed: work is carrying on from where it paused"),
        "{log}"
    );
}

#[test]
fn a_second_job_that_writes_is_refused_while_the_first_runs() {
    let _turn = one_at_a_time();
    let _after = CarryOnAfter;
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro(dir.path());
    let (named, card, _usb) = card_going_to_usb(dir.path(), &keevoro);
    let other = keevoro.start_backup("Drone").unwrap();

    keevoro.pause();
    keevoro.look_again(&named, None, None).unwrap();
    for refused in [
        keevoro.look(&named, &card, None),
        keevoro.look_again(&named, None, None),
    ] {
        let e = refused.unwrap_err();
        assert_eq!(
            (e.kind, e.message.as_str()),
            (
                Kind::Busy,
                "Keevoro is already looking through a source. Let that finish first."
            )
        );
    }
    // A backup may run beside a search, as it always could.
    keevoro.run_backup(&named, None).unwrap();
    for refused in [keevoro.run_backup(&named, None), keevoro.check_backup(&named, None)] {
        let e = refused.unwrap_err();
        assert_eq!(
            (e.kind, e.message.as_str()),
            (
                Kind::Busy,
                "Keevoro is already working on your backup. Let that finish first."
            )
        );
    }
    // Another backup is not held up, and neither is a survey.
    keevoro.look(&other, &card, None).unwrap();
    keevoro.survey(&card, Some(&named), None).unwrap();
    assert!(keevoro.busy());
    let e = keevoro.reset().unwrap_err();
    assert_eq!(e.kind, Kind::Busy);
    assert_eq!(
        e.message,
        "Keevoro is in the middle of something. Let that finish, or stop it, before starting again."
    );

    keevoro.carry_on();
    keevoro.wait_for_jobs();
    assert!(!keevoro.busy());
    assert_eq!(there(&named), 3, "the backup that ran beside the search finished");
    // With nothing running, the next one starts.
    let again = keevoro.check_backup(&named, None).unwrap();
    assert!(matches!(keevoro.wait(again.id), Some(Said::Send(e)) if e.problem.is_none()));
    keevoro.wait_for_jobs();
}

#[test]
fn a_backup_goes_to_a_folder_saying_each_step_and_records_how_it_went() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro(dir.path());
    let (named, _card, usb) = card_going_to_usb(dir.path(), &keevoro);
    let heard = Heard::default();

    let job = keevoro.run_backup(&named, heard.listener()).unwrap();
    assert_eq!((job.job, job.op), (JobKind::BackUp, Some("backup.run")));
    let Some(Said::Send(last)) = keevoro.wait(job.id) else {
        panic!("a backup ends with what it said")
    };
    keevoro.wait_for_jobs();
    assert_eq!((last.problem.as_deref(), last.outcome), (None, Some(Outcome::Worked)));
    assert!(
        last.summary.iter().any(|l| l.starts_with("Safely backed up 3 files")),
        "{last:?}"
    );

    let lines = heard.lines();
    assert_eq!(
        lines[0], "Getting ready: reading what Keevoro knows about this backup",
        "said at once"
    );
    assert!(
        lines.iter().any(|l| l.starts_with("3 of 3 safely backed up")),
        "{lines:?}"
    );
    assert_eq!(heard.said().last(), Some(&Said::Send(last.clone())));

    let backup = Backup::open(&named.home).unwrap();
    let history = backup.history().unwrap();
    assert_eq!(history.len(), 1);
    assert!(!history[0].stopped && history[0].bytes > 0);
    assert_eq!(history[0].to, format!("the folder {}", usb.display()));
    // Everything measured is kept beside it, for a person to look back on.
    let detail = history[0].detail.as_ref().expect("the whole of it is recorded");
    assert_eq!(detail.ended, keevoro_engine::history::Ended::Finished);
    assert_eq!(
        (detail.started_by.as_str(), detail.version.as_str()),
        ("the app", env!("CARGO_PKG_VERSION"))
    );
    assert!(detail.started_at <= history[0].finished_at);
    assert_eq!(detail.summary, last.summary, "the summary exactly as it was said");
    assert_eq!(detail.destinations.len(), 1);
    let at = &detail.destinations[0];
    assert!(at.reached && !at.over_network && at.problem.is_none());
    assert_eq!((at.originals.sent, at.originals.already_there), (3, 0));
    assert!(at.originals.sent_bytes > 0 && at.originals.not_sent.is_empty());
    assert_eq!(
        at.previews.as_ref().map(|p| p.sent),
        Some(6),
        "a thumb and a display copy each"
    );
    assert_eq!(at.indexed, Some(3));
    assert_eq!(
        detail.previews.as_ref().map(|p| (p.photos, p.photos_without)),
        Some((3, 0))
    );
    let stages: Vec<_> = detail.stages.iter().map(|s| s.kind).collect();
    use keevoro_core::estimate::Kind as Stage;
    for stage in [
        Stage::Placing,
        Stage::SendingOriginals,
        Stage::MakingPreviews,
        Stage::SendingPreviews,
        Stage::WritingIndex,
    ] {
        assert!(stages.contains(&stage), "{stage:?} in {stages:?}");
    }
    let sending = detail
        .stages
        .iter()
        .find(|s| s.kind == Stage::SendingOriginals)
        .unwrap();
    assert_eq!(sending.done, at.originals.sent_bytes);
    assert!(sending.finished);
    // One of them, by its number, newest first.
    let one = call(&keevoro, Op::BackupHistory, Some("Card photos"), json!({ "run": 1 })).unwrap();
    assert_eq!(one[0]["detail"]["destinations"][0]["originals"]["sent"], 3);
    let e = call(&keevoro, Op::BackupHistory, Some("Card photos"), json!({ "run": 2 })).unwrap_err();
    assert_eq!(e.kind, Kind::NotFound);
    assert_eq!(
        backup.last_summary().unwrap(),
        Some(serde_json::to_value(&last).unwrap()),
        "what it said is kept with the backup"
    );
    assert_eq!(there(&named), 3);
    assert!(keevoro
        .log_tail(5)
        .contains("Backup job finished: Safely backed up 3 files"));
    assert_eq!(keevoro.backup_step(), "");

    // Checking confirms it, and records nothing.
    let heard = Heard::default();
    let check = keevoro.check_backup(&named, heard.listener()).unwrap();
    assert_eq!(check.op, Some("backup.check"));
    let Some(Said::Send(checked)) = keevoro.wait(check.id) else {
        panic!("a check ends with what it found")
    };
    keevoro.wait_for_jobs();
    assert_eq!(
        (checked.problem.as_deref(), checked.outcome),
        (None, Some(Outcome::Worked))
    );
    assert!(!checked.summary.is_empty());
    assert_eq!(heard.lines()[0], "Getting ready to check");
    assert_eq!(backup.history().unwrap().len(), 1);
    assert_eq!(
        backup.last_summary().unwrap(),
        Some(serde_json::to_value(&last).unwrap())
    );
}

#[test]
fn a_backup_stopped_part_way_keeps_what_it_sent_and_carries_on_when_run_again() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro(dir.path());
    let (named, _card, _usb) = card_going_to_usb(dir.path(), &keevoro);
    Backup::open(&named.home).unwrap().set_previews(false).unwrap();

    // Stopped the moment it says the first file is safe.
    let (jobs, log) = (keevoro.jobs.clone(), keevoro.log_shared());
    let stopper: Listener = Arc::new(move |event: &Event| {
        if let Said::Send(SendEvent { line: Some(line), .. }) = &event.said {
            if line.starts_with("1 of 3 safely backed up") {
                jobs.stop(Which::Job(event.job), &log);
            }
        }
    });
    let job = keevoro.run_backup(&named, Some(stopper)).unwrap();
    let Some(Said::Send(last)) = keevoro.wait(job.id) else {
        panic!("a stopped backup still says how it ended")
    };
    keevoro.wait_for_jobs();
    assert_eq!((last.problem.as_deref(), last.outcome), (None, Some(Outcome::Stopped)));
    assert!(last.summary.iter().any(|l| l.starts_with("Stopped.")), "{last:?}");
    assert_eq!(there(&named), 1, "what was sent is kept");
    let backup = Backup::open(&named.home).unwrap();
    let stopped = &backup.history().unwrap()[0];
    assert!(stopped.stopped);
    let detail = stopped.detail.as_ref().unwrap();
    assert_eq!(detail.ended, keevoro_engine::history::Ended::Stopped);
    assert_eq!(detail.destinations[0].originals.sent, 1);
    assert_eq!(detail.previews, None, "this backup sends the originals alone");
    assert!(keevoro
        .log_tail(10)
        .contains("Asked to stop sending after the file in hand"));

    let again = keevoro.run_backup(&named, None).unwrap();
    let Some(Said::Send(last)) = keevoro.wait(again.id) else {
        panic!()
    };
    keevoro.wait_for_jobs();
    assert!(
        last.summary.iter().any(|l| l.starts_with("Safely backed up 2 files")),
        "it carries on from where it was: {last:?}"
    );
    assert_eq!(there(&named), 3);
    assert_eq!(backup.history().unwrap().len(), 2);
}

#[test]
fn a_backup_stopped_while_it_is_getting_ready_says_it_was_stopped() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro(dir.path());
    let (named, _card, usb) = card_going_to_usb(dir.path(), &keevoro);

    // Stopped the moment it says it is getting ready, before anything else.
    let (jobs, log) = (keevoro.jobs.clone(), keevoro.log_shared());
    let stopper: Listener = Arc::new(move |event: &Event| {
        if let Said::Send(SendEvent { line: Some(line), .. }) = &event.said {
            if line.starts_with("Getting ready") {
                jobs.stop(Which::Job(event.job), &log);
            }
        }
    });
    let job = keevoro.run_backup(&named, Some(stopper)).unwrap();
    let Some(Said::Send(last)) = keevoro.wait(job.id) else {
        panic!("a backup stopped early still says how it ended")
    };
    keevoro.wait_for_jobs();
    assert_eq!(last.problem, None);
    assert_eq!(last.outcome, Some(Outcome::Stopped));
    assert_eq!(last.summary, [keevoro_engine::send::STOPPED], "not an empty message");
    assert!(!usb.join("originals").exists(), "nothing was sent");
    assert_eq!(
        Backup::open(&named.home).unwrap().last_summary().unwrap(),
        Some(serde_json::to_value(&last).unwrap())
    );
}

#[test]
fn a_stopped_summary_is_never_given_while_a_backup_runs_however_it_is_asked() {
    let _turn = one_at_a_time();
    let _after = CarryOnAfter;
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro(dir.path());
    let (named, _card, _usb) = card_going_to_usb(dir.path(), &keevoro);
    // What a stopped backup left behind.
    Backup::open(&named.home)
        .unwrap()
        .remember_last_summary(&SendEvent::finished(
            format!("{}\n", keevoro_engine::send::STOPPED).as_bytes(),
            None,
            Outcome::Stopped,
        ))
        .unwrap();

    keevoro.pause();
    let job = keevoro.run_backup(&named, None).unwrap();
    // Asked the way the app asks on coming back to the screen, the way the
    // command line and an agent ask, and from another program.
    let other = self::keevoro(dir.path());
    for asked in [
        Backup::open(&named.home).unwrap().last_summary().unwrap(),
        Some(
            crate::call(
                &keevoro,
                Op::named("backup.last_summary").unwrap(),
                Some("Card photos"),
                json!(null),
            )
            .unwrap(),
        )
        .filter(|v| !v.is_null()),
        Some(
            crate::call(
                &other,
                Op::named("backup.last_summary").unwrap(),
                Some("Card photos"),
                json!(null),
            )
            .unwrap(),
        )
        .filter(|v| !v.is_null()),
    ] {
        assert_eq!(asked, None, "nothing from before is given while it runs");
    }
    let status = keevoro.jobs(Some(&named.id));
    assert!(!status[0].line.starts_with("Stopped"), "{}", status[0].line);

    keevoro.carry_on();
    let Some(Said::Send(last)) = keevoro.wait(job.id) else {
        panic!("it says how it ended")
    };
    keevoro.wait_for_jobs();
    assert_eq!(last.outcome, Some(Outcome::Worked));
    assert_eq!(
        Backup::open(&named.home).unwrap().last_summary().unwrap(),
        Some(serde_json::to_value(&last).unwrap())
    );
    // How fast it went is remembered for the next estimate, in Keevoro's
    // own data.
    let remembered = crate::rates::remembered(&keevoro.rates_file());
    assert!(remembered.preview_bytes_per_original.is_some(), "{remembered:?}");
    assert!(keevoro.rates_file().starts_with(dir.path()));
}

#[test]
fn a_destination_out_of_reach_is_said_in_a_word_and_in_a_sentence() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro(dir.path());
    let (named, _card, usb) = card_going_to_usb(dir.path(), &keevoro);
    // Where the folder was, there is now a file: it cannot be backed up to.
    std::fs::remove_dir_all(&usb).unwrap();
    std::fs::write(&usb, "in the way").unwrap();
    for start in [Keevoro::run_backup, Keevoro::check_backup] {
        let job = start(&keevoro, &named, None).unwrap();
        let Some(Said::Send(last)) = keevoro.wait(job.id) else {
            panic!("it says how it ended")
        };
        keevoro.wait_for_jobs();
        assert_eq!(last.outcome, Some(Outcome::Unreachable), "{last:?}");
        assert!(last.problem.is_some(), "{last:?}");
        assert!(last.summary.iter().any(|l| l.starts_with("Couldn't")), "{last:?}");
    }
}

#[test]
fn a_place_that_is_not_there_is_not_looked_through() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro(dir.path());
    let named = keevoro.start_backup("Card photos").unwrap();
    let nowhere = dir.path().join("Unplugged");
    for refused in [
        keevoro.look(&named, &nowhere, None),
        keevoro.survey(&nowhere, None, None),
    ] {
        let e = refused.unwrap_err();
        assert_eq!(e.kind, Kind::NotFound);
        assert!(e.message.contains("Is it plugged in?"), "{e}");
    }
    assert!(keevoro.jobs(None).is_empty());
}

#[test]
fn the_step_a_backup_is_on_is_noted_and_cleared() {
    let _turn = one_at_a_time();
    let _after = CarryOnAfter;
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro(dir.path());
    let (named, _card, _usb) = card_going_to_usb(dir.path(), &keevoro);
    assert_eq!(keevoro.activity().backup_step, None);

    keevoro.pause();
    keevoro.run_backup(&named, None).unwrap();
    assert!(!keevoro.backup_step().is_empty());
    assert!(keevoro.activity().backup_step.is_some());
    let status = keevoro.jobs(Some(&named.id));
    assert_eq!(status[0].line, keevoro.backup_step());
    let estimate = status[0].estimate.as_ref().expect("a backup has an estimate");
    assert!(
        estimate.headline.starts_with("Running for less than a minute"),
        "{}",
        estimate.headline
    );

    keevoro.carry_on();
    keevoro.wait_for_jobs();
    assert_eq!(keevoro.activity().backup_step, None);
}

#[test]
fn looking_again_goes_through_every_source_for_what_is_new() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro(dir.path());
    let named = keevoro.start_backup("Card photos").unwrap();
    let e = keevoro.look_again(&named, None, None).unwrap_err();
    assert_eq!(
        (e.kind, e.message.as_str()),
        (
            Kind::Refused,
            "This backup has no sources to look through again. Add one first."
        )
    );

    let card = card_looked_through_into(dir.path(), &named.home);
    let phone = dir.path().join("Phone");
    std::fs::create_dir_all(&phone).unwrap();
    std::fs::write(phone.join("IMG_20190101_120000.jpg"), large_photo(30)).unwrap();
    let job = keevoro.look(&named, &phone, None).unwrap();
    keevoro.wait(job.id);
    keevoro.wait_for_jobs();
    std::fs::write(card.join("DCIM/IMG_20200101_120000.jpg"), large_photo(200)).unwrap();
    std::fs::write(phone.join("IMG_20190102_120000.jpg"), large_photo(110)).unwrap();

    let heard = Heard::default();
    let job = keevoro.look_again(&named, None, heard.listener()).unwrap();
    assert_eq!(job.op, Some("sources.look_again"));
    keevoro.wait(job.id);
    keevoro.wait_for_jobs();
    let ends: Vec<String> = heard
        .said()
        .into_iter()
        .filter_map(|s| match s {
            Said::Scan(e) if e.finished => Some(e.name),
            _ => None,
        })
        .collect();
    assert_eq!(ends, ["Card", "Phone"], "one after another");
    assert_eq!(Backup::open(&named.home).unwrap().found().unwrap().photos, 6);

    let sources = Backup::open(&named.home).unwrap().sources().unwrap();
    let one = keevoro.look_again(&named, Some(sources[1].id), None).unwrap();
    keevoro.wait(one.id);
    keevoro.wait_for_jobs();
    let e = keevoro.look_again(&named, Some(9999), None).unwrap_err();
    assert_eq!(
        (e.kind, e.message.as_str()),
        (Kind::NotFound, "That source isn't part of this backup.")
    );
}

#[test]
fn the_check_after_an_agents_search_reads_only_the_folders_that_agent_may_look_in() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let agent = keevoro(dir.path()).as_front_end(FrontEnd::Agent);
    let named = agent.start_backup("Card photos").unwrap();
    let card = card_in(dir.path());
    std::fs::write(card.join("DCIM/IMG_20200101_120000.jpg"), large_photo(200)).unwrap();
    // A source the person added, with a large photo only sampled so far.
    let phone = dir.path().join("Phone");
    std::fs::create_dir_all(&phone).unwrap();
    std::fs::write(phone.join("IMG_20190101_120000.jpg"), large_photo(30)).unwrap();
    let mut catalog = Catalog::open_in_library(&named.home).unwrap();
    let quick = keevoro_scan::ScanOptions {
        quick: true,
        ..Default::default()
    };
    keevoro_engine::scan_into_with(&mut catalog, &phone, Some("Phone"), &quick, &mut |_| {}, &mut || {}).unwrap();
    drop(catalog);
    agent
        .set_agent_access(crate::agents::AgentsAccessRequest {
            look_in: Some(vec![card.display().to_string()]),
            back_up_to: None,
            may_change: Some(true),
        })
        .unwrap();
    let unchecked = || {
        let left = Catalog::open_in_library(&named.home).unwrap().unhashed().unwrap();
        left.into_iter().map(|f| f.path).collect::<Vec<_>>()
    };

    let job = agent.look(&named, &card, None).unwrap();
    agent.wait(job.id);
    agent.wait_for_jobs();
    let left = unchecked();
    assert_eq!(left.len(), 1, "{left:?}");
    assert!(left[0].starts_with(std::fs::canonicalize(&phone).unwrap()), "{left:?}");

    // A check started any other way reads what was left.
    let person = keevoro(dir.path());
    person.keep_checking(&named).unwrap();
    person.wait_for_jobs();
    assert!(unchecked().is_empty());
}

#[test]
fn a_survey_finds_the_folders_with_photos_and_can_be_stopped() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro(dir.path());
    let card = card_in(dir.path());
    let heard = Heard::default();

    let job = keevoro.survey(&card, None, heard.listener()).unwrap();
    assert_eq!(
        (job.job, job.op, job.backup.clone()),
        (JobKind::Survey, Some("places.survey"), None)
    );
    let Some(Said::Surveyed(found)) = keevoro.wait(job.id) else {
        panic!("a survey ends with what it found")
    };
    assert!(!found.stopped);
    assert_eq!(found.files, 5);
    assert!(found.folders >= 4, "{found:?}");
    assert!(!found.hints.is_empty());
    assert!(heard
        .said()
        .iter()
        .any(|s| matches!(s, Said::Survey(p) if p.files == 5 && p.folders == found.folders)));
    assert!(keevoro.log_tail(2).contains("Looked at 5 files in"));

    // Stopped after it had seen everything: nothing is said to be missing.
    let (jobs, log) = (keevoro.jobs.clone(), keevoro.log_shared());
    let stopper: Listener = Arc::new(move |event: &Event| {
        if matches!(event.said, Said::Survey(_)) {
            jobs.stop(Which::Job(event.job), &log);
        }
    });
    let job = keevoro.survey(&card, None, Some(stopper)).unwrap();
    let Some(Said::Surveyed(found)) = keevoro.wait(job.id) else {
        panic!()
    };
    assert!(!found.stopped, "it had seen everything before it was told to stop");

    // Stopped part-way: held by Pause before it lists a folder, then stopped.
    let _after = CarryOnAfter;
    keevoro.pause();
    let job = keevoro.survey(&card, None, None).unwrap();
    keevoro.jobs.stop(Which::Job(job.id), &keevoro.log_shared());
    keevoro.carry_on();
    let Some(Said::Surveyed(found)) = keevoro.wait(job.id) else {
        panic!()
    };
    assert!(found.stopped);
    assert!(keevoro.log_tail(2).contains("(stopped early)"));
    // What it saw is remembered as partly mapped.
    let map = keevoro.space_map(&crate::space::MapRequest::of(&card), None).unwrap();
    assert!(!map.complete && !map.sized_all, "{map:?}");
    let outline = keevoro.folders_inside(&card, None).unwrap();
    assert!(!outline.complete);
    keevoro.wait_for_jobs();
}

#[test]
fn the_check_for_duplicates_runs_after_a_search_and_gives_way() {
    let _turn = one_at_a_time();
    let _after = CarryOnAfter;
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro(dir.path());
    let named = keevoro.start_backup("Card photos").unwrap();
    let card = card_in(dir.path());
    std::fs::write(card.join("DCIM/IMG_20200101_120000.jpg"), large_photo(200)).unwrap();
    let unchecked = || Catalog::open_in_library(&named.home).unwrap().unhashed().unwrap().len();
    let heard = Heard::default();
    keevoro.listen({
        let heard = heard.clone();
        move |event| lock(&heard.0).push(event.clone())
    });

    // A search samples the large photo; the check that follows reads it.
    let job = keevoro.look(&named, &card, None).unwrap();
    keevoro.wait(job.id);
    keevoro.wait_for_jobs();
    assert_eq!(unchecked(), 0);
    assert!(
        heard.said().iter().any(|s| matches!(s, Said::Check(e) if e.finished)),
        "Keevoro's own listener heard the check it started"
    );
    assert!(keevoro.log_tail(10).contains("Checking 1 files in the background"));

    // A photo found outside any job is left sampled for the check to read.
    std::fs::write(card.join("DCIM/IMG_20200102_120000.jpg"), large_photo(220)).unwrap();
    let mut catalog = Catalog::open_in_library(&named.home).unwrap();
    let quick = keevoro_scan::ScanOptions {
        quick: true,
        ..Default::default()
    };
    keevoro_engine::scan_into_with(&mut catalog, &card, None, &quick, &mut |_| {}, &mut || {}).unwrap();
    drop(catalog);
    assert_eq!(unchecked(), 1);

    // Only one runs at a time. Held by the pause, the first is still
    // running when a second is asked for.
    keevoro.pause();
    let first = keevoro.keep_checking(&named).unwrap();
    assert_eq!(first.job, JobKind::Duplicates);
    assert_eq!(first.op, None, "it is Keevoro's own, not an operation");
    assert!(keevoro.keep_checking(&named).is_none());

    // It gives way to a search: one started while it waits makes it stop
    // as soon as it carries on, and the photo is left for later. The search
    // is held at the first thing it says until the check has given way.
    let (release, held) = std::sync::mpsc::channel::<()>();
    let held = Mutex::new(held);
    let once = AtomicBool::new(true);
    let hold: Listener = Arc::new(move |_: &Event| {
        if once.swap(false, Ordering::SeqCst) {
            let _ = lock(&held).recv_timeout(Duration::from_secs(30));
        }
    });
    let job = keevoro.look(&named, &card, Some(hold)).unwrap();
    keevoro.carry_on();
    keevoro.wait(first.id);
    let log = keevoro.log_tail(40);
    assert!(log.contains("Background checking paused after"), "{log}");
    release.send(()).unwrap();
    keevoro.wait(job.id);
    keevoro.wait_for_jobs();
    assert_eq!(unchecked(), 0, "the search starts it again, and it carries on");
}

#[test]
fn jobs_are_reached_by_name_with_json() {
    let _turn = one_at_a_time();
    let _after = CarryOnAfter;
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro(dir.path());
    let k = &keevoro;
    let named = keevoro.start_backup("Card photos").unwrap();
    let card = card_in(dir.path()).display().to_string();

    let started = call(k, Op::SourcesLook, Some("card photos"), json!({ "place": card })).unwrap();
    assert_eq!(started["job"], "look");
    assert_eq!(started["op"], "sources.look");
    assert_eq!(started["backup"], json!({ "id": named.id, "name": "Card photos" }));
    assert_eq!(started["startedBy"], "app");
    let id = started["id"].clone();
    let heard = call(k, Op::JobsFollow, None, json!({ "job": id })).unwrap();
    let heard = heard.as_array().unwrap();
    assert!(!heard.is_empty(), "following waits and gives what was said");
    let last = heard.last().unwrap();
    assert_eq!(last["event"], "scan");
    assert_eq!(last["job"], id);
    assert_eq!(last["said"]["finished"], true);
    keevoro.wait_for_jobs();

    assert_eq!(call(k, Op::JobsStatus, None, json!({})).unwrap(), json!([]));
    assert_eq!(call(k, Op::JobsFollow, None, Value::Null).unwrap(), json!([]));
    let e = call(k, Op::JobsStop, None, json!({ "job": 999 })).unwrap_err();
    assert_eq!(
        (e.kind, e.message.as_str()),
        (Kind::NotFound, "There is no job 999 running.")
    );
    assert_eq!(call(k, Op::JobsStop, None, json!({})).unwrap(), json!([]));
    let e = call(k, Op::SourcesLook, None, json!({})).unwrap_err();
    assert_eq!(e.kind, Kind::NotUnderstood);
    assert!(e.message.contains("place"), "{e}");

    assert_eq!(
        call(k, Op::JobsPause, None, Value::Null).unwrap(),
        json!({ "paused": true })
    );
    let started = call(k, Op::BackupRun, None, Value::Null).unwrap();
    let status = call(k, Op::JobsStatus, Some("Card photos"), json!({})).unwrap();
    assert_eq!(status[0]["op"], "backup.run");
    assert_eq!(status[0]["paused"], true);
    assert_eq!(
        call(k, Op::JobsStop, Some("Card photos"), json!({})).unwrap(),
        json!([started["id"]])
    );
    assert_eq!(
        call(k, Op::JobsCarryOn, None, Value::Null).unwrap(),
        json!({ "paused": false })
    );
    keevoro.wait_for_jobs();

    let survey = call(k, Op::PlacesSurvey, None, json!({ "place": card })).unwrap();
    assert_eq!(survey["op"], "places.survey");
    assert_eq!(
        survey["backup"]["name"], "Card photos",
        "the open backup, if there is one"
    );
    let found = keevoro.wait(serde_json::from_value(survey["id"].clone()).unwrap());
    assert!(matches!(found, Some(Said::Surveyed(view)) if view.hints.iter().any(|h| h.already_a_source)));
    let check = call(k, Op::BackupCheck, None, json!({})).unwrap();
    assert_eq!(check["op"], "backup.check");
    let again = call(k, Op::SourcesLookAgain, None, json!({})).unwrap();
    assert_eq!(again["op"], "sources.look_again");
    keevoro.wait_for_jobs();
}

/// Waits for something another program does in its own time, for a
/// generous while, looking every so often.
fn eventually<T>(what: &str, found: impl Fn() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(it) = found() {
            return it;
        }
        assert!(Instant::now() < deadline, "{what} never happened");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A second Keevoro in the same folder is a second program, as far as
/// sharing jobs goes: the command line, beside the app. It reads the
/// settings as it opens, so it is opened once the backups are started. (Pausing is one switch for the
/// whole process, so passing it on is tested with real programs, in
/// `tests/across_programs.rs`.)
fn command_line(dir: &Path) -> Keevoro {
    keevoro(dir).as_front_end(FrontEnd::CommandLine)
}

#[test]
fn a_job_another_program_has_just_started_is_seen_at_once() {
    let _turn = one_at_a_time();
    let _after = CarryOnAfter;
    let dir = tempfile::tempdir().unwrap();
    let app = keevoro(dir.path());
    let named = app.start_backup("Card photos").unwrap();
    let card = card_in(dir.path());
    let command_line = command_line(dir.path());

    app.pause();
    let job = app.look(&named, &card, None).unwrap();
    // Asked straight away, not after the other program's next tick.
    let seen = command_line.jobs(None);
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(seen[0].handle, job);
    assert!(seen[0].elsewhere);
    app.stop(Which::All);
    app.carry_on();
    app.wait_for_jobs();
}

#[test]
fn a_job_in_another_program_is_seen_refused_followed_waited_for_and_stopped() {
    let _turn = one_at_a_time();
    let _after = CarryOnAfter;
    let dir = tempfile::tempdir().unwrap();
    let app = keevoro(dir.path());
    let named = app.start_backup("Card photos").unwrap();
    let card = card_in(dir.path());
    let command_line = command_line(dir.path());

    // The app looks through the card, held before its first file.
    app.pause();
    let job = app.look(&named, &card, None).unwrap();
    assert!(command_line.busy(), "busy, in another program");
    assert!(!command_line.busy_here());
    assert!(app.busy_here());

    // The command line sees it, as the app last shared it.
    let seen = eventually("the app's job seen", || command_line.jobs(Some(&named.id)).pop());
    assert_eq!(seen.handle, job);
    assert!(seen.elsewhere && seen.paused && !seen.stopping);
    assert_eq!(seen.step, "Looking through Card");
    assert!(command_line.jobs(Some("another backup")).is_empty());
    assert!(!app.jobs(None)[0].elsewhere, "the app's own job is not elsewhere to it");
    let json = call(&command_line, Op::JobsStatus, Some("Card photos"), json!({})).unwrap();
    assert_eq!(json[0]["elsewhere"], true);
    assert_eq!(json[0]["startedBy"], "app");

    // Starting work of its own on that backup is refused, saying what and
    // where; another backup is free.
    for refused in [
        command_line.run_backup(&named, None).unwrap_err(),
        command_line.look(&named, &card, None).unwrap_err(),
        command_line.check_backup(&named, None).unwrap_err(),
    ] {
        assert_eq!(
            (refused.kind, refused.message.as_str()),
            (
                Kind::Busy,
                "Keevoro is already looking through a source for \"Card photos\" in the Keevoro app. Let that finish, or stop it, first."
            )
        );
    }
    assert!(
        command_line.keep_checking(&named).is_none(),
        "the check for duplicates leaves it to the app"
    );

    // It follows the job, and stops it from outside.
    let heard = Heard::default();
    assert!(command_line.follow(job.id, heard.listener().unwrap()));
    assert!(!command_line.follow(JobId(1), heard.listener().unwrap()), "no such job");
    assert_eq!(command_line.stop(Which::OnBackup(&named.id)), [job.id]);
    let Some(Said::Scan(last)) = command_line.wait(job.id) else {
        panic!("waiting from outside hears how it ended")
    };
    assert!(last.finished && last.stopped, "{last:?}");
    eventually("the follower told the end", || {
        heard.said().contains(&Said::Scan(last.clone())).then_some(())
    });
    assert!(lock(&heard.0).iter().all(|e| e.job == job.id && e.event == "scan"));
    app.wait_for_jobs();
    assert!(
        app.log_tail(20).contains("Asked to stop the search"),
        "the app says why it stopped"
    );

    // Once the app has finished, the command line may go ahead.
    assert!(!command_line.busy());
    assert_eq!(command_line.wait(JobId(1)), None, "a job never heard of");
    command_line.pause();
    let own = command_line.look(&named, &card, None).unwrap();
    assert_eq!(own.started_by, FrontEnd::CommandLine);
    assert_ne!(own.id, job.id);
    let seen = eventually("the command line's job seen by the app", || {
        app.jobs(None).into_iter().find(|s| s.handle.id == own.id)
    });
    assert!(seen.elsewhere);
    app.carry_on();
    // Waiting for it from the app hears how it ended.
    assert!(matches!(app.wait(own.id), Some(Said::Scan(e)) if e.finished && !e.stopped));
    command_line.wait_for_jobs();
    assert!(!app.busy());
}

/// Starts a search in `app` and waits until it is held, once its last
/// file is done, where it stays until the returned sender is dropped: the
/// moment between a search's end and its letting go of the backup.
///
/// The card gains a large photo, so the check for duplicates that follows
/// has a file to read and waits while paused.
fn a_search_held_at_its_end(app: &Keevoro, named: &Named, card: &Path) -> (JobId, std::sync::mpsc::Sender<()>) {
    std::fs::create_dir_all(card.join("Big")).unwrap();
    std::fs::write(card.join("Big/IMG_20200102_120000.jpg"), large_photo(220)).unwrap();
    let (go, wait) = std::sync::mpsc::channel::<()>();
    let (there, arrived) = std::sync::mpsc::channel::<()>();
    let wait = Mutex::new(Some((wait, there)));
    let listener: Listener = Arc::new(move |event: &Event| {
        if matches!(&event.said, Said::Scan(e) if e.finished) {
            if let Some((wait, there)) = lock(&wait).take() {
                let _ = there.send(());
                let _ = wait.recv();
            }
        }
    });
    let job = app.look(named, card, Some(listener)).unwrap();
    arrived
        .recv_timeout(Duration::from_secs(30))
        .expect("the search reaches its end");
    (job.id, go)
}

/// Waits for every job in `keevoro` to end, for a generous while. False
/// if they never did, which is what a pause nobody can lift looks like.
fn all_end_within_a_while(keevoro: &Keevoro) -> bool {
    let deadline = Instant::now() + Duration::from_secs(30);
    while keevoro.busy_here() || !keevoro.jobs(None).is_empty() {
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    keevoro.wait_for_jobs();
    true
}

#[test]
fn a_pause_another_program_asked_for_is_lifted_when_the_work_it_could_see_ends() {
    let _turn = one_at_a_time();
    let _after = CarryOnAfter;
    let dir = tempfile::tempdir().unwrap();
    let app = keevoro(dir.path());
    let named = app.start_backup("Card photos").unwrap();
    let card = card_in(dir.path());
    let command_line = command_line(dir.path());

    let (job, go) = a_search_held_at_its_end(&app, &named, &card);
    eventually("the app's search seen", || {
        command_line.jobs(None).into_iter().find(|s| s.handle.id == job)
    });
    // The command line asks for a pause just as the search ends. (Only
    // the asking: its own switch is the app's too, in one test process.)
    command_line.jobs.ask_everyone_else(Ask::Pause);
    eventually("the pause heard", || {
        (keevoro_core::pause::paused_by() == Some(keevoro_core::pause::PausedBy::AnotherProgram)).then_some(())
    });
    drop(go);

    // The search ends and lets go, so the command line can no longer see
    // the app or ask it to carry on: the pause goes with it, and the
    // check for duplicates that follows is not left waiting for nobody.
    assert!(all_end_within_a_while(&app), "nothing is left sitting paused");
    assert!(!keevoro_core::pause::is_paused());
    assert!(
        app.log_tail(30)
            .contains("the work another Keevoro program paused here has ended, so its pause is lifted"),
        "{}",
        app.log_tail(30)
    );
}

#[test]
fn the_persons_own_pause_outlasts_the_work_and_is_still_said() {
    let _turn = one_at_a_time();
    let _after = CarryOnAfter;
    let dir = tempfile::tempdir().unwrap();
    let app = keevoro(dir.path());
    let named = app.start_backup("Card photos").unwrap();
    let card = card_in(dir.path());

    let (job, go) = a_search_held_at_its_end(&app, &named, &card);
    app.pause();
    // Another program asking too does not make it the other program's.
    app.jobs.asked(&across::requests::Request {
        to: String::new(),
        ask: Ask::Pause,
        jobs: Vec::new(),
        by: FrontEnd::CommandLine,
        at: 0,
    });
    drop(go);
    app.wait(job);

    // Only the check for duplicates is left, and it waits; asked, Keevoro
    // says so rather than that nothing is running.
    let left = eventually("the check for duplicates waiting", || {
        app.jobs(None).into_iter().find(|s| s.handle.job == JobKind::Duplicates)
    });
    assert!(left.paused);
    assert!(keevoro_core::pause::is_paused(), "the person's pause stays");
    assert!(app.activity().paused, "and the app is told it is paused");
    let status = call(&app, Op::JobsStatus, None, json!({})).unwrap();
    assert_eq!(status[0]["job"], "duplicates", "{status}");
    assert_eq!(status[0]["paused"], true);

    app.carry_on();
    assert!(all_end_within_a_while(&app));
}

#[test]
fn a_search_and_a_survey_pass_by_a_folder_left_out_and_say_so() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro(dir.path());
    let named = keevoro.start_backup("Card photos").unwrap();
    let card = std::fs::canonicalize(card_in(dir.path())).unwrap();
    let holidays = card.join("Holidays");
    Backup::open(&named.home).unwrap().leave_out(&holidays, true).unwrap();
    // Asked to look through it, or a folder in it, Keevoro says why not.
    let refused = keevoro.look(&named, &holidays.join("Cornwall"), None).unwrap_err();
    assert_eq!(refused.kind, Kind::Refused);
    assert!(refused.message.contains("Put it back first"), "{refused}");

    let job = keevoro.look(&named, &card, None).unwrap();
    let Some(Said::Scan(found)) = keevoro.wait(job.id) else {
        panic!("the search ends")
    };
    let ours: Vec<_> = found.left_out.iter().filter(|l| l.what == "leftOutByYou").collect();
    assert_eq!(ours.len(), 1, "{:?}", found.left_out);
    assert!(ours[0].said.contains("as you chose"), "{}", ours[0].said);
    let backup = Backup::open(&named.home).unwrap();
    let paths: Vec<PathBuf> = backup
        .catalog()
        .load_reports()
        .unwrap()
        .iter()
        .flat_map(|r| r.items.iter().map(|i| i.path.clone()))
        .collect();
    assert!(
        !paths.is_empty() && paths.iter().all(|p| !p.starts_with(&holidays)),
        "{paths:?}"
    );

    let job = keevoro.survey(&card, Some(&named), None).unwrap();
    let Some(Said::Surveyed(view)) = keevoro.wait(job.id) else {
        panic!("the survey ends")
    };
    assert!(
        view.left_out.iter().any(|l| l.what == "leftOutByYou"),
        "{:?}",
        view.left_out
    );
    assert!(view.hints.iter().all(|h| !h.folder.contains("Holidays")));
    keevoro.wait_for_jobs();
}

#[test]
fn stopping_by_kind_on_one_backup_leaves_the_rest_running() {
    let _turn = one_at_a_time();
    let _after = CarryOnAfter;
    let dir = tempfile::tempdir().unwrap();
    let app = keevoro(dir.path());
    let card_photos = app.start_backup("Card photos").unwrap();
    let drone = app.start_backup("Drone").unwrap();
    let card = card_in(dir.path());
    let command_line = command_line(dir.path());

    app.pause();
    let mine = app.look(&card_photos, &card, None).unwrap();
    let theirs = command_line.look(&drone, &card, None).unwrap();
    eventually("the command line's job seen", || {
        app.jobs(Some(&drone.id)).into_iter().find(|s| s.elsewhere)
    });
    // Stopping searches on one backup asks only the program running that
    // one. (Stopping lifts the pause, so the app's own search then goes
    // on to the end.)
    assert_eq!(app.stop(Which::KindsOn(&[JobKind::Look], &drone.id)), [theirs.id]);
    assert!(matches!(app.wait(theirs.id), Some(Said::Scan(e)) if e.stopped));
    assert!(matches!(app.wait(mine.id), Some(Said::Scan(e)) if !e.stopped));
    app.wait_for_jobs();
    command_line.wait_for_jobs();
}

#[test]
fn reset_waits_while_another_program_is_working() {
    let _turn = one_at_a_time();
    let _after = CarryOnAfter;
    let dir = tempfile::tempdir().unwrap();
    let app = keevoro(dir.path());
    let named = app.start_backup("Card photos").unwrap();
    let card = card_in(dir.path());
    let command_line = command_line(dir.path());
    command_line.pause();
    let job = command_line.look(&named, &card, None).unwrap();
    assert_eq!(app.reset().unwrap_err().kind, Kind::Busy);
    command_line.stop(Which::Job(job.id));
    command_line.wait_for_jobs();
    app.reset().unwrap();
}

#[test]
fn work_can_start_again_the_moment_the_last_job_on_a_backup_is_heard_to_end() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro(dir.path());
    let named = keevoro.start_backup("Card photos").unwrap();
    let card = card_in(dir.path());
    // Waiting for a job used to answer before it had let go of its
    // backup, so starting another straight after could be refused as if
    // another program had it.
    for _ in 0..10 {
        let job = keevoro.look(&named, &card, None).unwrap();
        keevoro.wait(job.id);
        let again = keevoro.look_again(&named, None, None).unwrap();
        keevoro.wait(again.id);
    }
    keevoro.wait_for_jobs();
    let jobs = dir.path().join("data").join("jobs").join(&named.id);
    assert!(!jobs.join("holder.json").exists(), "the lock is let go");
}

#[test]
fn status_can_wait_a_while_and_says_how_a_job_ended_wherever_it_ran() {
    let _turn = one_at_a_time();
    let _after = CarryOnAfter;
    let dir = tempfile::tempdir().unwrap();
    let app = keevoro(dir.path());
    let named = app.start_backup("Card photos").unwrap();
    let card = card_in(dir.path());
    let command_line = command_line(dir.path());

    // Held, it is still running when the short wait is up.
    app.pause();
    let job = app.look(&named, &card, None).unwrap();
    let begun = Instant::now();
    let held = call(&app, Op::JobsStatus, None, json!({ "job": job.id, "waitSeconds": 1 })).unwrap();
    assert!(begun.elapsed() >= Duration::from_millis(900), "it waited");
    assert_eq!(held[0]["ended"], false);
    assert_eq!(held[0]["id"], job.id.0);

    // Let go, the wait ends when the job does, and says how it ended.
    app.carry_on();
    let begun = Instant::now();
    let ended = call(&app, Op::JobsStatus, None, json!({ "job": job.id, "waitSeconds": 30 })).unwrap();
    assert!(
        begun.elapsed() < Duration::from_secs(30),
        "not a moment longer than needed"
    );
    assert_eq!(ended.as_array().map(Vec::len), Some(1), "{ended}");
    assert_eq!(ended[0]["ended"], true);
    assert_eq!(ended[0]["op"], "sources.look");
    assert_eq!(ended[0]["last"]["photos"], 5, "{ended}");
    assert_eq!(ended[0]["elsewhere"], false);

    // Another program hears how it ended too, from what was shared.
    let elsewhere = eventually("the end shared", || command_line.ended_job(job.id).filter(|s| s.ended));
    assert!(elsewhere.elsewhere);
    assert!(matches!(elsewhere.last, Some(Said::Scan(ref e)) if e.finished));

    // Asked about every job, an ended one is not listed; a job never heard
    // of is nothing at all.
    app.wait_for_jobs();
    assert_eq!(call(&app, Op::JobsStatus, None, json!({})).unwrap(), json!([]));
    assert_eq!(
        call(&app, Op::JobsStatus, None, json!({ "job": 7, "waitSeconds": 0 })).unwrap(),
        json!([])
    );
}

#[test]
fn a_program_that_is_closing_stops_its_own_jobs_and_leaves_the_rest() {
    let _turn = one_at_a_time();
    let _after = CarryOnAfter;
    let dir = tempfile::tempdir().unwrap();
    let agent = keevoro(dir.path()).as_front_end(FrontEnd::Agent);
    let named = agent.start_backup("Card photos").unwrap();
    let other = agent.start_backup("Other").unwrap();
    let card = card_in(dir.path());
    let command_line = command_line(dir.path());

    command_line.pause();
    let theirs = command_line.look(&other, &card, None).unwrap();
    let mine = agent.look(&named, &card, None).unwrap();
    assert!(agent.stop_here_and_wait(Duration::from_secs(30)));
    assert!(!agent.busy_here());
    assert!(matches!(agent.wait(mine.id), Some(Said::Scan(e)) if e.stopped));
    // Both programs are one process here and share its pause, which the
    // closing one lifts to wake its own job. So the other's job may have
    // finished by now, and its duplicate check may have begun; what matters
    // is that nothing of the other program's was asked to stop.
    let still = command_line.jobs(None);
    assert!(command_line
        .wait(theirs.id)
        .is_none_or(|said| !matches!(said, Said::Scan(e) if e.stopped)));
    assert!(
        still.iter().all(|job| job.handle.id != mine.id && !job.stopping),
        "the other program's job is left alone: {still:?}"
    );
    command_line.stop(Which::All);
    command_line.wait_for_jobs();
    // With nothing running, it answers at once.
    assert!(agent.stop_here_and_wait(Duration::ZERO));
}

/// Every file under `folder`, by name, with its bytes.
fn files_under(folder: &Path) -> Vec<(String, Vec<u8>)> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(files_under(&path));
        } else {
            found.push((
                entry.file_name().to_string_lossy().into_owned(),
                std::fs::read(&path).unwrap(),
            ));
        }
    }
    found
}

#[test]
fn a_kind_added_that_keevoro_cannot_open_is_found_and_backed_up_exactly_with_no_preview_and_no_fuss() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro(dir.path());
    keevoro.add_file_kind(".XYZ", keevoro_core::MediaKind::Photo).unwrap();
    // Nothing inside either says what it is, and no decoder can open it.
    let pano: Vec<u8> = (0..5_000u32).map(|n| u8::try_from(n * 7 % 251).unwrap()).collect();
    let other: Vec<u8> = (0..3_000u32).map(|n| u8::try_from(n * 13 % 241).unwrap()).collect();
    let card = dir.path().join("Card");
    for (rel, bytes) in [
        ("DCIM/pano.xyz", &pano),
        // A copy in a folder called Icons: with no size known, not set aside.
        ("Icons/pano copy.xyz", &pano),
        ("DCIM/other.xyz", &other),
    ] {
        std::fs::create_dir_all(card.join(rel).parent().unwrap()).unwrap();
        std::fs::write(card.join(rel), bytes).unwrap();
    }

    // "Find where the photos are" counts them.
    let survey = keevoro.survey(&card, None, None).unwrap();
    let Some(Said::Surveyed(found)) = keevoro.wait(survey.id) else {
        panic!("a survey ends with what it found")
    };
    let counted: usize = found.hints.iter().map(|h| h.photos).sum();
    assert_eq!(counted, 3, "{found:?}");

    // Found, told apart from its copy, and not set aside.
    let named = keevoro.start_backup("Panoramas").unwrap();
    let look = keevoro.look(&named, &card, None).unwrap();
    assert!(matches!(keevoro.wait(look.id), Some(Said::Scan(e)) if e.photos == 3 && !e.stopped));
    keevoro.wait_for_jobs();
    let backup = Backup::open(&named.home).unwrap();
    let found = backup.found().unwrap();
    assert_eq!(
        (found.photos, found.duplicates, found.set_aside),
        (2, 1, 0),
        "{found:?}"
    );
    assert_eq!(backup.duplicates().unwrap().len(), 1);
    assert!(backup.set_aside().unwrap().is_empty());
    assert!(!backup.collections().unwrap().is_empty());
    let types = backup.file_types(backup.sources().unwrap()[0].id, false).unwrap();
    assert_eq!((types[0].ending.as_str(), types[0].files), ("xyz", 3));

    // No picture: the screens show their placeholder.
    let id = backup.catalog().load_reports().unwrap()[0].items[0]
        .id()
        .unwrap()
        .to_string();
    let e = keevoro.picture(&backup, &id, crate::PictureSize::Small).unwrap_err();
    assert!(e.message.starts_with("This one can't be previewed"), "{e}");

    // Backed up, byte for byte, with previews wanted, and in the index.
    let usb = dir.path().join("USB");
    keevoro.add_destination(&backup, &folder(&usb)).unwrap();
    let job = keevoro.run_backup(&named, None).unwrap();
    let Some(Said::Send(last)) = keevoro.wait(job.id) else {
        panic!("a backup ends with what it said")
    };
    keevoro.wait_for_jobs();
    assert_eq!(
        (last.problem.as_deref(), last.outcome),
        (None, Some(Outcome::Worked)),
        "{last:?}"
    );
    assert!(
        last.summary.iter().any(|l| l.starts_with("Safely backed up 2 files")),
        "{last:?}"
    );
    // One of the two copies of the panorama, whichever the duplicate
    // rules keep, and the other file: each exact.
    let mut sent: Vec<Vec<u8>> = files_under(&usb.join("originals"))
        .into_iter()
        .map(|(_, b)| b)
        .collect();
    sent.sort_by_key(Vec::len);
    assert_eq!(sent, [other.clone(), pano.clone()]);
    assert!(files_under(&usb.join("renditions")).is_empty(), "no preview was made");
    let gallery = std::fs::read_to_string(usb.join("index").join("gallery.json")).unwrap();
    assert!(gallery.contains(".xyz") && gallery.contains("other.xyz"), "{gallery}");

    // Nothing in the log about it failing.
    let log = keevoro.log_tail(200).to_lowercase();
    for noise in ["couldn't", "could not", "can't", "failed", "error"] {
        assert!(!log.contains(noise), "{noise}: {log}");
    }
    assert_eq!(
        std::fs::read(card.join("DCIM/pano.xyz")).unwrap(),
        pano,
        "the source is untouched"
    );
}

/// Every key in a JSON answer, however deep.
fn keys_in(value: &Value, into: &mut std::collections::BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map {
                into.insert(key.clone());
                keys_in(inner, into);
            }
        }
        Value::Array(list) => list.iter().for_each(|v| keys_in(v, into)),
        _ => {}
    }
}

#[test]
fn a_person_who_adds_no_kind_of_other_file_sees_nothing_new_anywhere() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro(dir.path());
    let card = dir.path().join("Card");
    for (rel, bytes) in [
        ("DCIM/IMG_20190601_120000.jpg", large_photo(40)),
        ("Documents/letter.pdf", b"%PDF a letter".to_vec()),
        ("Music/song.mp3", vec![1u8; 3_000]),
    ] {
        std::fs::create_dir_all(card.join(rel).parent().unwrap()).unwrap();
        std::fs::write(card.join(rel), bytes).unwrap();
    }
    let named = keevoro.start_backup("Card").unwrap();
    let mut seen = std::collections::BTreeSet::new();
    let survey = keevoro.survey(&card, Some(&named), None).unwrap();
    let Some(said) = keevoro.wait(survey.id) else {
        panic!("a survey ends with what it found")
    };
    keys_in(&serde_json::to_value(&said).unwrap(), &mut seen);
    assert!(!serde_json::to_value(&said).unwrap().to_string().contains("\"others\""));
    let last = look_and_wait(&keevoro, &named, &card);
    keys_in(&serde_json::to_value(&last).unwrap(), &mut seen);
    assert!(serde_json::to_value(&last).unwrap().get("others").is_none());
    for (op, request) in [
        (crate::Op::FoundSummary, json!({})),
        (crate::Op::CollectionsList, json!({})),
        (crate::Op::BackupWants, json!({})),
        (crate::Op::SourcesWants, json!({ "source": 1 })),
        (crate::Op::FileKindsList, json!({})),
        (crate::Op::BackupsList, json!({})),
        (crate::Op::PlacesFolders, json!({ "place": card.display().to_string() })),
        (
            crate::Op::PlacesFiles,
            json!({ "place": card.join("DCIM").display().to_string() }),
        ),
        (crate::Op::PlacesMap, json!({ "place": card.display().to_string() })),
    ] {
        let answer = call(&keevoro, op, Some("Card"), request).unwrap();
        keys_in(&answer, &mut seen);
    }
    // (`others` on the map has always meant everything else; where it is
    // new, it is checked below.)
    for new in [
        "carried",
        "carriedBytes",
        "byFolder",
        "isOther",
        "offersOthers",
        "changed",
    ] {
        assert!(!seen.contains(new), "{new} is said to a person who added nothing");
    }
    let found = serde_json::to_value(Backup::open(&named.home).unwrap().found().unwrap()).unwrap();
    assert!(found.get("others").is_none(), "{found}");
    assert_eq!(found["photos"], 1, "the letter and the song are not looked for");
    let wants = call(&keevoro, crate::Op::BackupWants, Some("Card"), json!({})).unwrap();
    assert_eq!(wants["said"], "This backup is for photos and videos.");
    let usb = dir.path().join("USB");
    keevoro
        .add_destination(&Backup::open(&named.home).unwrap(), &folder(&usb))
        .unwrap();
    let job = keevoro.run_backup(&named, None).unwrap();
    keevoro.wait(job.id);
    keevoro.wait_for_jobs();
    assert!(!usb.join("originals").join(keevoro_core::OTHER_FILES).exists());
    let guide = std::fs::read_to_string(usb.join(keevoro_core::BACKUP_GUIDE)).unwrap();
    assert_eq!(
        guide,
        keevoro_engine::layout::guide(false),
        "the guide reads as it always did"
    );
}

#[test]
fn other_files_carried_along_are_surveyed_found_kept_by_folder_backed_up_exactly_and_an_agent_keeps_within_its_folders()
{
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro(dir.path());
    let mum = dir.path().join("Mum");
    let songs: Vec<(String, Vec<u8>)> = (0..3u8)
        .map(|n| (format!("Music/Album/track {n}.flac"), vec![n; 2_000 + usize::from(n)]))
        .collect();
    let mut files: Vec<(String, Vec<u8>)> = songs.clone();
    files.push(("Documents/letter.pdf".into(), b"%PDF a letter".to_vec()));
    files.push((
        "Pictures/IMG_20190601_120000.jpg".into(),
        crate::backup::tests::large_photo(40),
    ));
    for (rel, bytes) in &files {
        std::fs::create_dir_all(mum.join(rel).parent().unwrap()).unwrap();
        std::fs::write(mum.join(rel), bytes).unwrap();
    }

    // With nothing added, nothing changes: only the photo is found.
    let named = keevoro.start_backup("Mum").unwrap();
    let last = look_and_wait(&keevoro, &named, &mum);
    assert_eq!((last.photos, last.others), (1, 0));
    let plain = serde_json::to_value(Backup::open(&named.home).unwrap().discover().unwrap()).unwrap();
    assert!(plain.get("others").is_none(), "{plain}");

    keevoro.add_file_kind("flac", keevoro_core::MediaKind::Other).unwrap();
    keevoro.add_file_kind("pdf", keevoro_core::MediaKind::Other).unwrap();
    let survey = keevoro.survey(&mum, None, None).unwrap();
    let Some(Said::Surveyed(found)) = keevoro.wait(survey.id) else {
        panic!("a survey ends with what it found")
    };
    assert_eq!(found.hints.iter().map(|h| h.others).sum::<usize>(), 4, "{found:?}");
    let last = look_and_wait(&keevoro, &named, &mum);
    assert_eq!((last.photos, last.others), (1, 4));

    let backup = Backup::open(&named.home).unwrap();
    let found = backup.found().unwrap();
    assert_eq!((found.photos, found.videos, found.others), (1, 0, 4));
    let folders: Vec<String> = backup
        .collections()
        .unwrap()
        .into_iter()
        .filter(|c| c.by_folder)
        .map(|c| c.key)
        .collect();
    assert_eq!(
        folders,
        ["Other files / Mum / Documents", "Other files / Mum / Music / Album"]
    );

    // An agent allowed this folder may read it all and leave out a folder
    // of other files, but may not change the kinds list.
    keevoro
        .set_agent_access(crate::agents::AgentsAccessRequest {
            look_in: Some(vec![mum.display().to_string()]),
            back_up_to: None,
            may_change: Some(true),
        })
        .unwrap();
    let listed = crate::call_for_agent(&keevoro, crate::Op::CollectionsList, Some("Mum"), json!({})).unwrap();
    assert!(listed.to_string().contains("Other files / Mum / Documents"), "{listed}");
    let inside = crate::call_for_agent(
        &keevoro,
        crate::Op::CollectionsPhotos,
        Some("Mum"),
        json!({ "collection": "Other files / Mum / Music / Album" }),
    )
    .unwrap();
    assert_eq!(inside.as_array().unwrap().len(), 3);
    assert!(
        inside[0]["isOther"] == true && inside[0]["bytes"].as_u64().is_some(),
        "{inside}"
    );
    crate::call_for_agent(
        &keevoro,
        crate::Op::CollectionsKeep,
        Some("Mum"),
        json!({ "collection": "Other files / Mum / Documents", "keep": false }),
    )
    .unwrap();
    let e = crate::call_for_agent(
        &keevoro,
        crate::Op::FileKindsAdd,
        None,
        json!({ "ending": "docx", "kind": "other" }),
    )
    .unwrap_err();
    assert_eq!(e.kind, crate::Kind::Refused);
    let listing = crate::call_for_agent(
        &keevoro,
        crate::Op::PlacesFiles,
        Some("Mum"),
        json!({ "place": mum.join("Music/Album").display().to_string(), "only": "others" }),
    )
    .unwrap();
    assert_eq!(listing["others"], 3, "{listing}");
    assert!(listing["files"]
        .as_array()
        .unwrap()
        .iter()
        .all(|f| f["isOther"] == true));
    let outside = dir.path().join("Elsewhere");
    std::fs::create_dir_all(&outside).unwrap();
    let e = crate::call_for_agent(
        &keevoro,
        crate::Op::PlacesFiles,
        Some("Mum"),
        json!({ "place": outside.display().to_string() }),
    )
    .unwrap_err();
    assert_eq!(e.kind, crate::Kind::Refused, "an agent stays in the folders allowed");

    // No picture is ever made of an other file, and it is never opened to try.
    let song = backup
        .catalog()
        .load_reports()
        .unwrap()
        .iter()
        .flat_map(|r| r.items.clone())
        .find(|i| i.kind == keevoro_core::MediaKind::Other)
        .unwrap();
    let e = keevoro
        .picture(&backup, song.id().unwrap(), crate::PictureSize::Small)
        .unwrap_err();
    assert!(e.message.contains("doesn't make pictures of other files"), "{e}");

    // Backed up exactly, in their folders; the folder left out is not.
    let usb = dir.path().join("USB");
    keevoro.add_destination(&backup, &folder(&usb)).unwrap();
    let job = keevoro.run_backup(&named, None).unwrap();
    let Some(Said::Send(last)) = keevoro.wait(job.id) else {
        panic!("a backup ends with what it said")
    };
    keevoro.wait_for_jobs();
    assert_eq!(last.outcome, Some(Outcome::Worked), "{last:?}");
    let at = |rel: &str| std::fs::read(usb.join("originals").join("Other files").join("Mum").join(rel)).ok();
    for (rel, bytes) in &songs {
        assert_eq!(at(rel).as_ref(), Some(bytes), "{rel}");
    }
    assert_eq!(at("Documents/letter.pdf"), None, "left out by the agent, so not sent");
    let check = keevoro.check_backup(&named, None).unwrap();
    let Some(Said::Send(checked)) = keevoro.wait(check.id) else {
        panic!("a check ends with what it said")
    };
    keevoro.wait_for_jobs();
    assert_eq!(checked.outcome, Some(Outcome::Worked), "{checked:?}");
    // The source is exactly as it was.
    for (rel, bytes) in &files {
        assert_eq!(&std::fs::read(mum.join(rel)).unwrap(), bytes, "{rel}");
    }
}

/// Looks through `place` for `named`, waits, and gives the last event.
fn look_and_wait(keevoro: &Keevoro, named: &Named, place: &Path) -> ScanEvent {
    let job = keevoro.look(named, place, None).unwrap();
    let Some(Said::Scan(last)) = keevoro.wait(job.id) else {
        panic!("a search ends with what it found")
    };
    keevoro.wait_for_jobs();
    last
}

/// Pictures with a library an older Keevoro left in it: its previews were
/// found as photos before Keevoro knew to leave them out.
fn pictures_with_an_old_library(dir: &Path) -> PathBuf {
    let pictures = dir.join("Pictures");
    for (rel, shade) in [
        ("Holiday/IMG_20190601_120000.jpg", 20u8),
        ("Keevoro Library/renditions/display/0a1b2c.jpg", 60),
        ("Keevoro Library/renditions/display/3d4e5f.jpg", 90),
    ] {
        std::fs::create_dir_all(pictures.join(rel).parent().unwrap()).unwrap();
        std::fs::write(pictures.join(rel), crate::backup::tests::large_photo(shade)).unwrap();
    }
    std::fs::create_dir_all(pictures.join("Keevoro Library/originals")).unwrap();
    pictures
}

/// What makes it recognisable as the old library.
fn mark_as_old_library(pictures: &Path) {
    let record = pictures.join("Keevoro Library").join(".keevoro");
    std::fs::create_dir_all(&record).unwrap();
    std::fs::write(record.join("catalog.sqlite"), b"an old record").unwrap();
}

#[test]
fn files_from_a_folder_keevoro_made_are_no_longer_offered_after_a_look_again_and_it_says_so() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro(dir.path());
    let named = keevoro.start_backup("Pictures").unwrap();
    let pictures = pictures_with_an_old_library(dir.path());
    // Found as photos, as by a version before this one.
    assert_eq!(look_and_wait(&keevoro, &named, &pictures).photos, 3);

    mark_as_old_library(&pictures);
    let again = look_and_wait(&keevoro, &named, &pictures);
    assert_eq!(again.photos, 1, "only the person's own photo");
    assert_eq!(again.left_out.len(), 1);
    let left = &again.left_out[0];
    assert_eq!(left.what, "oldLibrary");
    assert!(left.folder.ends_with("Keevoro Library"), "{left:?}");
    assert!(
        left.said
            .contains("a folder an older version of Keevoro made, which it no longer uses"),
        "{left:?}"
    );
    assert!(
        left.said
            .contains("of copies of your photos it gathered there (0 files)"),
        "{left:?}"
    );
    assert!(
        left.said
            .contains("has not checked that every copy is also in one of your backups"),
        "{left:?}"
    );
    assert!(
        left.said
            .contains("The 2 files found there before are no longer offered"),
        "{left:?}"
    );
    assert!(keevoro
        .log_tail(20)
        .contains("a folder an older version of Keevoro made"));
    let backup = Backup::open(&named.home).unwrap();
    assert_eq!(backup.found().unwrap().photos, 1);
    assert!(
        pictures.join("Keevoro Library/renditions/display/0a1b2c.jpg").is_file(),
        "never removed"
    );
}

#[test]
fn files_already_backed_up_from_a_folder_keevoro_made_stay_backed_up_when_no_longer_offered() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro(dir.path());
    let named = keevoro.start_backup("Pictures").unwrap();
    let pictures = pictures_with_an_old_library(dir.path());
    look_and_wait(&keevoro, &named, &pictures);
    let usb = dir.path().join("USB");
    keevoro
        .add_destination(&Backup::open(&named.home).unwrap(), &folder(&usb))
        .unwrap();
    let job = keevoro.run_backup(&named, None).unwrap();
    keevoro.wait(job.id);
    keevoro.wait_for_jobs();
    let sent_before = files_under(&usb.join("originals")).len();
    assert_eq!(sent_before, 3);

    mark_as_old_library(&pictures);
    let again = look_and_wait(&keevoro, &named, &pictures);
    assert_eq!(again.photos, 1);
    assert!(again.left_out[0]
        .said
        .contains("anything already backed up stays backed up"));
    assert_eq!(
        files_under(&usb.join("originals")).len(),
        sent_before,
        "nothing leaves the backup"
    );
    // And a backup now sends nothing more and removes nothing.
    let job = keevoro.run_backup(&named, None).unwrap();
    keevoro.wait(job.id);
    keevoro.wait_for_jobs();
    assert_eq!(files_under(&usb.join("originals")).len(), sent_before);
}

#[test]
fn a_backup_folder_inside_a_source_is_left_out_by_where_it_is_even_before_its_first_backup() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro(dir.path());
    let named = keevoro.start_backup("Pictures").unwrap();
    let pictures = pictures_with_an_old_library(dir.path());
    std::fs::remove_dir_all(pictures.join("Keevoro Library")).unwrap();
    let inside = pictures.join("Backup for Mum");
    std::fs::create_dir_all(&inside).unwrap();
    std::fs::write(inside.join("IMG_1.jpg"), crate::backup::tests::large_photo(5)).unwrap();
    keevoro
        .add_destination(&Backup::open(&named.home).unwrap(), &folder(&inside))
        .unwrap();
    let found = look_and_wait(&keevoro, &named, &pictures);
    assert_eq!(found.photos, 1);
    assert_eq!(found.left_out[0].what, "keevoroBackup");
    assert!(found.left_out[0].said.contains("it is a backup already, not a source"));
    // The survey says the same.
    let survey = keevoro.survey(&pictures, None, None).unwrap();
    let Some(Said::Surveyed(view)) = keevoro.wait(survey.id) else {
        panic!()
    };
    assert_eq!(view.left_out.len(), 1);
    assert!(view.left_out[0].folder.ends_with("Backup for Mum"));
}

/// Windows once limited a path to 260 characters. A photo whose full path
/// is longer is found, pictured and backed up to a folder whose own path
/// is longer too.
#[cfg(windows)]
#[test]
fn a_photo_with_a_path_longer_than_260_characters_is_found_pictured_and_backed_up() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro(dir.path());
    let long = "A folder with quite a long name for testing";
    let deep = (0..8).fold(dir.path().join("Card"), |p, n| p.join(format!("{long} {n}")));
    let photo = deep.join("IMG_20190601_120000 a photo with a long name too.jpg");
    assert!(photo.as_os_str().len() > 300, "{}", photo.display());
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::write(&photo, crate::backup::tests::large_photo(40)).unwrap();
    let named = keevoro.start_backup("Long").unwrap();
    let found = look_and_wait(&keevoro, &named, &dir.path().join("Card"));
    assert_eq!((found.photos, found.unreadable), (1, 0), "{found:?}");

    let backup = Backup::open(&named.home).unwrap();
    let id = backup.catalog().load_reports().unwrap()[0].items[0]
        .id()
        .unwrap()
        .to_string();
    let picture = keevoro.picture(&backup, &id, crate::PictureSize::Small).unwrap();
    assert_eq!(&picture[..4], b"RIFF");

    let usb = (0..6).fold(dir.path().join("USB"), |p, n| p.join(format!("{long} {n}")));
    keevoro.add_destination(&backup, &folder(&usb)).unwrap();
    let job = keevoro.run_backup(&named, None).unwrap();
    let Some(Said::Send(last)) = keevoro.wait(job.id) else {
        panic!()
    };
    keevoro.wait_for_jobs();
    assert_eq!(last.outcome, Some(Outcome::Worked), "{last:?}");
    let sent = files_under(&usb.join("originals"));
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].1, std::fs::read(&photo).unwrap(), "exact");
}

/// Everything but what starts with a dot, as `rm -rf USB/*` removes.
fn empty(folder: &Path) {
    for entry in std::fs::read_dir(folder).unwrap().filter_map(|e| e.ok()) {
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            std::fs::remove_dir_all(path).unwrap();
        } else {
            std::fs::remove_file(path).unwrap();
        }
    }
}

fn run_to_its_end(keevoro: &Keevoro, named: &Named) -> SendEvent {
    let job = keevoro.run_backup(named, None).unwrap();
    let Some(Said::Send(last)) = keevoro.wait(job.id) else {
        panic!("it says how it ended")
    };
    keevoro.wait_for_jobs();
    last
}

/// The first outside tester's report: a backup deleted from its folder
/// was said to be "already there".
#[test]
fn a_backup_deleted_from_its_folder_is_said_to_be_gone_before_anything_is_sent_and_is_sent_again() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro(dir.path());
    let (named, _card, usb) = card_going_to_usb(dir.path(), &keevoro);
    let first = run_to_its_end(&keevoro, &named);
    assert_eq!(first.outcome, Some(Outcome::Worked), "{first:?}");
    let all = there(&named);
    assert!(all > 0);

    empty(&usb);
    let heads_up = format!(
        "The folder {} looks emptied or replaced: it no longer holds any of the {all} files Keevoro backed up there. \
         They will be sent again.",
        usb.display()
    );
    // The rehearsal looks first, and says so before anything else.
    let rehearsal = call(&keevoro, Op::BackupRehearse, Some(&named.name), Value::Null).unwrap();
    assert_eq!(rehearsal["said"][0], json!(heads_up), "{rehearsal}");
    assert_eq!(rehearsal["files"], json!(all), "{rehearsal}");
    assert!(!rehearsal["said"].to_string().contains("already at"), "{rehearsal}");
    // So does the backup screen, through the same record.
    let backup = Backup::open(&named.home).unwrap();
    let place = &backup.destinations().unwrap()[0];
    assert_eq!((place.there, place.waiting), (0, all));
    assert_eq!(place.looked.as_ref().unwrap().said, std::slice::from_ref(&heads_up));

    // The backup says it first, sends them all again, and says so at the end
    // and in its history.
    let again = run_to_its_end(&keevoro, &named);
    assert_eq!(again.outcome, Some(Outcome::Worked), "{again:?}");
    assert_eq!(again.summary[0], heads_up, "{:?}", again.summary);
    assert!(
        again.summary.contains(&format!(
            "Restored {} that had gone from there.",
            keevoro_engine::summary::count(all, "file")
        )),
        "{:?}",
        again.summary
    );
    assert!(
        !again.summary.iter().any(|l| l.ends_with("already there.")),
        "{:?}",
        again.summary
    );
    assert_eq!(there(&named), all);
    let history = backup.history().unwrap();
    let detail = history[0].detail.as_ref().unwrap();
    assert_eq!(detail.destinations[0].originals.restored, all);
    let in_full = crate::words::run_in_full(&history[0]).join("\n");
    assert!(
        in_full.contains("sent again because they had gone from there"),
        "{in_full}"
    );
}

#[test]
fn what_check_they_all_arrived_finds_missing_backing_up_again_sends() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro(dir.path());
    let (named, _card, usb) = card_going_to_usb(dir.path(), &keevoro);
    run_to_its_end(&keevoro, &named);
    let all = there(&named);
    // One original goes missing.
    let mut originals = Vec::new();
    let mut stack = vec![usb.join("originals")];
    while let Some(at) = stack.pop() {
        for entry in std::fs::read_dir(at).unwrap().filter_map(|e| e.ok()) {
            if entry.path().is_dir() {
                stack.push(entry.path());
            } else {
                originals.push(entry.path());
            }
        }
    }
    std::fs::remove_file(&originals[0]).unwrap();

    let job = keevoro.check_backup(&named, None).unwrap();
    let Some(Said::Send(checked)) = keevoro.wait(job.id) else {
        panic!("it says how it ended")
    };
    keevoro.wait_for_jobs();
    assert!(
        checked
            .summary
            .iter()
            .any(|l| l == "Back up again to send anything that is missing."),
        "{checked:?}"
    );
    // What the check found is what every screen now says.
    assert_eq!(there(&named), all - 1);
    let again = run_to_its_end(&keevoro, &named);
    assert!(
        again.summary.iter().any(|l| l.starts_with("Safely backed up 1 file ")),
        "{:?}",
        again.summary
    );
    assert!(originals[0].is_file(), "it was sent again");
    assert_eq!(there(&named), all);
}

#[test]
fn a_destination_unplugged_is_said_to_be_out_of_reach_and_nothing_it_holds_is_forgotten() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro(dir.path());
    let (named, _card, usb) = card_going_to_usb(dir.path(), &keevoro);
    run_to_its_end(&keevoro, &named);
    let all = there(&named);
    let away = dir.path().join("unplugged");
    std::fs::rename(&usb, &away).unwrap();

    let rehearsal = call(&keevoro, Op::BackupRehearse, Some(&named.name), Value::Null).unwrap();
    let said = rehearsal["said"].to_string();
    assert!(said.contains("what is there can't be confirmed"), "{said}");
    assert!(said.contains("Is it plugged in?"), "{said}");
    assert!(!said.contains("no longer holds"), "{said}");
    assert_eq!(
        rehearsal["files"],
        json!(0),
        "nothing would be sent to where it cannot go"
    );
    assert_eq!(there(&named), all, "what was there is still counted as there");
    let room = call(&keevoro, Op::DestinationsRoom, Some(&named.name), Value::Null).unwrap();
    assert_eq!(room["places"][0]["files"], json!(0), "{room}");

    let last = run_to_its_end(&keevoro, &named);
    assert_eq!(last.outcome, Some(Outcome::Unreachable), "{last:?}");
    assert!(!usb.exists(), "the folder is never made again where the drive was");
    std::fs::rename(&away, &usb).unwrap();
    assert_eq!(there(&named), all);
    let back = run_to_its_end(&keevoro, &named);
    assert!(
        back.summary
            .iter()
            .any(|l| l == &format!("{} already there.", keevoro_engine::summary::count(all, "file"))),
        "{:?}",
        back.summary
    );
}
