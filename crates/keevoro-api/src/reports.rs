//! Reporting a problem, or suggesting an idea, to Keevoro's maker, and
//! hearing how each one is getting on.
//!
//! A report is made first (`reports.prepare`) and shown to the person
//! exactly as it will go, then sent (`reports.send`) as that same text, so
//! what they saw is what went. What goes in one, and how the log is
//! scrubbed first, is `keevoro_core::report`.
//!
//! It goes to a private list only Keevoro's maker reads, at the address
//! Keevoro was built with, or the one in `KEEVORO_REPORTS_URL` when that is
//! set (for tests, which never reach the real one). Nothing is ever sent
//! unless the person sends it: an AI agent cannot.
//!
//! A work computer may reach the internet only through a proxy, or look
//! inside secure connections. Keevoro goes through the proxy named in
//! `HTTPS_PROXY` (or `ALL_PROXY`, minding `NO_PROXY`) and, on Windows, the
//! one set in its Internet settings, chosen in `keevoro_core::proxy` as for
//! cloud storage. When it still cannot reach the service, the app's window,
//! which goes through the system's own network settings, sends the very same text and hands back the answer
//! (`reports.note`), and asks after reports the same way (`reports.asking`
//! and `reports.heard`). Why it could not be reached is logged and said in
//! plain words; a report and its token never are.
//!
//! A picture may go with a report: one the person chose, pasted or
//! dropped and saw first, never one Keevoro takes. It goes only once the
//! report has been accepted, as a second request with the report's own
//! token (`POST /v1/reports/<reference>/picture`), its kind told by its
//! first bytes: PNG, JPEG or WebP, at most 3 MB. It is held in memory only,
//! never written to a file or the log; `reports.json` keeps only whether
//! one went. If the report goes and its picture does not, the report stays
//! sent and listed, and the person is told why the picture did not go.
//!
//! A person may also have a reporting code from Keevoro's maker. It is
//! kept like a key, in this computer's keychain (`reports.code`), never in
//! a file, a log or anything an operation answers, and goes with each
//! report as its last field once the person has seen the rest. The preview
//! says only that it will go (`withCode`). The service may say whether it
//! recognised it; an older one says nothing, and so does Keevoro.
//!
//! The reports sent from this computer are kept in a file of their own,
//! `reports.json` in Keevoro's config folder, never in `settings.json`
//! (older versions write that back without parts they do not know). Each
//! has its reference, the token that lets Keevoro ask after it, its first
//! line and what was last heard of it and, from 0.59, the person's words in
//! full, the version and screen it was sent from, and only whether the log
//! and a way to reach them went with it, never either itself, so the person
//! can read again what they sent. The file is read a report at a time,
//! so one that cannot be understood costs that one alone, and is changed
//! under a lock, so the app and the `keevoro` command never undo each
//! other's changes.

use crate::error::{Error, Result};
use crate::keevoro::Keevoro;
use keevoro_core::report::{self, Draft, Facts, ReportKind};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

/// Whether a reporting code as typed is one, for a program to check before
/// it asks anything.
pub use keevoro_core::report::checked_code;

/// Whether bytes are a picture that can go with a report, and the line
/// that says one goes, for a program to check and say before it sends.
pub use keevoro_core::report::{checked_picture, picture_line};

/// A picture's bytes in standard Base64, as `reports.send` takes them.
pub fn picture_to_base64(bytes: &[u8]) -> String {
    crate::picture::base64(bytes)
}

/// The file, in Keevoro's config folder.
pub const FILE: &str = "reports.json";
/// Held while the file is changed.
const LOCK: &str = "reports.lock";
/// Where a file that could not all be understood is kept, as it was.
pub const KEPT_ASIDE: &str = "reports that could not be read.json";
/// Where reports go when Keevoro was built without an address of its own.
const USUAL_ADDRESS: &str = "https://reports.keevoro.com";
/// Set, the address reports go to instead: for tests.
pub const ADDRESS_VARIABLE: &str = "KEEVORO_REPORTS_URL";
/// How long to wait for the reports service before giving up, so the
/// window can try soon after.
const WAIT: Duration = Duration::from_secs(15);
/// How long to wait for a connection to it.
const CONNECT_WAIT: Duration = Duration::from_secs(6);
/// How many lines of the log are read before the report cuts it to size.
const LOG_LINES: usize = 3_000;
/// How many of the newest reports are asked after each time.
const ASKED_AFTER: usize = 50;
/// The most of an answer that is read.
const ANSWER_MOST: u64 = 64 * 1024;
/// The name the reporting code is kept under in this computer's keychain.
pub(crate) const CODE_ENTRY: &str = "reporting code";
const CODE_RECOGNISED: &str = "Your reporting code was recognised.";
const CODE_NOT_RECOGNISED: &str = "That reporting code was not recognised, so this was sent as an ordinary report. \
     Check the code with whoever gave it to you.";
/// Said after the thank you, as a person may wait for an email that never
/// comes: Keevoro sends none.
pub const NO_EMAIL: &str = "Keevoro does not send emails. This report is listed under Your reports below, \
     where you can see how it is getting on.";
/// That, when the person gave a way to reach them.
pub const NO_EMAIL_MAY_WRITE: &str = "Keevoro does not send an email to confirm. This report is listed under Your \
     reports below, and Keevoro's maker may write to you about it.";
/// Said under a report sent before its whole message was kept.
pub const ONLY_FIRST_LINE: &str = "Only the first line was kept for reports sent before this version.";

/// `problem` or `idea`, as a request names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum KindWord {
    /// Something went wrong.
    Problem,
    /// An idea for Keevoro.
    Idea,
}

impl From<KindWord> for ReportKind {
    fn from(word: KindWord) -> Self {
        match word {
            KindWord::Problem => ReportKind::Problem,
            KindWord::Idea => ReportKind::Idea,
        }
    }
}

/// `reports.prepare`
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrepareReportRequest {
    /// `problem` for something that went wrong, `idea` for an idea.
    pub kind: KindWord,
    /// The person's own words, at most 5,000 letters.
    pub message: String,
    /// True to include the end of Keevoro's log, scrubbed. It can name
    /// folders and files.
    #[serde(default)]
    pub with_log: bool,
    /// A way to reach the person, in their words, if they want one sent.
    #[serde(default)]
    pub contact: Option<String>,
    /// Which screen they were on, such as `Choose what to keep`.
    #[serde(default)]
    pub screen: Option<String>,
}

/// `reports.send`
#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SendReportRequest {
    /// The report exactly as `reports.prepare` gave it, as `body`.
    pub prepared: String,
    /// A reporting code to send with this report alone, not kept. Left
    /// out, the one kept on this computer goes, if there is one.
    #[serde(default)]
    pub code: Option<String>,
    /// A picture the person added, to send once the report is accepted,
    /// in standard Base64: PNG, JPEG or WebP, at most 3 MB. Never kept.
    #[serde(default)]
    pub picture: Option<String>,
}

// Written out by hand so a reporting code, or a picture, is never printed.
impl std::fmt::Debug for SendReportRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SendReportRequest")
            .field("prepared", &self.prepared)
            .field("code", &self.code.as_ref().map(|_| report::HIDDEN))
            .field(
                "picture",
                &self
                    .picture
                    .as_ref()
                    .map(|p| report::picture_line(p.len() as u64 / 4 * 3)),
            )
            .finish()
    }
}

/// The bytes of a picture sent to an operation in Base64, or why not.
pub fn picture_from_base64(text: &str) -> Result<Vec<u8>> {
    crate::picture::from_base64(text.trim())
        .ok_or_else(|| Error::refused("That picture couldn't be read. Add it again."))
}

/// `reports.code`: whether a reporting code is kept, and keeping or
/// forgetting one. Nothing here ever gives it back.
#[derive(Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReportingCodeRequest {
    /// The code to keep in this computer's keychain, in place of any kept
    /// before: letters, digits and hyphens, at most 64.
    #[serde(default)]
    pub code: Option<String>,
    /// True to forget the code kept on this computer.
    #[serde(default)]
    pub forget: Option<bool>,
}

// Written out by hand so a reporting code is never printed.
impl std::fmt::Debug for ReportingCodeRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReportingCodeRequest")
            .field("code", &self.code.as_ref().map(|_| report::HIDDEN))
            .field("forget", &self.forget)
            .finish()
    }
}

/// What `reports.code` answers: only whether one is kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeView {
    pub has_code: bool,
}

/// What `reports.prepare` made: the exact text that would be sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prepared {
    pub body: String,
    /// Where it goes, for the window to send it itself when Keevoro
    /// cannot reach it.
    pub send_to: String,
    /// True when a reporting code kept on this computer will go with it,
    /// after `body`. The code itself is never shown.
    pub with_code: bool,
}

/// `reports.note`: a report the app's window sent itself, with what the
/// service answered.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NoteReportRequest {
    /// The report exactly as `reports.prepare` gave it, as it was sent.
    pub prepared: String,
    /// The reference the service gave it, such as `K-1042`.
    pub reference: String,
    /// The token the service gave with it.
    pub token: String,
    /// Whether the service recognised the reporting code sent with it, if
    /// one was and it said.
    #[serde(default)]
    pub recognised: Option<bool>,
    /// True when the window sent a picture after it, and the service took it.
    #[serde(default)]
    pub with_picture: bool,
    /// Why a picture the window tried to send after it did not go, in the
    /// service's words or a sentence about the connection.
    #[serde(default)]
    pub picture_why: Option<String>,
}

/// `reports.heard`: what the app's window heard of the reports
/// `reports.asking` gave it.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HeardRequest {
    /// Each answer, as the service gave it.
    pub heard: Vec<HeardOne>,
    /// True when every report asked about was answered.
    #[serde(default)]
    pub all: bool,
}

/// One answer the window heard.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HeardOne {
    pub reference: String,
    pub answer: serde_json::Value,
}

/// `reports.asking`: for the app's window alone, what it needs to ask
/// after the newest reports itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Asking {
    pub send_to: String,
    pub reports: Vec<AskAfter>,
}

/// One report to ask after: its reference and its token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskAfter {
    pub reference: String,
    pub token: String,
}

/// Why a report did not go, for the app: the sentence, and when the
/// service could not be reached at all, why not in plain words, if that
/// can be told.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotSent {
    #[serde(flatten)]
    pub error: Error,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
    /// When the service could not be reached and a reporting code was to
    /// go: the report with it, for the window to send instead. This is the
    /// one time the code leaves Keevoro other than to the service, and only
    /// to the app's own window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub send_as: Option<SendAs>,
}

/// A report with its reporting code in it, as it is sent. Never printed.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct SendAs(pub String);

impl std::fmt::Debug for SendAs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SendAs(with a reporting code, not shown)")
    }
}

/// What `reports.send` answers once a report has gone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sent {
    /// Such as `K-1042`.
    pub reference: String,
    pub said: String,
    /// Whether the service recognised the reporting code sent with it,
    /// when one was and it said.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recognised: Option<bool>,
    /// That, in a sentence, to say quietly after `said`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_said: Option<String>,
    /// That no email comes, and where to see how it is getting on, to say
    /// quietly after the rest.
    pub email_said: String,
    /// True when a picture went with it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub with_picture: bool,
    /// When the report went and its picture did not: that, and why.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture_said: Option<String>,
}

/// One report sent from this computer, as the list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportView {
    pub reference: String,
    pub kind: ReportKind,
    pub first_line: String,
    /// The person's words in full, as sent, for reports sent from 0.59.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// When only the first line was kept, a sentence saying so.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kept_said: Option<String>,
    /// The Keevoro it was sent from, and the screen the person was on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screen: Option<String>,
    /// True when it went with the end of the log, and with a way to reach
    /// the person. Neither is ever kept.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub with_log: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub with_contact: bool,
    /// True when a picture went with it. The picture is never kept.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub with_picture: bool,
    /// When it was sent, in seconds since 1970.
    pub sent_at: i64,
    /// The service's word for how it is getting on, as last heard.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// That, in plain words: "Received", "Being worked on", "Done in
    /// version 0.47.0".
    pub status_said: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shipped_in: Option<String>,
    /// True when it went with a reporting code the service recognised.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub recognised: bool,
}

/// `reports.list`, `reports.refresh`
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportsView {
    /// Newest first.
    pub reports: Vec<ReportView>,
    /// True when each was asked after just now and answered.
    pub checked: bool,
    /// Why not, when it was asked after and could not be.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub said: Option<String>,
}

/// One report, as it is kept on this computer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Kept {
    reference: String,
    /// What lets Keevoro ask after this report, and nobody else.
    token: String,
    kind: ReportKind,
    first_line: String,
    /// The person's words in full, as sent (from 0.59). Older reports kept
    /// only the first line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    /// The Keevoro it was sent from, and the screen (from 0.59).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    app_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    screen: Option<String>,
    /// Only whether the log and a way to reach the person went (from
    /// 0.59), never either of them.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    with_log: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    with_contact: bool,
    /// Only whether a picture went (from 0.59), never the picture.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    with_picture: bool,
    sent_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shipped_in: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    updated_at: Option<String>,
    /// Whether the reporting code sent with it was recognised, when one
    /// was and the service said (from 0.57). Not a secret.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    recognised: Option<bool>,
}

impl Kept {
    fn view(&self) -> ReportView {
        ReportView {
            reference: self.reference.clone(),
            kind: self.kind,
            first_line: self.first_line.clone(),
            message: self.message.clone(),
            kept_said: self.message.is_none().then(|| ONLY_FIRST_LINE.to_string()),
            app_version: self.app_version.clone(),
            screen: self.screen.clone(),
            with_log: self.with_log,
            with_contact: self.with_contact,
            with_picture: self.with_picture,
            sent_at: self.sent_at,
            status: self.status.clone(),
            status_said: report::status_said(self.status.as_deref(), self.shipped_in.as_deref()),
            note: self.note.clone(),
            shipped_in: self.shipped_in.clone(),
            recognised: self.recognised == Some(true),
        }
    }
}

/// What the reports file holds.
#[derive(Debug, Clone, Default, PartialEq)]
struct ReportsFile {
    reports: Vec<Kept>,
    /// Reports this version could not read, and parts a later version
    /// wrote, kept as they were and written back.
    unread: Vec<serde_json::Value>,
    later: serde_json::Map<String, serde_json::Value>,
}

impl ReportsFile {
    fn read(config: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(config.join(FILE)) else {
            return Self::default();
        };
        let Ok(serde_json::Value::Object(all)) = serde_json::from_str::<serde_json::Value>(&text) else {
            keep_aside(config, &text);
            return Self::default();
        };
        let mut kept = Self::default();
        let mut whole = true;
        match all.get("reports") {
            Some(serde_json::Value::Array(each)) => {
                for one in each {
                    match serde_json::from_value::<Kept>(one.clone()) {
                        Ok(report) => kept.reports.push(report),
                        Err(_) => {
                            whole = false;
                            kept.unread.push(one.clone());
                        }
                    }
                }
            }
            None => {}
            Some(_) => whole = false,
        }
        if !whole {
            keep_aside(config, &text);
        }
        kept.later = all
            .into_iter()
            .filter(|(key, _)| key != "v" && key != "reports")
            .collect();
        kept
    }

    fn save(&self, config: &Path) -> std::io::Result<()> {
        let mut all = self.later.clone();
        all.insert("v".into(), 1.into());
        let mut each: Vec<serde_json::Value> = self
            .reports
            .iter()
            .filter_map(|r| serde_json::to_value(r).ok())
            .collect();
        each.extend(self.unread.iter().cloned());
        all.insert("reports".into(), serde_json::Value::Array(each));
        let text = serde_json::to_string_pretty(&all).map_err(std::io::Error::other)?;
        let temp = config.join(format!(".{FILE}.{}.part", std::process::id()));
        std::fs::write(&temp, text)?;
        std::fs::rename(&temp, config.join(FILE)).inspect_err(|_| {
            let _ = std::fs::remove_file(&temp);
        })
    }

    /// Makes a change and saves it, holding the lock while the file is
    /// read, changed and written.
    fn change(config: &Path, change: impl FnOnce(&mut Self)) -> std::io::Result<()> {
        std::fs::create_dir_all(config)?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(config.join(LOCK))?;
        fs4::FileExt::lock(&lock)?;
        let mut kept = Self::read(config);
        change(&mut kept);
        let saved = kept.save(config);
        let _ = fs4::FileExt::unlock(&lock);
        saved
    }
}

fn keep_aside(config: &Path, text: &str) {
    let aside = config.join(KEPT_ASIDE);
    if !aside.exists() {
        let _ = std::fs::write(aside, text);
    }
}

/// What the reports service answers, as far as Keevoro reads it.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Answer {
    #[serde(default)]
    ok: Option<bool>,
    #[serde(default)]
    reference: Option<String>,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    why: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    shipped_in: Option<String>,
    #[serde(default)]
    updated_at: Option<serde_json::Value>,
    #[serde(default)]
    recognised: Option<bool>,
}

/// Whether a picture went with a report, and if one was to and did not,
/// why not.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Pictured {
    None,
    Went,
    NotSent(String),
}

/// Why a picture did not go, when the service could not be reached for it
/// and why cannot be told.
const PICTURE_UNREACHED: &str = "This computer couldn't reach Keevoro's reports service to send it.";

const UNREACHABLE: &str =
    "Couldn't reach Keevoro's reports service just now. Check this computer is online, then try again.";

/// Why the reports service could not be reached at all, as far as can be
/// told. Never because it answered: a refusal is an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unreached {
    /// Its name could not be looked up.
    Name,
    /// No connection could be made, or it was cut.
    Connection,
    /// A secure connection could not be made, as when a network looks
    /// inside them.
    Secure,
    /// It took too long.
    Slow,
    /// The proxy would not pass it on.
    Proxy,
    Other,
}

impl Unreached {
    pub(crate) fn of(error: &ureq::Error) -> Self {
        use std::io::ErrorKind;
        match error {
            ureq::Error::HostNotFound => Self::Name,
            ureq::Error::ConnectionFailed => Self::Connection,
            ureq::Error::Timeout(_) => Self::Slow,
            ureq::Error::Tls(_) | ureq::Error::Rustls(_) | ureq::Error::Pem(_) | ureq::Error::TlsRequired => {
                Self::Secure
            }
            ureq::Error::ConnectProxyFailed(_) | ureq::Error::InvalidProxyUrl => Self::Proxy,
            ureq::Error::Io(e) => match e.kind() {
                ErrorKind::TimedOut => Self::Slow,
                ErrorKind::ConnectionRefused
                | ErrorKind::ConnectionReset
                | ErrorKind::ConnectionAborted
                | ErrorKind::NotConnected
                | ErrorKind::AddrNotAvailable
                | ErrorKind::NetworkUnreachable
                | ErrorKind::HostUnreachable
                | ErrorKind::NetworkDown
                | ErrorKind::BrokenPipe
                | ErrorKind::UnexpectedEof => Self::Connection,
                // What a secure connection that could not be agreed gives.
                ErrorKind::InvalidData => Self::Secure,
                _ => Self::Other,
            },
            _ => Self::Other,
        }
    }

    /// What it was, for the log.
    fn word(self) -> &'static str {
        match self {
            Self::Name => "the name could not be looked up",
            Self::Connection => "the connection failed",
            Self::Secure => "a secure connection could not be made",
            Self::Slow => "it took too long",
            Self::Proxy => "the proxy refused",
            Self::Other => "something else went wrong",
        }
    }

    /// Why, in plain words, when that can be told.
    pub(crate) fn said(self) -> Option<&'static str> {
        match self {
            Self::Name => Some("This computer couldn't look up the reports service's address, which usually means it is offline or its network blocks it."),
            Self::Connection => Some("This computer's network would not let Keevoro connect to the reports service."),
            Self::Secure => Some("This computer's network would not let Keevoro connect securely, as some work and school networks do."),
            Self::Slow => Some("The reports service took too long to answer, perhaps because the network is slow or held the connection up."),
            Self::Proxy => Some("This computer's proxy would not pass Keevoro's connection on."),
            Self::Other => None,
        }
    }

    /// The line for the log: what it was, and the error in full, with
    /// `secret` (a token in an address) taken out.
    fn logged(error: &ureq::Error, secret: Option<&str>) -> String {
        let mut text = format!("{}: {error}", Self::of(error).word());
        let mut source = std::error::Error::source(error);
        while let Some(cause) = source {
            text.push_str(&format!(": {cause}"));
            source = cause.source();
        }
        match secret.filter(|s| !s.is_empty()) {
            Some(secret) => text.replace(secret, "•••"),
            None => text,
        }
    }
}

/// The proxy to reach the service at `base` through, chosen as for
/// everything Keevoro sends: see `keevoro_core::proxy`.
fn proxy(base: &str) -> Option<ureq::Proxy> {
    ureq::Proxy::new(keevoro_core::proxy::for_address(base)?.url()).ok()
}

/// Why sending did not work: the service was not reached, or it answered.
enum Failure {
    Unreached(ureq::Error),
    Answered(Error),
}

/// A reference or a token fit to put in an address: letters, digits and
/// dashes, and not too long.
fn plain_word(text: &str, most: usize) -> bool {
    !text.is_empty() && text.len() <= most && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// A sentence from the service, fit to show: one line, not too long.
fn shown(text: &str) -> String {
    let line: String = text.chars().filter(|c| !c.is_control()).take(300).collect();
    line.trim().to_string()
}

/// The reports service over HTTPS, at one address.
struct Service {
    base: String,
    agent: ureq::Agent,
}

impl Service {
    fn at(base: &str) -> Self {
        // Trust what the operating system trusts, as the updater does, so a
        // school's or an office's own certificate is not refused.
        let tls = ureq::tls::TlsConfig::builder()
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build();
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(WAIT))
            .timeout_connect(Some(CONNECT_WAIT))
            .proxy(proxy(base))
            .http_status_as_error(false)
            .tls_config(tls)
            .build()
            .into();
        Self {
            base: base.trim().trim_end_matches('/').to_string(),
            agent,
        }
    }

    /// The status and the answer, or why the service could not be reached
    /// at all.
    fn read(
        response: std::result::Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    ) -> std::result::Result<(u16, Answer), ureq::Error> {
        let mut response = response?;
        let status = response.status().as_u16();
        let text = response
            .body_mut()
            .with_config()
            .limit(ANSWER_MOST)
            .read_to_string()
            .unwrap_or_default();
        Ok((status, serde_json::from_str(&text).unwrap_or_default()))
    }

    /// Sends `body`: the reference, the token and, if it said, whether the
    /// service recognised a reporting code in it.
    fn send(&self, body: &str) -> std::result::Result<(String, String, Option<bool>), Failure> {
        let response = self
            .agent
            .post(format!("{}/v1/reports", self.base))
            .header("Content-Type", "application/json")
            .header("User-Agent", concat!("Keevoro/", env!("CARGO_PKG_VERSION")))
            .send(body.as_bytes());
        let (status, answer) = Self::read(response).map_err(Failure::Unreached)?;
        if (200..300).contains(&status) && answer.ok == Some(true) {
            let reference = answer.reference.unwrap_or_default();
            let token = answer.token.unwrap_or_default();
            if plain_word(&reference, 40) && plain_word(&token, 128) {
                return Ok((reference, token, answer.recognised));
            }
        }
        Err(Failure::Answered(
            match answer.why.as_deref().map(shown).filter(|w| !w.is_empty()) {
                Some(why) if status >= 500 => Error::failed(why),
                Some(why) => Error::refused(why),
                None => Error::failed(format!(
                    "Keevoro's reports service didn't answer as expected (it said {status}). Try again later."
                )),
            },
        ))
    }

    /// Sends a picture after the report `reference`, which the service has
    /// taken: nothing when it took it, or why not in its words.
    fn send_picture(
        &self,
        reference: &str,
        token: &str,
        picture: &[u8],
        kind: &str,
    ) -> std::result::Result<(), Failure> {
        let response = self
            .agent
            .post(format!("{}/v1/reports/{reference}/picture", self.base))
            .query("token", token)
            .header("Content-Type", kind)
            .header("User-Agent", concat!("Keevoro/", env!("CARGO_PKG_VERSION")))
            .send(picture);
        let (status, answer) = Self::read(response).map_err(Failure::Unreached)?;
        if (200..300).contains(&status) && answer.ok == Some(true) {
            return Ok(());
        }
        let why = answer.why.as_deref().map(shown).unwrap_or_default();
        Err(Failure::Answered(Error::refused(why)))
    }

    /// How one report is getting on, or why the service could not be
    /// reached. An answer that is not about the report changes nothing.
    fn ask_after(&self, kept: &Kept) -> std::result::Result<Option<Answer>, ureq::Error> {
        let response = self
            .agent
            .get(format!("{}/v1/reports/{}", self.base, kept.reference))
            .query("token", &kept.token)
            .header("User-Agent", concat!("Keevoro/", env!("CARGO_PKG_VERSION")))
            .call();
        let (status, answer) = Self::read(response)?;
        Ok(about(&kept.reference, status, answer))
    }
}

/// An answer about the report `reference`, or none when it is not one.
fn about(reference: &str, status: u16, answer: Answer) -> Option<Answer> {
    let about_it = (200..300).contains(&status)
        && answer.ok == Some(true)
        && answer.reference.as_deref().is_none_or(|r| r == reference);
    about_it.then_some(answer)
}

/// A reference as the service gives them: `K-` and digits.
fn a_reference(text: &str) -> bool {
    text.strip_prefix("K-")
        .is_some_and(|n| !n.is_empty() && n.len() <= 12 && n.bytes().all(|b| b.is_ascii_digit()))
}

/// A token as the service gives them: 40 hexadecimal letters and digits.
fn a_token(text: &str) -> bool {
    text.len() == 40 && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

impl Keevoro {
    /// Where reports go: the address set for tests, the one Keevoro was
    /// built with, or the usual one.
    fn reports_address(&self) -> String {
        let address = self
            .environment_variable(ADDRESS_VARIABLE)
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty())
            .or_else(|| option_env!("KEEVORO_REPORTS_URL").map(str::to_string))
            .unwrap_or_else(|| USUAL_ADDRESS.to_string());
        address.trim_end_matches('/').to_string()
    }

    /// The person's home folder and the forms of their user name, which
    /// are scrubbed from the log.
    fn who(&self) -> (Option<String>, Vec<String>) {
        let home = dirs::home_dir();
        let mut names: Vec<String> = ["USER", "USERNAME", "LOGNAME"]
            .iter()
            .filter_map(|v| self.environment_variable(v).or_else(|| std::env::var(v).ok()))
            .collect();
        if let Some(name) = home.as_ref().and_then(|h| h.file_name()) {
            names.push(name.to_string_lossy().into_owned());
        }
        names.sort();
        names.dedup();
        (home.map(|h| h.display().to_string()), names)
    }

    /// `reports.prepare`: the report, exactly as it would be sent.
    pub fn prepare_report(&self, request: &PrepareReportRequest) -> Result<Prepared> {
        let (home, names) = self.who();
        let draft = Draft {
            kind: Some(request.kind.into()),
            message: request.message.clone(),
            with_log: request.with_log,
            contact: request.contact.clone(),
            screen: request.screen.clone(),
        };
        let facts = Facts {
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            os: sysinfo::System::long_os_version().or_else(|| Some(std::env::consts::OS.to_string())),
            log: if request.with_log {
                self.log_tail(LOG_LINES)
            } else {
                String::new()
            },
            home,
            names,
        };
        let body = report::prepare(&draft, &facts).map_err(Error::refused)?;
        Ok(Prepared {
            body,
            send_to: self.reports_address().trim_end_matches('/').to_string(),
            with_code: self.saved_code().is_some(),
        })
    }

    /// The reporting code kept on this computer, if there is one that is
    /// still a code. A keychain that cannot be read counts as none.
    fn saved_code(&self) -> Option<String> {
        self.saved_code_or_why().ok().flatten()
    }

    /// The reporting code kept, or that the keychain could not be read.
    fn saved_code_or_why(&self) -> std::result::Result<Option<String>, ()> {
        self.secrets()
            .get(CODE_ENTRY)
            .map(|code| code.and_then(|c| report::checked_code(&c).ok()))
            .map_err(|_| ())
    }

    /// `reports.code`: whether a reporting code is kept on this computer,
    /// keeping one given in place of any before, or forgetting it. It is
    /// never given back.
    pub fn reporting_code(&self, request: &ReportingCodeRequest) -> Result<CodeView> {
        let forget = request.forget == Some(true);
        match (&request.code, forget) {
            (Some(_), true) => {
                return Err(Error::not_understood(
                    "it gives a code to keep and asks to forget one: ask for one or the other",
                ))
            }
            (Some(code), false) => {
                let code = report::checked_code(code).map_err(Error::refused)?;
                self.secrets().set(CODE_ENTRY, &code).map_err(|_| {
                    Error::failed(
                        "Couldn't keep the reporting code in this computer's keychain, so it isn't saved.".into(),
                    )
                })?;
                self.log().say("Kept a reporting code in this computer's keychain");
            }
            (None, true) => {
                self.secrets().delete(CODE_ENTRY).map_err(|_| {
                    Error::failed("Couldn't take the reporting code out of this computer's keychain. Try again.".into())
                })?;
                self.log().say("Forgot the reporting code kept on this computer");
            }
            (None, false) => {}
        }
        Ok(CodeView {
            has_code: self.saved_code().is_some(),
        })
    }

    /// `reports.send`: sends a report exactly as `reports.prepare` made it,
    /// with the reporting code `typed`, or else the one kept, if any, and
    /// keeps a note of it on this computer. Not reached, it says why after
    /// the sentence, if that can be told.
    pub fn send_report(&self, prepared: &str, typed: Option<&str>) -> Result<Sent> {
        self.send_report_with_picture(prepared, typed, None)
    }

    /// [`Keevoro::send_report`], with a picture to follow the report once
    /// it is accepted. A picture that is not one, or is too big, is refused
    /// before anything is sent.
    pub fn send_report_with_picture(
        &self,
        prepared: &str,
        typed: Option<&str>,
        picture: Option<&[u8]>,
    ) -> Result<Sent> {
        self.send_report_saying_why(prepared, typed, picture)
            .map_err(|not| match not.cause {
                Some(cause) => Error::unreachable(format!("{} {cause}", not.error.message)),
                None => not.error,
            })
    }

    /// [`Keevoro::send_report`], for the app: whether the service could
    /// not be reached at all, when the window can try, and why apart from
    /// the sentence. Then, with a reporting code, the report with it in, for
    /// the window to send, as it cannot read the code any other way.
    pub fn send_report_saying_why(
        &self,
        prepared: &str,
        typed: Option<&str>,
        picture: Option<&[u8]>,
    ) -> std::result::Result<Sent, NotSent> {
        let plain = |error: Error| NotSent {
            error,
            cause: None,
            send_as: None,
        };
        let picture = match picture {
            Some(bytes) => Some((
                bytes,
                report::checked_picture(bytes).map_err(|e| plain(Error::refused(e)))?,
            )),
            None => None,
        };
        let code = match typed {
            Some(typed) => Some(report::checked_code(typed).map_err(|e| plain(Error::refused(e)))?),
            None => self.saved_code_or_why().unwrap_or_else(|()| {
                // Said once a report goes, without the code, so the person
                // can be told why it was not recognised.
                self.log().say(
                    "Couldn't read the reporting code from this computer's keychain, so the report went without it",
                );
                None
            }),
        };
        let report = self.checked(prepared).map_err(plain)?;
        let body = match &code {
            Some(code) => report::with_code(prepared, code),
            None => prepared.to_string(),
        };
        let service = Service::at(&self.reports_address());
        let (reference, token, recognised) = service.send(&body).map_err(|failure| match failure {
            Failure::Unreached(e) => {
                self.log().say(&format!(
                    "Couldn't send a report: {}",
                    Unreached::logged(&e, code.as_deref())
                ));
                NotSent {
                    error: Error::unreachable(UNREACHABLE.to_string()),
                    cause: Unreached::of(&e).said().map(str::to_string),
                    send_as: code.is_some().then(|| SendAs(body.clone())),
                }
            }
            Failure::Answered(error) => {
                self.log().say(&format!("Couldn't send a report: {}", error.message));
                plain(error)
            }
        })?;
        self.log().say(&format!("Sent a report, {reference}"));
        // The report has gone, whatever happens to its picture.
        let pictured = match picture {
            Some((bytes, kind)) => match service.send_picture(&reference, &token, bytes, kind) {
                Ok(()) => {
                    self.log().say(&format!("Sent the picture with report {reference}"));
                    Pictured::Went
                }
                Err(failure) => {
                    let why = match failure {
                        Failure::Unreached(e) => {
                            self.log().say(&format!(
                                "Couldn't send the picture with report {reference}: {}",
                                Unreached::logged(&e, Some(&token))
                            ));
                            Unreached::of(&e).said().unwrap_or(PICTURE_UNREACHED).to_string()
                        }
                        Failure::Answered(error) => {
                            self.log().say(&format!(
                                "Couldn't send the picture with report {reference}: {}",
                                if error.message.is_empty() {
                                    "the service didn't take it"
                                } else {
                                    &error.message
                                }
                            ));
                            error.message
                        }
                    };
                    Pictured::NotSent(why)
                }
            },
            None => Pictured::None,
        };
        // Only a code that went can have been recognised.
        Ok(self.keep_sent(
            &report,
            reference,
            token,
            recognised.filter(|_| code.is_some()),
            pictured,
        ))
    }

    /// The report as it was made and shown, or why it cannot go.
    fn checked(&self, prepared: &str) -> Result<report::Report> {
        let (home, names) = self.who();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        report::check_prepared(prepared, home.as_deref(), &names).map_err(Error::refused)
    }

    /// `reports.note`: keeps a report the app's window sent itself, as one
    /// sent from here, once its answer and its text are as they should be.
    pub fn note_report(&self, request: &NoteReportRequest) -> Result<Sent> {
        if !a_reference(&request.reference) || !a_token(&request.token) {
            return Err(Error::refused(
                "That isn't an answer from Keevoro's reports service, so the report isn't noted.",
            ));
        }
        let report = self.checked(&request.prepared)?;
        self.log()
            .say(&format!("Sent a report through the window, {}", request.reference));
        let pictured = match (&request.picture_why, request.with_picture) {
            (Some(why), _) => {
                let why = shown(why);
                self.log().say(&format!(
                    "Couldn't send the picture with report {} through the window: {why}",
                    request.reference
                ));
                Pictured::NotSent(why)
            }
            (None, true) => Pictured::Went,
            (None, false) => Pictured::None,
        };
        Ok(self.keep_sent(
            &report,
            request.reference.clone(),
            request.token.clone(),
            request.recognised,
            pictured,
        ))
    }

    /// Notes a report that has gone, and says thank you, and whether its
    /// reporting code was recognised when the service said.
    fn keep_sent(
        &self,
        report: &report::Report,
        reference: String,
        token: String,
        recognised: Option<bool>,
        pictured: Pictured,
    ) -> Sent {
        let kept = Kept {
            reference: reference.clone(),
            token,
            kind: report.kind,
            first_line: report::first_line(&report.message),
            message: Some(report.message.clone()),
            app_version: Some(report.app_version.clone()),
            screen: report.screen.clone(),
            with_log: report.log_tail.is_some(),
            with_contact: report.contact.is_some(),
            with_picture: pictured == Pictured::Went,
            sent_at: now(),
            status: Some("new".into()),
            note: None,
            shipped_in: None,
            updated_at: None,
            recognised,
        };
        // It has gone, whatever happens here: failing to note it down only
        // means it cannot be asked after from this computer.
        if let Err(e) = ReportsFile::change(self.backups().config_folder(), |all| all.reports.push(kept)) {
            self.log().say(&format!(
                "Couldn't keep a note of report {reference} on this computer: {e}"
            ));
        }
        Sent {
            said: format!("Thank you. Your reference is {reference}."),
            reference,
            recognised,
            code_said: recognised.map(|yes| if yes { CODE_RECOGNISED } else { CODE_NOT_RECOGNISED }.to_string()),
            email_said: if report.contact.is_some() {
                NO_EMAIL_MAY_WRITE
            } else {
                NO_EMAIL
            }
            .to_string(),
            with_picture: pictured == Pictured::Went,
            picture_said: match pictured {
                Pictured::NotSent(why) => Some(report::picture_not_sent(&why)),
                _ => None,
            },
        }
    }

    /// `reports.list`: the reports sent from this computer, newest first,
    /// as last heard of. Nothing is asked of the service.
    pub fn reports(&self) -> ReportsView {
        Self::listed(&ReportsFile::read(self.backups().config_folder()), false, None)
    }

    fn listed(kept: &ReportsFile, checked: bool, said: Option<String>) -> ReportsView {
        let mut reports: Vec<ReportView> = kept.reports.iter().map(Kept::view).collect();
        reports.sort_by_key(|r| std::cmp::Reverse(r.sent_at));
        ReportsView { reports, checked, said }
    }

    /// `reports.refresh`: asks how each of the newest reports sent from this
    /// computer is getting on, notes what it hears, and answers the list.
    /// Offline, it stops at the first that cannot be reached and answers
    /// the list as last heard of.
    pub fn refresh_reports(&self) -> ReportsView {
        let config = self.backups().config_folder();
        let mut asking = ReportsFile::read(config).reports;
        if asking.is_empty() {
            return Self::listed(&ReportsFile::default(), true, None);
        }
        asking.sort_by_key(|r| std::cmp::Reverse(r.sent_at));
        asking.truncate(ASKED_AFTER);
        let service = Service::at(&self.reports_address());
        let mut heard = Vec::new();
        let mut reached = true;
        for kept in &asking {
            match service.ask_after(kept) {
                Ok(Some(answer)) => heard.push((kept.reference.clone(), answer)),
                Ok(None) => {}
                Err(e) => {
                    self.log().say(&format!(
                        "Couldn't ask after reports: {}",
                        Unreached::logged(&e, Some(&kept.token))
                    ));
                    reached = false;
                    break;
                }
            }
        }
        self.note_heard(heard, reached)
    }

    /// `reports.asking`: for the app's window alone, the address and the
    /// newest reports with their tokens, to ask after them itself when
    /// Keevoro cannot.
    pub fn reports_asking(&self) -> Asking {
        let mut kept = ReportsFile::read(self.backups().config_folder()).reports;
        kept.sort_by_key(|r| std::cmp::Reverse(r.sent_at));
        kept.truncate(ASKED_AFTER);
        Asking {
            send_to: self.reports_address(),
            reports: kept
                .into_iter()
                .map(|k| AskAfter {
                    reference: k.reference,
                    token: k.token,
                })
                .collect(),
        }
    }

    /// `reports.heard`: notes what the window heard of each report, as
    /// `reports.refresh` notes what it hears, and answers the list.
    pub fn reports_heard(&self, request: &HeardRequest) -> ReportsView {
        let heard = request
            .heard
            .iter()
            .filter_map(|one| {
                let answer = serde_json::from_value(one.answer.clone()).ok()?;
                Some((one.reference.clone(), about(&one.reference, 200, answer)?))
            })
            .collect();
        self.note_heard(heard, request.all)
    }

    /// Notes what was heard of each report, and answers the list; `reached`
    /// when every one asked about was answered.
    fn note_heard(&self, heard: Vec<(String, Answer)>, reached: bool) -> ReportsView {
        let config = self.backups().config_folder();
        let text = |v: Option<String>| v.map(|t| shown(&t)).filter(|t| !t.is_empty());
        let changed = ReportsFile::change(config, |all| {
            for (reference, answer) in heard {
                if let Some(kept) = all.reports.iter_mut().find(|k| k.reference == reference) {
                    kept.status = text(answer.status).or(kept.status.take());
                    kept.note = text(answer.note);
                    kept.shipped_in = text(answer.shipped_in);
                    kept.updated_at = answer.updated_at.map(|v| match v {
                        serde_json::Value::String(s) => s,
                        other => other.to_string(),
                    });
                }
            }
        });
        if let Err(e) = changed {
            self.log()
                .say(&format!("Couldn't note how reports are getting on: {e}"));
        }
        let said = (!reached).then(|| "Couldn't check just now.".to_string());
        Self::listed(&ReportsFile::read(config), reached, said)
    }
}

#[cfg(test)]
mod tests;
