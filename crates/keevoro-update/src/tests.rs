use super::*;
use std::collections::BTreeMap;

/// A pretend GitHub holding one latest release.
struct FakeWeb {
    latest: Result<String, ()>,
    files: BTreeMap<String, Vec<u8>>,
}

impl Web for FakeWeb {
    fn text(&self, url: &str) -> Result<String, UpdateError> {
        assert_eq!(url, "https://api.github.com/repos/markcastle/keevoro/releases/latest");
        self.latest.clone().map_err(|()| UpdateError::NotFound)
    }
    fn bytes(&self, url: &str) -> Result<Vec<u8>, UpdateError> {
        self.files
            .get(url)
            .cloned()
            .ok_or_else(|| UpdateError::Network("gone".into()))
    }
}

const PROGRAM: &[u8] = b"pretend this is the keevoro program";

/// A release as GitHub describes it, with builds for every platform.
fn github(tag: &str) -> FakeWeb {
    let names = [
        "keevoro-windows-x64.exe",
        "keevoro-macos-universal",
        "keevoro-linux-x64",
        "Keevoro-windows-x64-setup.exe",
        "Keevoro-macos-universal.dmg",
    ];
    let mut files = BTreeMap::new();
    let mut assets = Vec::new();
    let mut sums = String::new();
    for (i, name) in names.iter().enumerate() {
        let url = format!("https://api.github.com/repos/markcastle/keevoro/releases/assets/{i}");
        files.insert(url.clone(), PROGRAM.to_vec());
        assets.push(serde_json::json!({ "name": name, "url": url, "size": PROGRAM.len() }));
        sums.push_str(&format!("{}  {name}\n", sha256_hex(PROGRAM)));
    }
    let sums_url = "https://api.github.com/repos/markcastle/keevoro/releases/assets/99".to_string();
    files.insert(sums_url.clone(), sums.into_bytes());
    assets.push(serde_json::json!({ "name": "SHA256SUMS", "url": sums_url }));
    let latest = serde_json::json!({
        "tag_name": tag,
        "html_url": format!("https://github.com/markcastle/keevoro/releases/tag/{tag}"),
        "assets": assets,
    });
    FakeWeb {
        latest: Ok(latest.to_string()),
        files,
    }
}

fn available(web: &FakeWeb, current: &str) -> Release {
    match check(web, current).unwrap() {
        Check::Available(r) => r,
        Check::UpToDate => panic!("expected an update from {current}"),
    }
}

#[test]
fn a_higher_release_is_offered_and_nothing_else_is() {
    let web = github("v0.3.0");
    let release = available(&web, "0.2.5");
    assert_eq!(release.version, "0.3.0");
    assert_eq!(
        release.page,
        "https://github.com/markcastle/keevoro/releases/tag/v0.3.0"
    );

    assert_eq!(check(&web, "0.3.0").unwrap(), Check::UpToDate);
    assert_eq!(check(&web, "0.4.0").unwrap(), Check::UpToDate, "never move down");
    assert_eq!(
        check(&github("nightly"), "0.1.0").unwrap(),
        Check::UpToDate,
        "an odd tag is not an update"
    );
}

#[test]
fn versions_compare_as_numbers() {
    assert!(is_newer("0.10.0", "0.9.9"));
    assert!(is_newer("v1.0.0", "0.99.0"));
    assert!(is_newer("0.2.1", "0.2.0"));
    assert!(!is_newer("0.2.0", "0.2.0"));
    assert!(!is_newer("0.9.0", "0.10.0"));
    for odd in ["", "1.2", "1.2.3.4", "one.two.three", "1.2.3-beta"] {
        assert!(!is_newer(odd, "0.0.1"), "{odd}");
        assert!(!is_newer("9.9.9", odd), "{odd}");
    }
}

#[test]
fn every_supported_computer_has_a_build() {
    assert_eq!(asset_for("windows", "x86_64"), Some("keevoro-windows-x64.exe"));
    assert_eq!(asset_for("macos", "aarch64"), Some("keevoro-macos-universal"));
    assert_eq!(asset_for("macos", "x86_64"), Some("keevoro-macos-universal"));
    assert_eq!(asset_for("linux", "x86_64"), Some("keevoro-linux-x64"));
    assert_eq!(asset_for("linux", "aarch64"), None);
    assert_eq!(asset_for("freebsd", "x86_64"), None);
    // This test itself runs on a supported one.
    assert!(asset_for(std::env::consts::OS, std::env::consts::ARCH).is_some());
}

#[test]
fn a_good_download_is_handed_over() {
    let web = github("v0.3.0");
    let release = available(&web, "0.2.0");
    for (os, arch) in [("windows", "x86_64"), ("macos", "aarch64"), ("linux", "x86_64")] {
        assert_eq!(download(&web, &release, os, arch).unwrap(), PROGRAM);
    }
}

#[test]
fn a_damaged_download_is_thrown_away() {
    let mut web = github("v0.3.0");
    let release = available(&web, "0.2.0");
    let linux = release
        .assets
        .iter()
        .find(|a| a.name == "keevoro-linux-x64")
        .unwrap()
        .url
        .clone();
    // Cut short on the way down.
    web.files.insert(linux, PROGRAM[..10].to_vec());
    let err = download(&web, &release, "linux", "x86_64").unwrap_err();
    assert!(matches!(err, UpdateError::BadChecksum));
    assert!(err.to_string().contains("Nothing was changed"));
    // The other builds are unaffected.
    assert!(download(&web, &release, "windows", "x86_64").is_ok());
}

#[test]
fn a_release_without_usable_checksums_is_not_trusted() {
    let web = github("v0.3.0");
    let mut release = available(&web, "0.2.0");
    let sums_url = release.assets.iter().find(|a| a.name == SUMS).unwrap().url.clone();

    // The checksum file lists other files but not ours.
    let mut partial = github("v0.3.0");
    partial
        .files
        .insert(sums_url.clone(), b"abc  something-else\n".to_vec());
    assert!(matches!(
        download(&partial, &release, "linux", "x86_64"),
        Err(UpdateError::BadChecksum)
    ));

    // It is not text at all.
    let mut garbage = github("v0.3.0");
    garbage.files.insert(sums_url, vec![0xFF, 0xFE, 0x00]);
    assert!(matches!(
        download(&garbage, &release, "linux", "x86_64"),
        Err(UpdateError::BadChecksum)
    ));

    // There is no checksum file on the release.
    release.assets.retain(|a| a.name != SUMS);
    assert!(matches!(
        download(&web, &release, "linux", "x86_64"),
        Err(UpdateError::BadChecksum)
    ));
}

#[test]
fn a_missing_build_says_which_computer_it_is_missing_for() {
    let web = github("v0.3.0");
    let mut release = available(&web, "0.2.0");
    let err = download(&web, &release, "linux", "riscv64").unwrap_err();
    assert_eq!(
        err.to_string(),
        "Release 0.3.0 has no build for this kind of computer (linux, riscv64)"
    );
    release.assets.retain(|a| a.name != "keevoro-macos-universal");
    assert!(matches!(
        download(&web, &release, "macos", "aarch64"),
        Err(UpdateError::NoBuild { .. })
    ));
}

#[test]
fn network_trouble_is_passed_on_plainly() {
    let mut web = github("v0.3.0");
    let release = available(&web, "0.2.0");
    web.files.clear();
    assert!(matches!(
        download(&web, &release, "linux", "x86_64"),
        Err(UpdateError::Network(_))
    ));

    web.latest = Err(());
    let err = check(&web, "0.2.0").unwrap_err();
    assert!(err.to_string().contains("set GITHUB_TOKEN"), "{err}");
}

#[test]
fn checksum_files_are_read_the_way_sha256sum_writes_them() {
    let hash = sha256_hex(b"abc");
    assert_eq!(hash, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    let sums = format!(
        "{}  other-file\n{}  keevoro-linux-x64\n{} *binary-mode\n",
        "0".repeat(64),
        hash.to_uppercase(),
        hash
    );
    assert_eq!(
        expected_sum(&sums, "keevoro-linux-x64").unwrap(),
        hash,
        "case does not matter"
    );
    assert_eq!(expected_sum(&sums, "binary-mode").unwrap(), hash);
    assert_eq!(expected_sum(&sums, "keevoro"), None, "a name must match whole");
    assert_eq!(expected_sum("tooshort  keevoro-linux-x64\n", "keevoro-linux-x64"), None);
    assert_eq!(expected_sum("", "keevoro-linux-x64"), None);
}

#[test]
fn odd_replies_from_github_do_not_crash() {
    assert!(matches!(
        parse_release("<html>rate limited</html>"),
        Err(UpdateError::Unexpected(_))
    ));
    assert!(matches!(parse_release("{}"), Err(UpdateError::Unexpected(_))));
    let bare =
        parse_release(r#"{"tag_name":"v1.2.3","assets":[{"name":"x"},{"url":"y"},{"name":"ok","url":"u"}]}"#).unwrap();
    assert_eq!(bare.version, "1.2.3");
    assert_eq!(bare.page, "https://github.com/markcastle/keevoro/releases");
    assert_eq!(
        bare.assets,
        [Asset {
            name: "ok".into(),
            url: "u".into()
        }]
    );
    assert!(parse_release(r#"{"tag_name":"v1.2.3"}"#).unwrap().assets.is_empty());
}

#[test]
fn a_token_is_optional_and_tidied() {
    assert_eq!(
        GitHub::new(Some("  ghp_abc \n".into())).token.as_deref(),
        Some("ghp_abc")
    );
    assert_eq!(GitHub::new(Some("   ".into())).token, None);
    assert_eq!(GitHub::new(None).token, None);
}

#[test]
fn an_address_that_is_not_listening_is_a_network_problem() {
    let web = GitHub::new(Some("token".into()));
    let err = web.text("http://127.0.0.1:9/nothing").unwrap_err();
    assert!(matches!(err, UpdateError::Network(_)), "{err:?}");
    assert!(matches!(
        web.bytes("http://127.0.0.1:9/nothing"),
        Err(UpdateError::Network(_))
    ));
}

/// A one-request web server on this machine, standing in for GitHub so the
/// real HTTP code is exercised. Returns its address and what it was sent.
fn serve_once(status: &str, body: &'static [u8]) -> (String, std::thread::JoinHandle<String>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let status = status.to_string();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        let mut buf = [0u8; 1024];
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = stream.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            request.extend_from_slice(&buf[..n]);
        }
        let head = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(head.as_bytes()).unwrap();
        stream.write_all(body).unwrap();
        String::from_utf8_lossy(&request).to_lowercase()
    });
    (address, handle)
}

#[test]
fn requests_carry_the_token_and_ask_for_the_right_thing() {
    let (address, sent) = serve_once("200 OK", b"{\"tag_name\":\"v0.3.0\"}");
    let web = GitHub::new(Some("ghp_secret".into()));
    assert_eq!(
        web.text(&format!("{address}/latest")).unwrap(),
        "{\"tag_name\":\"v0.3.0\"}"
    );
    let request = sent.join().unwrap();
    assert!(request.contains("authorization: bearer ghp_secret"), "{request}");
    assert!(request.contains("accept: application/vnd.github+json"), "{request}");
    assert!(request.contains("user-agent: keevoro-update"), "{request}");

    let (address, sent) = serve_once("200 OK", PROGRAM);
    let anonymous = GitHub::new(None);
    assert_eq!(anonymous.bytes(&format!("{address}/asset")).unwrap(), PROGRAM);
    let request = sent.join().unwrap();
    assert!(request.contains("accept: application/octet-stream"), "{request}");
    assert!(!request.contains("authorization"), "no token, no header: {request}");
}

#[test]
fn githubs_refusals_are_explained() {
    let web = GitHub::new(None);
    for (status, expected) in [
        ("404 Not Found", "set GITHUB_TOKEN"),
        ("401 Unauthorized", "GITHUB_TOKEN is still valid"),
        ("403 Forbidden", "GITHUB_TOKEN is still valid"),
        ("500 Internal Server Error", "something unexpected (status 500)"),
    ] {
        let (address, sent) = serve_once(status, b"{}");
        let err = web.text(&format!("{address}/latest")).unwrap_err().to_string();
        assert!(err.contains(expected), "{status}: {err}");
        sent.join().unwrap();
    }
}

#[test]
fn the_apps_installer_is_fetched_and_checked_like_any_other_file() {
    let web = github("v0.9.0");
    let release = available(&web, "0.8.0");
    let (name, bytes) = download_installer(&web, &release, "windows").unwrap();
    assert_eq!((name, bytes.as_slice()), ("Keevoro-windows-x64-setup.exe", PROGRAM));
    assert_eq!(
        download_installer(&web, &release, "macos").unwrap().0,
        "Keevoro-macos-universal.dmg"
    );

    // A tampered installer is refused.
    let mut damaged = github("v0.9.0");
    let url = release
        .assets
        .iter()
        .find(|a| a.name == "Keevoro-windows-x64-setup.exe")
        .unwrap()
        .url
        .clone();
    damaged.files.insert(url, b"not what was published".to_vec());
    assert!(matches!(
        download_installer(&damaged, &release, "windows"),
        Err(UpdateError::BadChecksum)
    ));
}

#[test]
fn computers_without_an_installer_are_told_so() {
    let web = github("v0.9.0");
    let mut release = available(&web, "0.8.0");
    assert!(matches!(
        download_installer(&web, &release, "linux"),
        Err(UpdateError::NoBuild { .. })
    ));
    // An older release from before the app had installers.
    release.assets.retain(|a| !a.name.starts_with("Keevoro-"));
    assert!(matches!(
        download_installer(&web, &release, "windows"),
        Err(UpdateError::NoBuild { .. })
    ));
}
