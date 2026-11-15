//! Updating Keevoro to the newest release.
//!
//! `keevoro update` asks GitHub for the latest release, and if it is newer
//! than the running copy, downloads the build for this computer, checks it
//! and swaps it in. The same release is what the one-line installers
//! (`install.sh`, `install.ps1`) fetch, so there is one source of truth for
//! "the current version".
//!
//! Three rules shape this file:
//!
//! - **Only ever move up.** A release that is not strictly higher than the
//!   running version is ignored. That is why rolling back a bad release
//!   means publishing the good code under a *higher* number (see the Roll
//!   back workflow), not deleting anything.
//! - **Check before swapping.** Every release carries a `SHA256SUMS` file.
//!   The download must match it exactly or nothing is installed, so a
//!   truncated or corrupted download can never replace a working program.
//! - **Never phone home unasked.** Keevoro contacts GitHub only when the
//!   user runs `keevoro update`. There is no background check.
//!
//! The network sits behind the small [`Web`] trait so the whole flow is
//! tested without one.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use sha2::{Digest, Sha256};
use std::time::Duration;

pub const REPO: &str = "markcastle/keevoro";
pub const SUMS: &str = "SHA256SUMS";
/// No Keevoro build is anywhere near this; a larger download is refused.
const MAX_DOWNLOAD: u64 = 500 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error(
        "Couldn't find a Keevoro release. If the repository has been made private, set GITHUB_TOKEN to a token that can read it"
    )]
    NotFound,
    #[error("GitHub refused the request. Check that GITHUB_TOKEN is still valid and can read the repository")]
    Refused,
    #[error("Couldn't reach GitHub ({0}). Check your internet connection and try again")]
    Network(String),
    #[error("GitHub sent something unexpected ({0})")]
    Unexpected(String),
    #[error("Release {version} has no build for this kind of computer ({os}, {arch})")]
    NoBuild { version: String, os: String, arch: String },
    #[error("The download didn't match the release's checksum, so it was thrown away. Nothing was changed. Try again")]
    BadChecksum,
    #[error("Couldn't replace the installed copy ({0}). You may need to run this from an administrator prompt, or reinstall with the install script")]
    Install(String),
}

/// The two things the updater needs from the internet.
pub trait Web {
    /// A small JSON or text document.
    fn text(&self, url: &str) -> Result<String, UpdateError>;
    /// A release file. `url` is the asset's API address.
    fn bytes(&self, url: &str) -> Result<Vec<u8>, UpdateError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// Without the leading `v`.
    pub version: String,
    /// The release's page, where its notes are.
    pub page: String,
    pub assets: Vec<Asset>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Check {
    UpToDate,
    Available(Release),
}

/// The release file for a platform, named by `std::env::consts::OS` and
/// `ARCH`. The names carry no version so "latest" links never change.
pub fn asset_for(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("windows", "x86_64") => Some("keevoro-windows-x64.exe"),
        // One file runs on both Apple silicon and Intel Macs.
        ("macos", "aarch64" | "x86_64") => Some("keevoro-macos-universal"),
        ("linux", "x86_64") => Some("keevoro-linux-x64"),
        _ => None,
    }
}

/// Reads the reply from GitHub's "latest release" address.
pub fn parse_release(json: &str) -> Result<Release, UpdateError> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| UpdateError::Unexpected(e.to_string()))?;
    let text = |value: &serde_json::Value, key: &str| value.get(key).and_then(|x| x.as_str()).map(str::to_string);
    let tag = text(&v, "tag_name").ok_or_else(|| UpdateError::Unexpected("no tag_name".into()))?;
    let assets = v
        .get("assets")
        .and_then(|a| a.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|a| {
                    Some(Asset {
                        name: text(a, "name")?,
                        url: text(a, "url")?,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Release {
        version: tag.trim_start_matches('v').to_string(),
        page: text(&v, "html_url").unwrap_or_else(|| format!("https://github.com/{REPO}/releases")),
        assets,
    })
}

/// Is `candidate` a strictly higher version than `current`? Compared as
/// numbers, so 0.10.0 is after 0.9.0. Anything unreadable is not newer.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    fn numbers(v: &str) -> Option<[u64; 3]> {
        let mut parts = v
            .trim()
            .trim_start_matches('v')
            .split('.')
            .map(|p| p.parse::<u64>().ok());
        let out = [parts.next()??, parts.next()??, parts.next()??];
        parts.next().is_none().then_some(out)
    }
    matches!((numbers(candidate), numbers(current)), (Some(a), Some(b)) if a > b)
}

/// Asks whether a newer release than `current` exists.
pub fn check(web: &dyn Web, current: &str) -> Result<Check, UpdateError> {
    let release = parse_release(&web.text(&format!("https://api.github.com/repos/{REPO}/releases/latest"))?)?;
    if is_newer(&release.version, current) {
        Ok(Check::Available(release))
    } else {
        Ok(Check::UpToDate)
    }
}

/// Downloads the build of `release` for this platform and proves it matches
/// the release's checksum file. Returns the program's bytes.
pub fn download(web: &dyn Web, release: &Release, os: &str, arch: &str) -> Result<Vec<u8>, UpdateError> {
    let no_build = || UpdateError::NoBuild {
        version: release.version.clone(),
        os: os.to_string(),
        arch: arch.to_string(),
    };
    let name = asset_for(os, arch).ok_or_else(no_build)?;
    let find = |wanted: &str| release.assets.iter().find(|a| a.name == wanted);
    let asset = find(name).ok_or_else(no_build)?;
    // A release without checksums is not one this updater will trust.
    let sums = find(SUMS).ok_or(UpdateError::BadChecksum)?;
    let sums = String::from_utf8(web.bytes(&sums.url)?).map_err(|_| UpdateError::BadChecksum)?;
    let expected = expected_sum(&sums, name).ok_or(UpdateError::BadChecksum)?;

    let bytes = web.bytes(&asset.url)?;
    if sha256_hex(&bytes) != expected {
        return Err(UpdateError::BadChecksum);
    }
    Ok(bytes)
}

/// Finds a file's checksum in `sha256sum` output: `<hex>  <name>` per line.
pub fn expected_sum(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.split_once(char::is_whitespace)?;
        let file = file.trim().trim_start_matches('*');
        (file == name && hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit())).then(|| hash.to_lowercase())
    })
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Replaces the running program with `program`. The swap is done by the
/// operating system in one step, so an interruption leaves either the old
/// copy or the new one, never half of each.
pub fn install(program: &[u8]) -> Result<(), UpdateError> {
    let fail = |e: std::io::Error| UpdateError::Install(e.to_string());
    let staged = std::env::temp_dir().join(format!("keevoro-update-{}", std::process::id()));
    std::fs::write(&staged, program).map_err(fail)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)).map_err(fail)?;
    }
    let swapped = self_replace::self_replace(&staged).map_err(fail);
    let _ = std::fs::remove_file(&staged);
    swapped
}

/// GitHub over HTTPS. A token is only needed while the repository is
/// private; it is sent to GitHub's API and nowhere else.
pub struct GitHub {
    agent: ureq::Agent,
    token: Option<String>,
}

impl GitHub {
    pub fn new(token: Option<String>) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(15 * 60)))
            .build()
            .into();
        Self {
            agent,
            token: token.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()),
        }
    }

    /// Reads the token from `KEEVORO_GITHUB_TOKEN` or `GITHUB_TOKEN`.
    pub fn from_env() -> Self {
        Self::new(
            std::env::var("KEEVORO_GITHUB_TOKEN")
                .or_else(|_| std::env::var("GITHUB_TOKEN"))
                .ok(),
        )
    }

    fn get(&self, url: &str, accept: &str) -> Result<ureq::http::Response<ureq::Body>, UpdateError> {
        let mut request = self
            .agent
            .get(url)
            .header("User-Agent", "keevoro-update")
            .header("Accept", accept);
        if let Some(token) = &self.token {
            request = request.header("Authorization", &format!("Bearer {token}"));
        }
        request.call().map_err(|e| match e {
            ureq::Error::StatusCode(404) => UpdateError::NotFound,
            ureq::Error::StatusCode(401 | 403) => UpdateError::Refused,
            ureq::Error::StatusCode(code) => UpdateError::Unexpected(format!("status {code}")),
            other => UpdateError::Network(other.to_string()),
        })
    }
}

impl Web for GitHub {
    fn text(&self, url: &str) -> Result<String, UpdateError> {
        self.get(url, "application/vnd.github+json")?
            .body_mut()
            .read_to_string()
            .map_err(|e| UpdateError::Network(e.to_string()))
    }

    fn bytes(&self, url: &str) -> Result<Vec<u8>, UpdateError> {
        self.get(url, "application/octet-stream")?
            .body_mut()
            .with_config()
            .limit(MAX_DOWNLOAD)
            .read_to_vec()
            .map_err(|e| UpdateError::Network(e.to_string()))
    }
}

#[cfg(test)]
mod tests;
