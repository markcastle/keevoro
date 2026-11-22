use super::*;
use crate::error::Kind;
use crate::{call, Op};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

/// What the fake service heard: each request's first line and its body.
type Heard = Arc<Mutex<Vec<(String, String)>>>;

/// A tiny stand-in for the reports service on this computer, which gives
/// these answers in turn, one to each request, and notes what it heard.
fn fake_service(answers: Vec<(u16, Value)>) -> (String, Heard) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let heard: Heard = Arc::default();
    let noted = heard.clone();
    std::thread::spawn(move || {
        for (status, answer) in answers {
            let Ok((stream, _)) = listener.accept() else { return };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line.trim().is_empty() {
                    break;
                }
                if let Some((name, value)) = line.split_once(':') {
                    if name.eq_ignore_ascii_case("content-length") {
                        length = value.trim().parse().unwrap();
                    }
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            noted
                .lock()
                .unwrap()
                .push((first.trim().to_string(), String::from_utf8(body).unwrap()));
            let text = answer.to_string();
            let mut stream = stream;
            write!(
                stream,
                "HTTP/1.1 {status} Answer\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                text.len()
            )
            .unwrap();
        }
    });
    (address, heard)
}

/// An address nothing answers at.
fn nowhere() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    address
}

fn keevoro_reporting_to(dir: &Path, address: &str) -> Keevoro {
    let address = address.to_string();
    crate::keevoro::tests::keevoro(dir)
        .with_environment(move |name| (name == ADDRESS_VARIABLE).then(|| address.clone()))
}

fn asked(kind: KindWord, message: &str, with_log: bool) -> PrepareReportRequest {
    PrepareReportRequest {
        kind,
        message: message.into(),
        with_log,
        contact: None,
        screen: Some("Choose what to keep".into()),
    }
}

const TOKEN: &str = "0123456789abcdef0123456789abcdef01234567";

fn received(reference: &str) -> (u16, Value) {
    (201, json!({ "ok": true, "reference": reference, "token": TOKEN }))
}

#[test]
fn what_is_sent_is_byte_for_byte_what_was_shown_and_the_log_is_scrubbed_first() {
    let dir = tempfile::tempdir().unwrap();
    let (address, heard) = fake_service(vec![received("K-1042")]);
    let keevoro = keevoro_reporting_to(dir.path(), &address);
    let home = dirs::home_dir();
    if let Some(home) = &home {
        keevoro
            .log()
            .say(&format!("Looking through {}", home.join("Family photos").display()));
    }
    keevoro.log().say("Connected with secret=abc123 to the bucket");

    let prepared = keevoro
        .prepare_report(&asked(KindWord::Problem, "The map went blank\nafter a search", true))
        .unwrap();
    let report: Value = serde_json::from_str(&prepared.body).unwrap();
    let log = report["logTail"].as_str().unwrap();
    assert!(log.contains("secret=•••"), "{log}");
    assert!(!log.contains("abc123"));
    if let Some(home) = &home {
        assert!(!log.contains(&home.display().to_string()), "{log}");
        assert!(
            log.contains(&format!("Looking through ~{}Family photos", std::path::MAIN_SEPARATOR)),
            "{log}"
        );
    }
    assert_eq!(report["appVersion"], env!("CARGO_PKG_VERSION"));
    assert!(report["os"].as_str().is_some_and(|os| !os.is_empty()));

    let sent = keevoro.send_report(&prepared.body).unwrap();
    assert_eq!(sent.reference, "K-1042");
    assert_eq!(sent.said, "Thank you. Your reference is K-1042.");
    let heard = heard.lock().unwrap().clone();
    assert_eq!(heard.len(), 1);
    assert_eq!(heard[0].0, "POST /v1/reports HTTP/1.1");
    assert_eq!(heard[0].1, prepared.body, "exactly what was shown");

    // Kept on this computer, without its token in what is shown.
    let listed = keevoro.reports();
    assert_eq!(listed.reports.len(), 1);
    let one = &listed.reports[0];
    assert_eq!(one.reference, "K-1042");
    assert_eq!(one.first_line, "The map went blank");
    assert_eq!(one.status_said, "Received");
    assert_eq!(one.kind, ReportKind::Problem);
    assert!(!serde_json::to_string(&listed).unwrap().contains(TOKEN));
    let file = std::fs::read_to_string(keevoro.backups().config_folder().join(FILE)).unwrap();
    assert!(file.contains(TOKEN), "the token is kept, to ask after it");
    assert!(!file.contains("after a search"), "only the first line is kept");
}

#[test]
fn without_the_log_switched_on_no_line_of_it_goes() {
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro_reporting_to(dir.path(), &nowhere());
    keevoro.log().say("Looking through Family photos");
    let prepared = keevoro
        .prepare_report(&asked(KindWord::Idea, "A calmer map", false))
        .unwrap();
    assert!(!prepared.body.contains("Family photos"));
    assert!(!prepared.body.contains("logTail"));
}

#[test]
fn a_refusal_says_why_in_the_services_words_and_nothing_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let why = "That's a lot of reports from one computer today. Please try again tomorrow.";
    let (address, _) = fake_service(vec![(429, json!({ "ok": false, "why": why }))]);
    let keevoro = keevoro_reporting_to(dir.path(), &address);
    let prepared = keevoro.prepare_report(&asked(KindWord::Idea, "Hello", false)).unwrap();
    let refused = keevoro.send_report(&prepared.body).unwrap_err();
    assert_eq!(refused.kind, Kind::Refused);
    assert_eq!(refused.message, why);
    assert!(keevoro.reports().reports.is_empty());
}

#[test]
fn a_service_that_cannot_be_reached_says_so_plainly() {
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro_reporting_to(dir.path(), &nowhere());
    let prepared = keevoro.prepare_report(&asked(KindWord::Idea, "Hello", false)).unwrap();
    let failed = keevoro.send_report(&prepared.body).unwrap_err();
    assert_eq!(failed.kind, Kind::Unreachable);
    assert_eq!(failed.message, UNREACHABLE);
}

#[test]
fn an_answer_that_is_not_one_is_said_without_pretending() {
    let dir = tempfile::tempdir().unwrap();
    let (address, _) = fake_service(vec![
        (502, json!("a proxy's page")),
        (201, json!({ "ok": true, "reference": "K-1/../x", "token": TOKEN })),
    ]);
    let keevoro = keevoro_reporting_to(dir.path(), &address);
    let prepared = keevoro.prepare_report(&asked(KindWord::Idea, "Hello", false)).unwrap();
    let failed = keevoro.send_report(&prepared.body).unwrap_err();
    assert_eq!(failed.kind, Kind::Failed);
    assert!(failed.message.contains("502"), "{}", failed.message);
    // A reference that could not be put in an address is not taken.
    assert_eq!(keevoro.send_report(&prepared.body).unwrap_err().kind, Kind::Failed);
}

#[test]
fn a_report_changed_after_it_was_made_is_refused_and_not_sent() {
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro_reporting_to(dir.path(), &nowhere());
    let prepared = keevoro.prepare_report(&asked(KindWord::Idea, "Hello", false)).unwrap();
    let changed = prepared.body.replacen('{', "{\"details\": \"more\",", 1);
    let refused = keevoro.send_report(&changed).unwrap_err();
    assert_eq!(refused.kind, Kind::Refused, "refused before anything is sent");
    let empty = keevoro
        .prepare_report(&asked(KindWord::Problem, "   ", false))
        .unwrap_err();
    assert_eq!(empty.message, "Say what went wrong first.");
}

#[test]
fn asking_after_reports_notes_what_is_heard_and_says_it_in_plain_words() {
    let dir = tempfile::tempdir().unwrap();
    let (address, heard) = fake_service(vec![
        received("K-1042"),
        (
            200,
            json!({ "ok": true, "reference": "K-1042", "status": "shipped", "note": "Thank you for this one.",
                    "shippedIn": "0.47.0", "updatedAt": "2026-11-20T10:00:00Z" }),
        ),
    ]);
    let keevoro = keevoro_reporting_to(dir.path(), &address);
    let prepared = keevoro
        .prepare_report(&asked(KindWord::Idea, "A dark map", false))
        .unwrap();
    keevoro.send_report(&prepared.body).unwrap();
    let view = keevoro.refresh_reports();
    assert!(view.checked);
    assert_eq!(view.said, None);
    let one = &view.reports[0];
    assert_eq!(one.status.as_deref(), Some("shipped"));
    assert_eq!(one.status_said, "Done in version 0.47.0");
    assert_eq!(one.note.as_deref(), Some("Thank you for this one."));
    assert_eq!(
        heard.lock().unwrap()[1].0,
        format!("GET /v1/reports/K-1042?token={TOKEN} HTTP/1.1")
    );
    // Remembered, for when the service cannot be reached.
    assert_eq!(keevoro.reports().reports[0].status_said, "Done in version 0.47.0");
}

#[test]
fn offline_the_list_is_as_last_heard_and_says_it_could_not_check() {
    let dir = tempfile::tempdir().unwrap();
    let (address, _) = fake_service(vec![received("K-7")]);
    let keevoro = keevoro_reporting_to(dir.path(), &address);
    let prepared = keevoro
        .prepare_report(&asked(KindWord::Problem, "It stopped", false))
        .unwrap();
    keevoro.send_report(&prepared.body).unwrap();
    let offline = keevoro_reporting_to(dir.path(), &nowhere());
    let view = offline.refresh_reports();
    assert!(!view.checked);
    assert_eq!(view.said.as_deref(), Some("Couldn't check just now."));
    assert_eq!(view.reports.len(), 1);
    assert_eq!(view.reports[0].status_said, "Received");
    // With nothing sent there is nothing to ask, and nothing to say.
    let other = tempfile::tempdir().unwrap();
    let none = keevoro_reporting_to(other.path(), &nowhere()).refresh_reports();
    assert!(none.reports.is_empty() && none.checked && none.said.is_none());
}

#[test]
fn the_reports_file_is_read_a_report_at_a_time_and_what_is_not_understood_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro_reporting_to(dir.path(), &nowhere());
    let config = keevoro.backups().config_folder().to_path_buf();
    std::fs::create_dir_all(&config).unwrap();
    let written = json!({
        "v": 2,
        "reports": [
            { "reference": "K-1", "token": "aa", "kind": "idea", "firstLine": "Older", "sentAt": 100 },
            { "reference": "K-2", "kind": "complaint" },
            { "reference": "K-3", "token": "bb", "kind": "problem", "firstLine": "Newer", "sentAt": 200,
              "status": "in_progress", "seenBy": "a later version" }
        ],
        "muted": true
    })
    .to_string();
    std::fs::write(config.join(FILE), &written).unwrap();
    let listed = keevoro.reports();
    let lines: Vec<&str> = listed.reports.iter().map(|r| r.first_line.as_str()).collect();
    assert_eq!(
        lines,
        ["Newer", "Older"],
        "newest first, the one not understood passed over"
    );
    assert_eq!(listed.reports[0].status_said, "Being worked on");
    assert_eq!(std::fs::read_to_string(config.join(KEPT_ASIDE)).unwrap(), written);
    // A change keeps what it could not read, and what a later version wrote.
    ReportsFile::change(&config, |_| {}).unwrap();
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(config.join(FILE)).unwrap()).unwrap();
    assert_eq!(saved["muted"], true);
    assert_eq!(saved["reports"].as_array().unwrap().len(), 3);
    assert!(saved.to_string().contains("complaint"));
    // A file that is not a list of reports at all counts as none.
    std::fs::write(config.join(FILE), "[1, 2").unwrap();
    assert!(keevoro.reports().reports.is_empty());
}

#[test]
fn the_operations_are_reached_by_name_and_a_reset_forgets_the_list() {
    let dir = tempfile::tempdir().unwrap();
    let (address, _) = fake_service(vec![received("K-9")]);
    let keevoro = keevoro_reporting_to(dir.path(), &address);
    let prepared = call(
        &keevoro,
        Op::ReportsPrepare,
        None,
        json!({ "kind": "idea", "message": "Bigger previews", "contact": "you@example.com" }),
    )
    .unwrap();
    let body = prepared["body"].as_str().unwrap();
    assert!(body.contains("you@example.com"));
    let sent = call(&keevoro, Op::ReportsSend, None, json!({ "prepared": body })).unwrap();
    assert_eq!(sent["reference"], "K-9");
    let listed = call(&keevoro, Op::ReportsList, None, Value::Null).unwrap();
    assert_eq!(listed["reports"][0]["statusSaid"], "Received");
    assert_eq!(listed["checked"], false);
    let refused = call(
        &keevoro,
        Op::ReportsPrepare,
        None,
        json!({ "kind": "rant", "message": "x" }),
    )
    .unwrap_err();
    assert_eq!(refused.kind, Kind::NotUnderstood);
    keevoro.reset().unwrap();
    assert!(keevoro.reports().reports.is_empty());
    assert!(!keevoro.backups().config_folder().join(FILE).exists());
}
