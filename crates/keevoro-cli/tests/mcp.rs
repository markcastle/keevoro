//! `keevoro mcp`, the real program, spoken to over its standard input and
//! output as an AI app speaks to it, while a person uses the `keevoro`
//! command beside it.
//!
//! Every run keeps Keevoro's folders in a temporary folder, through
//! `KEEVORO_HOME`. No test here keeps a key: a key given is refused by
//! storage that cannot be reached before it could be kept.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long any one answer may take, however slow the computer.
const PATIENCE: Duration = Duration::from_secs(120);

/// An AI app, with `keevoro mcp` started for it.
struct App {
    child: Child,
    input: Option<ChildStdin>,
    lines: Receiver<String>,
    /// Every line it wrote to standard output, in order.
    said: Vec<String>,
    /// Everything it wrote to standard error.
    err: Arc<Mutex<Vec<u8>>>,
    next: u64,
}

impl App {
    fn start(home: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_keevoro"))
            .arg("mcp")
            .env("KEEVORO_HOME", home)
            .env_remove("KEEVORO_ACCESS_KEY_ID")
            .env_remove("KEEVORO_SECRET_ACCESS_KEY")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let (tell, lines) = mpsc::channel();
        let out = child.stdout.take().unwrap();
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines() {
                let Ok(line) = line else { break };
                if tell.send(line).is_err() {
                    break;
                }
            }
        });
        let err = Arc::new(Mutex::new(Vec::new()));
        let mut stderr = child.stderr.take().unwrap();
        let heard = err.clone();
        std::thread::spawn(move || {
            let mut all = Vec::new();
            let _ = stderr.read_to_end(&mut all);
            heard.lock().unwrap().extend(all);
        });
        let mut app = Self {
            input: child.stdin.take(),
            child,
            lines,
            said: Vec::new(),
            err,
            next: 0,
        };
        let hello = app.ask(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "a test", "version": "1" }
            }),
        );
        assert_eq!(hello["result"]["serverInfo"]["name"], "keevoro", "{hello}");
        app.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        app
    }

    fn send(&mut self, message: Value) {
        let input = self.input.as_mut().unwrap();
        writeln!(input, "{message}").unwrap();
        input.flush().unwrap();
    }

    fn ask(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let line = self
                .lines
                .recv_timeout(PATIENCE)
                .unwrap_or_else(|e| panic!("no answer to {method}: {e}"));
            self.said.push(line.clone());
            let message: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("not JSON ({e}): {line}"));
            if message["id"] == id {
                return message;
            }
        }
    }

    /// Calls a tool, and gives its result.
    fn tool(&mut self, name: &str, arguments: Value) -> Value {
        let answer = self.ask("tools/call", json!({ "name": name, "arguments": arguments }));
        assert!(answer.get("error").is_none(), "{name}: {answer}");
        answer["result"].clone()
    }

    /// Calls a tool that must work, and gives the API's answer.
    fn works(&mut self, name: &str, arguments: Value) -> Value {
        let result = self.tool(name, arguments);
        assert_eq!(result["isError"], false, "{name}: {result}");
        serde_json::from_str(result["content"][1]["text"].as_str().unwrap()).unwrap()
    }

    /// Calls a tool that must be refused, and gives the sentence why.
    fn refused(&mut self, name: &str, arguments: Value) -> String {
        let result = self.tool(name, arguments);
        assert_eq!(result["isError"], true, "{name}: {result}");
        assert_eq!(result["structuredContent"]["kind"], "refused", "{name}: {result}");
        result["content"][0]["text"].as_str().unwrap().to_string()
    }

    /// Starts a job with a tool, waits for it to end, and gives how it
    /// ended.
    fn job(&mut self, name: &str, arguments: Value) -> Value {
        let started = self.works(name, arguments);
        let id = started["id"].clone();
        loop {
            let status = self.works("job_status", json!({ "job": id, "waitSeconds": 30 }));
            if status[0]["ended"] == true {
                return status[0].clone();
            }
        }
    }

    /// Closes its side, as an AI app does when it quits, and checks that
    /// the program ends by itself, having written only the protocol.
    fn close(mut self) -> String {
        drop(self.input.take());
        let begun = std::time::Instant::now();
        let status = loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                break status;
            }
            assert!(
                begun.elapsed() < PATIENCE,
                "keevoro mcp did not end when its input closed"
            );
            std::thread::sleep(Duration::from_millis(50));
        };
        assert!(status.success(), "{status}");
        while let Ok(line) = self.lines.recv_timeout(Duration::from_secs(5)) {
            self.said.push(line);
        }
        for line in &self.said {
            let message: Value =
                serde_json::from_str(line).unwrap_or_else(|e| panic!("not the protocol ({e}): {line}"));
            assert_eq!(message["jsonrpc"], "2.0", "not the protocol: {line}");
        }
        // Standard error has had time to be read once the program ended.
        std::thread::sleep(Duration::from_millis(100));
        let err = String::from_utf8_lossy(&self.err.lock().unwrap()).into_owned();
        assert_eq!(err, "", "keevoro mcp wrote to standard error");
        self.said.join("\n")
    }
}

/// The `keevoro` command, as a person types it beside the agent.
fn keevoro(home: &Path, args: &[&str]) -> (i32, String) {
    let ran = Command::new(env!("CARGO_BIN_EXE_keevoro"))
        .args(args)
        .env("KEEVORO_HOME", home)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&ran.stdout),
        String::from_utf8_lossy(&ran.stderr)
    );
    (ran.status.code().unwrap_or(-1), said)
}

fn text(path: &Path) -> String {
    path.display().to_string()
}

/// A memory card with two photos, in `dir`.
fn card(dir: &Path) -> PathBuf {
    let card = dir.join("Card");
    for (rel, shade) in [
        ("Trip/IMG_20240601_100000.jpg", 10),
        ("Trip/IMG_20240601_100500.jpg", 90),
    ] {
        let p = card.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        image::RgbImage::from_pixel(640, 480, image::Rgb([shade, 120, 60]))
            .save(p)
            .unwrap();
    }
    card
}

/// Every file under `dir`, with its bytes.
fn everything_in(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    let mut folders = vec![dir.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else if let Ok(bytes) = std::fs::read(&path) {
                found.push((path, bytes));
            }
        }
    }
    found.sort();
    found
}

#[test]
fn a_first_backup_by_an_agent_with_a_person_allowing_each_step() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let card = card(dir.path());
    let usb = dir.path().join("USB");
    std::fs::create_dir_all(&usb).unwrap();
    let outside = dir.path().join("Private");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("diary.jpg"), b"not for agents").unwrap();
    let card_before = everything_in(&card);

    let mut app = App::start(&home);
    let tools = app.ask("tools/list", json!({}));
    assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 41);

    // Out of the box the agent may only look.
    let access = app.works("my_access", json!({}));
    assert_eq!(access, json!({ "lookIn": [], "backUpTo": [], "mayChange": false }));
    let why = app.refused("start_backup", json!({ "name": "Card photos" }));
    assert!(why.contains("keevoro agents allow-changes on"), "{why}");
    let why = app.refused("survey_place", json!({ "place": text(&card) }));
    assert!(why.contains("hasn't allowed that folder"), "{why}");
    // And it cannot allow itself anything.
    let result = app.tool("my_access", json!({ "mayChange": true }));
    assert_eq!(result["isError"], true, "{result}");
    assert_eq!(app.works("my_access", json!({}))["mayChange"], false);

    // The person allows the card, the USB drive and changes, at the
    // command line, while the agent's program runs.
    let (code, said) = keevoro(
        &home,
        &["agents", "allow", "--look", &text(&card), "--back-up-to", &text(&usb)],
    );
    assert_eq!(code, 0, "{said}");
    let (code, said) = keevoro(&home, &["agents", "allow-changes", "on"]);
    assert_eq!(code, 0, "{said}");
    assert_eq!(app.works("my_access", json!({}))["mayChange"], true);

    // A first backup, from start to finish.
    let started = app.works("start_backup", json!({ "name": "Card photos" }));
    assert_eq!(started["name"], "Card photos");
    let looked = app.job("look_through", json!({ "backup": "Card photos", "place": text(&card) }));
    assert_eq!(looked["last"]["photos"], 2, "{looked}");
    assert_eq!(looked["startedBy"], "agent");
    let found = app.works("summarise_backup", json!({}));
    assert_eq!(found["photos"], 2);
    let collections = app.works("list_collections", json!({}));
    let key = collections[0]["key"].as_str().unwrap().to_string();
    app.works("keep_collection", json!({ "collection": key, "keep": true }));
    let family = usb.join("Family");
    app.works("add_destination", json!({ "kind": "folder", "place": text(&family) }));
    let rehearsal = app.works("rehearse_backup", json!({}));
    assert_eq!(rehearsal["files"], 2, "{rehearsal}");

    // The person can see the agent's work from the command line.
    let (code, said) = keevoro(&home, &["sources", "--json"]);
    assert_eq!(code, 0, "{said}");
    assert!(said.contains("Card"), "{said}");

    let ran = app.job("run_backup", json!({}));
    assert_eq!(ran["last"]["outcome"], "worked", "{ran}");
    let summary = app.works("last_summary", json!({}));
    assert!(summary.to_string().contains("Safely backed up 2 files"), "{summary}");
    let checked = app.job("check_arrived", json!({}));
    assert_eq!(checked["ended"], true);

    // The photos arrived, exact, and the card is byte for byte as it was.
    let arrived = everything_in(&family);
    for (_, bytes) in &card_before {
        assert!(
            arrived.iter().any(|(_, there)| there == bytes),
            "a photo did not arrive whole"
        );
    }
    assert_eq!(everything_in(&card), card_before, "the source was changed");

    // Anywhere outside what the person allowed is refused, however written.
    for place in [
        text(&outside),
        text(&card.join("..").join("Private")),
        text(&card.join("Trip").join("..").join("..").join("Private")),
        text(dir.path()),
    ] {
        let why = app.refused("look_through", json!({ "place": place }));
        assert!(why.contains("hasn't allowed that folder"), "{why}");
        app.refused("survey_place", json!({ "place": place }));
        app.refused("add_destination", json!({ "kind": "folder", "place": place }));
    }
    // Allowed to look in is not allowed to back up to.
    app.refused(
        "add_destination",
        json!({ "kind": "folder", "place": text(&card.join("Copies")) }),
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, card.join("shortcut")).unwrap();
        std::os::unix::fs::symlink(&outside, usb.join("shortcut")).unwrap();
        app.refused("look_through", json!({ "place": text(&card.join("shortcut")) }));
        app.refused(
            "add_destination",
            json!({ "kind": "folder", "place": text(&usb.join("shortcut").join("x")) }),
        );
    }
    #[cfg(windows)]
    {
        // Other ways of writing a path on Windows lead to the same place,
        // and are judged as it.
        let forward = text(&outside).replace('\\', "/");
        app.refused("look_through", json!({ "place": forward }));
        app.refused("look_through", json!({ "place": format!(r"\\?\{}", text(&outside)) }));
        app.refused("look_through", json!({ "place": format!("{}.", text(&card)) }));
    }

    // The person takes changes away; the agent may look again, and only that.
    keevoro(&home, &["agents", "allow-changes", "off"]);
    app.refused("run_backup", json!({}));
    app.works("list_destinations", json!({}));

    let said = app.close();
    assert!(!said.contains("not for agents"));

    // Everything it did is in Keevoro's log, in plain words.
    let (_, log) = keevoro(&home, &["log", "--lines", "200"]);
    for line in [
        "An AI agent used start_backup: refused.",
        "An AI agent used start_backup: it worked",
        "An AI agent used run_backup on backup \"Card photos\": started job",
        "An AI agent used look_through on backup \"Card photos\": refused.",
        "An AI agent's app closed Keevoro's agent server",
    ] {
        assert!(log.contains(line), "{line}\n\nnot in:\n{log}");
    }
}

#[test]
fn a_key_given_by_an_agent_comes_out_nowhere() {
    const ACCESS: &str = "AKIAKEEVOROMCPMARKER7";
    const SECRET: &str = "keevoro-mcp-secret-marker/99+zz";
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    keevoro(&home, &["backups", "start", "Card photos"]);
    let mut app = App::start(&home);
    let request = json!({
        "kind": "endpoint",
        // Nothing listens here, so the storage is refused before any key
        // could be kept.
        "place": "http://127.0.0.1:9",
        "bucket": "family",
        "accessKeyId": ACCESS,
        "secretAccessKey": SECRET,
    });
    // Refused while the agent may only look, and when it may change things,
    // refused by the storage itself; and a request it cannot understand.
    app.refused("add_destination", request.clone());
    keevoro(&home, &["agents", "allow-changes", "on"]);
    let unreachable = app.tool("add_destination", request.clone());
    assert_eq!(unreachable["structuredContent"]["kind"], "unreachable", "{unreachable}");
    let mut odd = request.clone();
    odd["kind"] = json!(ACCESS);
    let odd = app.tool("add_destination", odd);
    assert_eq!(odd["isError"], true);
    app.works("list_destinations", json!({}));
    app.works("my_access", json!({}));
    let log = app.ask("resources/read", json!({ "uri": "keevoro://log" }));
    assert!(log["result"]["contents"][0]["text"]
        .as_str()
        .unwrap()
        .contains("add_destination"));

    // Not in any answer, nor on standard error (which `close` checks is
    // empty), nor anywhere Keevoro wrote: its log, settings and records.
    let said = app.close();
    assert!(said.contains("add_destination") || said.contains("unreachable"));
    assert!(
        !said.contains(ACCESS) && !said.contains(SECRET),
        "a key came back:\n{said}"
    );
    let written = everything_in(&home);
    assert!(written.iter().any(|(path, _)| path.ends_with("keevoro.log")));
    for (path, bytes) in written {
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            !text.contains(ACCESS) && !text.contains(SECRET),
            "a key was written to {}",
            path.display()
        );
    }
}

#[test]
fn the_agent_and_the_command_line_share_one_backup_and_take_turns() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let card = card(dir.path());
    let usb = dir.path().join("USB");
    keevoro(&home, &["backups", "start", "Card photos"]);
    keevoro(&home, &["look", &text(&card)]);
    keevoro(&home, &["destinations", "add", "--folder", &text(&usb)]);
    keevoro(
        &home,
        &["agents", "allow", "--look", &text(&card), "--back-up-to", &text(&usb)],
    );
    keevoro(&home, &["agents", "allow-changes", "on"]);

    let mut app = App::start(&home);
    // The agent holds its backup paused, so the person sees it running.
    app.works("pause_jobs", json!({}));
    let started = app.works("run_backup", json!({}));
    let id = started["id"].clone();
    // What it is doing is shared a few times a second.
    let begun = std::time::Instant::now();
    let running = loop {
        let (code, said) = keevoro(&home, &["status", "--json"]);
        assert_eq!(code, 0, "{said}");
        let running: Value = serde_json::from_str(said.trim()).unwrap();
        if running[0]["id"] == id {
            break running;
        }
        assert!(begun.elapsed() < PATIENCE, "the agent's job was never seen: {said}");
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(running[0]["startedBy"], "agent");
    // The person cannot start a second backup of the same backup meanwhile.
    let (code, said) = keevoro(&home, &["run"]);
    assert_eq!(code, 4, "{said}");
    assert!(said.contains("for an AI agent"), "{said}");
    // The person lets it go on, from the command line.
    let (code, said) = keevoro(&home, &["carry-on"]);
    assert_eq!(code, 0, "{said}");
    loop {
        let status = app.works("job_status", json!({ "job": id, "waitSeconds": 30 }));
        if status[0]["ended"] == true {
            assert_eq!(status[0]["last"]["outcome"], "worked", "{status}");
            break;
        }
    }
    app.close();
    let (_, said) = keevoro(&home, &["history"]);
    assert!(said.contains("to the folder"), "{said}");
}

#[test]
fn closing_the_app_side_ends_the_program_at_once_when_nothing_is_running() {
    let dir = tempfile::tempdir().unwrap();
    let app = App::start(&dir.path().join("home"));
    let begun = std::time::Instant::now();
    app.close();
    assert!(
        begun.elapsed() < Duration::from_secs(10),
        "it took {:?}",
        begun.elapsed()
    );
}
