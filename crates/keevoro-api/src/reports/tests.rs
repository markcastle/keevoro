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

    let sent = keevoro.send_report(&prepared.body, None).unwrap();
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
    let refused = keevoro.send_report(&prepared.body, None).unwrap_err();
    assert_eq!(refused.kind, Kind::Refused);
    assert_eq!(refused.message, why);
    assert!(keevoro.reports().reports.is_empty());
}

#[test]
fn a_service_that_cannot_be_reached_says_so_plainly() {
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro_reporting_to(dir.path(), &nowhere());
    let prepared = keevoro.prepare_report(&asked(KindWord::Idea, "Hello", false)).unwrap();
    let failed = keevoro.send_report(&prepared.body, None).unwrap_err();
    assert_eq!(failed.kind, Kind::Unreachable);
    assert_eq!(
        failed.message,
        format!("{UNREACHABLE} {}", Unreached::Connection.said().unwrap())
    );
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
    let failed = keevoro.send_report(&prepared.body, None).unwrap_err();
    assert_eq!(failed.kind, Kind::Failed);
    assert!(failed.message.contains("502"), "{}", failed.message);
    // A reference that could not be put in an address is not taken.
    assert_eq!(
        keevoro.send_report(&prepared.body, None).unwrap_err().kind,
        Kind::Failed
    );
}

#[test]
fn a_report_changed_after_it_was_made_is_refused_and_not_sent() {
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro_reporting_to(dir.path(), &nowhere());
    let prepared = keevoro.prepare_report(&asked(KindWord::Idea, "Hello", false)).unwrap();
    let changed = prepared.body.replacen('{', "{\"details\": \"more\",", 1);
    let refused = keevoro.send_report(&changed, None).unwrap_err();
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
    keevoro.send_report(&prepared.body, None).unwrap();
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
    keevoro.send_report(&prepared.body, None).unwrap();
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
    call(&keevoro, Op::ReportsCode, None, json!({ "code": "TESTER-1234" })).unwrap();
    keevoro.reset().unwrap();
    assert!(keevoro.reports().reports.is_empty());
    assert!(!keevoro.backups().config_folder().join(FILE).exists());
    // And the reporting code goes from the keychain.
    assert_eq!(keevoro.secrets().get(CODE_ENTRY).unwrap(), None);
    let after = call(&keevoro, Op::ReportsCode, None, json!({})).unwrap();
    assert_eq!(after["hasCode"], false);
}

#[test]
fn not_reaching_the_service_is_told_apart_from_its_answer_and_said_in_plain_words() {
    use std::io::{Error as IoError, ErrorKind};
    for (error, cause) in [
        (ureq::Error::HostNotFound, Unreached::Name),
        (ureq::Error::ConnectionFailed, Unreached::Connection),
        (
            IoError::from(ErrorKind::ConnectionRefused).into(),
            Unreached::Connection,
        ),
        (IoError::from(ErrorKind::TimedOut).into(), Unreached::Slow),
        (ureq::Error::Timeout(ureq::Timeout::Connect), Unreached::Slow),
        (ureq::Error::Tls("the certificate was not trusted"), Unreached::Secure),
        (
            ureq::Error::ConnectProxyFailed("407 Proxy Authentication Required".into()),
            Unreached::Proxy,
        ),
        (ureq::Error::InvalidProxyUrl, Unreached::Proxy),
    ] {
        assert_eq!(Unreached::of(&error), cause, "{error}");
        assert!(cause.said().is_some_and(|s| !s.contains('—')), "{cause:?}");
    }
    assert_eq!(Unreached::of(&ureq::Error::TooManyRedirects), Unreached::Other);
    assert_eq!(Unreached::Other.said(), None);
    assert_eq!(
        Unreached::Secure.said(),
        Some("This computer's network would not let Keevoro connect securely, as some work and school networks do.")
    );

    // Nobody there: not reached, with why, said and logged.
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro_reporting_to(dir.path(), &nowhere());
    let prepared = keevoro.prepare_report(&asked(KindWord::Idea, "Hello", false)).unwrap();
    let failed = keevoro.send_report_saying_why(&prepared.body, None).unwrap_err();
    assert_eq!(
        (failed.error.kind, failed.error.message.as_str()),
        (Kind::Unreachable, UNREACHABLE)
    );
    assert_eq!(failed.cause.as_deref(), Unreached::Connection.said());
    let log = keevoro.log_tail(20);
    assert!(log.contains("Couldn't send a report: the connection failed: "), "{log}");
    assert!(!log.contains("Hello"), "never the report itself");

    // An answer, even a refusal, is not "not reached", and has no cause.
    let why = "That's a lot of reports from one computer today. Please try again tomorrow.";
    let (address, _) = fake_service(vec![(429, json!({ "ok": false, "why": why }))]);
    let answered = keevoro_reporting_to(dir.path(), &address)
        .send_report_saying_why(&prepared.body, None)
        .unwrap_err();
    assert_eq!((answered.error.kind, answered.cause), (Kind::Refused, None));
}

#[test]
fn a_report_the_window_sent_is_kept_as_one_sent_here_and_a_wrong_answer_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let address = nowhere();
    let keevoro = keevoro_reporting_to(dir.path(), &format!("{address}/"));
    let prepared = keevoro
        .prepare_report(&asked(KindWord::Problem, "It stopped\nhalf way", false))
        .unwrap();
    assert_eq!(
        prepared.send_to, address,
        "where the window sends it, without a slash at its end"
    );
    let note = |prepared: &str, reference: &str, token: &str| {
        call(
            &keevoro,
            Op::ReportsNote,
            None,
            json!({ "prepared": prepared, "reference": reference, "token": token }),
        )
    };
    for (reference, token) in [
        ("1042", TOKEN),
        ("K-", TOKEN),
        ("K-10a", TOKEN),
        ("K-1042/../x", TOKEN),
        ("K-1042", "0123456789abcdef"),
        ("K-1042", "0123456789abcdef0123456789abcdef0123456g"),
        ("K-1042", "0123456789ABCDEF0123456789ABCDEF01234567x"),
    ] {
        let refused = note(&prepared.body, reference, token).unwrap_err();
        assert_eq!(refused.kind, Kind::Refused, "{reference} {token}");
    }
    // Text that is not a report as Keevoro makes them passes no better
    // than it would to be sent.
    let changed = prepared.body.replace("\"kind\"", "\"sort\"");
    let refused = note(&changed, "K-1042", TOKEN).unwrap_err();
    assert_eq!(
        (refused.kind, refused.message.as_str()),
        (
            Kind::Refused,
            "That report has changed since it was made. Make it again, then send it."
        )
    );
    assert!(keevoro.reports().reports.is_empty(), "nothing kept from a wrong answer");

    let sent = note(&prepared.body, "K-1042", TOKEN).unwrap();
    assert_eq!(sent["said"], "Thank you. Your reference is K-1042.");
    let listed = keevoro.reports();
    assert_eq!(
        (
            listed.reports[0].reference.as_str(),
            listed.reports[0].first_line.as_str()
        ),
        ("K-1042", "It stopped")
    );
    let file = std::fs::read_to_string(keevoro.backups().config_folder().join(FILE)).unwrap();
    assert!(file.contains(TOKEN), "kept to ask after it");
    assert!(!keevoro.log_tail(20).contains(TOKEN), "never in the log");
}

#[test]
fn the_window_asks_after_reports_when_keevoro_cannot_and_what_it_heard_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let (address, _) = fake_service(vec![received("K-7")]);
    let keevoro = keevoro_reporting_to(dir.path(), &address);
    let prepared = keevoro
        .prepare_report(&asked(KindWord::Problem, "It stopped", false))
        .unwrap();
    keevoro.send_report(&prepared.body, None).unwrap();
    let asking = call(&keevoro, Op::ReportsAsking, None, Value::Null).unwrap();
    assert_eq!(asking["sendTo"], address);
    assert_eq!(asking["reports"], json!([{ "reference": "K-7", "token": TOKEN }]));
    let heard = call(
        &keevoro,
        Op::ReportsHeard,
        None,
        json!({
            "all": true,
            "heard": [
                { "reference": "K-7", "answer": { "ok": true, "reference": "K-7", "status": "shipped", "shippedIn": "0.47.0" } },
                { "reference": "K-8", "answer": { "ok": true, "status": "shipped" } },
            ],
        }),
    )
    .unwrap();
    assert_eq!(heard["checked"], true);
    assert_eq!(heard["reports"][0]["statusSaid"], "Done in version 0.47.0");
    assert_eq!(
        heard["reports"].as_array().map(Vec::len),
        Some(1),
        "an answer about no report here changes nothing"
    );
    // An answer about another report, or not an answer, changes nothing.
    let unchanged = call(
        &keevoro,
        Op::ReportsHeard,
        None,
        json!({ "all": false, "heard": [{ "reference": "K-7", "answer": { "ok": true, "reference": "K-9", "status": "new" } }] }),
    )
    .unwrap();
    assert_eq!(unchanged["reports"][0]["statusSaid"], "Done in version 0.47.0");
    assert_eq!(unchanged["checked"], false);
    assert_eq!(unchanged["said"], "Couldn't check just now.");
}

const CODE: &str = "TESTER-1234";

fn recognised(reference: &str, recognised: Option<bool>) -> (u16, Value) {
    let mut answer = json!({ "ok": true, "reference": reference, "token": TOKEN });
    if let Some(recognised) = recognised {
        answer["recognised"] = recognised.into();
    }
    (201, answer)
}

fn code(keevoro: &Keevoro, request: Value) -> crate::error::Result<Value> {
    call(keevoro, Op::ReportsCode, None, request)
}

#[test]
fn a_reporting_code_is_kept_in_the_keychain_sent_only_once_saved_and_never_shown_kept_or_logged() {
    let dir = tempfile::tempdir().unwrap();
    let (address, heard) = fake_service(vec![received("K-1"), recognised("K-2", Some(true))]);
    let keevoro = keevoro_reporting_to(dir.path(), &address);

    // None saved: none goes, and the report is sent exactly as shown.
    assert_eq!(code(&keevoro, json!({})).unwrap(), json!({ "hasCode": false }));
    let plain = keevoro.prepare_report(&asked(KindWord::Idea, "First", false)).unwrap();
    assert!(!plain.with_code);
    let sent = keevoro.send_report(&plain.body, None).unwrap();
    assert_eq!((sent.recognised, sent.code_said.as_deref()), (None, None));

    // Saved, trimmed, in the keychain, and only whether one is saved comes back.
    let saved = code(&keevoro, json!({ "code": format!("  {CODE} ") })).unwrap();
    assert_eq!(saved["hasCode"], true);
    assert!(!saved.to_string().contains(CODE), "{saved}");
    assert_eq!(keevoro.secrets().get(CODE_ENTRY).unwrap().as_deref(), Some(CODE));
    assert!(!code(&keevoro, json!({})).unwrap().to_string().contains(CODE));

    // Never in what is shown: the preview only says one will go.
    let prepared = keevoro.prepare_report(&asked(KindWord::Idea, "Second", false)).unwrap();
    assert!(prepared.with_code);
    assert!(!prepared.body.contains(CODE) && !prepared.body.contains("\"code\""));
    let shown = call(
        &keevoro,
        Op::ReportsPrepare,
        None,
        json!({ "kind": "idea", "message": "Second" }),
    )
    .unwrap();
    assert!(!shown.to_string().contains(CODE));
    assert_eq!(shown["withCode"], true);

    // Sent with the report, after everything shown, which is as it was.
    let sent = keevoro.send_report(&prepared.body, None).unwrap();
    assert_eq!(sent.recognised, Some(true));
    assert_eq!(sent.code_said.as_deref(), Some("Your reporting code was recognised."));
    assert!(!serde_json::to_string(&sent).unwrap().contains(CODE));
    let heard = heard.lock().unwrap().clone();
    assert_eq!(heard[0].1, plain.body, "none goes before one is saved");
    assert_eq!(heard[1].1, keevoro_core::report::with_code(&prepared.body, CODE));
    let went: Value = serde_json::from_str(&heard[1].1).unwrap();
    assert_eq!(went["code"], CODE);

    // The list marks the one recognised; the code is in no file and no log.
    let listed = keevoro.reports();
    let recognised: Vec<(&str, bool)> = listed
        .reports
        .iter()
        .map(|r| (r.reference.as_str(), r.recognised))
        .collect();
    assert!(recognised.contains(&("K-2", true)) && recognised.contains(&("K-1", false)));
    let config = keevoro.backups().config_folder();
    for file in std::fs::read_dir(config).unwrap().flatten() {
        if file.path().is_file() {
            let text = std::fs::read_to_string(file.path()).unwrap_or_default();
            assert!(!text.contains(CODE), "{}", file.path().display());
        }
    }
    let kept = std::fs::read_to_string(config.join(FILE)).unwrap();
    assert!(kept.contains("\"recognised\": true"), "{kept}");
    assert!(!keevoro.log_tail(200).contains(CODE));
    assert!(!call(&keevoro, Op::ReportsList, None, Value::Null)
        .unwrap()
        .to_string()
        .contains(CODE));
    assert!(!call(&keevoro, Op::ReportsAsking, None, Value::Null)
        .unwrap()
        .to_string()
        .contains(CODE));
}

#[test]
fn a_code_not_recognised_says_so_and_stays_saved_and_an_older_service_says_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (address, heard) = fake_service(vec![recognised("K-3", Some(false)), recognised("K-4", None)]);
    let keevoro = keevoro_reporting_to(dir.path(), &address);
    code(&keevoro, json!({ "code": "TESTER-9999" })).unwrap();
    let prepared = keevoro
        .prepare_report(&asked(KindWord::Problem, "It stopped", false))
        .unwrap();
    let sent = keevoro.send_report(&prepared.body, None).unwrap();
    assert_eq!(sent.said, "Thank you. Your reference is K-3.");
    assert_eq!(sent.recognised, Some(false));
    assert_eq!(
        sent.code_said.as_deref(),
        Some("That reporting code was not recognised, so this was sent as an ordinary report. Check the code with whoever gave it to you.")
    );
    assert_eq!(
        code(&keevoro, json!({})).unwrap()["hasCode"],
        true,
        "kept, to be put right"
    );
    // A service that does not know of codes says nothing of it.
    let sent = keevoro.send_report(&prepared.body, None).unwrap();
    assert_eq!((sent.recognised, sent.code_said), (None, None));
    assert!(heard.lock().unwrap()[1].1.contains("TESTER-9999"));
    assert!(keevoro.reports().reports.iter().all(|r| !r.recognised));
    let kept = std::fs::read_to_string(keevoro.backups().config_folder().join(FILE)).unwrap();
    assert!(kept.contains("\"recognised\": false"), "{kept}");
}

#[test]
fn a_code_given_only_with_one_report_goes_with_it_and_is_not_kept() {
    let dir = tempfile::tempdir().unwrap();
    let (address, heard) = fake_service(vec![recognised("K-5", Some(true))]);
    let keevoro = keevoro_reporting_to(dir.path(), &address);
    let prepared = keevoro.prepare_report(&asked(KindWord::Idea, "Hello", false)).unwrap();
    let sent = call(
        &keevoro,
        Op::ReportsSend,
        None,
        json!({ "prepared": prepared.body, "code": format!(" {CODE}") }),
    )
    .unwrap();
    assert_eq!(sent["codeSaid"], "Your reporting code was recognised.");
    assert_eq!(
        heard.lock().unwrap()[0].1,
        keevoro_core::report::with_code(&prepared.body, CODE)
    );
    assert_eq!(code(&keevoro, json!({})).unwrap()["hasCode"], false);
}

#[test]
fn forgetting_a_code_takes_it_from_the_keychain_and_none_goes_after() {
    let dir = tempfile::tempdir().unwrap();
    let (address, heard) = fake_service(vec![received("K-6")]);
    let keevoro = keevoro_reporting_to(dir.path(), &address);
    code(&keevoro, json!({ "code": CODE })).unwrap();
    let forgot = code(&keevoro, json!({ "forget": true })).unwrap();
    assert_eq!(forgot["hasCode"], false);
    assert_eq!(keevoro.secrets().get(CODE_ENTRY).unwrap(), None);
    let prepared = keevoro.prepare_report(&asked(KindWord::Idea, "Hello", false)).unwrap();
    assert!(!prepared.with_code);
    keevoro.send_report(&prepared.body, None).unwrap();
    assert_eq!(heard.lock().unwrap()[0].1, prepared.body);
    // Forgetting none is harmless; asking both at once is not understood.
    assert_eq!(code(&keevoro, json!({ "forget": true })).unwrap()["hasCode"], false);
    let both = code(&keevoro, json!({ "code": CODE, "forget": true })).unwrap_err();
    assert_eq!(both.kind, Kind::NotUnderstood);
    assert!(!both.message.contains(CODE), "{}", both.message);
}

#[test]
fn a_code_with_anything_but_letters_digits_and_hyphens_is_refused_before_anything_is_sent() {
    let dir = tempfile::tempdir().unwrap();
    // Nothing answers: a refusal must come before any sending.
    let keevoro = keevoro_reporting_to(dir.path(), &nowhere());
    for bad in ["TESTER 1234", "tester_1", "a;b", "", &"a".repeat(65)] {
        let refused = code(&keevoro, json!({ "code": bad })).unwrap_err();
        assert_eq!(refused.kind, Kind::Refused, "{bad}");
        assert_eq!(keevoro.secrets().get(CODE_ENTRY).unwrap(), None);
    }
    let prepared = keevoro.prepare_report(&asked(KindWord::Idea, "Hello", false)).unwrap();
    let refused = keevoro.send_report(&prepared.body, Some("TESTER 1234")).unwrap_err();
    assert_eq!(
        (refused.kind, refused.message.as_str()),
        (
            Kind::Refused,
            "A reporting code has only letters, digits and hyphens in it. Check it and try again."
        )
    );
    assert!(!keevoro.log_tail(50).contains("TESTER"));
}

#[test]
fn when_keevoro_cannot_reach_the_service_the_window_is_given_the_report_with_the_code_once() {
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro_reporting_to(dir.path(), &nowhere());
    let prepared = keevoro.prepare_report(&asked(KindWord::Idea, "Hello", false)).unwrap();
    let not = keevoro.send_report_saying_why(&prepared.body, None).unwrap_err();
    assert_eq!(not.send_as, None, "without a code the window sends what it was shown");
    code(&keevoro, json!({ "code": CODE })).unwrap();
    let not = keevoro.send_report_saying_why(&prepared.body, None).unwrap_err();
    assert_eq!(not.error.kind, Kind::Unreachable);
    let send_as = not.send_as.clone().unwrap();
    assert_eq!(send_as.0, keevoro_core::report::with_code(&prepared.body, CODE));
    assert!(
        !format!("{not:?}").contains(CODE),
        "never in what is printed for a developer"
    );
    assert!(!keevoro.log_tail(50).contains(CODE));
    // An answer the service refused is no reason to give it out.
    let (address, _) = fake_service(vec![(429, json!({ "ok": false, "why": "Not today." }))]);
    let refused = keevoro_reporting_to(dir.path(), &address)
        .send_report_saying_why(&prepared.body, None)
        .unwrap_err();
    assert_eq!(refused.send_as, None);

    // What the window heard is kept, recognised or not.
    let sent = call(
        &keevoro,
        Op::ReportsNote,
        None,
        json!({ "prepared": prepared.body, "reference": "K-1042", "token": TOKEN, "recognised": true }),
    )
    .unwrap();
    assert_eq!(sent["codeSaid"], "Your reporting code was recognised.");
    assert!(keevoro.reports().reports[0].recognised);
}

#[test]
fn an_agent_never_reads_a_code_and_saves_or_forgets_one_only_once_allowed_to_make_changes() {
    let dir = tempfile::tempdir().unwrap();
    let keevoro = keevoro_reporting_to(dir.path(), &nowhere());
    let agent = |request: Value| crate::call_for_agent(&keevoro, Op::ReportsCode, None, request);
    assert_eq!(agent(json!({})).unwrap(), json!({ "hasCode": false }));
    let refused = agent(json!({ "code": CODE })).unwrap_err();
    assert_eq!(refused.message, crate::for_agents::LOOK_ONLY);
    assert_eq!(
        agent(json!({ "forget": true })).unwrap_err().message,
        crate::for_agents::LOOK_ONLY
    );
    assert_eq!(keevoro.secrets().get(CODE_ENTRY).unwrap(), None);
    call(&keevoro, Op::AgentsAccess, None, json!({ "mayChange": true })).unwrap();
    assert_eq!(agent(json!({ "code": CODE })).unwrap()["hasCode"], true);
    let looked = agent(json!({})).unwrap();
    assert_eq!(looked, json!({ "hasCode": true }), "only whether one is saved");
    let bad = agent(json!({ "code": "TESTER 1234!" })).unwrap_err();
    assert!(!bad.message.contains("TESTER"), "{}", bad.message);
    assert_eq!(agent(json!({ "forget": true })).unwrap()["hasCode"], false);
    let log = keevoro.log_tail(200);
    assert!(log.contains("An AI agent used reporting_code"), "{log}");
    assert!(!log.contains("TESTER"), "{log}");
    // Sending a report stays the person's.
    assert!(Op::ReportsSend.tool().is_none());
}
