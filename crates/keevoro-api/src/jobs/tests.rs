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
        keevoro_core::pause::set_paused(false);
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
        "bytesDone",
        "bytesTotal",
        "speed",
        "secondsLeft",
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
                Kind::Refused,
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
                Kind::Refused,
                "Keevoro is already working on your backup. Let that finish first."
            )
        );
    }
    // Another backup is not held up, and neither is a survey.
    keevoro.look(&other, &card, None).unwrap();
    keevoro.survey(&card, Some(&named), None).unwrap();
    assert!(keevoro.busy());
    let e = keevoro.reset().unwrap_err();
    assert_eq!(e.kind, Kind::Refused);
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
    assert_eq!(last.problem, None);
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
    assert_eq!(checked.problem, None);
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
    assert_eq!(last.problem, None);
    assert!(last.summary.iter().any(|l| l.starts_with("Stopped.")), "{last:?}");
    assert_eq!(there(&named), 1, "what was sent is kept");
    let backup = Backup::open(&named.home).unwrap();
    assert!(backup.history().unwrap()[0].stopped);
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
    assert!(status[0].bytes_total > 0, "it knows what it sets out to send");

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

    // Stopped the moment it first says how far it has got.
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
    assert!(found.stopped);
    assert!(keevoro.log_tail(1).ends_with("(stopped early)"));
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

    // While something outside the API holds it back, the check that a
    // search starts gives way at once, and the new photo stays sampled.
    let hold = keevoro.give_way();
    std::fs::write(card.join("DCIM/IMG_20200102_120000.jpg"), large_photo(220)).unwrap();
    let job = keevoro.look(&named, &card, None).unwrap();
    keevoro.wait(job.id);
    keevoro.wait_for_jobs();
    assert_eq!(unchecked(), 1);
    assert!(keevoro.log_tail(3).contains("Background checking paused after"));

    // Only one runs at a time. Held by the pause, the first is still
    // running when a second is asked for.
    keevoro.pause();
    drop(hold);
    let first = keevoro.keep_checking(&named).unwrap();
    assert_eq!(first.job, JobKind::Duplicates);
    assert_eq!(first.op, None, "it is Keevoro's own, not an operation");
    assert!(keevoro.keep_checking(&named).is_none());
    keevoro.carry_on();
    keevoro.wait(first.id);
    keevoro.wait_for_jobs();
    assert_eq!(unchecked(), 0, "and it carries on from where it was");
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
