//! The app's wiring, proved without a real window.
//!
//! The screens call commands and listen for events; the commands start the
//! API's jobs, and what the jobs say goes back to the window through the
//! listener `lib.rs` sets up. Every piece of that is tested on its own
//! elsewhere. This test drives the whole path at once, the way the screens
//! do: it builds the app exactly as `run` does (the same plugins, commands,
//! listener, `tauri.conf.json` and capabilities), on Tauri's stand-in for a
//! window (`tauri::test`), with Keevoro's folders in a temporary folder. It
//! then invokes commands by the names `ui/src/backend.ts` uses, hears the
//! events by the names it subscribes to, and checks each answer and each
//! event against the types in `backend.ts`.
//!
//! What the stand-in cannot show is the last step inside the web page:
//! that the page's JavaScript receives an event that Tauri has emitted.
//! That is Tauri's own work, and `subscribe` in `backend.ts` is a thin
//! wrapper over it.

use crate::events::tests::shape_of;
use crate::Shared;
use keevoro_api::{Folders, Keevoro};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{get_ipc_response, mock_builder, MockRuntime, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{Listener, Manager, WebviewWindow};

/// Storage keys kept in memory, so the test never touches the computer's
/// keychain.
#[derive(Default)]
struct KeysInMemory(Mutex<BTreeMap<String, String>>);

impl keevoro_engine::connect::SecretStore for KeysInMemory {
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

/// The events the screens subscribe to (`subscribe('...'` in
/// `backend.ts`), with the type each payload has there.
const EVENTS: &[(&str, &str)] = &[
    ("scan", "ScanEvent"),
    ("survey", "SurveyProgress"),
    ("send", "SendEvent"),
    ("check", "CheckEvent"),
];

/// One event the window was sent: its name and what came with it.
type Sent = (String, Value);

/// Every event the window was sent, in order.
#[derive(Clone, Default)]
struct Heard(Arc<(Mutex<Vec<Sent>>, Condvar)>);

impl Heard {
    fn push(&self, name: &str, payload: Value) {
        self.0 .0.lock().unwrap().push((name.to_string(), payload));
        self.0 .1.notify_all();
    }

    fn all(&self) -> Vec<Sent> {
        self.0 .0.lock().unwrap().clone()
    }

    /// The first event called `name` that matches, waiting for it if need be.
    fn wait_for(&self, name: &str, matches: impl Fn(&Value) -> bool) -> Value {
        self.wait_for_nth(name, 0, matches)
    }

    /// The `nth` (from 0) event called `name` that matches, waiting a
    /// generous while for it if need be.
    fn wait_for_nth(&self, name: &str, nth: usize, matches: impl Fn(&Value) -> bool) -> Value {
        let (list, changed) = &*self.0;
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut list = list.lock().unwrap();
        loop {
            if let Some((_, payload)) = list.iter().filter(|(n, p)| n == name && matches(p)).nth(nth) {
                return payload.clone();
            }
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "no {name} event came; heard {:?}", *list);
            list = changed.wait_timeout(list, left).unwrap().0;
        }
    }
}

/// The app, built as `run` builds it, with Keevoro in `dir`, and its main
/// window, listening for every event the screens listen for.
fn the_app(dir: &Path) -> (tauri::App<MockRuntime>, WebviewWindow<MockRuntime>, Heard) {
    let folders = Folders::under(dir);
    // No icon by the clock on the stand-in, and no list of programs the
    // computer starts with: the app is told it has an icon, as on Windows,
    // and is given a list in memory.
    let desktop = crate::Desktop::StandIn {
        tray: crate::closing::Tray::Shown,
    };
    let mut app = crate::app(mock_builder(), desktop, move |_| {
        Ok(Keevoro::open(&folders)
            .with_secrets(KeysInMemory::default())
            .with_environment(|_| None))
    })
    .build(crate::context())
    .expect("the app builds");
    // Starting it as `run` does: the windows `tauri.conf.json` describes
    // are opened, then the app's own setup runs. On the stand-in runtime one
    // turn of the event loop does this and returns; Tauri marks it
    // deprecated only because calling it in a loop would spin.
    #[allow(deprecated)]
    app.run_iteration(|_, _| {});
    // The window its capabilities name.
    let window = app.get_webview_window("main").expect("the app opens its main window");
    let heard = Heard::default();
    for (name, _) in EVENTS {
        let heard = heard.clone();
        app.listen_any(*name, move |event| {
            heard.push(name, serde_json::from_str(event.payload()).expect("events are JSON"));
        });
    }
    (app, window, heard)
}

/// Calls a command as `invoke(command, args)` in `backend.ts` does.
fn invoke(window: &WebviewWindow<MockRuntime>, command: &str, args: Value) -> Result<Value, Value> {
    get_ipc_response(
        window,
        InvokeRequest {
            cmd: command.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: if cfg!(windows) {
                "http://tauri.localhost"
            } else {
                "tauri://localhost"
            }
            .parse()
            .unwrap(),
            body: InvokeBody::Json(args),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map(|body| body.deserialize::<Value>().unwrap())
}

/// Checks a value has the shape of a type in `backend.ts`: nothing it does
/// not name, and everything it does not mark as optional.
fn fits(value: &Value, ty: &str) {
    let shape = shape_of(ty);
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("a {ty} should be an object: {value}"));
    for key in object.keys() {
        assert!(shape.contains_key(key), "{ty} has no field {key}: {value}");
    }
    for (field, optional) in &shape {
        assert!(*optional || object.contains_key(field), "{ty} needs {field}: {value}");
    }
}

/// A photo as a camera might write it.
fn photo() -> Vec<u8> {
    let img = image::RgbImage::from_fn(640, 480, |x, y| {
        image::Rgb([40, u8::try_from(x % 256).unwrap(), u8::try_from(y % 256).unwrap()])
    });
    let mut bytes = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
        .unwrap();
    bytes
}

/// Every file under a folder, with its bytes and when it was last changed.
fn snapshot(folder: &Path) -> BTreeMap<PathBuf, (Vec<u8>, SystemTime)> {
    let mut files = BTreeMap::new();
    let mut folders = vec![folder.to_path_buf()];
    while let Some(next) = folders.pop() {
        for entry in fs::read_dir(&next).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
            } else {
                let changed = fs::metadata(&path).unwrap().modified().unwrap();
                files.insert(path.clone(), (fs::read(&path).unwrap(), changed));
            }
        }
    }
    files
}

#[test]
fn the_app_keeps_its_things_where_tauri_would_and_the_command_line_looks() {
    // The command line finds the app's backups through the API's idea of
    // where an app keeps its things; this is Tauri's idea, for this app.
    let dir = tempfile::tempdir().unwrap();
    let (app, _window, _heard) = the_app(dir.path());
    assert_eq!(app.config().identifier, keevoro_api::keevoro::IDENTIFIER);
    let paths = app.path();
    let shared = Folders::for_this_computer().expect("this computer says where an app's folders go");
    assert_eq!(shared.config, paths.app_config_dir().unwrap());
    assert_eq!(shared.data, paths.app_data_dir().unwrap());
    assert_eq!(shared.cache, paths.app_cache_dir().unwrap().join("previews"));
    assert_eq!(shared.logs, paths.app_log_dir().unwrap());
}

#[test]
fn the_kinds_of_file_looked_for_are_changed_by_the_names_and_shapes_the_screens_use() {
    let dir = tempfile::tempdir().unwrap();
    let (_app, window, _heard) = the_app(dir.path());
    let list = invoke(&window, "file_kinds", json!({})).unwrap();
    for kind in list.as_array().unwrap() {
        fits(kind, "FileKind");
    }
    let added = invoke(&window, "add_file_kind", json!({ "ending": "heix", "kind": "photo" })).unwrap();
    fits(&added, "FileKind");
    // As `changeFileKind` sends it: every part, with null for those left alone.
    let off = json!({ "ending": "heix", "kind": null, "on": false, "to": null });
    assert_eq!(invoke(&window, "change_file_kind", off).unwrap()["on"], false);
    let renamed = json!({ "ending": "heix", "kind": "video", "on": null, "to": "heiz" });
    let renamed = invoke(&window, "change_file_kind", renamed).unwrap();
    assert_eq!(
        (renamed["ending"].as_str(), renamed["kind"].as_str()),
        (Some("heiz"), Some("video"))
    );
    let refused = invoke(&window, "remove_file_kind", json!({ "ending": "jpg" })).unwrap_err();
    assert!(refused.as_str().unwrap().contains("can be switched off"), "{refused}");
    fits(
        &invoke(&window, "remove_file_kind", json!({ "ending": "heiz" })).unwrap(),
        "FileKind",
    );
    let reset = invoke(&window, "reset_file_kinds", json!({})).unwrap();
    assert_eq!(reset.as_array().unwrap().len(), list.as_array().unwrap().len());
}

#[test]
fn backups_are_listed_renamed_removed_and_put_back_by_the_names_and_shapes_the_screens_use() {
    let dir = tempfile::tempdir().unwrap();
    let (_app, window, _heard) = the_app(dir.path());
    invoke(&window, "start_backup", json!({ "name": "My photos" })).unwrap();
    let state = invoke(&window, "start_backup", json!({ "name": "Test 2" })).unwrap();
    fits(&state, "AppState");
    let id = state["backup"]["id"].as_str().unwrap().to_string();
    let listed = invoke(&window, "backup_details", json!({ "id": id })).unwrap();
    fits(&listed, "BackupListing");
    fits(&listed["found"], "BackupCounts");
    assert_eq!(listed["open"], true);

    let refused = invoke(&window, "rename_backup", json!({ "id": id, "to": "my photos" })).unwrap_err();
    assert_eq!(
        refused,
        "There is already a backup called \"my photos\". Choose another name."
    );
    let renamed = invoke(&window, "rename_backup", json!({ "id": id, "to": "Trying it out" })).unwrap();
    assert_eq!(renamed["backup"]["name"], "Trying it out");

    // Asked first, as `removeBackup(id, true)` sends it.
    let asked = invoke(&window, "remove_backup", json!({ "id": id, "dryRun": true })).unwrap();
    fits(&asked, "Removal");
    assert_eq!(asked["removed"], false);
    let home = std::path::PathBuf::from(state["library"].as_str().unwrap());
    assert!(home.is_dir(), "asking changes nothing");
    let removed = invoke(&window, "remove_backup", json!({ "id": id, "dryRun": false })).unwrap();
    fits(&removed, "Removal");
    assert!(!home.exists());
    let state = invoke(&window, "app_state", json!({})).unwrap();
    assert_eq!(state["backup"]["name"], "My photos", "the first left is opened");

    let waiting = invoke(&window, "removed_backups", json!({})).unwrap();
    fits(&waiting[0], "RemovedBackup");
    let back = invoke(&window, "put_back_backup", json!({ "id": id })).unwrap();
    assert_eq!(back["backups"].as_array().map(Vec::len), Some(2));
    assert!(home.is_dir(), "put back where it was");
    invoke(&window, "remove_backup", json!({ "id": id, "dryRun": false })).unwrap();
    let cleared = invoke(&window, "clear_away_backup", json!({ "id": id })).unwrap();
    fits(&cleared, "RemovedBackup");
    assert_eq!(invoke(&window, "removed_backups", json!({})).unwrap(), json!([]));
}

#[test]
fn unfinished_copies_are_said_then_cleared_by_the_names_and_shapes_the_screens_use() {
    let dir = tempfile::tempdir().unwrap();
    let (_app, window, _heard) = the_app(dir.path());
    let folder = dir.path().join("Old backup");
    let old = folder.join(".keevoro-uploads").join("tmp-abandoned");
    std::fs::create_dir_all(old.parent().unwrap()).unwrap();
    std::fs::write(&old, [0u8; 100]).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&old)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(7200))
        .unwrap();
    let place = folder.display().to_string();
    // As `tidyFolder` sends it.
    let first = invoke(&window, "tidy_folder", json!({ "folder": place, "dryRun": true })).unwrap();
    fits(&first, "Tidied");
    assert_eq!(first["files"], 1);
    assert!(old.is_file());
    let done = invoke(&window, "tidy_folder", json!({ "folder": place, "dryRun": false })).unwrap();
    fits(&done, "Tidied");
    assert_eq!(done["cleared"], true);
    assert!(!old.exists());
}

#[test]
fn a_folder_is_looked_inside_by_the_names_and_shapes_the_screens_use_and_nothing_is_added() {
    let dir = tempfile::tempdir().unwrap();
    let (_app, window, _heard) = the_app(dir.path());
    let card = dir.path().join("Card");
    std::fs::create_dir_all(card.join("DCIM")).unwrap();
    std::fs::write(card.join("DCIM").join("IMG_1.jpg"), photo()).unwrap();
    let before = snapshot(&card);
    let place = card.display().to_string();
    invoke(&window, "start_backup", json!({ "name": "Card photos" })).unwrap();
    // Not surveyed yet: no outline, so the screen surveys it first.
    let none = invoke(&window, "folders_inside", json!({ "place": place })).unwrap();
    assert_eq!(none, Value::Null);
    invoke(&window, "survey_place", json!({ "path": place })).unwrap();
    let outline = invoke(&window, "folders_inside", json!({ "place": place })).unwrap();
    fits(&outline, "FoldersView");
    // Where the space goes, from the same survey.
    let map = invoke(
        &window,
        "space_map",
        json!({ "request": { "place": place, "depth": 3, "smallest": 1 } }),
    )
    .unwrap();
    fits(&map, "MapView");
    fits(&map["top"], "MapNode");
    fits(&map["top"]["children"][0], "MapNode");
    fits(&map["trail"][0], "TrailStep");
    fits(&map["kinds"][0], "KindView");
    fits(&outline["folders"][0], "FolderView");
    let folder = outline["folders"][0]["folder"].as_str().unwrap().to_string();
    let page = invoke(
        &window,
        "folder_files",
        json!({ "request": { "place": folder, "from": 0, "count": 100, "sort": "newest", "only": "photos" } }),
    )
    .unwrap();
    fits(&page, "FilesView");
    fits(&page["files"][0], "FileView");
    let path = page["files"][0]["path"].as_str().unwrap().to_string();
    for large in [false, true] {
        invoke(&window, "file_picture", json!({ "path": path, "large": large })).unwrap();
    }
    assert_eq!(snapshot(&card), before, "the folder is exactly as it was");
    let discover = invoke(&window, "discover", json!({})).unwrap();
    assert_eq!(discover["sources"].as_array().unwrap().len(), 0, "nothing was added");
}

#[test]
fn the_screens_commands_start_the_apis_jobs_and_their_events_reach_the_window() {
    let dir = tempfile::tempdir().unwrap();
    let card = dir.path().join("Card");
    let taken = card.join("DCIM").join("100CANON").join("IMG_20190601_120000.jpg");
    fs::create_dir_all(taken.parent().unwrap()).unwrap();
    fs::write(&taken, photo()).unwrap();
    let usb = dir.path().join("USB");
    fs::create_dir_all(&usb).unwrap();
    let card_before = snapshot(&card);
    let photo_bytes = fs::read(&taken).unwrap();
    let arrived = |usb: &Path| snapshot(usb).values().any(|(bytes, _)| *bytes == photo_bytes);

    let (app, window, heard) = the_app(dir.path());
    let keevoro = || app.state::<Shared>().keevoro.wait_for_jobs();

    // The first screen asks what there is, and there is nothing yet.
    let state = invoke(&window, "app_state", json!({})).unwrap();
    fits(&state, "AppState");
    assert_eq!(state["backup"], Value::Null);
    // A refusal reaches the screen as the sentence to show.
    let refused = invoke(&window, "send", json!({})).unwrap_err();
    assert_eq!(refused, "Start a backup first.");
    let state = invoke(&window, "start_backup", json!({ "name": "Card photos" })).unwrap();
    fits(&state, "AppState");
    assert_eq!(state["backup"]["name"], "Card photos");

    // Surveying answers with its findings, and says how far it has got as
    // `survey` events on the way.
    let card_path = card.display().to_string();
    let survey = invoke(&window, "survey_place", json!({ "path": card_path })).unwrap();
    fits(&survey, "Survey");
    assert_eq!(survey["files"], 1, "{survey}");
    let hints = survey["hints"].as_array().unwrap();
    assert!(!hints.is_empty(), "{survey}");
    for hint in hints {
        fits(hint, "Hint");
    }
    // A first word may come before any file is counted, on a busy machine.
    assert_eq!(heard.wait_for("survey", |p| p["files"] == 1)["files"], 1);

    // A search stopped before it gets going ends at once, saying so. It is
    // held before its first file by pausing, which stopping lifts.
    invoke(&window, "pause_work", json!({ "paused": true })).unwrap();
    invoke(&window, "scan_place", json!({ "path": card_path })).unwrap();
    // Updating would close the app and cut the search short, so it waits.
    let update = invoke(&window, "install_update", json!({})).unwrap();
    fits(&update, "UpdateStarted");
    assert_eq!(update["waiting"].as_str(), Some(crate::updates::WAITING));
    invoke(&window, "stop_scanning", json!({})).unwrap();
    let stopped = heard.wait_for("scan", |e| e["finished"] == true);
    assert_eq!(stopped["stopped"], true, "{stopped}");
    assert_eq!(invoke(&window, "activity", json!({})).unwrap()["paused"], false);
    keevoro();

    // Looking through returns at once; the search is heard as `scan` events.
    let started = invoke(&window, "scan_place", json!({ "path": card_path })).unwrap();
    assert_eq!(started, Value::Null);
    let found = heard.wait_for_nth("scan", 1, |e| e["finished"] == true);
    assert_eq!(found["name"], "Card");
    assert_eq!((found["photos"].as_u64(), found["problem"].as_str()), (Some(1), None));
    // The background check for duplicates that a search starts is heard too.
    heard.wait_for_nth("check", 1, |e| e["finished"] == true);
    keevoro();
    let discover = invoke(&window, "discover", json!({})).unwrap();
    assert_eq!(discover["photos"], 1, "{discover}");
    // Looking again with the card in a drawer passes it over, says so, and
    // forgets nothing found on it.
    let drawer = dir.path().join("In a drawer");
    fs::rename(&card, &drawer).unwrap();
    let refused = invoke(&window, "look_again", json!({})).unwrap_err();
    assert!(
        refused.as_str().is_some_and(|said| said.contains("plugged in")),
        "{refused}"
    );
    fs::rename(&drawer, &card).unwrap();
    invoke(&window, "look_again", json!({ "source": discover["sources"][0]["id"] })).unwrap();
    let again = heard.wait_for_nth("scan", 2, |e| e["finished"] == true);
    fits(&again, "ScanEvent");
    keevoro();
    let discover = invoke(&window, "discover", json!({})).unwrap();
    assert_eq!(discover["photos"], 1, "{discover}");

    // Backing up by itself: set, shown, skipped, and stopped.
    let none = invoke(&window, "schedule", json!({})).unwrap();
    fits(&none, "ScheduleView");
    assert_eq!(none["every"], Value::Null);
    let set = invoke(
        &window,
        "set_schedule",
        json!({ "request": { "every": "day", "at": "02:30" } }),
    )
    .unwrap();
    fits(&set, "ScheduleView");
    fits(&set["choice"], "ScheduleChoice");
    assert_eq!(set["every"], "Every day at 02:30");
    let skipped = invoke(&window, "skip_next_run", json!({ "undo": false })).unwrap();
    assert_eq!(skipped["skipping"], true);
    invoke(&window, "window_used", json!({})).unwrap();
    let off = invoke(&window, "set_schedule", json!({ "request": { "every": "off" } })).unwrap();
    assert_eq!(off["every"], Value::Null);
    assert_eq!(off["last"], json!([]));
    // While you were away: nothing ran by itself yet, in the shape the
    // screen reads, and Done is heard.
    let away = invoke(&window, "away", json!({})).unwrap();
    fits(&away, "AwayView");
    assert_eq!(
        (away["backups"].clone(), away["waiting"].clone()),
        (json!([]), json!([]))
    );
    invoke(&window, "away_seen", json!({})).unwrap();

    // How it is arranged, asked once something is found, by the names and
    // shapes the screens use.
    let arranged = invoke(&window, "backup_arrange", json!({})).unwrap();
    fits(&arranged, "ArrangeView");
    assert_eq!(arranged["asked"], false, "a new backup is asked");
    let chosen = invoke(&window, "set_backup_arrange", json!({ "arrange": "folders" })).unwrap();
    fits(&chosen, "ArrangeView");
    assert_eq!(
        (chosen["arrange"].as_str(), chosen["asked"].as_bool()),
        (Some("folders"), Some(true))
    );
    let id = discover["sources"][0]["id"].as_i64().unwrap();
    // Backing it up when it is plugged in: off unless asked, and said in
    // the list of sources once on.
    let arrival = invoke(&window, "source_on_arrival", json!({ "source": id, "on": true })).unwrap();
    fits(&arrival, "OnArrivalView");
    assert_eq!(arrival["on"], true);
    let listed = invoke(&window, "discover", json!({})).unwrap();
    assert_eq!(listed["sources"][0]["onArrival"], true);
    let arrival = invoke(&window, "source_on_arrival", json!({ "source": id, "on": false })).unwrap();
    assert_eq!(arrival["on"], false);
    let source = invoke(&window, "set_source_arrange", json!({ "id": id, "arrange": "byDate" })).unwrap();
    fits(&source, "SourceArrangeView");
    fits(
        &invoke(&window, "source_arrange", json!({ "id": id })).unwrap(),
        "SourceArrangeView",
    );
    invoke(&window, "set_source_arrange", json!({ "id": id, "arrange": "backup" })).unwrap();
    invoke(&window, "set_backup_arrange", json!({ "arrange": "byDate" })).unwrap();

    // A folder to back up to.
    let request = json!({ "request": { "kind": "folder", "place": usb.display().to_string() } });
    let added = invoke(&window, "connect_destination", request).unwrap();
    assert!(added.as_str().is_some_and(|s| !s.is_empty()), "{added}");

    // Nothing running: the progress says so, in the shape the screen reads.
    let idle = invoke(&window, "send_progress", json!({})).unwrap();
    fits(&idle, "SendProgress");
    assert_eq!(idle["running"], false);

    // Pause and carry on reach the API, as the Activity page shows.
    invoke(&window, "pause_work", json!({ "paused": true })).unwrap();
    assert_eq!(invoke(&window, "activity", json!({})).unwrap()["paused"], true);
    invoke(&window, "pause_work", json!({ "paused": false })).unwrap();
    assert_eq!(invoke(&window, "activity", json!({})).unwrap()["paused"], false);

    // Stop, pressed the moment the backup first says anything, while the
    // screen shows "Getting ready". The progress the screen asks for then
    // says it is stopping, and it ends having sent nothing.
    let when_stopped = Arc::new(Mutex::new(None));
    {
        let window = window.clone();
        let when_stopped = when_stopped.clone();
        let pressed = std::sync::Once::new();
        app.listen_any("send", move |event| {
            let said: Value = serde_json::from_str(event.payload()).unwrap();
            if said["finished"] == false {
                pressed.call_once(|| {
                    invoke(&window, "stop_sending", json!({})).unwrap();
                    let progress = invoke(&window, "send_progress", json!({})).unwrap();
                    *when_stopped.lock().unwrap() = Some(progress);
                });
            }
        });
    }
    assert_eq!(invoke(&window, "send", json!({})).unwrap(), Value::Null);
    let first = heard.wait_for("send", |_| true);
    assert!(
        first["line"].as_str().is_some_and(|l| l.starts_with("Getting ready")),
        "{first}"
    );
    let ended = heard.wait_for("send", |e| e["finished"] == true);
    keevoro();
    assert_eq!(ended["problem"], Value::Null, "{ended}");
    let progress = when_stopped.lock().unwrap().clone().expect("stop was pressed");
    fits(&progress, "SendProgress");
    assert_eq!(
        (&progress["running"], &progress["stopping"]),
        (&json!(true), &json!(true))
    );
    assert!(!arrived(&usb), "a backup stopped before its first file sends nothing");

    // Backing up again sends the photo, heard as `send` events to the end.
    invoke(&window, "send", json!({})).unwrap();
    let progress = invoke(&window, "send_progress", json!({})).unwrap();
    fits(&progress, "SendProgress");
    // A running backup brings its whole estimate, in the shapes the screen reads.
    if progress["running"] == true {
        let estimate = &progress["estimate"];
        fits(estimate, "BackupEstimate");
        for stage in estimate["stages"].as_array().unwrap() {
            fits(stage, "BackupStage");
        }
    }
    let done = heard.wait_for_nth("send", 1, |e| e["finished"] == true);
    keevoro();
    assert_eq!(done["problem"], Value::Null, "{done}");
    assert!(!done["summary"].as_array().unwrap().is_empty(), "{done}");
    assert!(arrived(&usb), "the photo is in the destination folder");
    let said = invoke(&window, "last_backup_said", json!({})).unwrap();
    assert_eq!(said["summary"], done["summary"]);

    // What Keevoro found when it last looked at the folder reaches the
    // backup screen in the shape it reads.
    let discover = invoke(&window, "discover", json!({})).unwrap();
    let place = &discover["destinations"][0];
    fits(place, "Destination");
    fits(&place["looked"], "Looked");
    assert_eq!(
        (place["looked"]["fresh"].as_bool(), place["there"].as_u64()),
        (Some(true), Some(1))
    );
    // The backup is deleted from the folder, and the screen is opened
    // after the last look has grown old: the room it asks for looks first,
    // and what was found is what the screen then reads.
    for entry in fs::read_dir(&usb).unwrap().map(Result::unwrap) {
        if !entry.file_name().to_string_lossy().starts_with('.') {
            let path = entry.path();
            if path.is_dir() {
                fs::remove_dir_all(path).unwrap();
            } else {
                fs::remove_file(path).unwrap();
            }
        }
    }
    let home = app.state::<Shared>().keevoro.backups().current().unwrap().home;
    {
        let catalog = keevoro_catalog::Catalog::open_in_library(&home).unwrap();
        let target = keevoro_engine::connect::destinations(&catalog).unwrap()[0]
            .target
            .clone()
            .unwrap();
        let mut last = keevoro_engine::confirm::last_look(&catalog, &target).unwrap().unwrap();
        last.at -= 3_600;
        catalog
            .set_setting(&format!("looked:{target}"), &serde_json::to_string(&last).unwrap())
            .unwrap();
    }
    let room = invoke(&window, "room", json!({})).unwrap();
    fits(&room, "Room");
    let discover = invoke(&window, "discover", json!({})).unwrap();
    let place = &discover["destinations"][0];
    assert_eq!(
        (place["there"].as_u64(), discover["waitingToGather"].as_u64()),
        (Some(0), Some(1))
    );
    let gone = place["looked"]["said"][0].as_str().unwrap_or_default();
    assert!(
        gone.ends_with(
            "looks emptied or replaced: it no longer holds the file Keevoro backed up there. It will be sent again."
        ),
        "{place}"
    );
    // Backing up again sends it, and says so.
    invoke(&window, "send", json!({})).unwrap();
    let again = heard.wait_for_nth("send", 2, |e| e["finished"] == true);
    keevoro();
    let summary = again["summary"].to_string();
    assert!(
        summary.contains("Restored 1 file that had gone from there."),
        "{summary}"
    );
    assert!(arrived(&usb), "the photo is back in the destination folder");

    // Every event the window was sent has the shape its screen expects.
    for (name, payload) in heard.all() {
        let ty = EVENTS.iter().find(|(n, _)| *n == name).unwrap().1;
        fits(&payload, ty);
    }
    // And the source was only ever read.
    assert_eq!(snapshot(&card), card_before, "the source is exactly as it was");
}

#[test]
fn the_check_for_duplicates_says_when_it_waits_for_a_source_that_is_not_plugged_in() {
    let dir = tempfile::tempdir().unwrap();
    let (app, window, heard) = the_app(dir.path());
    invoke(&window, "start_backup", json!({ "name": "Card photos" })).unwrap();
    // Two large photos found by a quick search, so the check has them to read.
    let card = dir.path().join("Card");
    fs::create_dir_all(&card).unwrap();
    for (name, seed) in [("IMG_1.jpg", 1u8), ("IMG_2.jpg", 2u8)] {
        let mut bytes = photo();
        bytes.extend((0..3_000_000u32).map(|n| u8::try_from(n % 251).unwrap() ^ seed));
        fs::write(card.join(name), bytes).unwrap();
    }
    let home = app.state::<Shared>().keevoro.backups().current().unwrap().home;
    let mut catalog = keevoro_catalog::Catalog::open_in_library(&home).unwrap();
    let quick = keevoro_scan::ScanOptions {
        quick: true,
        ..Default::default()
    };
    keevoro_engine::scan_into_with(&mut catalog, &card, None, &quick, &mut |_| {}, &mut || {}).unwrap();
    assert_eq!(catalog.unhashed_count().unwrap(), 2);
    fs::rename(&card, dir.path().join("In a drawer")).unwrap();

    // As the screens ask when they open: the check ends at once and says why.
    invoke(&window, "keep_checking", json!({})).unwrap();
    let ended = heard.wait_for("check", |e| e["finished"] == true);
    fits(&ended, "CheckEvent");
    let said = ended["said"][0].as_str().unwrap_or_default().to_string();
    assert!(
        said.ends_with(
            "can't be found, so its 2 files not yet checked for duplicates will be checked when its drive is back."
        ),
        "{ended}"
    );
    app.state::<Shared>().keevoro.wait_for_jobs();
    // Opened again, the foot says the same, and asking says why nothing happens.
    let left = invoke(&window, "keep_checking", json!({})).unwrap();
    fits(&left, "DuplicatesLeft");
    assert_eq!(left["said"][0], said.as_str());
    app.state::<Shared>().keevoro.wait_for_jobs();
    assert_eq!(
        invoke(&window, "check_duplicates_now", json!({})).unwrap_err(),
        said.as_str()
    );
    assert_eq!(catalog.unhashed_count().unwrap(), 2, "nothing found is changed");
}

#[test]
fn closing_minimising_and_starting_with_the_computer_are_set_by_the_names_and_shapes_the_screens_use() {
    let dir = tempfile::tempdir().unwrap();
    let (app, window, _heard) = the_app(dir.path());
    // As it comes on a computer that shows the icon: closing keeps Keevoro
    // running, and nothing else is on.
    let view = invoke(&window, "keep_running", json!({})).unwrap();
    fits(&view, "KeepRunningView");
    assert_eq!(
        (
            &view["keepRunning"],
            &view["minimiseToTray"],
            &view["startWithComputer"],
            &view["tray"]
        ),
        (&json!(true), &json!(false), &json!(false), &json!("shown"))
    );
    // As `setKeepRunning` sends it: only the parts changed.
    let change = json!({ "change": { "keepRunning": false, "minimiseToTray": true } });
    let view = invoke(&window, "set_keep_running", change).unwrap();
    assert_eq!(
        (&view["keepRunning"], &view["minimiseToTray"]),
        (&json!(false), &json!(true))
    );
    let view = invoke(
        &window,
        "set_keep_running",
        json!({ "change": { "startWithComputer": true } }),
    )
    .unwrap();
    assert_eq!(view["startWithComputer"], true);
    assert_eq!(view["notify"], true, "notices are on until switched off");
    let view = invoke(&window, "set_keep_running", json!({ "change": { "notify": false } })).unwrap();
    assert_eq!(view["notify"], false);
    assert!(app.state::<Shared>().desk.startup.is_on().unwrap());
    // Kept in its own file, never in the settings file.
    let kept = app.state::<Shared>().keevoro.window_settings();
    assert_eq!((kept.keep_running, kept.minimise_to_tray), (Some(false), true));
    // Starting again from nothing takes Keevoro off the computer's list too.
    invoke(&window, "reset_everything", json!({})).unwrap();
    assert!(!app.state::<Shared>().desk.startup.is_on().unwrap());
    assert_eq!(invoke(&window, "keep_running", json!({})).unwrap()["keepRunning"], true);
}

#[test]
fn what_waits_for_the_window_is_taken_once_and_quitting_with_work_in_hand_asks_first() {
    let dir = tempfile::tempdir().unwrap();
    let (app, window, _heard) = the_app(dir.path());
    // Opened as Keevoro starts, nothing waits.
    assert_eq!(invoke(&window, "window_ask", json!({})).unwrap(), Value::Null);
    // The icon's "Settings…" brings the window forward on its Settings page.
    crate::window::show(app.handle(), Some("settings"));
    let asked = invoke(&window, "window_ask", json!({})).unwrap();
    fits(&asked, "WindowAsk");
    assert_eq!(asked["page"], "settings");
    assert_eq!(
        invoke(&window, "window_ask", json!({})).unwrap(),
        Value::Null,
        "taken once"
    );

    // A search held by the pause is work in hand: quitting asks first.
    let card = dir.path().join("Card");
    fs::create_dir_all(card.join("DCIM")).unwrap();
    fs::write(card.join("DCIM").join("IMG_1.jpg"), photo()).unwrap();
    invoke(&window, "start_backup", json!({ "name": "Card photos" })).unwrap();
    invoke(&window, "pause_work", json!({ "paused": true })).unwrap();
    invoke(&window, "scan_place", json!({ "path": card.display().to_string() })).unwrap();
    let answer = invoke(&window, "quit_keevoro", json!({ "stop": false })).unwrap();
    fits(&answer, "QuitAnswer");
    assert_eq!(
        answer["asked"],
        "Keevoro is looking through a source. Stop it and quit? Nothing is lost, and it can look again later."
    );
    invoke(&window, "stop_scanning", json!({})).unwrap();
    invoke(&window, "pause_work", json!({ "paused": false })).unwrap();
    app.state::<Shared>().keevoro.wait_for_jobs();
}
