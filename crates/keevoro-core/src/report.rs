//! A report of a problem, or an idea, for Keevoro's maker: what goes in it.
//!
//! A person writes a few words and Keevoro sends them, with as little else
//! as will help. By default that is only the kind of report, their words,
//! Keevoro's version, the operating system and the screen they were on.
//! The end of Keevoro's log goes only when they switch it on, and a way to
//! reach them only when they write one.
//!
//! Before the log goes anywhere it is scrubbed ([`scrub`]): the person's
//! home folder and user name become `~`, and anything that looks like a key,
//! a secret or a token is hidden. Keevoro never logs keys, so this is belt
//! and braces, but the log can still name folders and files, which is why
//! the person is told so before they switch it on.
//!
//! What is sent is made once, here, as text ([`prepare`]). The person is
//! shown that text, and that same text is what is sent, so what they saw
//! is exactly what went. Everything here is pure: no file is read and
//! nothing is sent.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// The most a person's words may be, in letters.
pub const MESSAGE_MOST: usize = 5_000;
/// The most of the log that goes, in bytes. It is cut from the front, so
/// the newest lines, which say what just happened, are the ones kept.
pub const LOG_MOST: usize = 120_000;
/// The most the operating system, the screen and a way to reach the person
/// may be, each, in letters.
pub const SHORT_MOST: usize = 200;
/// The most the whole report may be, in bytes, as the service takes it.
pub const BODY_MOST: usize = 220_000;
/// What stands in for anything hidden from the log.
pub const HIDDEN: &str = "•••";

/// Whether a report is about something that went wrong, or an idea.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReportKind {
    Problem,
    Idea,
}

/// What is sent, field by field, in the order it is written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Report {
    pub kind: ReportKind,
    pub message: String,
    pub app_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_tail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact: Option<String>,
}

/// What the person chose to send.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Draft {
    pub kind: Option<ReportKind>,
    pub message: String,
    /// True when they switched on "Include the end of Keevoro's log".
    pub with_log: bool,
    /// What they wrote under "Include a way to reach me", if they switched
    /// it on.
    pub contact: Option<String>,
    /// Which screen they were on.
    pub screen: Option<String>,
}

/// What Keevoro knows about itself and the computer, for a report.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Facts {
    pub app_version: String,
    pub os: Option<String>,
    /// The end of the log as it is, before scrubbing.
    pub log: String,
    /// The person's home folder, as the computer writes it.
    pub home: Option<String>,
    /// The person's user name, in each form the computer gives it.
    pub names: Vec<String>,
}

/// Makes the report, as the exact text that will be sent, or says in
/// words why it cannot be sent.
pub fn prepare(draft: &Draft, facts: &Facts) -> Result<String, String> {
    let kind = draft
        .kind
        .ok_or_else(|| "Say whether something went wrong or you have an idea.".to_string())?;
    let message = checked_message(kind, &draft.message)?;
    let contact = checked_contact(draft.contact.as_deref())?;
    let names: Vec<&str> = facts.names.iter().map(String::as_str).collect();
    let mut log = draft
        .with_log
        .then(|| scrub(&facts.log, facts.home.as_deref(), &names))
        .map(|log| keep_the_end(&log, LOG_MOST).to_string());
    loop {
        let report = Report {
            kind,
            message: message.clone(),
            app_version: facts.app_version.clone(),
            os: short(facts.os.as_deref()),
            screen: short(draft.screen.as_deref()),
            log_tail: log.clone().filter(|l| !l.is_empty()),
            contact: contact.clone(),
        };
        let body = written(&report)?;
        // Writing it out can make the log longer (every \ becomes \\), so
        // the oldest lines are cut until it fits.
        match log.as_deref() {
            Some(text) if body.len() > BODY_MOST && !text.is_empty() => {
                let over = body.len() - BODY_MOST;
                let most = text.len().saturating_sub(over * 2 + 64);
                log = Some(keep_the_end(text, most).to_string());
            }
            _ if body.len() > BODY_MOST => return Err("That report is too long to send. Make it shorter.".into()),
            _ => return Ok(body),
        }
    }
}

/// Reads back a report [`prepare`] made, to send it, and checks it is still
/// one: everything within its limits, and a log that was scrubbed. Anything
/// else is refused rather than sent.
pub fn check_prepared(body: &str, home: Option<&str>, names: &[&str]) -> Result<Report, String> {
    let changed = || "That report has changed since it was made. Make it again, then send it.".to_string();
    if body.len() > BODY_MOST {
        return Err("That report is too long to send. Make it shorter.".into());
    }
    let report: Report = serde_json::from_str(body).map_err(|_| changed())?;
    checked_message(report.kind, &report.message)?;
    checked_contact(report.contact.as_deref())?;
    let short_enough = |text: &Option<String>| text.as_ref().is_none_or(|t| t.chars().count() <= SHORT_MOST);
    if report.app_version.is_empty() || !short_enough(&report.os) || !short_enough(&report.screen) {
        return Err(changed());
    }
    if let Some(log) = &report.log_tail {
        if log.len() > LOG_MOST || scrub(log, home, names) != *log {
            return Err(changed());
        }
    }
    Ok(report)
}

fn written(report: &Report) -> Result<String, String> {
    // Set out a field to a line, so the person can read what they are shown.
    serde_json::to_string_pretty(report).map_err(|e| format!("Keevoro couldn't write the report: {e}."))
}

fn checked_message(kind: ReportKind, message: &str) -> Result<String, String> {
    let message = message.trim();
    if message.is_empty() {
        return Err(match kind {
            ReportKind::Problem => "Say what went wrong first.".into(),
            ReportKind::Idea => "Say what your idea is first.".into(),
        });
    }
    let letters = message.chars().count();
    if letters > MESSAGE_MOST {
        return Err(format!(
            "Keep it to {} letters or fewer. This is {}.",
            crate::words::thousands(MESSAGE_MOST),
            crate::words::thousands(letters)
        ));
    }
    Ok(message.to_string())
}

fn checked_contact(contact: Option<&str>) -> Result<Option<String>, String> {
    let Some(contact) = contact.map(str::trim).filter(|c| !c.is_empty()) else {
        return Ok(None);
    };
    if contact.chars().count() > SHORT_MOST {
        return Err(format!("Keep the way to reach you to {SHORT_MOST} letters or fewer."));
    }
    Ok(Some(contact.to_string()))
}

/// A short field, trimmed, and cut to [`SHORT_MOST`] letters.
fn short(text: Option<&str>) -> Option<String> {
    let text = text?.trim();
    (!text.is_empty()).then(|| text.chars().take(SHORT_MOST).collect())
}

/// The end of `text`, at most `most` bytes, starting at a whole line where
/// it can.
pub fn keep_the_end(text: &str, most: usize) -> &str {
    if text.len() <= most {
        return text;
    }
    let mut start = text.len() - most;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    let rest = &text[start..];
    // Begin after the line that was cut part-way, unless that is all there is.
    match rest.find('\n') {
        Some(at) if at + 1 < rest.len() => &rest[at + 1..],
        _ => rest,
    }
}

/// The log, made safe to send: the home folder and the user name become
/// `~`, and anything that looks like a key, a secret or a token is hidden.
/// Scrubbing what was already scrubbed changes nothing.
pub fn scrub(log: &str, home: Option<&str>, names: &[&str]) -> String {
    let mut text = log.to_string();
    if let Some(home) = home.map(|h| h.trim_end_matches(['/', '\\'])).filter(|h| h.len() > 3) {
        // Written either way round: C:\Users\You and C:/Users/You.
        for form in [home.to_string(), home.replace('\\', "/"), home.replace('/', "\\")] {
            text = replace_word(&text, &form, "~");
        }
    }
    for name in names.iter().map(|n| n.trim()).filter(|n| n.chars().count() >= 2) {
        text = replace_word(&text, name, "~");
    }
    hide_secrets(&text)
}

/// Every `what`, ignoring case, that is not part of a longer word, as `with`.
fn replace_word(text: &str, what: &str, with: &str) -> String {
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let mut pattern = String::from("(?i)");
    if word(what.chars().next()) {
        pattern.push_str(r"\b");
    }
    pattern.push_str(&regex::escape(what));
    if word(what.chars().last()) {
        pattern.push_str(r"\b");
    }
    match Regex::new(&pattern) {
        Ok(found) => found.replace_all(text, regex::NoExpand(with)).into_owned(),
        // An escaped name always makes a pattern; if one somehow does not,
        // it is still replaced, only exactly as written.
        Err(_) => text.replace(what, with),
    }
}

fn re(pattern: &str) -> Regex {
    // Patterns are compile-time constants covered by tests.
    #[allow(clippy::expect_used)]
    Regex::new(pattern).expect("built-in pattern must compile")
}

fn hide_secrets(text: &str) -> String {
    // "Bearer abc...": what follows is a credential.
    static SCHEME: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b(bearer)\s+[A-Za-z0-9._~+/=-]+"));
    // A value given a name that says what it is: "secret=...", "token: ...".
    static LABELLED: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"(?i)(\b(?:secret[_ -]?access[_ -]?key|access[_ -]?key[_ -]?id|access[_ -]?key|secret[_ -]?key|api[_ -]?key|secret|token|password|passwd|authorization)\b\s*[:=]\s*)\S+",
        )
    });
    // The shape of an Amazon access key id.
    static KEY_ID: LazyLock<Regex> = LazyLock::new(|| re(r"\b(?:(?:AKIA|ASIA)[A-Z0-9]{16}|GOOG[A-Z0-9]{16,})\b"));
    let text = SCHEME.replace_all(text, format!("$1 {HIDDEN}").as_str());
    let text = LABELLED.replace_all(&text, format!("${{1}}{HIDDEN}").as_str());
    let text = KEY_ID.replace_all(&text, HIDDEN);
    hide_long_runs(&text)
}

/// A run of letters and digits long and mixed enough to be a key or a
/// token, rather than a word or a name.
fn hide_long_runs(text: &str) -> String {
    let token = |c: char| c.is_ascii_alphanumeric() || matches!(c, '+' | '=' | '_' | '-' | '/');
    let mut out = String::with_capacity(text.len());
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut String| {
        out.push_str(&hidden_run(run));
        run.clear();
    };
    for c in text.chars() {
        if token(c) {
            run.push(c);
        } else {
            flush(&mut run, &mut out);
            out.push(c);
        }
    }
    flush(&mut run, &mut out);
    out
}

fn hidden_run(run: &str) -> String {
    let mixed = |s: &str| s.chars().any(|c| c.is_ascii_digit()) && s.chars().any(|c| c.is_ascii_alphabetic());
    let looks_secret = |s: &str| s.len() >= 32 && mixed(s);
    // A secret access key is 40 letters, digits, + and /. A path is not that
    // shape: its pieces are short words between the slashes.
    let both_cases = run.chars().any(|c| c.is_ascii_uppercase()) && run.chars().any(|c| c.is_ascii_lowercase());
    if run.contains('/') {
        if run.len() == 40 && mixed(run) && both_cases && !run.contains(['_', '-']) {
            return HIDDEN.to_string();
        }
        return run
            .split('/')
            .map(|piece| if looks_secret(piece) { HIDDEN } else { piece })
            .collect::<Vec<_>>()
            .join("/");
    }
    if looks_secret(run) {
        HIDDEN.to_string()
    } else {
        run.to_string()
    }
}

/// The first line of what the person wrote, short enough for a list.
pub fn first_line(message: &str) -> String {
    const MOST: usize = 80;
    let line = message.trim().lines().next().unwrap_or_default().trim();
    if line.chars().count() <= MOST {
        return line.to_string();
    }
    let cut: String = line.chars().take(MOST - 1).collect();
    format!("{}…", cut.trim_end())
}

/// How a report is getting on, in words: "Received", "Accepted", "Being
/// worked on", "Done in version 0.47.0", "Not planned". A word Keevoro
/// does not know yet counts as received.
pub fn status_said(status: Option<&str>, shipped_in: Option<&str>) -> String {
    match status.unwrap_or("new") {
        "accepted" => "Accepted".into(),
        "in_progress" => "Being worked on".into(),
        "shipped" => match shipped_in.map(str::trim).filter(|v| !v.is_empty()) {
            Some(version) => format!("Done in version {}", version.trim_start_matches('v')),
            None => "Done".into(),
        },
        "declined" => "Not planned".into(),
        _ => "Received".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(log: &str) -> Facts {
        Facts {
            app_version: "9.8.0".into(),
            os: Some("Windows 11 (26100)".into()),
            log: log.into(),
            home: Some(r"C:\Users\You".into()),
            names: vec!["You".into()],
        }
    }

    fn draft(kind: ReportKind, message: &str) -> Draft {
        Draft {
            kind: Some(kind),
            message: message.into(),
            screen: Some("Choose what to keep".into()),
            ..Draft::default()
        }
    }

    #[test]
    fn by_default_a_report_carries_only_the_words_the_version_the_system_and_the_screen() {
        let body = prepare(
            &draft(ReportKind::Problem, "  The map went blank.  "),
            &facts("a log line"),
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "kind": "problem",
                "message": "The map went blank.",
                "appVersion": "9.8.0",
                "os": "Windows 11 (26100)",
                "screen": "Choose what to keep",
            })
        );
        assert!(!body.contains("log"), "no log unless asked: {body}");
    }

    #[test]
    fn the_log_and_a_way_to_reach_them_go_only_when_switched_on() {
        let mut idea = draft(ReportKind::Idea, "A dark map, please");
        idea.with_log = true;
        idea.contact = Some(" you@example.com ".into());
        let body = prepare(&idea, &facts(r"Looking through C:\Users\You\Pictures\Family photos")).unwrap();
        let report: Report = serde_json::from_str(&body).unwrap();
        assert_eq!(report.kind, ReportKind::Idea);
        assert_eq!(
            report.log_tail.as_deref(),
            Some(r"Looking through ~\Pictures\Family photos")
        );
        assert_eq!(report.contact.as_deref(), Some("you@example.com"));
        // A switch on with nothing written sends nothing.
        idea.contact = Some("   ".into());
        idea.with_log = false;
        let body = prepare(&idea, &facts("x")).unwrap();
        assert!(!body.contains("contact") && !body.contains("logTail"), "{body}");
    }

    #[test]
    fn empty_words_and_too_many_are_refused_in_plain_words() {
        assert_eq!(
            prepare(&draft(ReportKind::Problem, "  \n "), &facts("")).unwrap_err(),
            "Say what went wrong first."
        );
        assert_eq!(
            prepare(&draft(ReportKind::Idea, ""), &facts("")).unwrap_err(),
            "Say what your idea is first."
        );
        let long = "a".repeat(MESSAGE_MOST + 1);
        assert_eq!(
            prepare(&draft(ReportKind::Idea, &long), &facts("")).unwrap_err(),
            "Keep it to 5,000 letters or fewer. This is 5,001."
        );
        assert!(prepare(&draft(ReportKind::Idea, &"é".repeat(MESSAGE_MOST)), &facts("")).is_ok());
        let mut far = draft(ReportKind::Idea, "Hello");
        far.contact = Some("x".repeat(SHORT_MOST + 1));
        assert_eq!(
            prepare(&far, &facts("")).unwrap_err(),
            "Keep the way to reach you to 200 letters or fewer."
        );
        let mut neither = draft(ReportKind::Idea, "Hello");
        neither.kind = None;
        assert!(prepare(&neither, &facts("")).is_err());
    }

    #[test]
    fn a_long_log_is_cut_from_the_front_so_the_newest_lines_go() {
        let log: String = (0..20_000)
            .map(|n| format!("2026-11-16 14:02:11  Line {n}\n"))
            .collect();
        let mut asked = draft(ReportKind::Problem, "It stopped");
        asked.with_log = true;
        let body = prepare(&asked, &facts(&log)).unwrap();
        let report: Report = serde_json::from_str(&body).unwrap();
        let tail = report.log_tail.unwrap();
        assert!(tail.len() <= LOG_MOST);
        assert!(tail.ends_with("Line 19999\n"), "the end survives");
        assert!(tail.starts_with("2026-11-16"), "from a whole line: {}", &tail[..40]);
        assert!(!tail.contains("Line 0\n"));
    }

    #[test]
    fn a_log_full_of_backslashes_is_cut_until_the_whole_report_fits() {
        let log: String = (0..30_000).map(|n| format!("D:{}{n}\n", "\\".repeat(60))).collect();
        let mut asked = draft(ReportKind::Problem, "It stopped");
        asked.with_log = true;
        let body = prepare(&asked, &facts(&log)).unwrap();
        assert!(body.len() <= BODY_MOST, "{}", body.len());
        let report: Report = serde_json::from_str(&body).unwrap();
        assert!(
            report.log_tail.unwrap().len() < LOG_MOST,
            "cut further than the log's own limit"
        );
        assert!(body.contains(r"\\29999\n"), "the newest line is kept");
    }

    #[test]
    fn keeping_the_end_starts_at_a_whole_line_and_a_whole_letter() {
        assert_eq!(keep_the_end("one\ntwo\nthree", 100), "one\ntwo\nthree");
        assert_eq!(keep_the_end("one\ntwo\nthree", 9), "three");
        assert_eq!(keep_the_end("one line only", 4), "only");
        assert_eq!(keep_the_end("ééé", 3), "é");
        assert_eq!(keep_the_end("x\n", 1), "\n");
    }

    #[test]
    fn the_home_folder_and_user_name_become_a_tilde_either_way_round_and_in_any_case() {
        let names = ["You"];
        let home = Some(r"C:\Users\You");
        assert_eq!(
            scrub(r"Looking through C:\Users\You\Pictures", home, &names),
            r"Looking through ~\Pictures"
        );
        assert_eq!(
            scrub("Copied c:/users/you/Desktop/a.jpg", home, &names),
            "Copied ~/Desktop/a.jpg"
        );
        assert_eq!(
            scrub(r"Signed in as YOU on E:\", home, &names),
            r"Signed in as ~ on E:\"
        );
        // Only the whole name, never part of a longer word.
        assert_eq!(scrub("Young yours", home, &names), "Young yours");
        assert_eq!(scrub(r"C:\Users\Younger\x", home, &names), r"C:\Users\Younger\x");
        assert_eq!(
            scrub("/home/sam/Pictures and /home/samuel", Some("/home/sam/"), &["sam"]),
            "~/Pictures and /home/samuel"
        );
        // Too short a home or name to replace safely is left alone.
        assert_eq!(scrub(r"C:\ and a", Some(r"C:\"), &["a"]), r"C:\ and a");
    }

    #[test]
    fn anything_that_looks_like_a_key_a_secret_or_a_token_is_hidden() {
        let none: [&str; 0] = [];
        let cases = [
            ("key AKIAIOSFODNN7EXAMPLE used", "key ••• used"),
            (
                "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY was the secret",
                "••• was the secret",
            ),
            ("secret=abc123", "secret=•••"),
            ("Token: abc", "Token: •••"),
            ("password = hunter2 then", "password = ••• then"),
            ("secret_access_key: xyz", "secret_access_key: •••"),
            (
                "Authorization: Bearer eyJhbGciOi.eyJzdWIi.sig",
                "Authorization: ••• •••",
            ),
            (
                "hash 9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
                "hash •••",
            ),
            ("r2 key 0123456789abcdef0123456789abcdef", "r2 key •••"),
            // Google's HMAC access keys, for a person and for a service account.
            ("key GOOGEXAMPLE1234567890AB used", "key ••• used"),
            (
                "key GOOG1EEXAMPLEEXAMPLEEXAMPLEEXAMPLEEXAMPLEEXAMPLE0123456789 used",
                "key ••• used",
            ),
        ];
        for (log, scrubbed) in cases {
            assert_eq!(scrub(log, None, &none), scrubbed, "{log}");
        }
        // Words, dates, paths and the sizes of things stay.
        for plain in [
            "2026-11-16 14:02:11  Reading a large file (3.9 GB): K:\\Video\\wedding.mov",
            "/Users/You/Pictures/Family photos/2004/Holiday in Spain/IMG_0042.JPG",
            "Looked through E:\\ in 89 seconds: 238 photos and videos",
            "the token file was missing",
            "Couldn't reach the bucket family-photos-2004",
        ] {
            assert_eq!(scrub(plain, None, &none), plain);
        }
    }

    #[test]
    fn scrubbing_what_was_scrubbed_changes_nothing() {
        let names = ["You", "hidden"];
        let home = Some(r"C:\Users\You");
        for log in [
            r"C:\Users\You\a token=abc AKIAIOSFODNN7EXAMPLE hidden Bearer x",
            "password: ••• and ~",
            "",
        ] {
            let once = scrub(log, home, &names);
            assert_eq!(scrub(&once, home, &names), once, "{log}");
        }
    }

    #[test]
    fn a_prepared_report_reads_back_and_one_changed_is_refused() {
        let mut asked = draft(ReportKind::Problem, "It stopped");
        asked.with_log = true;
        let facts = facts(r"C:\Users\You\a secret=abc");
        let body = prepare(&asked, &facts).unwrap();
        let read = check_prepared(&body, facts.home.as_deref(), &["You"]).unwrap();
        assert_eq!(read.log_tail.as_deref(), Some("~\\a secret=•••"));
        let unscrubbed = body.replace("~\\\\a", r"C:\\Users\\You\\a");
        assert!(unscrubbed.contains("Users"), "{unscrubbed}");
        assert!(check_prepared(&unscrubbed, facts.home.as_deref(), &["You"]).is_err());
        assert!(check_prepared("{\"kind\":\"problem\"}", None, &[]).is_err());
        let extra = body.replacen('{', "{\"details\":\"more\",", 1);
        assert!(check_prepared(&extra, None, &[]).is_err(), "nothing unknown goes");
        let empty = body.replace("It stopped", " ");
        assert_eq!(
            check_prepared(&empty, None, &[]).unwrap_err(),
            "Say what went wrong first."
        );
    }

    #[test]
    fn the_first_line_is_short_enough_for_a_list() {
        assert_eq!(first_line("  The map\nwent blank"), "The map");
        let long = format!("{} end", "word ".repeat(30));
        let line = first_line(&long);
        assert_eq!(line.chars().count(), 80);
        assert!(line.ends_with('…'));
    }

    #[test]
    fn how_a_report_is_getting_on_is_said_in_plain_words() {
        assert_eq!(status_said(Some("new"), None), "Received");
        assert_eq!(status_said(None, None), "Received");
        assert_eq!(status_said(Some("accepted"), None), "Accepted");
        assert_eq!(status_said(Some("in_progress"), None), "Being worked on");
        assert_eq!(status_said(Some("shipped"), Some("v0.47.0")), "Done in version 0.47.0");
        assert_eq!(status_said(Some("shipped"), Some(" ")), "Done");
        assert_eq!(status_said(Some("declined"), None), "Not planned");
        assert_eq!(status_said(Some("merged_elsewhere"), None), "Received");
    }
}
