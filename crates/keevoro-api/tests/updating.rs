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
    drone: PathBuf,
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
            ("DRONE", &self.drone),
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
        drone: dir.join("Drone"),
        usb: dir.join("USB"),
        folders: Folders::under(&dir.join("home")),
    };
    std::fs::create_dir_all(&places.folders.config).unwrap();
    std::fs::write(
        places.folders.config.join("settings.json"),
        places.fill(&fixture("settings.json")),
    )
    .unwrap();
    for (id, sql) in [("backup-1", "family photos.sql"), ("backup-2", "drone footage.sql")] {
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
    assert_eq!(names(&backups), ["Family photos", "Drone footage"]);
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
    assert_eq!(names(&sources), ["Mum's card", "Drone"]);
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

    // What the backup and each source are for: photos only for the backup,
    // videos only for the drone, which 0.39.0 lost.
    let wants = ask(&keevoro, Op::BackupWants, family, Value::Null);
    assert_eq!(wants["wants"], json!(["photos"]), "{wants}");
    let drone = ask(&keevoro, Op::SourcesWants, family, json!({ "source": 2 }));
    assert_eq!(
        (drone["own"].as_bool(), &drone["wants"]),
        (Some(true), &json!(["videos"])),
        "{drone}"
    );
    let card = ask(&keevoro, Op::SourcesWants, family, json!({ "source": 1 }));
    assert_eq!(card["own"], false, "the card still follows its backup: {card}");
    let wants = ask(&keevoro, Op::BackupWants, Some("Drone footage"), Value::Null);
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
    // video passed over), two drone videos, one photo not kept.
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

    // Whether previews are made, each backup's own choice.
    assert_eq!(ask(&keevoro, Op::BackupPreviews, family, Value::Null), true);
    assert_eq!(
        ask(&keevoro, Op::BackupPreviews, Some("Drone footage"), Value::Null),
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
        json!({ "backup": "Drone footage", "to": "Drone films" }),
    );
    let again = ask(&keevoro, Op::BackupsList, None, Value::Null);
    assert_eq!(names(&again), ["Family photos", "Drone films"]);
    assert_eq!(
        ask(&keevoro, Op::KeevoroSettings, None, Value::Null)["ownCodecs"]["on"],
        false
    );
    assert_eq!(ask(&keevoro, Op::AgentsAccess, None, Value::Null)["mayChange"], true);
    assert_eq!(
        ask(&keevoro, Op::BackupWants, family, Value::Null)["wants"],
        json!(["photos"])
    );

    // A backup 0.37.1 kept can be removed, saying where it went and what it
    // looked through, and put back with everything in its record in place.
    let removal = ask(&keevoro, Op::BackupsForget, None, json!({ "backup": "Family photos" }));
    assert_eq!(removal["removed"], true);
    assert_eq!(removal["destinations"].as_array().map(Vec::len), Some(2), "{removal}");
    assert_eq!(removal["keysFor"], json!(["the Cloudflare R2 bucket family-photos"]));
    assert_eq!(
        names(&ask(&keevoro, Op::BackupsList, None, Value::Null)),
        ["Drone films"]
    );
    ask(&keevoro, Op::BackupsPutBack, None, json!({ "backup": "Family photos" }));
    assert_eq!(
        names(&ask(&keevoro, Op::SourcesList, family, Value::Null)),
        ["Mum's card", "Drone"]
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
