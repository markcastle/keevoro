use super::*;
use crate::jobs::tests::{card_going_to_usb_for, keevoro_for_tests as keevoro, one_at_a_time};
use crate::{call, call_for_agent, Kind, Op, Said};
use serde_json::{json, Value};
use std::path::PathBuf;

/// Is `was_at`, as a catalog kept it (a source's real path), the folder `place`?
fn same_folder(was_at: &Value, place: &Path) -> bool {
    std::fs::canonicalize(was_at.as_str().unwrap()).ok() == std::fs::canonicalize(place).ok()
}

fn at(place: &Path) -> Value {
    json!({ "kind": "folder", "place": place.display().to_string() })
}

/// Every file under `folder`, by its path, with its bytes.
fn everything_in(folder: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(everything_in(&path));
        } else {
            found.push((path.clone(), std::fs::read(&path).unwrap()));
        }
    }
    found.sort();
    found
}

/// What the last backup sent of the originals, and found already there.
fn originals_sent(named: &Named) -> (usize, usize) {
    let history = Backup::open(&named.home).unwrap().history().unwrap();
    let last = history.first().unwrap().detail.as_ref().unwrap();
    let originals = &last.destinations[0].originals;
    (originals.sent, originals.already_there)
}

/// The old computer backs a card up to a USB drive. Answers the card, the
/// drive and how many originals were sent.
fn backed_up_on_the_old_computer(dir: &Path) -> (PathBuf, PathBuf, usize) {
    let old = keevoro(&dir.join("old computer"));
    let (named, card, usb) = card_going_to_usb_for(dir, &old);
    let ran = old.run_backup(&named, None).unwrap();
    old.wait(ran.id);
    old.wait_for_jobs();
    let (sent, _) = originals_sent(&named);
    assert!(sent > 0);
    assert!(
        usb.join(notes::NOTES).is_file(),
        "the notes are written with the backup"
    );
    (card, usb, sent)
}

#[test]
fn a_backup_is_opened_on_a_new_computer_from_its_destination_and_carries_on_without_sending_anything_again() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let (card, usb, sent) = backed_up_on_the_old_computer(dir.path());
    let card_before = everything_in(&card);
    // The old computer is gone: the new one has none of its folders.
    std::fs::remove_dir_all(dir.path().join("old computer")).unwrap();
    let new = keevoro(&dir.path().join("new computer"));

    // What is there, said before anything is changed.
    let found = call(&new, Op::BackupsFindAt, None, at(&usb)).unwrap();
    assert_eq!(found["found"], "notes");
    assert_eq!(found["name"], "Card photos");
    assert_eq!(found["files"], sent);
    assert!(same_folder(&found["sources"][0]["wasAt"], &card), "{found}");
    assert!(found["madeBy"].as_str().unwrap().starts_with("Keevoro "));
    assert!(found["lastBackedUpSaid"].is_string());
    assert!(found.get("alreadyOpen").is_none());
    let said = found["said"][0].as_str().unwrap();
    assert!(said.starts_with("\"Card photos\": "), "{said}");
    assert!(new.backups().all().is_empty(), "looking changes nothing");

    // Opened: a backup here, going to the same place, waiting for its source.
    let opened = call(&new, Op::BackupsOpenFrom, None, at(&usb)).unwrap();
    assert_eq!(opened["name"], "Card photos");
    assert!(same_folder(&opened["waiting"][0]["wasAt"], &card), "{opened}");
    let named = new.backups().current().unwrap();
    assert_eq!(named.name, "Card photos");
    let destinations = call(&new, Op::DestinationsList, None, json!({})).unwrap();
    assert_eq!(destinations[0]["there"], sent, "{destinations}");
    let sources = call(&new, Op::SourcesList, None, json!({})).unwrap();
    assert!(
        sources[0]["waitingSaid"]
            .as_str()
            .unwrap()
            .starts_with("Keevoro does not know where these photos live on this computer"),
        "{sources}"
    );

    // The same backup again is the one already here, never a second.
    let again = call(&new, Op::BackupsFindAt, None, at(&usb)).unwrap();
    assert_eq!(again["alreadyOpen"], "Card photos");
    let again = call(&new, Op::BackupsOpenFrom, None, at(&usb)).unwrap();
    assert_eq!(again["alreadyOpen"], true);
    assert_eq!(new.backups().all().len(), 1);

    // It is never looked for where it was on the old computer.
    let refused = new.look_again(&named, None, None, None).unwrap_err();
    assert_eq!(refused.kind, Kind::Unreachable, "{refused}");

    // The person connects the card again, and it is looked through.
    let source = sources[0]["id"].as_i64().unwrap();
    let connected = call(
        &new,
        Op::SourcesConnectAgain,
        None,
        json!({ "source": source, "place": card.display().to_string() }),
    )
    .unwrap();
    assert_eq!(connected["said"], CONNECTED_SAID);
    let looked = new.look_again(&named, Some(source), None, None).unwrap();
    new.wait(looked.id);
    new.wait_for_jobs();

    // Backing up sends nothing again.
    let ran = new.run_backup(&named, None).unwrap();
    new.wait(ran.id);
    new.wait_for_jobs();
    assert_eq!(originals_sent(&named), (0, sent));

    // And putting it right, which reads every copy back, finds it whole.
    let proved = new.put_right(&named, true, None).unwrap();
    match new.wait(proved.id) {
        Some(Said::Send(last)) => assert_eq!(last.outcome, Some(crate::Outcome::Worked), "{last:?}"),
        other => panic!("{other:?}"),
    }
    new.wait_for_jobs();
    let found = call(&new, Op::BackupsFindAt, None, at(&usb)).unwrap();
    assert!(
        found["lastProvedSaid"].is_string(),
        "the proof is in the notes: {found}"
    );
    assert_eq!(everything_in(&card), card_before, "the card is untouched");
}

#[test]
fn damaged_notes_fall_back_to_the_copy_before_them_and_a_newer_keevoros_are_refused_plainly() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let (_, usb, _) = backed_up_on_the_old_computer(dir.path());
    let new = keevoro(&dir.path().join("new computer"));

    // Only one generation yet: damaged, and nothing to fall back to.
    assert!(!usb.join(notes::EARLIER).exists(), "no earlier copy yet");
    std::fs::write(usb.join(notes::NOTES), b"scrambled").unwrap();
    let found = call(&new, Op::BackupsFindAt, None, at(&usb)).unwrap();
    assert_eq!(found["found"], "damaged");
    let e = call(&new, Op::BackupsOpenFrom, None, at(&usb)).unwrap_err();
    assert_eq!(e.kind, Kind::Refused);
    assert!(
        e.message.contains("its notes and the copy before them are damaged"),
        "{e}"
    );

    // With an earlier copy, that one is used, and said.
    let catalog = Catalog::in_memory().unwrap();
    let (file, _) = notes::encode(&catalog.notes_for("nowhere").unwrap(), "Card photos", 1).unwrap();
    std::fs::write(usb.join(notes::EARLIER), file).unwrap();
    let found = call(&new, Op::BackupsFindAt, None, at(&usb)).unwrap();
    assert_eq!(found["found"], "earlierNotes");
    assert!(
        found["said"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s.as_str().unwrap().starts_with("Its newest notes are damaged")),
        "{found}"
    );

    // A newer Keevoro's notes are refused, and nothing is opened.
    let mut newer = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut newer, b"{\"keevoroNotes\":9,\"madeBy\":\"Keevoro 9.0.0\"}\n").unwrap();
    std::fs::write(usb.join(notes::NOTES), newer.finish().unwrap()).unwrap();
    let found = call(&new, Op::BackupsFindAt, None, at(&usb)).unwrap();
    assert_eq!(found["found"], "newer");
    let e = call(&new, Op::BackupsOpenFrom, None, at(&usb)).unwrap_err();
    assert_eq!(e.kind, Kind::Refused);
    assert!(e.message.contains("made by a newer Keevoro (Keevoro 9.0.0)"), "{e}");
    assert!(new.backups().all().is_empty());
}

#[test]
fn a_folder_with_no_backup_or_no_folder_at_all_is_said_plainly() {
    let dir = tempfile::tempdir().unwrap();
    let k = keevoro(dir.path());
    let empty = dir.path().join("Empty");
    std::fs::create_dir_all(&empty).unwrap();
    let found = call(&k, Op::BackupsFindAt, None, at(&empty)).unwrap();
    assert_eq!(found["found"], "nothing");
    assert_eq!(
        found["said"][0],
        format!("Keevoro found no backup in the folder {}.", empty.display())
    );
    assert_eq!(
        call(&k, Op::BackupsOpenFrom, None, at(&empty)).unwrap_err().kind,
        Kind::NotFound
    );
    let gone = call(&k, Op::BackupsFindAt, None, at(&dir.path().join("Unplugged"))).unwrap_err();
    assert_eq!(gone.kind, Kind::Unreachable);
    assert!(gone.message.contains("Is it plugged in?"), "{gone}");
    assert!(k.backups().all().is_empty());
}

#[test]
fn an_agent_may_read_what_is_at_a_folder_it_was_allowed_and_nowhere_else_and_never_connects_a_source() {
    let dir = tempfile::tempdir().unwrap();
    let k = keevoro(dir.path());
    let allowed = dir.path().join("Allowed");
    std::fs::create_dir_all(&allowed).unwrap();
    let elsewhere = dir.path().join("Elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    call(
        &k,
        Op::AgentsAccess,
        None,
        json!({ "lookIn": [allowed.display().to_string()] }),
    )
    .unwrap();
    assert_eq!(
        call_for_agent(&k, Op::BackupsFindAt, None, at(&allowed)).unwrap()["found"],
        "nothing"
    );
    let e = call_for_agent(&k, Op::BackupsFindAt, None, at(&elsewhere)).unwrap_err();
    assert_eq!(e.kind, Kind::Refused);
    let e = call_for_agent(&k, Op::SourcesConnectAgain, None, json!({ "source": 1, "place": "x" })).unwrap_err();
    assert_eq!(e.kind, Kind::Refused);
}
