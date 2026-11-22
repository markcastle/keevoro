//! Nothing a person has set, and nothing Keevoro has recorded, may be lost
//! by an update.
//!
//! `fixtures/0.37.1` holds a settings file, two backups' catalogs and the
//! remembered speeds exactly as Keevoro 0.37.1 wrote them (the version the
//! first outside tester used). This opens them with the Keevoro of today
//! and checks everything a person would notice is as they left it. When a
//! stored form changes, add what the old form held to the fixture (or a
//! fixture of its own for the version before the change) and a line here
//! that it is still there: see "When what Keevoro keeps changes" in
//! `docs/developing.md`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use keevoro_api::{call, Folders, Keevoro, Op, SecretStore};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Default)]
struct KeysInMemory(Mutex<BTreeMap<String, String>>);

impl SecretStore for KeysInMemory {
    fn get(&self, name: &str) -> anyhow::Result<Option<String>> {
        Ok(self.0.lock().unwrap().get(name).cloned())
    }
    fn set(&self, name: &str, value: &str) -> anyhow::Result<()> {
        self.0.lock().unwrap().insert(name.into(), value.into());
        Ok(())
    }
    fn delete(&self, name: &str) -> anyhow::Result<()> {
        self.0.lock().unwrap().remove(name);
        Ok(())
    }
}

/// The places the fixture names, made real in a temporary folder.
struct Places {
    card: PathBuf,
    beach: PathBuf,
    usb: PathBuf,
    folders: Folders,
}

impl Places {
    /// A fixture file with its places filled in: `{CARD}` as the path is,
    /// `{json:CARD}` as it is written inside JSON.
    fn fill(&self, text: &str) -> String {
        let mut text = text.to_string();
        let data = self.folders.data.clone();
        for (name, path) in [
            ("CARD", &self.card),
            ("BEACH", &self.beach),
            ("USB", &self.usb),
            ("DATA", &data),
        ] {
            let plain = path.display().to_string();
            let json = serde_json::to_string(&plain).unwrap();
            text = text.replace(&format!("{{json:{name}}}"), &json[1..json.len() - 1]);
            text = text.replace(&format!("{{{name}}}"), &plain);
        }
        text
    }
}

fn fixture(name: &str) -> String {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/0.37.1")
        .join(name);
    std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()))
}

/// Keevoro's folders as 0.37.1 left them, opened by the Keevoro of today.
fn as_0_37_1_left_it(dir: &Path) -> (Keevoro, Places) {
    let places = Places {
        card: dir.join("Card"),
        beach: dir.join("Beach"),
        usb: dir.join("USB"),
        folders: Folders::under(&dir.join("home")),
    };
    std::fs::create_dir_all(&places.folders.config).unwrap();
    std::fs::write(
        places.folders.config.join("settings.json"),
        places.fill(&fixture("settings.json")),
    )
    .unwrap();
    for (id, sql) in [("backup-1", "family photos.sql"), ("backup-2", "holiday videos.sql")] {
        let catalog = places
            .folders
            .data
            .join("backups")
            .join(id)
            .join(keevoro_catalog::CATALOG_PATH);
        std::fs::create_dir_all(catalog.parent().unwrap()).unwrap();
        rusqlite::Connection::open(&catalog)
            .unwrap()
            .execute_batch(&places.fill(&fixture(sql)))
            .unwrap();
    }
    std::fs::write(
        places.folders.data.join("measured rates.json"),
        places.fill(&fixture("measured rates.json")),
    )
    .unwrap();
    let keevoro = Keevoro::open(&places.folders)
        .with_secrets(KeysInMemory::default())
        .with_environment(|_| None);
    (keevoro, places)
}

fn ask(keevoro: &Keevoro, op: Op, backup: Option<&str>, request: Value) -> Value {
    call(keevoro, op, backup, request).unwrap_or_else(|e| panic!("{op:?}: {e:?}"))
}

/// The names of a list of things, in order.
fn names(list: &Value) -> Vec<&str> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|b| b["name"].as_str().unwrap())
        .collect()
}

#[test]
fn everything_a_person_set_and_keevoro_recorded_in_0_37_1_is_there_after_updating() {
    let dir = tempfile::tempdir().unwrap();
    let (keevoro, places) = as_0_37_1_left_it(dir.path());
    let family = Some("Family photos");

    // The backups, and the one that was open.
    let backups = ask(&keevoro, Op::BackupsList, None, Value::Null);
    assert_eq!(names(&backups), ["Family photos", "Holiday videos"]);
    assert_eq!(backups[1]["open"], true, "{backups}");

    // Keevoro's own settings: the computer's codecs switched off, what an
    // AI agent may do, and the kinds of file changed.
    let settings = ask(&keevoro, Op::KeevoroSettings, None, Value::Null);
    assert_eq!(settings["ownCodecs"]["on"], false, "{settings}");
    let agents = ask(&keevoro, Op::AgentsAccess, None, Value::Null);
    assert_eq!(agents["lookIn"], json!([places.card.display().to_string()]));
    assert_eq!(agents["backUpTo"], json!([places.usb.display().to_string()]));
    assert_eq!(agents["mayChange"], true);
    let kinds = ask(&keevoro, Op::FileKindsList, None, Value::Null);
    let kind = |ending: &str| {
        kinds
            .as_array()
            .unwrap()
            .iter()
            .find(|k| k["ending"] == ending)
            .cloned()
    };
    assert_eq!(kind("xyz").unwrap()["kind"], "video", "an ending a person added");
    assert_eq!(
        kind("png").unwrap()["on"],
        false,
        "a built-in kind a person switched off"
    );

    // The sources, with their names, what was found, the folder left out
    // and the files only online.
    let sources = ask(&keevoro, Op::SourcesList, family, Value::Null);
    assert_eq!(names(&sources), ["Mum's card", "Beach"]);
    assert_eq!(
        (sources[0]["items"].as_u64(), sources[1]["items"].as_u64()),
        (Some(6), Some(3))
    );
    let left_out = sources[0]["leftOut"][0].as_str().unwrap_or_default();
    assert!(
        left_out.starts_with(&places.card.display().to_string()) && left_out.ends_with("Old"),
        "{sources}"
    );
    assert_eq!(sources[0]["onlineOnly"], 3);
    // 0.37.1 only counted them, so their size is not known (catalog step
    // 13 keeps it from the next look) and they are said as before.
    assert_eq!(sources[0].get("onlineOnlyBytes"), None, "{sources}");
    // Catalog step 14: every source before was a folder, and still is.
    assert!(
        sources.as_array().unwrap().iter().all(|s| s.get("bucket").is_none()),
        "{sources}"
    );
    let said = sources[0]["onlineOnlySaid"].as_str().unwrap();
    assert!(
        said.starts_with("3 files here are in ") && said.ends_with("first, then look again."),
        "{said}"
    );

    // What the backup and each source are for: photos only for the backup,
    // videos only for the Beach source, which 0.39.0 lost.
    let wants = ask(&keevoro, Op::BackupWants, family, Value::Null);
    assert_eq!(wants["wants"], json!(["photos"]), "{wants}");
    let beach = ask(&keevoro, Op::SourcesWants, family, json!({ "source": 2 }));
    assert_eq!(
        (beach["own"].as_bool(), &beach["wants"]),
        (Some(true), &json!(["videos"])),
        "{beach}"
    );
    let card = ask(&keevoro, Op::SourcesWants, family, json!({ "source": 1 }));
    assert_eq!(card["own"], false, "the card still follows its backup: {card}");
    let wants = ask(&keevoro, Op::BackupWants, Some("Holiday videos"), Value::Null);
    assert_eq!(wants["wants"], json!(["videos"]), "{wants}");

    // The endings left out of a source, and why each kind is not offered.
    let types = ask(&keevoro, Op::SourcesFileTypes, family, json!({ "source": 1 }));
    let ending = |e: &str| {
        types
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["ending"] == e)
            .cloned()
            .unwrap()
    };
    assert_eq!(ending("png")["included"], false);
    assert_eq!(ending("mp4")["leftOutBy"], "backup");

    // So what is offered is what was offered: three photos (one in the
    // folder left out, the screenshot's ending left out and the card's
    // video passed over), two beach videos, one photo not kept.
    let found = ask(&keevoro, Op::FoundSummary, family, Value::Null);
    assert_eq!(
        (found["photos"].as_u64(), found["videos"].as_u64()),
        (Some(3), Some(2)),
        "{found}"
    );
    assert_eq!(found["waiting"], 4, "four kept, none yet at both places: {found}");
    // Nobody could stop the check for duplicates on its own then, so a
    // catalog from 0.37.1 never reads as stopped, and it carries on.
    assert_eq!(found.get("checkStopped"), None, "{found}");

    // Where it goes, and what had reached each place.
    let destinations = ask(&keevoro, Op::DestinationsList, family, Value::Null);
    assert_eq!(
        names(&destinations),
        [
            format!("the folder {}", places.usb.display()).as_str(),
            "the Cloudflare R2 bucket family-photos"
        ]
    );
    assert_eq!(
        (destinations[0]["there"].as_u64(), destinations[1]["there"].as_u64()),
        (Some(2), Some(0))
    );

    // A backup from before the choice of arranging existed is arranged by
    // date, as it always was, and is not asked; nor is the other, which
    // has a source too.
    for backup in [family, Some("Holiday videos")] {
        let arrange = ask(&keevoro, Op::BackupArrange, backup, Value::Null);
        assert_eq!(
            (arrange["arrange"].as_str(), arrange["asked"].as_bool()),
            (Some("byDate"), Some(true)),
            "{arrange}"
        );
    }

    for source in [1, 2] {
        let arranged = ask(&keevoro, Op::SourcesArrange, family, json!({ "source": source }));
        assert_eq!(
            (arranged["own"].as_bool(), arranged["arrange"].as_str()),
            (Some(false), Some("byDate"))
        );
    }

    // Whether previews are made, each backup's own choice.
    assert_eq!(ask(&keevoro, Op::BackupPreviews, family, Value::Null), true);
    assert_eq!(
        ask(&keevoro, Op::BackupPreviews, Some("Holiday videos"), Value::Null),
        false
    );

    // The history, in full where it was kept in full, and what the last
    // backup said.
    let history = ask(&keevoro, Op::BackupHistory, family, Value::Null);
    assert_eq!(history.as_array().unwrap().len(), 2);
    assert_eq!(history[0]["detail"]["destinations"][0]["originals"]["sent"], 2);
    assert_eq!(history[0]["detail"]["version"], "0.37.1");
    assert_eq!(history[1]["stopped"], true);
    let last = ask(&keevoro, Op::BackupLastSummary, family, Value::Null);
    assert_eq!(last["summary"][0], "Safely backed up 2 files (8.8 MB).");

    // The speeds earlier backups measured, for the next estimate.
    let rates: keevoro_core::estimate::Remembered =
        serde_json::from_slice(&std::fs::read(places.folders.data.join("measured rates.json")).unwrap()).unwrap();
    assert_eq!(rates.preview_bytes_per_original, Some(41_000.0));
    assert_eq!(rates.rates.len(), 3);

    // Asked again, after Keevoro of today has written to all of it, it is
    // all still there.
    ask(
        &keevoro,
        Op::BackupsRename,
        None,
        json!({ "backup": "Holiday videos", "to": "Holiday films" }),
    );
    let again = ask(&keevoro, Op::BackupsList, None, Value::Null);
    assert_eq!(names(&again), ["Family photos", "Holiday films"]);
    assert_eq!(
        ask(&keevoro, Op::KeevoroSettings, None, Value::Null)["ownCodecs"]["on"],
        false
    );
    assert_eq!(ask(&keevoro, Op::AgentsAccess, None, Value::Null)["mayChange"], true);
    assert_eq!(
        ask(&keevoro, Op::BackupWants, family, Value::Null)["wants"],
        json!(["photos"])
    );

    // A folder destination 0.37.1 saved has no disk remembered, and goes on
    // as before; once it is reached, the disk it is on is remembered, so the
    // empty folder an unplugged drive can leave behind is never taken for it.
    let home = places.folders.data.join("backups").join("backup-1");
    let volume = || {
        let catalog = keevoro_catalog::Catalog::open_in_library(&home).unwrap();
        keevoro_engine::connect::destinations(&catalog).unwrap()[0]
            .volume
            .clone()
    };
    assert_eq!(volume(), None);

    // The places 0.37.1 agreed are exactly as they were: catalog step 11
    // only adds a way to look them up without regard to case.
    let catalog = keevoro_catalog::Catalog::open_in_library(&home).unwrap();
    let placed: Vec<String> = catalog.collected().unwrap().into_iter().map(|c| c.path).collect();
    assert_eq!(
        placed,
        [
            "originals/DCIM/IMG_20240601_100000.jpg",
            "originals/DCIM/IMG_20240601_100500.jpg"
        ]
    );
    assert!(catalog.place_taken("originals/dcim/img_20240601_100000.JPG").unwrap());
    drop(catalog);

    // Choosing to keep the folders now moves nothing already backed up and
    // loses nothing sent: the same places, both still at the folder.
    let changed = ask(&keevoro, Op::BackupArrange, family, json!({ "arrange": "folders" }));
    assert_eq!(
        changed["said"],
        "From now on, new files are kept in your folders. The 2 files already backed up stay where they are."
    );
    let catalog = keevoro_catalog::Catalog::open_in_library(&home).unwrap();
    let after: Vec<String> = catalog.collected().unwrap().into_iter().map(|c| c.path).collect();
    assert_eq!(after, placed);
    drop(catalog);
    let destinations = ask(&keevoro, Op::DestinationsList, family, Value::Null);
    assert_eq!(destinations[0]["there"].as_u64(), Some(2), "{destinations}");
    ask(&keevoro, Op::BackupArrange, family, json!({ "arrange": "byDate" }));
    std::fs::create_dir_all(&places.usb).unwrap();
    ask(&keevoro, Op::DestinationsRoom, family, Value::Null);
    assert!(volume().is_some(), "known by its disk once reached");

    // A backup 0.37.1 kept can be removed, saying where it went and what it
    // looked through, and put back with everything in its record in place.
    let removal = ask(&keevoro, Op::BackupsForget, None, json!({ "backup": "Family photos" }));
    assert_eq!(removal["removed"], true);
    assert_eq!(removal["destinations"].as_array().map(Vec::len), Some(2), "{removal}");
    assert_eq!(removal["keysFor"], json!(["the Cloudflare R2 bucket family-photos"]));
    assert_eq!(
        names(&ask(&keevoro, Op::BackupsList, None, Value::Null)),
        ["Holiday films"]
    );
    ask(&keevoro, Op::BackupsPutBack, None, json!({ "backup": "Family photos" }));
    assert_eq!(
        names(&ask(&keevoro, Op::SourcesList, family, Value::Null)),
        ["Mum's card", "Beach"]
    );
    assert_eq!(
        ask(&keevoro, Op::BackupHistory, family, Value::Null)
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(
        ask(&keevoro, Op::BackupWants, family, Value::Null)["wants"],
        json!(["photos"])
    );
}

/// 0.42 remembers the drive a source was looked through on, so that
/// another card at the same place never replaces it. A source 0.37.1
/// looked through has none: it is looked through as before, and its drive
/// is remembered from then on. One that is not plugged in is passed over,
/// and nothing found on it is forgotten.
#[test]
fn a_source_0_37_1_looked_through_is_looked_through_as_before_and_its_drive_remembered() {
    let dir = tempfile::tempdir().unwrap();
    let (keevoro, places) = as_0_37_1_left_it(dir.path());
    let family = Some("Family photos");
    let home = places.folders.data.join("backups").join("backup-1");
    let mark = || {
        keevoro_catalog::Catalog::open_in_library(&home)
            .unwrap()
            .source_mark(&places.card)
            .unwrap()
    };
    assert_eq!(mark(), None, "0.37.1 remembered no drive");
    let card_items = || {
        let sources = ask(&keevoro, Op::SourcesList, family, Value::Null);
        sources
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == "Mum's card")
            .unwrap()["items"]
            .clone()
    };
    let before = card_items();
    assert!(before.as_u64().unwrap() > 0);

    // Not plugged in: said, and nothing forgotten.
    let refused = call(&keevoro, Op::SourcesLookAgain, family, json!({ "source": 1 })).unwrap_err();
    assert!(
        refused.message.contains("so it was not looked through"),
        "{}",
        refused.message
    );
    assert_eq!(card_items(), before);

    // Plugged in: looked through as ever, and its drive remembered.
    std::fs::create_dir_all(places.card.join("DCIM")).unwrap();
    let job = ask(&keevoro, Op::SourcesLookAgain, family, json!({ "source": 1 }));
    keevoro.wait(serde_json::from_value(job["id"].clone()).unwrap());
    keevoro.wait_for_jobs();
    let card = std::fs::canonicalize(&places.card).unwrap();
    let remembered = keevoro_catalog::Catalog::open_in_library(&home)
        .unwrap()
        .source_mark(&card)
        .unwrap()
        .expect("remembered once looked through");
    assert_eq!(remembered.volume, keevoro_engine::reach::volume_of(&card));
}

/// Backing up by itself (0.42) keeps its schedule and its record in each
/// backup's catalog, never in `settings.json`. A backup 0.37.1 kept has
/// neither: it has no schedule, nothing runs by itself, and the settings
/// file gains nothing.
#[test]
fn a_backup_0_37_1_kept_has_no_schedule_and_nothing_runs_by_itself() {
    let dir = tempfile::tempdir().unwrap();
    let (keevoro, places) = as_0_37_1_left_it(dir.path());
    for backup in ["Family photos", "Holiday videos"] {
        let shown = ask(&keevoro, Op::ScheduleShow, Some(backup), Value::Null);
        assert_eq!(
            shown,
            json!({ "paused": false, "skipping": false, "last": [] }),
            "{backup}"
        );
    }
    assert!(keevoro.run_schedules_due().is_empty());
    // Setting one leaves the settings file exactly as it was.
    let settings = places.folders.config.join("settings.json");
    let before = std::fs::read_to_string(&settings).unwrap();
    ask(
        &keevoro,
        Op::ScheduleSet,
        Some("Family photos"),
        json!({ "every": "day", "at": "02:30" }),
    );
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), before);
    let shown = ask(&keevoro, Op::ScheduleShow, Some("Family photos"), Value::Null);
    assert_eq!(shown["every"], "Every day at 02:30");
}

/// How the app's window behaves (0.45: keeping running by the clock when
/// it is closed, minimising to the icon, notices, and having said so) is
/// kept in a file of its own, `window.json`, never in `settings.json`. A
/// Keevoro 0.37.1 left has no such file: every choice is as it comes,
/// which keeps Keevoro running only where the computer shows icons by the
/// clock, and choosing leaves the settings file exactly as it was.
#[test]
fn keevoro_0_37_1_left_has_no_window_choices_and_making_them_leaves_the_settings_file_alone() {
    let dir = tempfile::tempdir().unwrap();
    let (keevoro, places) = as_0_37_1_left_it(dir.path());
    let window = keevoro.window_settings();
    assert_eq!(window, keevoro_api::window::WindowSettings::default());
    assert!(window.keep_running_or(true) && !window.keep_running_or(false));
    assert!(window.notifies() && !window.explained_close && !window.minimise_to_tray);
    let settings = places.folders.config.join("settings.json");
    let before = std::fs::read_to_string(&settings).unwrap();
    keevoro
        .change_window_settings(|w| {
            w.keep_running = Some(true);
            w.explained_close = true;
        })
        .unwrap();
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), before);
    assert!(keevoro.window_settings().explained_close);
    // And the backups are all still there, as 0.37.1 left them.
    let listed = ask(&keevoro, Op::BackupsList, None, Value::Null);
    assert_eq!(names(&listed), ["Family photos", "Holiday videos"]);
}

/// Putting a backup right (0.43) writes down when each copy was proved by
/// reading it back in full (catalog step 15, `proved_at`). A backup 0.37.1
/// kept has none: nothing counts as proved, every record still counts as
/// there as it did, and the plan of putting it right reads it as it is.
#[test]
fn a_backup_0_37_1_kept_has_nothing_proved_and_its_records_count_as_they_did() {
    let dir = tempfile::tempdir().unwrap();
    let (keevoro, places) = as_0_37_1_left_it(dir.path());
    let home = places.folders.data.join("backups").join("backup-1");
    let catalog = keevoro_catalog::Catalog::open_in_library(&home).unwrap();
    let target = format!("folder:{}", places.usb.display());
    let records = catalog.sent_to(&target).unwrap();
    assert_eq!(records.len(), 2, "{records:?}");
    assert!(records.iter().all(|r| r.seen == keevoro_catalog::Seen::There));
    assert!(
        catalog.proved_since(&target, 0).unwrap().is_empty(),
        "nothing was proved this way"
    );
    for r in &records {
        assert!(catalog.is_uploaded(&target, &r.key).unwrap());
    }
    // Proving one changes nothing else about it.
    catalog
        .set_proved(&target, &[records[0].key.clone()], 1_795_000_000)
        .unwrap();
    assert_eq!(catalog.proved_since(&target, 1).unwrap().len(), 1);
    assert_eq!(catalog.sent_to(&target).unwrap(), records);
    // The plan reads it, and asks nothing of the destination.
    let plan = ask(&keevoro, Op::BackupPutRightPlan, Some("Family photos"), Value::Null);
    assert_eq!(plan["known"], 0, "{plan}");
}

/// What a backup could not send is remembered for each destination from
/// 0.43 (the catalog setting `not_sent:<target>`), so the next backup can
/// say "These 12 files could not be backed up again". A backup 0.37.1 kept
/// has none: nothing is said as failing again, and backing up works as
/// before.
#[test]
fn a_backup_0_37_1_kept_remembers_nothing_as_failing_last_time() {
    let dir = tempfile::tempdir().unwrap();
    let (_keevoro, places) = as_0_37_1_left_it(dir.path());
    let home = places.folders.data.join("backups").join("backup-1");
    let catalog = keevoro_catalog::Catalog::open_in_library(&home).unwrap();
    let target = format!("folder:{}", places.usb.display());
    assert_eq!(catalog.setting(&format!("not_sent:{target}")).unwrap(), None);
    assert_eq!(catalog.setting(&format!("putting_right:{target}")).unwrap(), None);
}

/// Reports of a problem or an idea sent to Keevoro's maker (0.46) are kept
/// in a file of their own, `reports.json`, never in `settings.json`. A
/// Keevoro 0.37.1 left has none: the list is empty, there is nothing to ask
/// after, and sending one leaves the settings file exactly as it was.
#[test]
fn keevoro_0_37_1_left_has_no_reports_and_sending_one_leaves_the_settings_file_alone() {
    use std::io::{BufRead, BufReader, Read, Write};
    let dir = tempfile::tempdir().unwrap();
    let (keevoro, places) = as_0_37_1_left_it(dir.path());
    // A stand-in for the reports service, which takes one report.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut length = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line.trim().is_empty() {
                break;
            }
            if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                length = value.trim().parse().unwrap();
            }
        }
        reader.read_exact(&mut vec![0; length]).unwrap();
        let answer = r#"{"ok":true,"reference":"K-1","token":"abc123"}"#;
        write!(
            stream,
            "HTTP/1.1 201 Created\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
            answer.len()
        )
        .unwrap();
    });
    let keevoro = keevoro.with_environment(move |name| (name == "KEEVORO_REPORTS_URL").then(|| address.clone()));

    let listed = ask(&keevoro, Op::ReportsList, None, Value::Null);
    assert_eq!(listed["reports"], json!([]));
    let settings = places.folders.config.join("settings.json");
    let before = std::fs::read_to_string(&settings).unwrap();
    let prepared = ask(
        &keevoro,
        Op::ReportsPrepare,
        None,
        json!({ "kind": "idea", "message": "Thank you" }),
    );
    let sent = ask(&keevoro, Op::ReportsSend, None, json!({ "prepared": prepared["body"] }));
    assert_eq!(sent["reference"], "K-1");
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), before);
    let listed = ask(&keevoro, Op::ReportsList, None, Value::Null);
    assert_eq!(listed["reports"][0]["reference"], "K-1");
    // And the backups are all still there, as 0.37.1 left them.
    let backups = ask(&keevoro, Op::BackupsList, None, Value::Null);
    assert_eq!(names(&backups), ["Family photos", "Holiday videos"]);
}

/// Looking again quickly (0.49) keeps when each file last changed, in a
/// new column of the catalog. What 0.37.1 found has none, so each of
/// those files is read as ever by the next look, which fills it in.
#[test]
fn files_0_37_1_found_have_no_time_kept_and_the_next_look_keeps_one() {
    let dir = tempfile::tempdir().unwrap();
    let (keevoro, places) = as_0_37_1_left_it(dir.path());
    let home = places.folders.data.join("backups").join("backup-1");
    let found = || {
        let catalog = keevoro_catalog::Catalog::open_in_library(&home).unwrap();
        let root = catalog
            .sources()
            .unwrap()
            .into_iter()
            .find(|s| s.root.ends_with("Card"))
            .unwrap()
            .root;
        catalog.found_at(&root).unwrap()
    };
    let before = found();
    assert!(!before.is_empty());
    assert!(before.iter().all(|i| i.modified.is_none()), "0.37.1 kept none");

    std::fs::create_dir_all(places.card.join("DCIM")).unwrap();
    std::fs::write(
        places.card.join("DCIM/IMG_0048.jpg"),
        b"\xFF\xD8\xFF\xE0 a made-up photo",
    )
    .unwrap();
    let job = ask(
        &keevoro,
        Op::SourcesLookAgain,
        Some("Family photos"),
        json!({ "source": 1 }),
    );
    keevoro.wait(serde_json::from_value(job["id"].clone()).unwrap());
    keevoro.wait_for_jobs();
    let after = found();
    assert_eq!(after.len(), 1, "{after:?}");
    assert!(after[0].modified.is_some());
}
