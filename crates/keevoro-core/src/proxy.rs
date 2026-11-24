//! Which proxy this computer would send a connection through, for one
//! address.
//!
//! An office or school network may let traffic out only through a proxy.
//! Everything Keevoro sends over the internet (a report, and every request
//! to cloud storage) asks here, so all of it goes the same way:
//!
//! 1. An address on this computer or the local network (`localhost`, a
//!    name with no dot in it, a `.local` name, a private or loopback IP
//!    address) is always reached directly, as is any address `NO_PROXY`
//!    names.
//! 2. Otherwise the proxy named in the environment is used: `HTTPS_PROXY`
//!    for a secure address (`HTTP_PROXY` for a plain one), else
//!    `ALL_PROXY`, each in capitals or not.
//! 3. When the environment names none, a secure address goes through the
//!    proxy set in Windows' Internet settings, unless they say to reach it
//!    directly.
//!
//! The choice itself ([`choose`]) is pure and is given what to read from.
//! [`for_address`] is the one place that reads this computer's settings:
//! its environment and, on Windows, the registry. A proxy's address can
//! hold a name and password, so [`ProxyAddress`] never shows them.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// A proxy set in Windows' Internet settings, for secure addresses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemProxy {
    pub host: String,
    pub port: u16,
    /// The addresses it is not used for, as Windows lists them.
    pub not_for: Vec<String>,
}

/// The proxy for secure addresses that Windows' Internet settings name:
/// `ProxyEnable`, `ProxyServer` (`host:port` for everything, or
/// `http=host:port;https=host:port;...` one for each kind of address) and
/// `ProxyOverride` (addresses to reach directly, by `;`). None when it is
/// off, or none is set for secure addresses.
pub fn system_proxy_from(enabled: bool, server: &str, overrides: &str) -> Option<SystemProxy> {
    if !enabled {
        return None;
    }
    let parts: Vec<&str> = server.split(';').map(str::trim).filter(|p| !p.is_empty()).collect();
    let chosen = if parts.iter().any(|p| p.contains('=')) {
        parts.iter().find_map(|p| {
            let (kind, place) = p.split_once('=')?;
            kind.trim().eq_ignore_ascii_case("https").then_some(place.trim())
        })?
    } else {
        parts.first().copied()?
    };
    let place = chosen
        .strip_prefix("http://")
        .or_else(|| chosen.strip_prefix("https://"))
        .unwrap_or(chosen)
        .trim_end_matches('/');
    let (host, port) = place.rsplit_once(':')?;
    let port = port.parse().ok()?;
    if host.is_empty() {
        return None;
    }
    // `<local>` is Windows' word for names with no dot in them, which are
    // always reached directly here anyway.
    let not_for = overrides
        .split(';')
        .map(str::trim)
        .filter(|o| !o.is_empty() && !o.eq_ignore_ascii_case("<local>"))
        .map(str::to_string)
        .collect();
    Some(SystemProxy {
        host: host.to_string(),
        port,
        not_for,
    })
}

/// A proxy's address, `http://proxy.example.com:8080`, perhaps with a name
/// and password in it. Shown, logged or debugged, it is only ever its
/// scheme, host and port.
#[derive(Clone, PartialEq, Eq)]
pub struct ProxyAddress(String);

impl ProxyAddress {
    /// As it is set: with `http://` put in front when it has no scheme.
    pub fn from_setting(setting: &str) -> Self {
        let setting = setting.trim();
        if setting.contains("://") {
            Self(setting.to_string())
        } else {
            Self(format!("http://{setting}"))
        }
    }

    /// The whole address, name and password included, to hand to the
    /// connection that uses it and nowhere else.
    pub fn url(&self) -> &str {
        &self.0
    }

    /// The address without any name or password: fit to show.
    pub fn shown(&self) -> String {
        let (scheme, rest) = self.0.split_once("://").unwrap_or(("http", &self.0));
        let rest = rest.rsplit_once('@').map_or(rest, |(_, after)| after);
        format!("{scheme}://{}", rest.trim_end_matches('/'))
    }
}

impl fmt::Debug for ProxyAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ProxyAddress({})", self.shown())
    }
}

impl fmt::Display for ProxyAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.shown())
    }
}

/// The host an address names, in lower case, without its scheme, port,
/// path, or any name and password: `storage.googleapis.com` from
/// `https://storage.googleapis.com/family-photos`.
pub fn host_of(address: &str) -> Option<String> {
    let address = address.trim();
    let rest = address.split_once("://").map_or(address, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let authority = authority.rsplit_once('@').map_or(authority, |(_, after)| after);
    let host = if let Some(inside) = authority.strip_prefix('[') {
        inside.split(']').next().unwrap_or_default()
    } else {
        authority.split(':').next().unwrap_or_default()
    };
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// Is this host on this computer or the local network, where a proxy is
/// never wanted?
fn is_local(host: &str) -> bool {
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => local_v4(ip),
        Ok(IpAddr::V6(ip)) => local_v6(ip),
        Err(_) => {
            host == "localhost" || !host.contains('.') || host.ends_with(".local") || host.ends_with(".localhost")
        }
    }
}

fn local_v4(ip: Ipv4Addr) -> bool {
    ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified()
}

fn local_v6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    // Unique local (fc00::/7) and link-local (fe80::/10) addresses.
    ip.is_loopback() || ip.is_unspecified() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
}

/// Does one rule from `NO_PROXY` or Windows' list of addresses to reach
/// directly cover this host? `*` covers every host; a rule with `*` in it
/// is matched as a pattern (`*.example.com`, `10.*`); any other covers the
/// name itself and every name under it (`example.com` and `.example.com`
/// both cover `storage.example.com`). A port on the rule is ignored.
fn covers(rule: &str, host: &str) -> bool {
    let rule = rule.trim().to_ascii_lowercase();
    let rule = rule.trim_start_matches('[');
    let rule = match rule.rsplit_once(':') {
        Some((before, port)) if !before.contains(':') && port.chars().all(|c| c.is_ascii_digit()) => before,
        _ => rule,
    };
    let rule = rule.trim_end_matches(']');
    if rule.is_empty() {
        return false;
    }
    if rule == "*" {
        return true;
    }
    if rule.contains('*') {
        return pattern_matches(rule.as_bytes(), host.as_bytes());
    }
    let rule = rule.trim_start_matches('.');
    host == rule || host.strip_suffix(rule).is_some_and(|before| before.ends_with('.'))
}

/// `*` stands for any run of characters, including none.
fn pattern_matches(pattern: &[u8], text: &[u8]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some((b'*', rest)) => (0..=text.len()).any(|skip| pattern_matches(rest, &text[skip..])),
        Some((c, rest)) => text
            .split_first()
            .is_some_and(|(t, more)| t == c && pattern_matches(rest, more)),
    }
}

/// The proxy to reach `address` through, or `None` to reach it directly.
/// `environment` gives the value of an environment variable; `system`
/// gives Windows' setting, and is asked only when the environment names
/// no proxy and the address is a secure one.
pub fn choose(
    address: &str,
    environment: &dyn Fn(&str) -> Option<String>,
    system: &dyn Fn() -> Option<SystemProxy>,
) -> Option<ProxyAddress> {
    let host = host_of(address)?;
    if is_local(&host) {
        return None;
    }
    let set = |names: &[&str]| {
        names.iter().find_map(|name| {
            environment(name)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        })
    };
    if set(&["NO_PROXY", "no_proxy"]).is_some_and(|rules| rules.split(',').any(|rule| covers(rule, &host))) {
        return None;
    }
    let plain = address.trim().to_ascii_lowercase().starts_with("http://");
    let named = if plain {
        set(&["HTTP_PROXY", "http_proxy"])
    } else {
        set(&["HTTPS_PROXY", "https_proxy"])
    };
    if let Some(named) = named.or_else(|| set(&["ALL_PROXY", "all_proxy"])) {
        return Some(ProxyAddress::from_setting(&named));
    }
    if plain {
        return None;
    }
    let windows = system()?;
    if windows.not_for.iter().any(|rule| covers(rule, &host)) {
        return None;
    }
    Some(ProxyAddress(format!("http://{}:{}", windows.host, windows.port)))
}

/// The proxy this computer would reach `address` through, from its
/// environment and, on Windows, its Internet settings.
pub fn for_address(address: &str) -> Option<ProxyAddress> {
    choose(address, &|name| std::env::var(name).ok(), &system_proxy)
}

/// The proxy set in this user's Windows Internet settings, if any.
#[cfg(windows)]
fn system_proxy() -> Option<SystemProxy> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
    let settings = winreg::RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings", KEY_READ)
        .ok()?;
    let enabled = settings.get_value::<u32, _>("ProxyEnable").is_ok_and(|on| on == 1);
    let server: String = settings.get_value("ProxyServer").unwrap_or_default();
    let overrides: String = settings.get_value("ProxyOverride").unwrap_or_default();
    system_proxy_from(enabled, &server, &overrides)
}

#[cfg(not(windows))]
fn system_proxy() -> Option<SystemProxy> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOGLE: &str = "https://storage.googleapis.com";

    fn with(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let vars: Vec<(String, String)> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |name| vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
    }

    fn no_windows() -> Option<SystemProxy> {
        None
    }

    fn windows() -> Option<SystemProxy> {
        system_proxy_from(true, "office-proxy.example.com:3128", "*.example.com;intranet;<local>")
    }

    fn chosen(address: &str, vars: &[(&str, &str)], system: &dyn Fn() -> Option<SystemProxy>) -> Option<String> {
        choose(address, &with(vars), system).map(|p| p.url().to_string())
    }

    #[test]
    fn the_proxy_named_in_the_environment_is_used_for_a_secure_address() {
        let https = [("HTTPS_PROXY", "http://proxy.example.com:8080")];
        assert_eq!(
            chosen(GOOGLE, &https, &no_windows).as_deref(),
            Some("http://proxy.example.com:8080")
        );
        // In lower case, or only as the proxy for everything, or with no
        // scheme written: the same.
        let lower = [("https_proxy", "proxy.example.com:8080")];
        assert_eq!(
            chosen(GOOGLE, &lower, &no_windows).as_deref(),
            Some("http://proxy.example.com:8080")
        );
        let all = [("ALL_PROXY", "http://all.example.com:3128")];
        assert_eq!(
            chosen(GOOGLE, &all, &no_windows).as_deref(),
            Some("http://all.example.com:3128")
        );
        // The one for secure addresses comes before the one for everything.
        let both = [
            ("ALL_PROXY", "http://all.example.com:3128"),
            ("HTTPS_PROXY", "http://proxy.example.com:8080"),
        ];
        assert_eq!(
            chosen(GOOGLE, &both, &no_windows).as_deref(),
            Some("http://proxy.example.com:8080")
        );
        // It comes before Windows' own setting, too.
        assert_eq!(
            chosen(GOOGLE, &https, &windows).as_deref(),
            Some("http://proxy.example.com:8080")
        );
        // A plain address goes through the one for plain addresses.
        let http = [
            ("HTTP_PROXY", "http://web.example.com:80"),
            ("HTTPS_PROXY", "x.example.com:1"),
        ];
        assert_eq!(
            chosen("http://storage.example.org:9000", &http, &no_windows).as_deref(),
            Some("http://web.example.com:80")
        );
    }

    #[test]
    fn an_address_no_proxy_names_is_reached_directly() {
        let vars = [
            ("HTTPS_PROXY", "http://proxy.example.com:8080"),
            ("NO_PROXY", "localhost, .googleapis.com,example.org:443"),
        ];
        assert_eq!(chosen(GOOGLE, &vars, &no_windows), None);
        assert_eq!(chosen("https://storage.example.org/photos", &vars, &no_windows), None);
        assert!(chosen("https://s3.eu-west-2.amazonaws.com", &vars, &no_windows).is_some());
        // A name is not covered by one that only ends the same way.
        let near = [("HTTPS_PROXY", "proxy.example.com:8080"), ("no_proxy", "apis.com")];
        assert!(chosen(GOOGLE, &near, &no_windows).is_some());
        // `*` covers everything, and a pattern is a pattern.
        let every = [("HTTPS_PROXY", "proxy.example.com:8080"), ("NO_PROXY", "*")];
        assert_eq!(chosen(GOOGLE, &every, &no_windows), None);
        let pattern = [
            ("HTTPS_PROXY", "proxy.example.com:8080"),
            ("NO_PROXY", "*.googleapis.com"),
        ];
        assert_eq!(chosen(GOOGLE, &pattern, &no_windows), None);
        // It covers Windows' setting as well.
        assert_eq!(chosen(GOOGLE, &[("NO_PROXY", "googleapis.com")], &windows), None);
    }

    #[test]
    fn with_nothing_set_every_address_is_reached_directly() {
        assert_eq!(chosen(GOOGLE, &[], &no_windows), None);
        assert_eq!(chosen(GOOGLE, &[("HTTPS_PROXY", "  ")], &no_windows), None, "empty");
        // A plain address never goes through Windows' secure proxy.
        assert_eq!(chosen("http://storage.example.org", &[], &windows), None);
    }

    #[test]
    fn windows_proxy_is_used_when_the_environment_names_none() {
        assert_eq!(
            chosen(GOOGLE, &[], &windows).as_deref(),
            Some("http://office-proxy.example.com:3128")
        );
        // Except for the addresses it lists to reach directly.
        assert_eq!(chosen("https://files.example.com", &[], &windows), None);
        // Windows is not asked at all when the environment answers.
        let asked = std::cell::Cell::new(false);
        let asking = || {
            asked.set(true);
            None
        };
        let _ = chosen(GOOGLE, &[("HTTPS_PROXY", "proxy.example.com:8080")], &asking);
        assert!(!asked.get());
    }

    #[test]
    fn this_computer_and_the_local_network_are_always_reached_directly() {
        let vars = [
            ("HTTPS_PROXY", "proxy.example.com:8080"),
            ("HTTP_PROXY", "proxy.example.com:8080"),
        ];
        for local in [
            "http://localhost:9000",
            "http://127.0.0.1:9000",
            "https://192.168.1.20",
            "http://10.0.0.5:9000",
            "http://172.16.4.2",
            "http://[::1]:9000",
            "https://[fd12:3456::1]/",
            "http://nas:9000",
            "http://nas.local:9000",
        ] {
            assert_eq!(chosen(local, &vars, &windows), None, "{local}");
        }
        assert!(chosen("https://203.0.113.9", &vars, &no_windows).is_some());
    }

    #[test]
    fn a_proxys_name_and_password_are_never_shown() {
        let p = ProxyAddress::from_setting("http://someone:pa55word@proxy.example.com:8080/");
        assert_eq!(p.url(), "http://someone:pa55word@proxy.example.com:8080/");
        for said in [p.shown(), format!("{p:?}"), p.to_string()] {
            assert!(!said.contains("someone") && !said.contains("pa55word"), "{said}");
            assert!(said.contains("proxy.example.com:8080"), "{said}");
        }
    }

    #[test]
    fn the_host_is_taken_from_an_address() {
        assert_eq!(host_of(GOOGLE).as_deref(), Some("storage.googleapis.com"));
        assert_eq!(
            host_of("https://Someone:x@NAS.example.com:9000/b?x").as_deref(),
            Some("nas.example.com")
        );
        assert_eq!(host_of("http://[::1]:9000").as_deref(), Some("::1"));
        assert_eq!(host_of("storage.example.org").as_deref(), Some("storage.example.org"));
        assert_eq!(host_of("https://"), None);
    }

    #[test]
    fn the_windows_proxy_setting_is_read_in_each_of_its_forms() {
        let proxy = system_proxy_from;
        // One for everything.
        let all = proxy(true, "proxy.example.com:8080", "").unwrap();
        assert_eq!((all.host.as_str(), all.port), ("proxy.example.com", 8080));
        assert!(all.not_for.is_empty());
        // One for each kind of address: the secure one is the one used.
        let each = proxy(
            true,
            "http=web.example.com:80;https=secure.example.com:3128;ftp=files.example.com:21",
            "",
        )
        .unwrap();
        assert_eq!((each.host.as_str(), each.port), ("secure.example.com", 3128));
        // Written with its scheme, or with spaces.
        let written = proxy(true, " https = http://secure.example.com:8443 ", "").unwrap();
        assert_eq!((written.host.as_str(), written.port), ("secure.example.com", 8443));
        // None for secure addresses, switched off, or nothing usable: none.
        assert_eq!(proxy(true, "http=web.example.com:80", ""), None);
        assert_eq!(proxy(false, "proxy.example.com:8080", ""), None);
        assert_eq!(proxy(true, "", ""), None);
        assert_eq!(proxy(true, "proxy.example.com", ""), None, "no port");
        assert_eq!(proxy(true, "proxy.example.com:port", ""), None);
        // The addresses it is not used for, without Windows' own word for
        // names with no dot in them.
        let skipping = proxy(true, "proxy.example.com:8080", "*.example.com; intranet ;<local>;;").unwrap();
        assert_eq!(skipping.not_for, ["*.example.com", "intranet"]);
    }
}
