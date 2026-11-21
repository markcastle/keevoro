//! The server, spoken to as an AI app speaks to it: lines of JSON-RPC in,
//! lines out, over a pipe in memory, against Keevoro in a temporary folder
//! with a keychain in memory.

use super::*;
use keevoro_api::{AgentsAccessRequest, Folders, FrontEnd, SecretStore};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines};

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

fn keevoro(dir: &Path) -> Keevoro {
    Keevoro::open(&Folders::under(&dir.join("home")))
        .as_front_end(FrontEnd::Agent)
        .with_secrets(KeysInMemory::default())
        .with_environment(|_| None)
}

/// An AI app, connected to the server.
struct App {
    to_server: DuplexStream,
    from_server: Lines<BufReader<DuplexStream>>,
    server: tokio::task::JoinHandle<std::io::Result<()>>,
    next: u64,
}

impl App {
    async fn connect(keevoro: Keevoro) -> Self {
        let (to_server, server_reads) = tokio::io::duplex(1 << 20);
        let (server_writes, from_server) = tokio::io::duplex(1 << 20);
        let server = tokio::spawn(Server::new(keevoro).serve(server_reads, server_writes));
        let mut app = Self {
            to_server,
            from_server: BufReader::new(from_server).lines(),
            server,
            next: 0,
        };
        let hello = app
            .ask(
                "initialize",
                json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "1" } }),
            )
            .await;
        assert_eq!(hello["result"]["serverInfo"]["name"], "keevoro");
        app.say("notifications/initialized").await;
        app
    }

    async fn send(&mut self, message: Value) {
        let mut line = message.to_string();
        line.push('\n');
        self.to_server.write_all(line.as_bytes()).await.unwrap();
    }

    async fn say(&mut self, method: &str) {
        self.send(json!({ "jsonrpc": "2.0", "method": method })).await;
    }

    /// Asks, and waits for the answer with the same id.
    async fn ask(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await;
        loop {
            let line = tokio::time::timeout(Duration::from_secs(60), self.from_server.next_line())
                .await
                .expect("an answer in time")
                .unwrap()
                .expect("the server still there");
            let message: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("{e}: {line}"));
            assert_eq!(message["jsonrpc"], "2.0", "{line}");
            if message["id"] == id {
                return message;
            }
        }
    }

    async fn tool(&mut self, name: &str, arguments: Value) -> Value {
        let answer = self
            .ask("tools/call", json!({ "name": name, "arguments": arguments }))
            .await;
        answer["result"].clone()
    }

    async fn close(self) {
        drop(self.to_server);
        tokio::time::timeout(Duration::from_secs(60), self.server)
            .await
            .expect("the server ends when the app closes its side")
            .unwrap()
            .unwrap();
    }
}

fn text_of(result: &Value, at: usize) -> String {
    result["content"][at]["text"].as_str().unwrap_or_default().to_string()
}

#[tokio::test]
async fn an_app_sees_every_tool_with_what_it_takes_and_whether_it_changes_things() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::connect(keevoro(dir.path())).await;
    let listed = app.ask("tools/list", json!({})).await;
    let tools = listed["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 46);
    let kinds = tools.iter().find(|t| t["name"] == "list_file_kinds").unwrap();
    assert_eq!(
        kinds["annotations"]["readOnlyHint"], true,
        "an agent may read the list, not change it"
    );
    let run = tools.iter().find(|t| t["name"] == "run_backup").unwrap();
    assert_eq!(run["annotations"]["readOnlyHint"], false);
    assert_eq!(run["annotations"]["destructiveHint"], false);
    assert_eq!(run["inputSchema"]["type"], "object");
    assert_eq!(run["title"], "Run backup");
    let list = tools.iter().find(|t| t["name"] == "list_backups").unwrap();
    assert_eq!(list["annotations"]["readOnlyHint"], true);
    assert!(tools
        .iter()
        .all(|t| t["name"] != "forget_backup" && t["name"] != "reset"));

    let resources = app.ask("resources/list", json!({})).await;
    assert_eq!(resources["result"]["resources"][0]["uri"], LOG);
    app.close().await;
}

#[tokio::test]
async fn answers_are_a_summary_and_the_apis_json_and_problems_say_why() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::connect(keevoro(dir.path())).await;

    let listed = app.tool("list_backups", json!({})).await;
    assert_eq!(listed["isError"], false);
    assert_eq!(
        text_of(&listed, 0),
        "There are no backups yet. start_backup starts one."
    );
    assert_eq!(text_of(&listed, 1), "[]");

    let access = app.tool("my_access", json!({})).await;
    assert_eq!(
        access["structuredContent"],
        json!({ "lookIn": [], "backUpTo": [], "mayChange": false })
    );

    // Refused while the person allows only looking, with the API's words.
    let refused = app.tool("start_backup", json!({ "name": "Drone" })).await;
    assert_eq!(refused["isError"], true);
    assert_eq!(text_of(&refused, 0), keevoro_api::for_agents::LOOK_ONLY);
    assert_eq!(refused["structuredContent"]["kind"], "refused");

    // A request that cannot be understood says so.
    let wrong = app.tool("list_backups", json!({ "everything": true })).await;
    assert_eq!(wrong["structuredContent"]["kind"], "not_understood");
    let wrong = app.tool("list_sources", json!({ "backup": 3 })).await;
    assert_eq!(wrong["structuredContent"]["kind"], "not_understood");
    let none = app.tool("list_sources", json!({})).await;
    assert_eq!(text_of(&none, 0), "Choose a backup first.");
    assert_eq!(none["structuredContent"]["kind"], "no_backup");

    // A tool that is not there is a protocol error, as the protocol says.
    let missing = app
        .ask("tools/call", json!({ "name": "forget_backup", "arguments": {} }))
        .await;
    assert_eq!(missing["error"]["code"], -32602);
    let missing = app.ask("resources/read", json!({ "uri": "file:///etc/passwd" })).await;
    assert!(missing["error"]["code"].is_i64(), "{missing}");

    let pong = app.ask("ping", json!({})).await;
    assert_eq!(pong["result"], json!({}));

    let log = app.ask("resources/read", json!({ "uri": LOG })).await;
    let log = log["result"]["contents"][0]["text"].as_str().unwrap().to_string();
    assert!(log.contains("An AI agent used start_backup: refused."), "{log}");
    app.close().await;
}

#[tokio::test]
async fn a_long_job_answers_at_once_and_is_followed_with_job_status() {
    let dir = tempfile::tempdir().unwrap();
    let card = dir.path().join("Card");
    std::fs::create_dir_all(card.join("Trip")).unwrap();
    image::RgbImage::from_pixel(64, 48, image::Rgb([10, 120, 60]))
        .save(card.join("Trip").join("IMG_20240601_100000.jpg"))
        .unwrap();
    let keevoro = keevoro(dir.path());
    keevoro
        .set_agent_access(AgentsAccessRequest {
            look_in: Some(vec![card.display().to_string()]),
            back_up_to: None,
            may_change: Some(false),
        })
        .unwrap();
    let mut app = App::connect(keevoro).await;

    // A survey changes nothing, so it may run while the agent only looks.
    let started = app
        .tool("survey_place", json!({ "place": card.display().to_string() }))
        .await;
    assert_eq!(started["isError"], false, "{started}");
    let id = started["structuredContent"]["id"].clone();
    assert!(text_of(&started, 0).starts_with(&format!("Started job {id}.")));
    let status = app.tool("job_status", json!({ "job": id, "waitSeconds": 30 })).await;
    let said = text_of(&status, 0);
    assert!(said.starts_with(&format!("Job {id} has ended.")), "{said}");
    assert!(said.contains("1 photo and 0 videos"), "{said}");
    let json: Value = serde_json::from_str(&text_of(&status, 1)).unwrap();
    assert_eq!(json[0]["ended"], true);
    assert_eq!(json[0]["startedBy"], "agent");
    app.close().await;
}

/// The first outside tester's report, as an agent hears it: a backup
/// deleted from its folder is said to be gone before anything is sent, and
/// is sent again.
#[tokio::test]
async fn an_agent_hears_that_a_backup_deleted_from_its_folder_is_gone_and_is_sent_again() {
    let dir = tempfile::tempdir().unwrap();
    let card = dir.path().join("Card");
    std::fs::create_dir_all(card.join("Trip")).unwrap();
    // Big enough not to be set aside as a tiny image.
    image::RgbImage::from_pixel(640, 480, image::Rgb([10, 120, 60]))
        .save(card.join("Trip").join("IMG_20240601_100000.jpg"))
        .unwrap();
    let usb = dir.path().join("USB");
    let keevoro = keevoro(dir.path());
    // Backed up once by the person, outside the async world the server runs in.
    let (card_at, usb_at) = (card.clone(), usb.clone());
    let keevoro = tokio::task::spawn_blocking(move || {
        back_up_once(&keevoro, &card_at, &usb_at);
        keevoro
    })
    .await
    .unwrap();
    // rm -rf USB/*
    for entry in std::fs::read_dir(&usb).unwrap().filter_map(Result::ok) {
        if !entry.file_name().to_string_lossy().starts_with('.') {
            let path = entry.path();
            if path.is_dir() {
                std::fs::remove_dir_all(path).unwrap();
            } else {
                std::fs::remove_file(path).unwrap();
            }
        }
    }
    let gone = format!(
        "The folder {} looks emptied or replaced: it no longer holds the file Keevoro backed up there. It will be \
         sent again.",
        usb.display()
    );
    let mut app = App::connect(keevoro).await;
    let rehearsal = app.tool("rehearse_backup", json!({ "backup": "T" })).await;
    let said = text_of(&rehearsal, 0);
    assert!(said.starts_with(&gone), "{said}");
    assert!(!said.contains("already at"), "{said}");

    let started = app.tool("run_backup", json!({ "backup": "T" })).await;
    assert_eq!(started["isError"], false, "{started}");
    let id = started["structuredContent"]["id"].clone();
    let status = app.tool("job_status", json!({ "job": id, "waitSeconds": 30 })).await;
    let said = text_of(&status, 0);
    assert!(said.contains(&gone), "{said}");
    assert!(said.contains("Restored 1 file that had gone from there."), "{said}");
    assert!(!said.contains("already there"), "{said}");
    app.close().await;
}

/// A backup "T" of `card`, made by the person and sent once to `usb`, with
/// an agent then allowed to back it up there.
fn back_up_once(keevoro: &Keevoro, card: &Path, usb: &Path) {
    let named = keevoro.start_backup("T").unwrap();
    let looked = keevoro.look(&named, card, None).unwrap();
    keevoro.wait(looked.id);
    keevoro_api::call(
        keevoro,
        Op::DestinationsAdd,
        Some("T"),
        json!({ "kind": "folder", "place": usb.display().to_string() }),
    )
    .unwrap();
    let ran = keevoro.run_backup(&named, None).unwrap();
    keevoro.wait(ran.id);
    keevoro.wait_for_jobs();
    keevoro
        .set_agent_access(AgentsAccessRequest {
            look_in: Some(vec![card.display().to_string()]),
            back_up_to: Some(vec![usb.display().to_string()]),
            may_change: Some(true),
        })
        .unwrap();
}
