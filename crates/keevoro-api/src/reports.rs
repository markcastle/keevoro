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
//! The reports sent from this computer are kept in a file of their own,
//! `reports.json` in Keevoro's config folder, never in `settings.json`
//! (older versions write that back without parts they do not know). Each
//! has its reference, the token that lets Keevoro ask after it, its first
//! line and what was last heard of it. The file is read a report at a time,
//! so one that cannot be understood costs that one alone, and is changed
//! under a lock, so the app and the `keevoro` command never undo each
//! other's changes.

use crate::error::{Error, Result};
use crate::keevoro::Keevoro;
use keevoro_core::report::{self, Draft, Facts, ReportKind};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

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
/// How long to wait for the reports service before giving up.
const WAIT: Duration = Duration::from_secs(15);
/// How many lines of the log are read before the report cuts it to size.
const LOG_LINES: usize = 3_000;
/// How many of the newest reports are asked after each time.
const ASKED_AFTER: usize = 50;
/// The most of an answer that is read.
const ANSWER_MOST: u64 = 64 * 1024;

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
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SendReportRequest {
    /// The report exactly as `reports.prepare` gave it, as `body`.
    pub prepared: String,
}

/// What `reports.prepare` made: the exact text that would be sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prepared {
    pub body: String,
}

/// What `reports.send` answers once a report has gone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sent {
    /// Such as `K-1042`.
    pub reference: String,
    pub said: String,
}

/// One report sent from this computer, as the list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportView {
    pub reference: String,
    pub kind: ReportKind,
    pub first_line: String,
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
    sent_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shipped_in: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    updated_at: Option<String>,
}

impl Kept {
    fn view(&self) -> ReportView {
        ReportView {
            reference: self.reference.clone(),
            kind: self.kind,
            first_line: self.first_line.clone(),
            sent_at: self.sent_at,
            status: self.status.clone(),
            status_said: report::status_said(self.status.as_deref(), self.shipped_in.as_deref()),
            note: self.note.clone(),
            shipped_in: self.shipped_in.clone(),
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
}

const UNREACHABLE: &str =
    "Couldn't reach Keevoro's reports service just now. Check this computer is online, then try again.";

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
            .http_status_as_error(false)
            .tls_config(tls)
            .build()
            .into();
        Self {
            base: base.trim().trim_end_matches('/').to_string(),
            agent,
        }
    }

    /// The status and the answer, or `None` when the service could not be
    /// reached at all.
    fn read(response: std::result::Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> Option<(u16, Answer)> {
        let mut response = response.ok()?;
        let status = response.status().as_u16();
        let text = response
            .body_mut()
            .with_config()
            .limit(ANSWER_MOST)
            .read_to_string()
            .unwrap_or_default();
        Some((status, serde_json::from_str(&text).unwrap_or_default()))
    }

    fn send(&self, body: &str) -> Result<(String, String)> {
        let response = self
            .agent
            .post(format!("{}/v1/reports", self.base))
            .header("Content-Type", "application/json")
            .header("User-Agent", concat!("Keevoro/", env!("CARGO_PKG_VERSION")))
            .send(body.as_bytes());
        let (status, answer) = Self::read(response).ok_or_else(|| Error::unreachable(UNREACHABLE.to_string()))?;
        if (200..300).contains(&status) && answer.ok == Some(true) {
            let reference = answer.reference.unwrap_or_default();
            let token = answer.token.unwrap_or_default();
            if plain_word(&reference, 40) && plain_word(&token, 128) {
                return Ok((reference, token));
            }
        }
        match answer.why.as_deref().map(shown).filter(|w| !w.is_empty()) {
            Some(why) if status >= 500 => Err(Error::failed(why)),
            Some(why) => Err(Error::refused(why)),
            None => Err(Error::failed(format!(
                "Keevoro's reports service didn't answer as expected (it said {status}). Try again later."
            ))),
        }
    }

    /// How one report is getting on, or `None` when the service could not
    /// be reached. An answer that is not about the report changes nothing.
    fn ask_after(&self, kept: &Kept) -> Option<Option<Answer>> {
        let response = self
            .agent
            .get(format!("{}/v1/reports/{}", self.base, kept.reference))
            .query("token", &kept.token)
            .header("User-Agent", concat!("Keevoro/", env!("CARGO_PKG_VERSION")))
            .call();
        let (status, answer) = Self::read(response)?;
        let about_it = (200..300).contains(&status)
            && answer.ok == Some(true)
            && answer.reference.as_deref().is_none_or(|r| r == kept.reference);
        Some(about_it.then_some(answer))
    }
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
        self.environment_variable(ADDRESS_VARIABLE)
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty())
            .or_else(|| option_env!("KEEVORO_REPORTS_URL").map(str::to_string))
            .unwrap_or_else(|| USUAL_ADDRESS.to_string())
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
        Ok(Prepared { body })
    }

    /// `reports.send`: sends a report exactly as `reports.prepare` made it,
    /// and keeps a note of it on this computer.
    pub fn send_report(&self, prepared: &str) -> Result<Sent> {
        let (home, names) = self.who();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let report = report::check_prepared(prepared, home.as_deref(), &names).map_err(Error::refused)?;
        let service = Service::at(&self.reports_address());
        let (reference, token) = service.send(prepared).inspect_err(|e| {
            self.log().say(&format!("Couldn't send a report: {}", e.message));
        })?;
        self.log().say(&format!("Sent a report, {reference}"));
        let kept = Kept {
            reference: reference.clone(),
            token,
            kind: report.kind,
            first_line: report::first_line(&report.message),
            sent_at: now(),
            status: Some("new".into()),
            note: None,
            shipped_in: None,
            updated_at: None,
        };
        // It has gone, whatever happens here: failing to note it down only
        // means it cannot be asked after from this computer.
        if let Err(e) = ReportsFile::change(self.backups().config_folder(), |all| all.reports.push(kept)) {
            self.log().say(&format!(
                "Couldn't keep a note of report {reference} on this computer: {e}"
            ));
        }
        Ok(Sent {
            said: format!("Thank you. Your reference is {reference}."),
            reference,
        })
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
                Some(Some(answer)) => heard.push((kept.reference.clone(), answer)),
                Some(None) => {}
                None => {
                    reached = false;
                    break;
                }
            }
        }
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
