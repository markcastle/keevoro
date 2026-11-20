//! The command line, run as a person or a script runs it, against Keevoro
//! in a temporary folder with a keychain in memory and no network.

use crate::commands::{program, ROWS};
use crate::{run, Code, Failure, Io};
use keevoro_api::{call, entries, FrontEnd, Keevoro, Op, SecretStore};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Storage keys kept in memory, so no test touches the real keychain.
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

/// Pausing is one switch for the whole program, and stopping a job lifts
/// it, so tests that run jobs take turns.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// Takes a turn, and lifts any pause however the test ends.
struct Turn(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

fn one_at_a_time() -> Turn {
    Turn(ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
}

impl Drop for Turn {
    fn drop(&mut self) {
        lift_the_pause();
    }
}

/// Lifts the pause, through the API as any program would.
fn lift_the_pause() {
    let dir = tempfile::tempdir().unwrap();
    keevoro_in(dir.path(), FrontEnd::App).carry_on();
}

/// No network: there is never an update.
struct Offline;

impl keevoro_update::Web for Offline {
    fn text(&self, _: &str) -> Result<String, keevoro_update::UpdateError> {
        Err(keevoro_update::UpdateError::Network("offline".into()))
    }
    fn bytes(&self, _: &str) -> Result<Vec<u8>, keevoro_update::UpdateError> {
        Err(keevoro_update::UpdateError::Network("offline".into()))
    }
}

/// Keevoro's folders, as one of its programs would open them.
fn keevoro_in(dir: &Path, front_end: FrontEnd) -> Keevoro {
    Keevoro::open(&keevoro_api::Folders::under(&dir.join("home")))
        .as_front_end(front_end)
        .with_secrets(KeysInMemory::default())
        .with_web(Offline)
        .with_progress_every(std::time::Duration::ZERO)
}

/// What one command did.
struct Ran {
    code: Code,
    out: String,
    err: String,
}

impl Ran {
    fn json(&self) -> Value {
        serde_json::from_str(self.out.trim()).unwrap_or_else(|e| panic!("{e}: {}", self.out))
    }
}

/// A person, or a script, at a command line.
struct At<'a> {
    dir: &'a Path,
    env: Vec<(&'static str, String)>,
    stdin: String,
    watched: bool,
    keyboard: bool,
}

impl<'a> At<'a> {
    fn new(dir: &'a Path) -> Self {
        Self {
            dir,
            env: Vec::new(),
            stdin: String::new(),
            watched: false,
            keyboard: false,
        }
    }

    fn keevoro(&self, args: &[&str]) -> Ran {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut input = std::io::Cursor::new(self.stdin.clone().into_bytes());
        let env = self.env.clone();
        let lookup = move |name: &str| env.iter().find(|(n, _)| *n == name).map(|(_, v)| v.clone());
        let mut io = Io {
            out: &mut out,
            err: &mut err,
            input: &mut input,
            env: &lookup,
            watched: self.watched,
            keyboard: self.keyboard,
            ask_hidden: &|_| None,
        };
        let dir = self.dir.to_path_buf();
        let opener = move |_: &Io<'_>| -> Result<Keevoro, Failure> { Ok(keevoro_in(&dir, FrontEnd::CommandLine)) };
        let mut all = vec!["keevoro".into()];
        all.extend(args.iter().map(|a| std::ffi::OsString::from(*a)));
        let code = run(all, &mut io, &opener);
        Ran {
            code,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
        }
    }
}

fn jpeg(shade: u8) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(640, 480, image::Rgb([shade, 120, 60]));
    let mut bytes = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
        .unwrap();
    bytes
}

/// A memory card with an album, a dated photo, a duplicate and a picture
/// too small to be a photo.
fn card(dir: &Path) -> PathBuf {
    let card = dir.join("Card");
    for (rel, shade) in [
        ("Holidays/Cornwall/IMG_20080812_101500.jpg", 10),
        ("Holidays/Cornwall/IMG_20080813_090000.jpg", 90),
        ("DCIM/100CANON/IMG_20120304_153012.jpg", 170),
        ("Backup/IMG_20080812_101500.jpg", 10),
    ] {
        let p = card.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, jpeg(shade)).unwrap();
    }
    let tiny = image::RgbImage::from_pixel(240, 180, image::Rgb([90, 90, 90]));
    tiny.save(card.join("DCIM/1952.png")).unwrap();
    card
}

fn text(path: &Path) -> String {
    path.display().to_string()
}

/// Every file under `dir`.
fn everything_written(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    let mut folders = vec![dir.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else if let Ok(bytes) = std::fs::read(&path) {
                found.push((path, bytes));
            }
        }
    }
    found
}

#[test]
fn every_operation_the_register_offers_the_command_line_has_a_command() {
    let called: HashSet<Op> = ROWS.iter().flat_map(|row| row.ops.iter().copied()).collect();
    for entry in entries().filter(|e| e.command_line) {
        assert!(
            called.contains(&entry.op),
            "{} is offered to the command line, but no command calls it. Add a row for it in commands.rs.",
            entry.name
        );
    }
    for row in ROWS {
        for op in row.ops {
            assert!(
                op.entry().command_line,
                "{} calls {}, which is not for the command line",
                row.typed(),
                op.name()
            );
        }
    }
    let typed: HashSet<&[&str]> = ROWS.iter().map(|row| row.words).collect();
    assert_eq!(typed.len(), ROWS.len(), "two commands are typed the same way");
}

#[test]
fn the_server_for_an_ai_agent_is_offered_and_takes_no_backup() {
    let dir = tempfile::tempdir().unwrap();
    let at = At::new(dir.path());
    let ran = at.keevoro(&["--help"]);
    assert!(ran.out.contains("mcp"), "{}", ran.out);
    let ran = at.keevoro(&["mcp", "--help"]);
    assert_eq!(ran.code, Code::Worked);
    assert!(ran.out.contains("Model Context Protocol"), "{}", ran.out);
    for args in [&["mcp", "-b", "Card photos"][..], &["-b", "Card photos", "mcp"][..]] {
        let ran = at.keevoro(args);
        assert_eq!(ran.code, Code::NotUnderstood, "{args:?}");
        assert!(ran.err.contains("takes no -b"), "{}", ran.err);
    }
}

#[test]
fn what_each_command_does_is_the_registers_own_words() {
    let help = program().render_long_help().to_string();
    for row in ROWS {
        let about = row.about();
        assert!(!about.contains('\u{2014}') && !about.contains('\u{2013}'), "{about}");
        assert!(about.ends_with('.'), "{about}");
        if row.about.is_none() {
            assert_eq!(about, format!("{}.", row.ops[0].entry().does));
        }
        if row.words.len() == 1 {
            assert!(help.contains(&about), "`keevoro --help` should say: {about}");
        }
        // A job can be left to run on its own, because the register says
        // it is one.
        let top = program();
        let first = top.find_subcommand(row.words[0]).unwrap();
        let command = match row.words.get(1) {
            Some(second) => first.find_subcommand(second).unwrap(),
            None => first,
        };
        let detach = command.get_arguments().any(|a| a.get_id() == "detach");
        assert_eq!(detach, row.starts_a_job(), "{}", row.typed());
    }
    let ran = At::new(Path::new(".")).keevoro(&["--help"]);
    assert_eq!(ran.code, Code::Worked);
    assert!(ran.out.contains("Usage: keevoro"), "{}", ran.out);
}

#[test]
fn a_first_backup_from_start_to_finish() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let card = card(dir.path());
    let usb = dir.path().join("USB");
    let at = At::new(dir.path());

    let ran = at.keevoro(&["backups"]);
    assert!(ran.out.contains("There are no backups yet"), "{}", ran.out);
    let ran = at.keevoro(&["backups", "start", "Drone footage"]);
    assert_eq!(ran.code, Code::Worked);
    assert!(
        ran.out.starts_with("Started the backup \"Drone footage\"."),
        "{}",
        ran.out
    );

    let ran = at.keevoro(&["look", &text(&card), "-b", "drone footage"]);
    assert_eq!(ran.code, Code::Worked, "{}", ran.err);
    assert!(ran.out.contains("Looked through Card: found"), "{}", ran.out);

    let ran = at.keevoro(&["found"]);
    assert!(ran.out.starts_with("Found 3 photos and 0 videos"), "{}", ran.out);
    assert!(ran.out.contains("1 duplicate left out"), "{}", ran.out);
    assert!(ran.out.contains("1 picture set aside"), "{}", ran.out);

    let ran = at.keevoro(&["collections"]);
    assert!(ran.out.contains("Holidays / Cornwall"), "{}", ran.out);
    let ran = at.keevoro(&["photos", "Cornwall"]);
    assert_eq!(ran.out.lines().count(), 2, "{}", ran.out);
    let first = ran.out.split_whitespace().next().unwrap().to_string();
    let ran = at.keevoro(&["skip", "Cornwall"]);
    assert!(
        ran.out.starts_with("Leaving out that collection (2 files)"),
        "{}",
        ran.out
    );
    let ran = at.keevoro(&["keep", "--photo", &first]);
    assert!(ran.out.starts_with("Keeping that one"), "{}", ran.out);
    let ran = at.keevoro(&["set-aside"]);
    assert!(ran.out.contains("1952.png"), "{}", ran.out);

    let ran = at.keevoro(&["destinations", "add", "--folder", &text(&usb)]);
    assert_eq!(ran.code, Code::Worked, "{}", ran.err);
    assert!(ran.out.contains("works, and will back up there"), "{}", ran.out);
    let ran = at.keevoro(&["rehearse"]);
    assert!(ran.out.contains("2 files"), "{}", ran.out);

    let ran = at.keevoro(&["run"]);
    assert_eq!(ran.code, Code::Worked, "{}{}", ran.out, ran.err);
    assert!(ran.out.contains("Safely backed up 2 files"), "{}", ran.out);
    assert!(
        ran.err.contains("Getting ready"),
        "the steps are said as it goes: {}",
        ran.err
    );
    assert!(usb.join("originals").is_dir());

    let ran = at.keevoro(&["history"]);
    assert!(
        ran.out.starts_with("1. ") && ran.out.contains("to the folder"),
        "{}",
        ran.out
    );
    let ran = at.keevoro(&["history", "1"]);
    for said in [
        "Stages, with time paused left out:",
        "Originals:",
        "2 files sent",
        "What it said at the end:",
    ] {
        assert!(ran.out.contains(said), "{said}: {}", ran.out);
    }
    let ran = at.keevoro(&["history", "1", "--json"]);
    assert_eq!(ran.json()[0]["detail"]["destinations"][0]["originals"]["sent"], 2);
    assert_eq!(at.keevoro(&["history", "9"]).code, Code::NotFound);
    let ran = at.keevoro(&["last-backup"]);
    assert!(ran.out.contains("Safely backed up 2 files"), "{}", ran.out);
    let ran = at.keevoro(&["check"]);
    assert_eq!(ran.code, Code::Worked, "{}{}", ran.out, ran.err);
    assert!(
        ran.out.contains("Your backup: all 2 originals are there."),
        "{}",
        ran.out
    );
    let ran = at.keevoro(&["status"]);
    assert_eq!(ran.out.trim(), "Nothing is running.");
    // A job asked after by its number once it has ended says how it went.
    let ran = at.keevoro(&["look", "--again", "--json"]);
    let started: Value = serde_json::from_str(ran.out.lines().next().unwrap()).unwrap();
    let ran = at.keevoro(&["status", &started["id"].to_string()]);
    assert!(
        ran.out
            .starts_with(&format!("Job {} has ended.\nLooked through Card: found", started["id"])),
        "{}",
        ran.out
    );

    let ran = at.keevoro(&["destinations", "remove", "1"]);
    assert!(ran.out.starts_with("No longer backing up to the folder"), "{}", ran.out);
    assert!(usb.join("originals").is_dir(), "nothing stored there was touched");
    let ran = at.keevoro(&["sources", "forget", "Card"]);
    assert_eq!(ran.code, Code::Worked, "{}", ran.err);
    assert!(
        card.join("DCIM/1952.png").is_file(),
        "nothing on the source was touched"
    );
}

#[test]
fn json_is_exactly_what_the_api_answers() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let card = card(dir.path());
    let at = At::new(dir.path());
    at.keevoro(&["backups", "start", "Card photos"]);
    at.keevoro(&["look", &text(&card)]);
    let api = keevoro_in(dir.path(), FrontEnd::App);
    for (typed, op, request) in [
        (vec!["backups"], Op::BackupsList, json!({})),
        (vec!["sources"], Op::SourcesList, Value::Null),
        (vec!["found"], Op::FoundSummary, Value::Null),
        (vec!["collections"], Op::CollectionsList, Value::Null),
        (
            vec!["photos", "Holidays / Cornwall"],
            Op::CollectionsPhotos,
            json!({ "collection": "Holidays / Cornwall" }),
        ),
        (vec!["set-aside"], Op::SetAsideList, Value::Null),
        (vec!["duplicates"], Op::DuplicatesList, Value::Null),
        (vec!["destinations"], Op::DestinationsList, Value::Null),
        (vec!["room"], Op::DestinationsRoom, json!({})),
        (vec!["rehearse"], Op::BackupRehearse, json!({})),
        (vec!["history"], Op::BackupHistory, json!({})),
        (vec!["previews"], Op::BackupPreviews, json!({})),
        (vec!["about"], Op::KeevoroAbout, Value::Null),
        (vec!["settings"], Op::KeevoroSettings, json!({})),
        (vec!["agents"], Op::AgentsAccess, json!({})),
        (vec!["update", "--check"], Op::KeevoroUpdate, json!({})),
    ] {
        let mut args = typed.clone();
        args.push("--json");
        let ran = at.keevoro(&args);
        assert_eq!(ran.code, Code::Worked, "{typed:?}: {}", ran.err);
        assert_eq!(ran.json(), call(&api, op, None, request).unwrap(), "{typed:?}");
    }

    // A job says each thing on a line of its own: how it started, then the
    // same events the app hears, the last one last.
    let ran = at.keevoro(&["look", "--again", "--json"]);
    let lines: Vec<Value> = ran.out.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines[0]["op"], "sources.look_again");
    assert_eq!(lines[0]["startedBy"], "commandLine");
    assert!(lines[1..]
        .iter()
        .all(|e| e["job"] == lines[0]["id"] && e["event"] == "scan"));
    assert_eq!(lines.last().unwrap()["said"]["finished"], true);
    assert!(ran.err.is_empty(), "nothing else is said: {}", ran.err);
}

#[test]
fn exit_codes_say_what_happened_without_reading_the_words() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let card = card(dir.path());
    let at = At::new(dir.path());

    assert_eq!(at.keevoro(&["frobnicate"]).code, Code::NotUnderstood);
    assert_eq!(at.keevoro(&["backups", "-b", "x"]).code, Code::NotUnderstood);
    assert_eq!(at.keevoro(&["keep"]).code, Code::NotUnderstood);

    let none = at.keevoro(&["found"]);
    assert_eq!((none.code, none.err.trim()), (Code::NotFound, "Choose a backup first."));
    at.keevoro(&["backups", "start", "Card photos"]);
    assert_eq!(at.keevoro(&["found", "-b", "Holidays"]).code, Code::NotFound);
    assert_eq!(
        at.keevoro(&["look", &text(&dir.path().join("Unplugged"))]).code,
        Code::NotFound
    );
    assert_eq!(at.keevoro(&["look", &text(&card)]).code, Code::Worked);
    assert_eq!(at.keevoro(&["photos", "Narnia"]).code, Code::NotFound);
    assert_eq!(
        at.keevoro(&["stop"]).code,
        Code::Worked,
        "nothing to stop is no problem"
    );
    assert_eq!(at.keevoro(&["stop", "12"]).code, Code::NotFound);
    assert_eq!(at.keevoro(&["follow", "12"]).code, Code::NotFound);

    // Nowhere to back up to is refused; a destination out of reach is said.
    assert_eq!(at.keevoro(&["run"]).code, Code::Refused);
    let usb = dir.path().join("USB");
    at.keevoro(&["destinations", "add", "--folder", &text(&usb)]);
    std::fs::remove_dir_all(&usb).unwrap();
    std::fs::write(&usb, "in the way").unwrap();
    let ran = at.keevoro(&["run"]);
    assert_eq!(ran.code, Code::Unreachable, "{}{}", ran.out, ran.err);
    assert_eq!(at.keevoro(&["check"]).code, Code::Unreachable);
    std::fs::remove_file(&usb).unwrap();

    // A file gone from its source since it was found cannot be backed up.
    std::fs::remove_file(card.join("DCIM/100CANON/IMG_20120304_153012.jpg")).unwrap();
    let ran = at.keevoro(&["run"]);
    assert_eq!(ran.code, Code::SomeFiles, "{}{}", ran.out, ran.err);
    assert!(ran.out.contains("Couldn't back up 1 file"), "{}", ran.out);

    // Another program working on the backup.
    let app = keevoro_in(dir.path(), FrontEnd::App);
    app.pause();
    let named = app.backups().find("Card photos").unwrap();
    let job = app.look(&named, &card, None).unwrap();
    let busy = at.keevoro(&["look", "--again"]);
    assert_eq!(busy.code, Code::Busy);
    assert!(busy.err.contains("in the Keevoro app"), "{}", busy.err);
    // The app shares what it is doing a few times a second.
    let started = std::time::Instant::now();
    let ran = loop {
        let ran = at.keevoro(&["status"]);
        if !ran.out.starts_with("Nothing is running") || started.elapsed().as_secs() > 20 {
            break ran;
        }
        std::thread::yield_now();
    };
    assert!(
        ran.out
            .contains("Looking through a source for \"Card photos\" in the Keevoro app"),
        "{}",
        ran.out
    );
    let ran = at.keevoro(&["stop"]);
    assert!(ran.out.starts_with("Asked it to stop"), "{}", ran.out);
    app.carry_on();
    app.wait(job.id);
    app.wait_for_jobs();

    // Resetting is asked about first, and only a yes will do.
    assert_eq!(
        at.keevoro(&["reset"]).code,
        Code::NotUnderstood,
        "no keyboard and no --yes"
    );
    let mut asked = At::new(dir.path());
    asked.keyboard = true;
    asked.stdin = "no\n".into();
    assert_eq!(asked.keevoro(&["reset"]).code, Code::Refused);
    assert_eq!(at.keevoro(&["backups", "--json"]).json().as_array().unwrap().len(), 1);
    let ran = at.keevoro(&["reset", "--yes"]);
    assert_eq!(ran.code, Code::Worked, "{}", ran.err);
    assert!(ran.out.contains("1 backup forgotten"), "{}", ran.out);
}

#[test]
fn the_app_and_the_command_line_see_each_others_backups_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let at = At::new(dir.path());
    // The app is open the whole time, from before anything was made.
    let app = keevoro_in(dir.path(), FrontEnd::App);
    app.start_backup("Family photos").unwrap();
    assert!(at.keevoro(&["backups"]).out.starts_with("Family photos"));
    at.keevoro(&["backups", "start", "Drone footage"]);
    at.keevoro(&["backups", "rename", "Family photos", "Family"]);
    let names: Vec<String> = app.list_backups().into_iter().map(|b| b.name).collect();
    assert_eq!(names, ["Family", "Drone footage"]);
    app.forget_backup("Drone footage").unwrap();
    assert_eq!(at.keevoro(&["backups", "--json"]).json().as_array().unwrap().len(), 1);
}

#[test]
fn progress_is_one_line_rewritten_when_watched_and_plain_lines_when_not() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let card = card(dir.path());
    let mut at = At::new(dir.path());
    at.keevoro(&["backups", "start", "Card photos"]);
    let piped = at.keevoro(&["look", &text(&card)]);
    assert!(
        !piped.err.contains('\r') && !piped.err.contains('\u{1b}'),
        "{:?}",
        piped.err
    );
    assert!(piped.err.lines().count() >= 1, "{:?}", piped.err);
    at.watched = true;
    let watched = at.keevoro(&["look", &text(&card)]);
    assert!(watched.err.contains("\r\u{1b}[2K  "), "{:?}", watched.err);
    assert!(
        watched.err.ends_with("\r\u{1b}[2K"),
        "and it is cleared at the end: {:?}",
        watched.err
    );
}

#[test]
fn smaller_things_a_backup_can_be_asked() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let card = card(dir.path());
    let at = At::new(dir.path());
    at.keevoro(&["backups", "start", "Card photos"]);
    at.keevoro(&["look", &text(&card)]);

    let ran = at.keevoro(&["sources"]);
    assert!(ran.out.starts_with("1. Card  "), "{}", ran.out);
    let ran = at.keevoro(&["sources", "types", "1", "--leave-out", "png"]);
    assert!(ran.out.starts_with("Leaving out .png files"), "{}", ran.out);
    let ran = at.keevoro(&["sources", "types", &text(&card)]);
    assert!(ran.out.contains(".png") && ran.out.contains("left out"), "{}", ran.out);

    let ran = at.keevoro(&["photos", "--json", "Cornwall"]);
    let photo = ran.json()[0]["id"].as_str().unwrap().to_string();
    let to = dir.path().join("look.webp");
    let ran = at.keevoro(&["picture", &photo, "--large", "--to", &text(&to)]);
    assert_eq!(ran.code, Code::Worked, "{}", ran.err);
    assert_eq!(&std::fs::read(&to).unwrap()[..4], b"RIFF");
    assert_eq!(
        at.keevoro(&["picture", &photo]).code,
        Code::NotUnderstood,
        "it says where"
    );

    let ran = at.keevoro(&["photos-or-videos", "videos"]);
    assert!(
        ran.out
            .starts_with("This backup is for videos only. Anything already backed up stays backed up"),
        "{}",
        ran.out
    );
    let ran = at.keevoro(&["sources", "types", "1"]);
    assert!(
        ran.out.contains("left out: this backup is for videos only"),
        "{}",
        ran.out
    );
    let ran = at.keevoro(&["sources", "photos-or-videos", "1", "both"]);
    assert!(
        ran.out.starts_with("This source is set to photos and videos."),
        "{}",
        ran.out
    );
    at.keevoro(&["sources", "photos-or-videos", "1", "backup"]);
    let ran = at.keevoro(&["photos-or-videos", "both"]);
    assert_eq!(ran.out.trim(), "This backup is for photos and videos.");
    let ran = at.keevoro(&["backups", "start", "Drone", "--videos-only", "--json"]);
    assert_eq!(ran.code, Code::Worked, "{}", ran.err);
    let ran = at.keevoro(&["-b", "Drone", "photos-or-videos"]);
    assert_eq!(ran.out.trim(), "This backup is for videos only.");
    let ran = at.keevoro(&["previews", "off"]);
    assert!(ran.out.contains("originals alone"), "{}", ran.out);
    let ran = at.keevoro(&["settings", "set", "own-codecs", "off"]);
    assert!(ran.out.starts_with("own-codecs: off"), "{}", ran.out);
    at.keevoro(&["settings", "set", "own-codecs", "on"]);
    let photos = text(&dir.path().join("Photos"));
    let ran = at.keevoro(&["agents", "allow", "--look", &photos]);
    assert!(ran.out.contains(&photos), "{}", ran.out);
    let ran = at.keevoro(&["agents", "allow-changes", "on"]);
    assert!(ran.out.contains("It may make changes."), "{}", ran.out);
    let ran = at.keevoro(&["agents", "allow", "--none"]);
    assert!(ran.out.contains("may look through no folders"), "{}", ran.out);

    for (typed, says) in [
        (vec!["about"], "Keevoro "),
        (vec!["activity"], ""),
        (vec!["places"], ""),
        (vec!["duplicates"], "also IMG_20080812_101500.jpg"),
        (vec!["update", "--check"], "There is no newer Keevoro"),
        (vec!["log", "--lines", "3"], "20"),
        (vec!["pause"], "Paused"),
        (vec!["carry-on"], "Carrying on"),
        (vec!["follow"], "Nothing is running."),
    ] {
        let ran = at.keevoro(&typed);
        assert_eq!(ran.code, Code::Worked, "{typed:?}: {}", ran.err);
        assert!(ran.out.contains(says), "{typed:?}: {}", ran.out);
    }
    let ran = at.keevoro(&["backups", "forget", "Card photos"]);
    assert!(
        ran.out.contains("Nothing at any source or destination was touched"),
        "{}",
        ran.out
    );
}

#[test]
fn the_kinds_of_file_looked_for_are_listed_added_switched_and_put_back() {
    let dir = tempfile::tempdir().unwrap();
    let at = At::new(dir.path());
    let ran = at.keevoro(&["kinds"]);
    assert_eq!(ran.code, Code::Worked, "{}", ran.err);
    assert!(ran.out.contains(".jpg        photo  built in  on"), "{}", ran.out);

    let ran = at.keevoro(&["kinds", "add", ".HEIX", "--photo"]);
    assert_eq!(
        ran.out.lines().next(),
        Some("Keevoro looks for .heix files, as photos."),
        "{}",
        ran.out
    );
    let ran = at.keevoro(&["kinds", "add", "heix", "--video"]);
    assert_eq!(ran.code, Code::Refused);
    assert!(
        ran.err.contains("Keevoro already looks for .heix files."),
        "{}",
        ran.err
    );
    assert_ne!(
        at.keevoro(&["kinds", "add", "heiy"]).code,
        Code::Worked,
        "photo or video must be said"
    );
    assert_eq!(at.keevoro(&["kinds", "add", "exe", "--photo"]).code, Code::Refused);

    let ran = at.keevoro(&["kinds", "off", "avi"]);
    assert!(
        ran.out.starts_with("Keevoro no longer looks for .avi files."),
        "{}",
        ran.out
    );
    let ran = at.keevoro(&["kinds", "change", "heix", "--video", "--to", "heiz"]);
    assert!(
        ran.out.starts_with("Keevoro looks for .heiz files, as videos."),
        "{}",
        ran.out
    );
    let ran = at.keevoro(&["kinds", "--json"]);
    let list = ran.json();
    let line = |ending: &str| list.as_array().unwrap().iter().find(|k| k["ending"] == ending).cloned();
    assert_eq!(line("avi").unwrap()["on"], false);
    assert_eq!(line("heiz").unwrap()["builtIn"], false);
    assert_eq!(at.keevoro(&["kinds", "remove", "jpg"]).code, Code::Refused);
    assert_eq!(at.keevoro(&["kinds", "remove", "nothing"]).code, Code::NotFound);

    let ran = at.keevoro(&["kinds", "reset"]);
    assert!(ran.out.contains(".avi        video  built in  on"), "{}", ran.out);
    assert!(ran.out.contains(".heiz       video  yours     on"), "{}", ran.out);
    let ran = at.keevoro(&["kinds", "remove", ".HEIZ"]);
    assert!(ran.out.starts_with("Took .heiz off the list."), "{}", ran.out);
    assert_eq!(at.keevoro(&["kinds", "on", "avi"]).code, Code::Worked);
}

#[test]
fn a_key_given_is_never_shown_logged_or_written_down() {
    const ACCESS: &str = "AKIAKEEVOROCLIMARKER01";
    const SECRET: &str = "keevoro-cli-secret-marker/0002+abc";
    let dir = tempfile::tempdir().unwrap();
    let at = At::new(dir.path());
    at.keevoro(&["backups", "start", "Card photos"]);
    // Nothing listens here, so the storage cannot be reached and is not added.
    let add = [
        "destinations",
        "add",
        "--endpoint",
        "http://127.0.0.1:9",
        "--bucket",
        "family",
    ];

    let mut from_stdin = At::new(dir.path());
    from_stdin.stdin = format!("{ACCESS}\n{SECRET}\n");
    let mut args = add.to_vec();
    args.push("--keys-from-stdin");
    let mut ran = vec![from_stdin.keevoro(&args)];
    let mut json = args.clone();
    json.push("--json");
    ran.push(from_stdin.keevoro(&json));

    let mut from_env = At::new(dir.path());
    from_env.env = vec![
        (crate::keys::ACCESS_VAR, ACCESS.to_string()),
        (crate::keys::SECRET_VAR, SECRET.to_string()),
    ];
    ran.push(from_env.keevoro(&add));

    let mut typed = add.to_vec();
    typed.extend(["--access-key-id", ACCESS, "--secret-access-key", SECRET]);
    ran.push(at.keevoro(&typed));

    for said in &ran {
        assert_eq!(said.code, Code::Unreachable, "{}", said.err);
        for text in [&said.out, &said.err] {
            assert!(!text.contains(ACCESS) && !text.contains(SECRET), "{text}");
        }
    }
    let written = everything_written(&dir.path().join("home"));
    assert!(written.iter().any(|(path, _)| path.ends_with("keevoro.log")));
    for (path, bytes) in written {
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            !text.contains(ACCESS) && !text.contains(SECRET),
            "a key was written to {}",
            path.display()
        );
    }

    // Without any keys, and nobody at the keyboard to ask, it says how.
    let ran = at.keevoro(&add);
    assert_eq!(ran.code, Code::NotUnderstood);
    assert!(ran.err.contains("--keys-from-stdin"), "{}", ran.err);
    // The help says where keys typed on the command end up.
    let help = program()
        .find_subcommand("destinations")
        .and_then(|d| d.find_subcommand("add"))
        .unwrap()
        .clone()
        .render_long_help()
        .to_string();
    assert!(help.contains("kept in your shell's history"), "{help}");
}

#[test]
fn tidy_says_first_what_it_would_clear_then_clears_it_in_the_apps_own_words() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("Old backup");
    let work = folder.join(".keevoro-uploads");
    std::fs::create_dir_all(&work).unwrap();
    let old = work.join("tmp-abandoned");
    std::fs::write(&old, [0u8; 2048]).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&old)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 3600))
        .unwrap();
    let at = At::new(dir.path());
    let first = at.keevoro(&["tidy", &text(&folder), "--dry-run"]);
    assert_eq!(first.code, Code::Worked, "{}", first.err);
    assert!(
        first
            .out
            .starts_with("There are 2.0 KB of unfinished copies (1 file) in"),
        "{}",
        first.out
    );
    assert!(old.is_file(), "a dry run clears nothing");
    let api = keevoro_in(dir.path(), FrontEnd::App);
    let ran = at.keevoro(&["tidy", &text(&folder)]);
    assert!(
        ran.out.starts_with("Cleared 2.0 KB of unfinished copies (1 file) from"),
        "{}",
        ran.out
    );
    assert!(!old.exists());
    let again = call(&api, Op::DestinationsTidy, None, json!({ "place": text(&folder) })).unwrap();
    let words: Vec<String> = again["said"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| format!("{}\n", l.as_str().unwrap()))
        .collect();
    assert_eq!(
        at.keevoro(&["tidy", &text(&folder)]).out,
        words.concat(),
        "the same words as the API"
    );
}

#[test]
fn a_folder_is_looked_inside_and_a_picture_saved_without_adding_anything() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let card = card(dir.path());
    let at = At::new(dir.path());
    let before = everything_written(&card);
    let refused = at.keevoro(&["folders", &text(&card)]);
    assert_ne!(refused.code, Code::Worked);
    assert!(refused.err.contains("Find where the photos are"), "{}", refused.err);
    at.keevoro(&["survey", &text(&card)]);
    let folders = at.keevoro(&["folders", &text(&card)]);
    assert_eq!(folders.code, Code::Worked, "{}", folders.err);
    assert!(folders.out.contains("Cornwall"), "{}", folders.out);
    let cornwall = card.join("Holidays").join("Cornwall");
    let files = at.keevoro(&["files", &text(&cornwall), "--count", "1"]);
    assert_eq!(files.code, Code::Worked, "{}", files.err);
    assert!(files.out.contains(", 1 of them here:"), "{}", files.out);
    let videos = at.keevoro(&["files", &text(&cornwall), "--videos", "--sort", "newest"]);
    assert_eq!(videos.code, Code::Worked, "{}", videos.err);
    assert!(!videos.out.contains("photo "), "{}", videos.out);
    let mixed = at.keevoro(&["files", &text(&cornwall), "--photos", "--videos"]);
    assert_ne!(mixed.code, Code::Worked, "one kind or the other");
    let first = at.keevoro(&["files", &text(&cornwall), "--json"]).json()["files"][0]["path"]
        .as_str()
        .unwrap()
        .to_string();
    let to = dir.path().join("look.webp");
    let saved = at.keevoro(&["picture", "--file", &first, "--to", &text(&to)]);
    assert_eq!(saved.code, Code::Worked, "{}", saved.err);
    assert_eq!(&std::fs::read(&to).unwrap()[8..12], b"WEBP");
    assert_eq!(everything_written(&card), before, "the folder is exactly as it was");
    let api = keevoro_in(dir.path(), FrontEnd::App);
    assert!(api.backups().all().is_empty(), "nothing was added to any backup");
}

#[test]
fn where_the_space_goes_is_said_from_the_survey_by_folder_and_by_kind() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let card = card(dir.path());
    std::fs::create_dir_all(card.join("Games")).unwrap();
    std::fs::write(card.join("Games").join("textures.zip"), vec![0u8; 50_000]).unwrap();
    let at = At::new(dir.path());
    let before = everything_written(&card);
    let refused = at.keevoro(&["map", &text(&card)]);
    assert_ne!(refused.code, Code::Worked);
    assert!(refused.err.contains("Find where the photos are"), "{}", refused.err);
    at.keevoro(&["survey", &text(&card)]);
    let map = at.keevoro(&["map", &text(&card)]);
    assert_eq!(map.code, Code::Worked, "{}", map.err);
    assert!(map.out.starts_with("Where the space goes in "), "{}", map.out);
    assert!(map.out.contains("(mapped just now)"), "{}", map.out);
    assert!(map.out.contains("Games  1 other file"), "{}", map.out);
    assert!(map.out.contains("By kind: photos "), "{}", map.out);
    let kinds = at.keevoro(&["map", &text(&card), "--kinds"]);
    assert!(kinds.out.contains("zip"), "{}", kinds.out);
    assert!(kinds.out.contains("1 file, everything else"), "{}", kinds.out);
    let inside = at.keevoro(&[
        "map",
        &text(&card),
        "--folder",
        &text(&card.join("Games")),
        "--depth",
        "1",
        "--json",
    ]);
    assert_eq!(inside.json()["top"]["name"], "Games");
    let too_deep = at.keevoro(&["map", &text(&card), "--depth", "9"]);
    assert_ne!(too_deep.code, Code::Worked);
    // Forgotten, it is surveyed again when next asked about.
    let forgot = at.keevoro(&["forget-maps"]);
    assert_eq!(forgot.code, Code::Worked, "{}", forgot.err);
    assert!(forgot.out.starts_with("Forgot 1 remembered map."), "{}", forgot.out);
    assert_ne!(at.keevoro(&["map", &text(&card)]).code, Code::Worked);
    assert_eq!(everything_written(&card), before, "nothing on it was changed");
}

#[test]
fn the_map_says_which_folder_is_a_source_and_how_much_of_it_is_backed_up() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let card = card(dir.path());
    let at = At::new(dir.path());
    at.keevoro(&["backups", "start", "Card photos"]);
    at.keevoro(&["look", &text(&card.join("Holidays"))]);
    at.keevoro(&["survey", &text(&card)]);
    let before = everything_written(&card);
    let map = at.keevoro(&["map", &text(&card)]);
    assert_eq!(map.code, Code::Worked, "{}", map.err);
    let holidays = map.out.lines().find(|l| l.contains("Holidays")).unwrap();
    assert!(holidays.contains("a source of this backup"), "{}", map.out);
    assert!(holidays.contains("backed up: 0 of "), "{}", map.out);
    assert_eq!(everything_written(&card), before, "nothing on it was changed");
}

#[test]
fn a_folder_is_left_out_of_a_backup_and_put_back_from_the_command_line() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let card = card(dir.path());
    let at = At::new(dir.path());
    at.keevoro(&["backups", "start", "Card photos"]);
    at.keevoro(&["look", &text(&card)]);
    let before = everything_written(&card);
    let cornwall = card.join("Holidays").join("Cornwall");
    let out = at.keevoro(&["sources", "leave-out", &text(&cornwall)]);
    assert_eq!(out.code, Code::Worked, "{}", out.err);
    assert!(out.out.starts_with("Left out "), "{}", out.out);
    assert!(out.out.contains("nothing already backed up is touched"), "{}", out.out);
    let sources = at.keevoro(&["sources"]);
    assert!(sources.out.contains("Left out: "), "{}", sources.out);
    let back = at.keevoro(&["sources", "leave-out", &text(&cornwall), "--put-back"]);
    assert_eq!(back.code, Code::Worked, "{}", back.err);
    assert!(back.out.starts_with("Put back "), "{}", back.out);
    assert!(!at.keevoro(&["sources"]).out.contains("Left out: "));
    let gone = at.keevoro(&["sources", "leave-out", &text(&dir.path().join("Nowhere"))]);
    assert_ne!(gone.code, Code::Worked);
    assert_eq!(everything_written(&card), before, "the source is exactly as it was");
}
