//! What cloud storage costs, from Stratus, for the person choosing where to
//! back up to.
//!
//! Stratus, made by Adam Sparshott, compares what cloud storage costs. When
//! the person opens the comparison, Keevoro asks it for the options that
//! fit a size, cheapest first (`GET /quote?need=240GB&currency=GBP`), and
//! the window shows them. It is told the size and the currency and nothing
//! else, with Keevoro's key in a header (see [`key`]). Prices are fetched
//! afresh each time and kept only in memory: never in a file or the log,
//! as a copied price goes stale and a wrong price is worse than none.
//!
//! It goes through the same proxy and the same trust in the system's
//! certificates as everything Keevoro sends (`keevoro_core::proxy`), so it
//! works on a work network, and the window itself never reaches Stratus.
//! What comes back is checked field by field: anything odd is left out.
//!
//! The address is Stratus's own, or the one in `KEEVORO_STRATUS_URL` when
//! that is set, for tests, which never reach the real one. Only Stratus's
//! own address is ever given the real key.

mod key;

use crate::error::{Error, Result};
use crate::keevoro::Keevoro;
use crate::reports::Unreached;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Stratus's own address.
const USUAL_ADDRESS: &str = "https://stratus-api.adam-sparshott.workers.dev/api/v1";
/// Set, the address asked instead: for tests.
pub const ADDRESS_VARIABLE: &str = "KEEVORO_STRATUS_URL";
/// What any other address is given in place of the key.
const STAND_IN_KEY: &str = "keevoro-stand-in";
/// How long to wait for the prices before saying they could not be
/// fetched: the person is looking at the sheet.
const WAIT: Duration = Duration::from_secs(12);
/// How long to wait for a connection.
const CONNECT_WAIT: Duration = Duration::from_secs(6);
/// The most of an answer that is read.
const ANSWER_MOST: u64 = 1024 * 1024;
/// The most options that are kept from one answer.
const MOST_OPTIONS: usize = 200;
/// The biggest size that can be asked about, in GB.
const BIGGEST: u64 = 1_000_000;
/// How every problem is said first.
const COULD_NOT: &str = "Couldn't fetch the prices from Stratus just now.";

/// A currency Stratus gives prices in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "UPPERCASE")]
pub enum Currency {
    /// Pounds sterling.
    Gbp,
    /// US dollars.
    Usd,
    /// Euros.
    Eur,
}

impl Currency {
    fn code(self) -> &'static str {
        match self {
            Self::Gbp => "GBP",
            Self::Usd => "USD",
            Self::Eur => "EUR",
        }
    }
}

/// `storage.compare`
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompareRequest {
    /// How much storage to compare for, in gigabytes as storage is sold (a
    /// thousand million bytes each), from 1 to 1,000,000.
    pub gigabytes: u64,
    /// The currency to show prices in: GBP, USD or EUR.
    pub currency: Currency,
}

/// The options that fit, cheapest first, as Stratus gave them.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    /// The size asked about, in GB.
    pub gigabytes: u64,
    pub currency: Currency,
    /// When Stratus last fetched its prices, as a time in UTC, if it said.
    pub fetched_at: Option<String>,
    /// Stratus's names for the sources of prices it could not refresh when
    /// it last looked, such as `apple`: those rows' prices are older.
    pub not_refreshed: Vec<String>,
    /// Cheapest first.
    pub options: Vec<StorageOption>,
}

/// One plan or kind of storage, with the fields of Stratus's own answer
/// that Keevoro uses, each checked.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageOption {
    /// Stratus's id for it, such as `r2-standard`.
    pub id: String,
    /// Stratus's id for the provider, such as `cloudflare-r2`.
    pub provider: String,
    /// The provider as people know it: "Cloudflare R2".
    pub provider_name: String,
    /// The plan or class: "R2 Standard".
    pub name: String,
    /// `object` for storage rented by the amount kept, `consumer` for a
    /// plan of a fixed size, or whatever else Stratus says.
    pub kind: String,
    /// How much a plan of a fixed size holds, in GB. None when paid for by
    /// the amount kept.
    #[serde(rename = "capacityGB")]
    pub capacity_gb: Option<f64>,
    /// True when paid for by the amount kept.
    pub metered: bool,
    /// True for a plan paid for once; its price a month is spread over
    /// years, as Stratus spreads it.
    pub lifetime: bool,
    /// The price a month for the size asked, in the currency asked.
    pub monthly: f64,
    /// The price a year, when Stratus gives one.
    pub yearly: Option<f64>,
    /// The provider's own page, when it gave a safe one.
    pub url: Option<String>,
}

/// Stratus's answer, as much of it as is read. Anything missing is taken
/// as nothing, and each option is checked on its own.
#[derive(Deserialize)]
struct Answer {
    #[serde(default)]
    meta: Meta,
    data: Vec<serde_json::Value>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Meta {
    #[serde(default)]
    fetched_at: Option<serde_json::Value>,
    #[serde(default)]
    sources: Vec<serde_json::Value>,
}

/// A short line of plain text, or nothing: control characters and runs of
/// spaces made one space, and not too long to show.
fn words(value: Option<&serde_json::Value>, longest: usize) -> Option<String> {
    let text = value?.as_str()?;
    let plain = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (!plain.is_empty() && plain.chars().count() <= longest).then_some(plain)
}

/// One of Stratus's ids: lower case letters, digits and hyphens.
fn an_id(value: Option<&serde_json::Value>) -> Option<String> {
    let text = value?.as_str()?;
    let fits = !text.is_empty()
        && text.len() <= 64
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    fits.then(|| text.to_string())
}

/// A price or a size: a number, or a number written as text, that is not
/// below nothing and not absurd.
fn amount(value: Option<&serde_json::Value>) -> Option<f64> {
    let number = match value? {
        serde_json::Value::Number(n) => n.as_f64()?,
        serde_json::Value::String(s) => s.trim().parse().ok()?,
        _ => return None,
    };
    (number.is_finite() && (0.0..1e12).contains(&number)).then_some(number)
}

/// An address safe to open in the person's browser: `https`, a real host
/// name, nobody's name or password in it, and not absurdly long.
fn safe_https(value: Option<&serde_json::Value>) -> Option<String> {
    let text = value?.as_str()?;
    let rest = text.strip_prefix("https://")?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let fits = text.len() <= 2000
        && !text.chars().any(|c| c.is_whitespace() || c.is_control() || c == '\\')
        && host.contains('.')
        && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    fits.then(|| text.to_string())
}

/// An option as Stratus gave it, or nothing when any part Keevoro needs is
/// missing or odd.
fn option_from(raw: &serde_json::Value) -> Option<StorageOption> {
    let flag = |name: &str| raw.get(name).and_then(serde_json::Value::as_bool).unwrap_or(false);
    Some(StorageOption {
        id: an_id(raw.get("id"))?,
        provider: an_id(raw.get("provider"))?,
        provider_name: words(raw.get("providerName"), 80)?,
        name: words(raw.get("name"), 80)?,
        kind: an_id(raw.get("kind")).unwrap_or_default(),
        capacity_gb: amount(raw.get("capacityGB")),
        metered: flag("metered"),
        lifetime: flag("lifetime"),
        monthly: amount(raw.get("monthly"))?,
        yearly: amount(raw.get("yearly")),
        url: safe_https(raw.get("url")),
    })
}

/// A time as Stratus writes them, said again in UTC, or nothing.
fn a_time(value: Option<&serde_json::Value>) -> Option<String> {
    let time: jiff::Timestamp = value?.as_str()?.parse().ok()?;
    Some(time.round(jiff::Unit::Second).ok()?.to_string())
}

/// What Stratus answered, read and checked, or nothing when it is not an
/// answer of prices at all.
fn read_answer(text: &str, request: &CompareRequest) -> Option<Comparison> {
    let answer: Answer = serde_json::from_str(text).ok()?;
    let mut options: Vec<StorageOption> = answer.data.iter().filter_map(option_from).take(MOST_OPTIONS).collect();
    options.sort_by(|a, b| a.monthly.total_cmp(&b.monthly));
    let not_refreshed = answer
        .meta
        .sources
        .iter()
        .filter(|s| s.get("ok").and_then(serde_json::Value::as_bool) == Some(false))
        .filter_map(|s| an_id(s.get("name")))
        .collect();
    Some(Comparison {
        gigabytes: request.gigabytes,
        currency: request.currency,
        fetched_at: a_time(answer.meta.fetched_at.as_ref()),
        not_refreshed,
        options,
    })
}

/// The key to send to `base`: Keevoro's own only to Stratus itself, and a
/// stand-in to anywhere else, such as a test's.
fn key_for(base: &str) -> String {
    if base.trim().trim_end_matches('/') == USUAL_ADDRESS {
        key::stratus_key()
    } else {
        STAND_IN_KEY.to_string()
    }
}

/// Why, in plain words, Stratus could not be reached, when that can be told.
fn unreached_said(why: Unreached) -> Option<&'static str> {
    match why {
        Unreached::Name => {
            Some("This computer couldn't look up Stratus's address, which usually means it is offline or its network blocks it.")
        }
        Unreached::Connection => Some("This computer's network would not let Keevoro connect to Stratus."),
        Unreached::Secure => {
            Some("This computer's network would not let Keevoro connect securely, as some work and school networks do.")
        }
        Unreached::Slow => Some("Stratus took too long to answer. The network may be slow."),
        Unreached::Proxy => Some("This computer's proxy would not pass Keevoro's connection on."),
        Unreached::Other => None,
    }
}

fn said(more: &str) -> String {
    format!("{COULD_NOT} {more}")
}

/// Asks Stratus at `base` for the options that fit, waiting at most
/// `wait`. A problem comes with the line for the log.
fn fetch(base: &str, request: &CompareRequest, wait: Duration) -> std::result::Result<Comparison, (Error, String)> {
    if request.gigabytes == 0 || request.gigabytes > BIGGEST {
        let refused = Error::refused("Choose a size from 1 GB to 1,000,000 GB to compare.");
        return Err((refused, "the size asked was out of range".into()));
    }
    let base = base.trim().trim_end_matches('/');
    // Trust what the operating system trusts, as the reports do, so a
    // school's or an office's own certificate is not refused.
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(wait))
        .timeout_connect(Some(CONNECT_WAIT.min(wait)))
        .proxy(keevoro_core::proxy::for_address(base).and_then(|p| ureq::Proxy::new(p.url()).ok()))
        .http_status_as_error(false)
        .tls_config(tls)
        .build()
        .into();
    let response = agent
        .get(format!("{base}/quote"))
        .query("need", format!("{}GB", request.gigabytes))
        .query("currency", request.currency.code())
        .header("X-API-Key", key_for(base))
        .header("Accept", "application/json")
        .header("User-Agent", concat!("Keevoro/", env!("CARGO_PKG_VERSION")))
        .call();
    let mut response = match response {
        Ok(response) => response,
        Err(error) => {
            let why = Unreached::of(&error);
            let message = unreached_said(why).map_or_else(|| COULD_NOT.to_string(), said);
            return Err((Error::unreachable(message), Unreached::logged(&error, None)));
        }
    };
    let status = response.status().as_u16();
    let text = response.body_mut().with_config().limit(ANSWER_MOST).read_to_string();
    let problem = |more: &str, logged: String| Err((Error::failed(said(more)), logged));
    match status {
        200..=299 => {}
        401 | 403 => return problem("Stratus turned Keevoro's request away.", format!("it said {status}")),
        429 => return problem("Stratus is busy. Try again in a minute.", format!("it said {status}")),
        500.. => {
            return problem(
                "Stratus had a problem of its own. Try again later.",
                format!("it said {status}"),
            )
        }
        _ => {
            return problem(
                "Stratus didn't understand what Keevoro asked.",
                format!("it said {status}"),
            )
        }
    }
    let Ok(text) = text else {
        return problem("Its answer couldn't be read.", "its answer could not be read".into());
    };
    read_answer(&text, request).map_or_else(
        || {
            problem(
                "Keevoro didn't understand its answer.",
                "its answer was not prices".into(),
            )
        },
        Ok,
    )
}

impl Keevoro {
    /// Where Stratus is: the address set for tests, or its own.
    fn stratus_address(&self) -> String {
        self.environment_variable(ADDRESS_VARIABLE)
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty())
            .unwrap_or_else(|| USUAL_ADDRESS.to_string())
    }

    /// `storage.compare`: the options that fit a size, cheapest first, as
    /// Stratus prices them now. Nothing is kept.
    pub fn compare_storage(&self, request: &CompareRequest) -> Result<Comparison> {
        let asked = format!("{} GB in {}", request.gigabytes, request.currency.code());
        match fetch(&self.stratus_address(), request, WAIT) {
            Ok(comparison) => {
                self.log()
                    .say(&format!("Fetched cloud storage prices from Stratus for {asked}"));
                Ok(comparison)
            }
            Err((error, logged)) => {
                self.log().say(&format!(
                    "Couldn't fetch cloud storage prices from Stratus for {asked}: {logged}"
                ));
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests;
