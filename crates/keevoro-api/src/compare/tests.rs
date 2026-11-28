use super::*;
use crate::error::Kind;
use crate::{call, Op};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// What the stand-in heard: each request's first line and its headers,
/// with their names in lower case.
type Heard = Arc<Mutex<Vec<(String, Vec<(String, String)>)>>>;

/// A trimmed answer from Stratus, in its real shape, with made-up prices.
const ANSWER: &str = include_str!("answer.json");

fn answer() -> Value {
    serde_json::from_str(ANSWER).unwrap()
}

/// A tiny stand-in for Stratus on this computer, which gives these answers
/// in turn, one to each request, and notes what it heard. An answer of
/// `None` is never sent: the connection is held open, as a slow network
/// would.
fn stand_in(answers: Vec<(u16, Option<String>)>) -> (String, Heard) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}/api/v1", listener.local_addr().unwrap());
    let heard: Heard = Arc::default();
    let noted = heard.clone();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for (status, body) in answers {
            let Ok((stream, _)) = listener.accept() else { return };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            let mut headers = Vec::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line.trim().is_empty() {
                    break;
                }
                if let Some((name, value)) = line.split_once(':') {
                    headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
                }
            }
            noted.lock().unwrap().push((first.trim().to_string(), headers));
            let Some(text) = body else {
                held.push(stream);
                continue;
            };
            let mut stream = stream;
            write!(
                stream,
                "HTTP/1.1 {status} Answer\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                text.len()
            )
            .unwrap();
        }
        // Held until the test has given up on it.
        std::thread::sleep(Duration::from_secs(5));
        drop(held);
    });
    (address, heard)
}

/// An address nothing answers at.
fn nowhere() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}/api/v1", listener.local_addr().unwrap());
    drop(listener);
    address
}

fn asked(gigabytes: u64, currency: Currency) -> CompareRequest {
    CompareRequest { gigabytes, currency }
}

fn keevoro_asking(dir: &Path, address: &str) -> Keevoro {
    let address = address.to_string();
    crate::keevoro::tests::keevoro(dir)
        .with_environment(move |name| (name == ADDRESS_VARIABLE).then(|| address.clone()))
}

/// Every file under `dir`, read as bytes.
fn everything_under(dir: &Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(everything_under(&path));
        } else {
            found.push((path.clone(), std::fs::read(&path).unwrap_or_default()));
        }
    }
    found
}

fn holds(bytes: &[u8], part: &str) -> bool {
    bytes.windows(part.len()).any(|w| w == part.as_bytes())
}

#[test]
fn it_asks_only_for_the_size_and_the_currency_with_the_key_in_its_header() {
    let (address, heard) = stand_in(vec![(200, Some(ANSWER.to_string()))]);
    fetch(&address, &asked(240, Currency::Gbp), WAIT).unwrap();
    let heard = heard.lock().unwrap();
    let (first, headers) = &heard[0];
    assert_eq!(first, "GET /api/v1/quote?need=240GB&currency=GBP HTTP/1.1");
    let header = |name: &str| headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone());
    // A stand-in is never given the real key: only Stratus is.
    assert_eq!(header("x-api-key").as_deref(), Some(STAND_IN_KEY));
    assert!(header("user-agent").unwrap().starts_with("Keevoro/"));
    assert_eq!(header("cookie"), None);
    assert_eq!(header("referer"), None);
}

#[test]
fn the_real_key_goes_only_to_stratus_itself() {
    let key = key::stratus_key();
    // Shaped like Stratus's keys, without saying it.
    assert!(key.len() >= 32 && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
    assert_eq!(key_for(USUAL_ADDRESS), key);
    assert_eq!(key_for(&format!("{USUAL_ADDRESS}/")), key);
    for elsewhere in [
        "http://127.0.0.1:9/api/v1",
        "http://stratus-api.adam-sparshott.workers.dev/api/v1",
        "https://stratus-api.adam-sparshott.workers.dev.example.com/api/v1",
        "https://example.com/api/v1",
    ] {
        assert_eq!(key_for(elsewhere), STAND_IN_KEY, "{elsewhere}");
    }
}

/// The part of the key that would give it away, if it were written down.
fn middle_of_the_key() -> String {
    let key = key::stratus_key();
    let third = key.len() / 3;
    key[third..key.len() - third].to_string()
}

#[test]
fn the_key_is_written_nowhere_in_the_repository_or_the_built_window() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let middle = middle_of_the_key();
    match std::process::Command::new("git")
        .args(["grep", "--files-with-matches", "--fixed-strings", "-e"])
        .arg(&middle)
        .current_dir(&root)
        .output()
    {
        // `git grep` finds nothing with 1, and something with 0.
        Ok(out) if out.status.code() == Some(1) || out.status.code() == Some(0) => {
            assert!(
                out.status.code() == Some(1),
                "the key is written out in {}",
                String::from_utf8_lossy(&out.stdout)
            );
        }
        // Not a checkout, or no git: nothing tracked to look through.
        _ => eprintln!("git grep could not be run; the tracked files were not looked through"),
    }
    // Nor in the window as built, when it has been.
    for built in ["dist", "ui/dist"] {
        for (path, bytes) in everything_under(&root.join(built)) {
            assert!(!holds(&bytes, &middle), "the key is in {}", path.display());
        }
    }
}

#[test]
fn a_real_answer_is_read_cheapest_first_with_what_was_not_refreshed() {
    let (address, _) = stand_in(vec![(200, Some(ANSWER.to_string()))]);
    let got = fetch(&address, &asked(240, Currency::Gbp), WAIT).unwrap();
    assert_eq!(got.gigabytes, 240);
    assert_eq!(got.currency, Currency::Gbp);
    assert_eq!(got.fetched_at.as_deref(), Some("2026-11-02T06:15:00Z"));
    let names: Vec<&str> = got.options.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "S3 Glacier Deep Archive",
            "GCS Archive",
            "B2 Cloud Storage",
            "Wasabi Hot Cloud Storage",
            "R2 Standard",
            "pCloud Lifetime 500GB",
            "Azure Blob Hot (LRS)",
            "iCloud+ 2TB"
        ]
    );
    let b2 = &got.options[2];
    assert_eq!(b2.provider, "backblaze-b2");
    assert_eq!(b2.provider_name, "Backblaze B2");
    assert_eq!(b2.kind, "object");
    assert!(b2.metered);
    assert_eq!(b2.capacity_gb, None);
    assert_eq!(b2.monthly, 1.4);
    assert_eq!(b2.yearly, Some(16.8));
    assert_eq!(
        b2.url.as_deref(),
        Some("https://www.backblaze.com/cloud-storage/pricing")
    );
    let icloud = got.options.last().unwrap();
    assert_eq!(icloud.kind, "consumer");
    assert_eq!(icloud.capacity_gb, Some(2000.0));
    assert!(!icloud.metered);
    assert!(got.options[5].lifetime);
    // Sources Stratus could not refresh are named, so their rows can say so.
    assert_eq!(got.not_refreshed, ["gcs", "apple"]);
    // As the window is given it, in the service's own words.
    let shown = serde_json::to_value(&got).unwrap();
    assert_eq!(shown["options"][2]["providerName"], "Backblaze B2");
    assert_eq!(shown["options"][7]["capacityGB"], 2000.0);
    assert_eq!(shown["fetchedAt"], "2026-11-02T06:15:00Z");
    assert_eq!(shown["currency"], "GBP");
}

#[test]
fn what_cannot_be_trusted_in_an_answer_is_left_out() {
    let mut odd = answer();
    let first = odd["data"][0].clone();
    let with = |change: &dyn Fn(&mut Value)| {
        let mut option = first.clone();
        change(&mut option);
        option
    };
    let extra = [
        with(&|o| o["provider"] = json!("Not An Id!")),
        with(&|o| o["providerName"] = json!("x".repeat(500))),
        with(&|o| o["monthly"] = json!(-1)),
        with(&|o| o["monthly"] = json!("cheap")),
        with(&|o| {
            o["id"] = json!("odd-but-kept");
            o["name"] = json!("Kept\u{0007}  but\nplain");
            o["url"] = json!("http://example.com/not-secure");
            o["monthly"] = json!(0.01);
            o["yearly"] = json!(null);
        }),
        with(&|o| {
            o["id"] = json!("costly");
            o["url"] = json!("https://user:secret@example.com/");
            o["monthly"] = json!(99);
        }),
    ];
    odd["data"].as_array_mut().unwrap().extend(extra);
    odd["meta"]["fetchedAt"] = json!("yesterday-ish");
    let (address, _) = stand_in(vec![(200, Some(odd.to_string()))]);
    let got = fetch(&address, &asked(240, Currency::Gbp), WAIT).unwrap();
    let ids: Vec<&str> = got.options.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids.len(), 10, "{ids:?}");
    // Put in price order whatever order it came in.
    assert_eq!(ids[0], "odd-but-kept");
    assert_eq!(*ids.last().unwrap(), "costly");
    assert_eq!(got.options[0].name, "Kept but plain");
    assert_eq!(got.options[0].url, None);
    assert_eq!(got.options[0].yearly, None);
    assert_eq!(got.options.last().unwrap().url, None);
    assert_eq!(got.fetched_at, None, "a time that is not one is not shown");
}

#[test]
fn not_reaching_stratus_or_a_poor_answer_is_said_in_plain_words() {
    let plain = |error: &Error| {
        assert!(
            !error.message.contains("ureq") && !error.message.contains("os error"),
            "{}",
            error.message
        );
        assert!(error.message.starts_with(COULD_NOT), "{}", error.message);
    };
    // Nothing there at all.
    let (error, _) = fetch(&nowhere(), &asked(240, Currency::Gbp), WAIT).unwrap_err();
    assert_eq!(error.kind, Kind::Unreachable);
    plain(&error);
    assert!(
        error
            .message
            .contains("network would not let Keevoro connect to Stratus"),
        "{}",
        error.message
    );
    // Too slow: said so, and not waited on for ever.
    let (address, _) = stand_in(vec![(200, None)]);
    let started = std::time::Instant::now();
    let (error, logged) = fetch(&address, &asked(240, Currency::Gbp), Duration::from_millis(300)).unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(4));
    assert_eq!(error.kind, Kind::Unreachable);
    plain(&error);
    assert!(
        error.message.contains("Stratus took too long to answer"),
        "{}",
        error.message
    );
    assert!(logged.contains("it took too long"), "{logged}");
    // Answers that are not prices.
    for (status, body, says) in [
        (
            401,
            json!({ "error": "Missing or invalid API key" }).to_string(),
            "Stratus turned Keevoro's request away",
        ),
        (429, json!({ "error": "Slow down" }).to_string(), "Stratus is busy"),
        (503, "<html>Down</html>".to_string(), "Stratus had a problem of its own"),
        (200, "<html>Not JSON</html>".to_string(), "didn't understand"),
        (200, json!({ "data": "nope" }).to_string(), "didn't understand"),
    ] {
        let (address, _) = stand_in(vec![(status, Some(body))]);
        let (error, _) = fetch(&address, &asked(240, Currency::Gbp), WAIT).unwrap_err();
        plain(&error);
        assert!(error.message.contains(says), "{status}: {}", error.message);
        assert_ne!(error.kind, Kind::NotUnderstood, "{status}");
    }
}

#[test]
fn prices_are_never_written_down_and_the_key_is_never_logged_or_printed() {
    let dir = tempfile::tempdir().unwrap();
    let (address, _) = stand_in(vec![(200, Some(ANSWER.to_string())), (401, Some("{}".into()))]);
    let keevoro = keevoro_asking(dir.path(), &address);
    let got = keevoro.compare_storage(&asked(240, Currency::Gbp)).unwrap();
    assert_eq!(got.options.len(), 8);
    let error = keevoro.compare_storage(&asked(240, Currency::Gbp)).unwrap_err();
    let key = key::stratus_key();
    let printed = format!("{:?} {got:?} {error:?} {error}", asked(240, Currency::Gbp));
    for secret in [key.as_str(), STAND_IN_KEY] {
        assert!(!printed.contains(secret));
    }
    for (path, bytes) in everything_under(dir.path()) {
        for secret in [key.as_str(), STAND_IN_KEY] {
            assert!(!holds(&bytes, secret), "a key is in {}", path.display());
        }
        // Nothing of what Stratus said is kept: no provider, plan or price.
        for said in ["Backblaze", "B2 Cloud Storage", "iCloud", "8.99", "16.8"] {
            assert!(!holds(&bytes, said), "{said} is kept in {}", path.display());
        }
    }
    // The log says what was asked and how it went, and no more.
    let log = keevoro.log_tail(50);
    assert!(
        log.contains("Fetched cloud storage prices from Stratus for 240 GB in GBP"),
        "{log}"
    );
    assert!(
        log.contains("Couldn't fetch cloud storage prices from Stratus"),
        "{log}"
    );
}

#[test]
fn the_operation_is_reached_by_name_and_checks_what_it_is_asked() {
    let dir = tempfile::tempdir().unwrap();
    let (address, heard) = stand_in(vec![(200, Some(ANSWER.to_string()))]);
    let keevoro = keevoro_asking(dir.path(), &address);
    let got = call(
        &keevoro,
        Op::StorageCompare,
        None,
        json!({ "gigabytes": 2000, "currency": "EUR" }),
    )
    .unwrap();
    assert_eq!(got["options"][0]["provider"], "aws-s3");
    assert_eq!(got["gigabytes"], 2000);
    assert_eq!(
        heard.lock().unwrap()[0].0,
        "GET /api/v1/quote?need=2000GB&currency=EUR HTTP/1.1"
    );
    for wrong in [
        json!({ "gigabytes": 240, "currency": "JPY" }),
        json!({ "gigabytes": 240 }),
        json!({ "gigabytes": -1, "currency": "GBP" }),
        json!({ "gigabytes": 240, "currency": "GBP", "email": "you@example.com" }),
    ] {
        let e = call(&keevoro, Op::StorageCompare, None, wrong.clone()).unwrap_err();
        assert_eq!(e.kind, Kind::NotUnderstood, "{wrong}");
    }
    for size in [0, 2_000_000] {
        let e = call(
            &keevoro,
            Op::StorageCompare,
            None,
            json!({ "gigabytes": size, "currency": "GBP" }),
        )
        .unwrap_err();
        assert_eq!(e.kind, Kind::Refused, "{size}");
        assert_eq!(e.message, "Choose a size from 1 GB to 1,000,000 GB to compare.");
    }
    assert_eq!(
        heard.lock().unwrap().len(),
        1,
        "nothing is asked for a request that is refused"
    );
}

/// Reaches the real Stratus, so it is left out of an ordinary run:
/// `cargo test -p keevoro-api --lib compare -- --ignored`.
#[test]
#[ignore = "reaches the real Stratus over the internet"]
fn the_real_stratus_answers_with_prices() {
    let got = fetch(USUAL_ADDRESS, &asked(240, Currency::Gbp), WAIT)
        .map_err(|(e, logged)| format!("{e}: {logged}"))
        .unwrap();
    assert!(got.options.len() > 5);
    assert!(got.options.windows(2).all(|w| w[0].monthly <= w[1].monthly));
    assert!(got.options.iter().any(|o| o.provider == "cloudflare-r2"));
    assert!(got.fetched_at.is_some());
}
