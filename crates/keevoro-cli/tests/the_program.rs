//! The `keevoro` program itself, run as a separate process, as a script
//! runs it: its exit codes, a job left to run on its own, and storage keys
//! that must not come out anywhere.
//!
//! Every run keeps Keevoro's folders in a temporary folder, through
//! `KEEVORO_HOME`, and never uses a keychain: a key given here is refused
//! by storage that cannot be reached before it could be kept.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

struct Ran {
    code: i32,
    out: String,
    err: String,
}

fn keevoro(home: &Path, args: &[&str], env: &[(&str, &str)], stdin: &str) -> Ran {
    let mut command = Command::new(env!("CARGO_BIN_EXE_keevoro"));
    command
        .args(args)
        .env("KEEVORO_HOME", home)
        .env_remove("KEEVORO_ACCESS_KEY_ID")
        .env_remove("KEEVORO_SECRET_ACCESS_KEY")
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    {
        use std::io::Write;
        let mut input = child.stdin.take().unwrap();
        input.write_all(stdin.as_bytes()).unwrap();
    }
    let Output { status, stdout, stderr } = child.wait_with_output().unwrap();
    Ran {
        code: status.code().unwrap_or(-1),
        out: String::from_utf8(stdout).unwrap(),
        err: String::from_utf8(stderr).unwrap(),
    }
}

fn card(dir: &Path) -> PathBuf {
    let card = dir.join("Card");
    for (rel, shade) in [
        ("Trip/IMG_20240601_100000.jpg", 10),
        ("Trip/IMG_20240601_100500.jpg", 90),
    ] {
        let p = card.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let img = image::RgbImage::from_pixel(640, 480, image::Rgb([shade, 120, 60]));
        img.save(p).unwrap();
    }
    card
}

#[test]
fn exit_codes_are_the_ones_documented() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    assert_eq!(keevoro(&home, &["--version"], &[], "").code, 0);
    let ran = keevoro(&home, &["frobnicate"], &[], "");
    assert_eq!(ran.code, 64, "{}", ran.err);
    let ran = keevoro(&home, &["found"], &[], "");
    assert_eq!((ran.code, ran.err.trim()), (3, "Choose a backup first."));
    assert_eq!(keevoro(&home, &["backups", "start", "Card photos"], &[], "").code, 0);
    let ran = keevoro(&home, &["run"], &[], "");
    assert_eq!(ran.code, 5, "nowhere to back up to: {}", ran.err);
    assert!(ran.err.contains("Choose where to back up to first."), "{}", ran.err);
}

#[test]
fn a_detached_job_carries_on_after_the_command_has_come_back() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let card = card(dir.path());
    keevoro(&home, &["backups", "start", "Card photos"], &[], "");
    let ran = keevoro(
        &home,
        &["look", &card.display().to_string(), "--detach", "--json"],
        &[],
        "",
    );
    assert_eq!(ran.code, 0, "{}", ran.err);
    let started: serde_json::Value = serde_json::from_str(ran.out.trim()).unwrap();
    assert_eq!(started["op"], "sources.look");
    assert_eq!(started["startedBy"], "commandLine");

    // The program that runs it is another one, so its work is seen here.
    let begun = Instant::now();
    loop {
        let ran = keevoro(&home, &["sources", "--json"], &[], "");
        let sources: serde_json::Value = serde_json::from_str(ran.out.trim()).unwrap();
        if sources.as_array().is_some_and(|s| s.len() == 1) {
            break;
        }
        assert!(
            begun.elapsed() < Duration::from_secs(60),
            "the detached look never finished"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    // And a detached job that cannot start says why, with its exit code.
    let ran = keevoro(
        &home,
        &["look", &dir.path().join("Unplugged").display().to_string(), "--detach"],
        &[],
        "",
    );
    assert_eq!(ran.code, 3, "{}", ran.err);
    assert!(ran.err.contains("Is it plugged in?"), "{}", ran.err);
}

#[test]
fn a_key_given_never_comes_out_of_the_program() {
    const ACCESS: &str = "AKIAKEEVOROPROGRAMMARKER";
    const SECRET: &str = "keevoro-program-secret-marker/42+x";
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    keevoro(&home, &["backups", "start", "Card photos"], &[], "");
    let add = [
        "destinations",
        "add",
        "--endpoint",
        "http://127.0.0.1:9",
        "--bucket",
        "family",
    ];
    let mut runs = Vec::new();
    let mut from_stdin = add.to_vec();
    from_stdin.push("--keys-from-stdin");
    runs.push(keevoro(&home, &from_stdin, &[], &format!("{ACCESS}\n{SECRET}\n")));
    runs.push(keevoro(
        &home,
        &add,
        &[("KEEVORO_ACCESS_KEY_ID", ACCESS), ("KEEVORO_SECRET_ACCESS_KEY", SECRET)],
        "",
    ));
    let mut typed = add.to_vec();
    typed.extend(["--access-key-id", ACCESS, "--secret-access-key", SECRET, "--json"]);
    runs.push(keevoro(&home, &typed, &[], ""));
    for ran in &runs {
        assert_eq!(ran.code, 2, "storage that cannot be reached: {}", ran.err);
        assert!(!ran.out.contains(ACCESS) && !ran.out.contains(SECRET), "{}", ran.out);
        assert!(!ran.err.contains(ACCESS) && !ran.err.contains(SECRET), "{}", ran.err);
    }
    let mut folders = vec![home.clone()];
    let mut files = 0;
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(folder).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else {
                files += 1;
                let text = String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned();
                assert!(
                    !text.contains(ACCESS) && !text.contains(SECRET),
                    "a key was written to {}",
                    path.display()
                );
            }
        }
    }
    assert!(files > 0, "Keevoro wrote its settings and log");
}
