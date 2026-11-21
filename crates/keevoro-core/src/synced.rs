//! Folders a cloud service keeps in step with this computer: OneDrive,
//! Dropbox, Google Drive and iCloud Drive.
//!
//! People keep photos in these without thinking of them as a place, so the
//! first screen offers each by the name the person knows ("OneDrive",
//! "Dropbox (Acme)") beside the drives. Where each service puts its folder
//! differs by system, by version and by account, so the deciding is kept
//! here, pure: it is given what the computer said ([`ComputerFacts`]: the
//! environment, the text of a few small settings files, one or two folder
//! listings, the drives' labels, and which of the folders it asked about
//! exist) and decides which folders to offer. Tests give it the facts of a
//! Windows computer, a Mac and Linux, on any computer. Gathering the facts
//! is the caller's (`keevoro-api`'s `synced`), a few milliseconds of small
//! reads, done off the window's thread and kept for half a minute.
//!
//! Nothing here, or in the gathering, opens a file in the person's cloud
//! folder: only the services' own settings files and folder listings.

use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

/// A cloud service a person would know by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Service {
    OneDrive,
    Dropbox,
    GoogleDrive,
    #[serde(rename = "iCloud")]
    ICloud,
}

impl Service {
    /// What the person calls it: "OneDrive", "iCloud Drive".
    pub fn name(self) -> &'static str {
        match self {
            Self::OneDrive => "OneDrive",
            Self::Dropbox => "Dropbox",
            Self::GoogleDrive => "Google Drive",
            Self::ICloud => "iCloud Drive",
        }
    }

    /// Whose cloud it is kept in step with, for "Synced with Microsoft's
    /// cloud".
    pub fn whose_cloud(self) -> &'static str {
        match self {
            Self::OneDrive => "Microsoft’s cloud",
            Self::Dropbox => "Dropbox’s cloud",
            Self::GoogleDrive => "Google’s cloud",
            Self::ICloud => "Apple’s iCloud",
        }
    }
}

/// Which system the facts came from: where each service puts its folder
/// depends on it, and so does how paths are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum System {
    Windows,
    Mac,
    Other,
}

/// What the computer said, gathered beforehand. Paths are written as the
/// system writes them, so a Windows computer's facts can be decided on any
/// computer.
#[derive(Debug, Clone)]
pub struct ComputerFacts {
    pub system: System,
    /// The person's home folder: `C:\Users\Mark`, `/Users/mark`.
    pub home: String,
    /// The few variables of the environment asked for (see
    /// [`ENVIRONMENT`]), by name.
    pub environment: BTreeMap<String, String>,
    /// The text of each settings file asked for ([`files_to_read`]) that
    /// could be read, by its path.
    pub files: BTreeMap<String, String>,
    /// The names in each folder asked for ([`folders_to_list`]) that could
    /// be listed, by its path.
    pub listings: BTreeMap<String, Vec<String>>,
    /// The drives mounted, each as (where, its label).
    pub volumes: Vec<(String, String)>,
    /// Which of the folders asked about ([`places_to_check`]) exist.
    pub existing: BTreeSet<String>,
}

impl ComputerFacts {
    /// Facts with nothing yet known but the system and home folder.
    pub fn new(system: System, home: &str) -> Self {
        Self {
            system,
            home: home.to_string(),
            environment: BTreeMap::new(),
            files: BTreeMap::new(),
            listings: BTreeMap::new(),
            volumes: Vec::new(),
            existing: BTreeSet::new(),
        }
    }

    fn join(&self, base: &str, rest: &[&str]) -> String {
        let sep = if self.system == System::Windows { '\\' } else { '/' };
        let mut path = base.trim_end_matches(['\\', '/']).to_string();
        for part in rest {
            path.push(sep);
            path.push_str(part);
        }
        path
    }

    fn var(&self, name: &str) -> Option<&str> {
        self.environment
            .get(name)
            .map(|v| v.trim().trim_end_matches(['\\', '/']))
            .filter(|v| !v.is_empty())
    }

    /// The Windows profile folder, from the environment or the home.
    fn profile(&self) -> String {
        self.var("USERPROFILE").unwrap_or(&self.home).to_string()
    }
}

/// The variables of the environment the gathering reads, on Windows.
/// OneDrive sets the last three for each person signed in to it.
pub const ENVIRONMENT: &[&str] = &[
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "OneDrive",
    "OneDriveConsumer",
    "OneDriveCommercial",
];

/// A folder a cloud service keeps in step with this computer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncedFolder {
    pub service: Service,
    /// What to call it: "OneDrive", "OneDrive - Contoso", "Dropbox (Acme)".
    pub name: String,
    /// Where it is, as the system writes it.
    pub path: String,
    /// The account or organisation, when the folder names one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

impl SyncedFolder {
    /// The line under its name: "Synced with Microsoft's cloud ·
    /// C:\Users\Mark\OneDrive".
    pub fn detail(&self) -> String {
        format!("Synced with {} · {}", self.service.whose_cloud(), self.path)
    }
}

/// The settings files to read: where Dropbox says its folders are.
pub fn files_to_read(facts: &ComputerFacts) -> Vec<String> {
    match facts.system {
        System::Windows => ["LOCALAPPDATA", "APPDATA"]
            .iter()
            .filter_map(|v| facts.var(v))
            .map(|base| facts.join(base, &["Dropbox", "info.json"]))
            .collect(),
        System::Mac | System::Other => vec![facts.join(&facts.home, &[".dropbox", "info.json"])],
    }
}

/// The folders to list: on a Mac, where the newer services keep theirs.
pub fn folders_to_list(facts: &ComputerFacts) -> Vec<String> {
    match facts.system {
        System::Mac => vec![cloud_storage(facts)],
        System::Windows | System::Other => Vec::new(),
    }
}

fn cloud_storage(facts: &ComputerFacts) -> String {
    facts.join(&facts.home, &["Library", "CloudStorage"])
}

/// Every folder worth asking whether it exists, given the settings files
/// and listings already read.
pub fn places_to_check(facts: &ComputerFacts) -> Vec<String> {
    let mut places: Vec<String> = candidates(facts).into_iter().map(|c| c.path).collect();
    places.sort();
    places.dedup();
    places
}

/// The folders to offer, those that exist, once each, in the order the
/// services are listed in [`Service`] and then by name. Pure.
pub fn synced_folders(facts: &ComputerFacts) -> Vec<SyncedFolder> {
    let case_matters = facts.system == System::Other;
    let same = |a: &str, b: &str| {
        let (a, b) = (a.trim_end_matches(['\\', '/']), b.trim_end_matches(['\\', '/']));
        if case_matters {
            a == b
        } else {
            a.eq_ignore_ascii_case(b)
        }
    };
    let mut found: Vec<SyncedFolder> = Vec::new();
    for candidate in candidates(facts) {
        let there = facts.existing.iter().any(|e| same(e, &candidate.path));
        if there && !found.iter().any(|f| same(&f.path, &candidate.path)) {
            found.push(candidate);
        }
    }
    // With a work OneDrive beside a personal one, the personal one is
    // named as Windows names it, so the two are told apart.
    let business = found
        .iter()
        .any(|f| f.service == Service::OneDrive && f.account.is_some());
    for f in &mut found {
        if business && f.service == Service::OneDrive && f.account.is_none() {
            f.name = "OneDrive - Personal".to_string();
        }
    }
    // Two Google accounts are told apart by their address.
    let googles = found.iter().filter(|f| f.service == Service::GoogleDrive).count();
    for f in &mut found {
        if let (true, Some(account)) = (googles > 1 && f.service == Service::GoogleDrive, &f.account) {
            f.name = format!("Google Drive ({account})");
        }
    }
    found.sort_by(|a, b| a.service.cmp(&b.service).then_with(|| a.name.cmp(&b.name)));
    found
}

fn folder(service: Service, name: &str, path: String, account: Option<&str>) -> SyncedFolder {
    SyncedFolder {
        service,
        name: name.to_string(),
        path,
        account: account.map(str::to_string),
    }
}

/// Every folder the facts suggest, whether or not it exists, best evidence
/// first, so the name given by the best evidence is the one kept.
fn candidates(facts: &ComputerFacts) -> Vec<SyncedFolder> {
    let mut out = Vec::new();
    match facts.system {
        System::Windows => windows(facts, &mut out),
        System::Mac => mac(facts, &mut out),
        System::Other => {}
    }
    dropbox(facts, &mut out);
    out
}

fn windows(facts: &ComputerFacts, out: &mut Vec<SyncedFolder>) {
    let profile = facts.profile();
    // OneDrive names each folder it keeps: "OneDrive" for a personal
    // account, "OneDrive - Contoso" for a work one.
    for var in ["OneDriveCommercial", "OneDriveConsumer", "OneDrive"] {
        if let Some(path) = facts.var(var) {
            out.push(onedrive_folder(path.to_string(), var == "OneDriveCommercial"));
        }
    }
    out.push(folder(
        Service::OneDrive,
        "OneDrive",
        facts.join(&profile, &["OneDrive"]),
        None,
    ));
    // Google Drive's own drive, by its label (G: unless the person chose
    // another letter), then a folder it mirrors onto the disk.
    for (mount, label) in &facts.volumes {
        if label.trim().eq_ignore_ascii_case("Google Drive") {
            out.push(folder(Service::GoogleDrive, "Google Drive", mount.clone(), None));
        }
    }
    out.push(folder(
        Service::GoogleDrive,
        "Google Drive",
        facts.join(&profile, &["My Drive"]),
        None,
    ));
    out.push(folder(
        Service::ICloud,
        "iCloud Drive",
        facts.join(&profile, &["iCloudDrive"]),
        None,
    ));
    out.push(folder(
        Service::ICloud,
        "iCloud Photos",
        facts.join(&profile, &["Pictures", "iCloud Photos"]),
        None,
    ));
}

/// A OneDrive folder, named by its own folder name: a work account's is
/// "OneDrive - Contoso".
fn onedrive_folder(path: String, work: bool) -> SyncedFolder {
    let last = last_part(&path).to_string();
    match last
        .strip_prefix("OneDrive - ")
        .map(str::trim)
        .filter(|o| !o.is_empty())
    {
        Some(org) => folder(Service::OneDrive, &last, path.clone(), Some(org)),
        None if work => folder(Service::OneDrive, "OneDrive - Work", path, Some("Work")),
        None => folder(Service::OneDrive, "OneDrive", path, None),
    }
}

fn mac(facts: &ComputerFacts, out: &mut Vec<SyncedFolder>) {
    // The newer services all keep theirs in ~/Library/CloudStorage, one
    // folder each, named by the service, a dash, and the account.
    let storage = cloud_storage(facts);
    let mut names = facts.listings.get(&storage).cloned().unwrap_or_default();
    names.sort();
    for name in names {
        let path = facts.join(&storage, &[&name]);
        let (service, account) = name.split_once('-').map_or((name.as_str(), ""), |(s, a)| (s, a));
        match service {
            "OneDrive" if account.starts_with("SharedLibraries") => {}
            "OneDrive" if account.is_empty() || account == "Personal" => {
                out.push(folder(Service::OneDrive, "OneDrive", path, None));
            }
            "OneDrive" => out.push(folder(
                Service::OneDrive,
                &format!("OneDrive - {account}"),
                path,
                Some(account),
            )),
            "Dropbox" if account.is_empty() => out.push(folder(Service::Dropbox, "Dropbox", path, None)),
            "Dropbox" => out.push(folder(
                Service::Dropbox,
                &format!("Dropbox ({account})"),
                path,
                Some(account),
            )),
            // Google Drive's folder holds "My Drive" and "Shared drives";
            // the person's own files are in the first.
            "GoogleDrive" => out.push(folder(
                Service::GoogleDrive,
                "Google Drive",
                facts.join(&path, &["My Drive"]),
                (!account.is_empty()).then_some(account),
            )),
            _ => {}
        }
    }
    out.push(folder(
        Service::ICloud,
        "iCloud Drive",
        facts.join(&facts.home, &["Library", "Mobile Documents", "com~apple~CloudDocs"]),
        None,
    ));
    // Before the services moved to CloudStorage.
    out.push(folder(
        Service::OneDrive,
        "OneDrive",
        facts.join(&facts.home, &["OneDrive"]),
        None,
    ));
    for (mount, label) in &facts.volumes {
        if label.trim().eq_ignore_ascii_case("GoogleDrive") || label.trim().eq_ignore_ascii_case("Google Drive") {
            out.push(folder(
                Service::GoogleDrive,
                "Google Drive",
                facts.join(mount, &["My Drive"]),
                None,
            ));
        }
    }
}

/// Dropbox says where its folders are in `info.json`, one entry for a
/// personal account and one for a business one.
fn dropbox(facts: &ComputerFacts, out: &mut Vec<SyncedFolder>) {
    for file in files_to_read(facts) {
        let Some(text) = facts.files.get(&file) else { continue };
        let Ok(info) = serde_json::from_str::<serde_json::Value>(text) else {
            continue;
        };
        for (kind, business) in [("personal", false), ("business", true)] {
            let Some(path) = info.get(kind).and_then(|k| k.get("path")).and_then(|p| p.as_str()) else {
                continue;
            };
            let path = path.trim_end_matches(['\\', '/']).to_string();
            let last = last_part(&path).to_string();
            let team = last
                .strip_prefix("Dropbox (")
                .and_then(|t| t.strip_suffix(')'))
                .map(str::to_string);
            out.push(match (business, team) {
                (_, Some(team)) => folder(Service::Dropbox, &last, path, Some(&team)),
                (true, None) => folder(Service::Dropbox, "Dropbox (work)", path, Some("work")),
                (false, None) => folder(Service::Dropbox, "Dropbox", path, None),
            });
        }
    }
    out.push(folder(
        Service::Dropbox,
        "Dropbox",
        facts.join(&facts.home, &["Dropbox"]),
        None,
    ));
}

fn last_part(path: &str) -> &str {
    path.trim_end_matches(['\\', '/'])
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(path)
}

/// Which service keeps `path` in step, guessed from the names of its
/// folders as each service names them, for saying "in OneDrive only". Pure,
/// and needs nothing gathered, so a source's sentence costs nothing.
pub fn service_of(path: &Path) -> Option<Service> {
    path.components().rev().find_map(|c| {
        let Component::Normal(part) = c else { return None };
        let part = part.to_string_lossy();
        let starts = |p: &str| part.get(..p.len()).is_some_and(|s| s.eq_ignore_ascii_case(p));
        if starts("OneDrive") {
            Some(Service::OneDrive)
        } else if starts("Dropbox") {
            Some(Service::Dropbox)
        } else if starts("GoogleDrive") || starts("Google Drive") || part == "My Drive" {
            Some(Service::GoogleDrive)
        } else if starts("iCloud") || part == "com~apple~CloudDocs" {
            Some(Service::ICloud)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn windows() -> ComputerFacts {
        let mut facts = ComputerFacts::new(System::Windows, r"C:\Users\Mark");
        for (k, v) in [
            ("USERPROFILE", r"C:\Users\Mark"),
            ("APPDATA", r"C:\Users\Mark\AppData\Roaming"),
            ("LOCALAPPDATA", r"C:\Users\Mark\AppData\Local"),
        ] {
            facts.environment.insert(k.into(), v.into());
        }
        facts
    }

    fn exists(facts: &mut ComputerFacts, paths: &[&str]) {
        facts.existing.extend(paths.iter().map(|p| (*p).to_string()));
    }

    fn names(found: &[SyncedFolder]) -> Vec<(&str, &str)> {
        found.iter().map(|f| (f.name.as_str(), f.path.as_str())).collect()
    }

    #[test]
    fn a_windows_onedrive_is_found_from_what_onedrive_sets_and_named_as_explorer_names_it() {
        let mut facts = windows();
        facts
            .environment
            .insert("OneDrive".into(), r"C:\Users\Mark\OneDrive".into());
        facts
            .environment
            .insert("OneDriveConsumer".into(), r"C:\Users\Mark\OneDrive".into());
        exists(&mut facts, &[r"C:\Users\Mark\OneDrive"]);
        let found = synced_folders(&facts);
        assert_eq!(names(&found), [("OneDrive", r"C:\Users\Mark\OneDrive")]);
        assert_eq!(
            found[0].detail(),
            r"Synced with Microsoft’s cloud · C:\Users\Mark\OneDrive"
        );

        // With a work account too, each is named as Explorer names it.
        facts
            .environment
            .insert("OneDriveCommercial".into(), r"D:\Work\OneDrive - Contoso".into());
        exists(&mut facts, &[r"D:\Work\OneDrive - Contoso"]);
        let found = synced_folders(&facts);
        assert_eq!(
            names(&found),
            [
                ("OneDrive - Contoso", r"D:\Work\OneDrive - Contoso"),
                ("OneDrive - Personal", r"C:\Users\Mark\OneDrive"),
            ]
        );
        assert_eq!(found[0].account.as_deref(), Some("Contoso"));
    }

    #[test]
    fn a_folder_that_is_not_there_is_never_offered() {
        let mut facts = windows();
        facts
            .environment
            .insert("OneDrive".into(), r"C:\Users\Mark\OneDrive".into());
        assert!(
            synced_folders(&facts).is_empty(),
            "OneDrive signed out, its folder gone"
        );
        assert!(places_to_check(&facts).contains(&r"C:\Users\Mark\OneDrive".to_string()));
    }

    #[test]
    fn the_same_folder_found_two_ways_is_offered_once_whatever_its_case_on_windows() {
        let mut facts = windows();
        facts
            .environment
            .insert("OneDrive".into(), r"c:\users\mark\onedrive\".into());
        exists(&mut facts, &[r"C:\Users\Mark\OneDrive"]);
        assert_eq!(synced_folders(&facts).len(), 1);
    }

    #[test]
    fn dropbox_is_found_where_its_own_settings_say_personal_and_business() {
        let mut facts = windows();
        let info = r#"{"personal": {"path": "C:\\Users\\Mark\\Dropbox", "host": 1},
                       "business": {"path": "E:\\Dropbox (Acme)", "host": 2}}"#;
        let file = r"C:\Users\Mark\AppData\Local\Dropbox\info.json";
        assert!(files_to_read(&facts).contains(&file.to_string()));
        facts.files.insert(file.into(), info.into());
        exists(&mut facts, &[r"C:\Users\Mark\Dropbox", r"E:\Dropbox (Acme)"]);
        let found = synced_folders(&facts);
        assert_eq!(
            names(&found),
            [
                ("Dropbox", r"C:\Users\Mark\Dropbox"),
                ("Dropbox (Acme)", r"E:\Dropbox (Acme)")
            ]
        );
        assert_eq!(found[1].detail(), r"Synced with Dropbox’s cloud · E:\Dropbox (Acme)");

        // A settings file that cannot be read costs only itself.
        facts.files.insert(file.into(), "{ not json".into());
        assert_eq!(names(&synced_folders(&facts)), [("Dropbox", r"C:\Users\Mark\Dropbox")]);
    }

    #[test]
    fn google_drive_is_its_own_drive_by_label_or_a_mirrored_folder() {
        let mut facts = windows();
        facts.volumes.push((r"G:\".into(), "Google Drive".into()));
        facts.volumes.push((r"E:\".into(), "KINGSTON".into()));
        exists(&mut facts, &[r"G:\", r"C:\Users\Mark\My Drive"]);
        let found = synced_folders(&facts);
        assert_eq!(
            names(&found),
            [("Google Drive", r"G:\"), ("Google Drive", r"C:\Users\Mark\My Drive")]
        );
        assert_eq!(found[0].detail(), r"Synced with Google’s cloud · G:\");
    }

    #[test]
    fn icloud_for_windows_offers_its_drive_and_its_photos() {
        let mut facts = windows();
        exists(
            &mut facts,
            &[r"C:\Users\Mark\iCloudDrive", r"C:\Users\Mark\Pictures\iCloud Photos"],
        );
        assert_eq!(
            names(&synced_folders(&facts)),
            [
                ("iCloud Drive", r"C:\Users\Mark\iCloudDrive"),
                ("iCloud Photos", r"C:\Users\Mark\Pictures\iCloud Photos")
            ]
        );
    }

    #[test]
    fn on_a_mac_every_service_in_cloud_storage_is_named_by_its_folder() {
        let mut facts = ComputerFacts::new(System::Mac, "/Users/mark");
        let storage = "/Users/mark/Library/CloudStorage";
        assert_eq!(folders_to_list(&facts), [storage]);
        facts.listings.insert(
            storage.into(),
            [
                "OneDrive-Personal",
                "OneDrive-Contoso",
                "OneDrive-SharedLibraries-Contoso",
                "Dropbox",
                "GoogleDrive-mark@example.com",
                "GoogleDrive-work@example.com",
                "Box-Box",
                ".DS_Store",
            ]
            .map(String::from)
            .to_vec(),
        );
        exists(
            &mut facts,
            &[
                "/Users/mark/Library/CloudStorage/OneDrive-Personal",
                "/Users/mark/Library/CloudStorage/OneDrive-Contoso",
                "/Users/mark/Library/CloudStorage/OneDrive-SharedLibraries-Contoso",
                "/Users/mark/Library/CloudStorage/Dropbox",
                "/Users/mark/Library/CloudStorage/GoogleDrive-mark@example.com/My Drive",
                "/Users/mark/Library/CloudStorage/GoogleDrive-work@example.com/My Drive",
                "/Users/mark/Library/CloudStorage/Box-Box",
                "/Users/mark/Library/Mobile Documents/com~apple~CloudDocs",
            ],
        );
        assert_eq!(
            names(&synced_folders(&facts)),
            [
                (
                    "OneDrive - Contoso",
                    "/Users/mark/Library/CloudStorage/OneDrive-Contoso"
                ),
                (
                    "OneDrive - Personal",
                    "/Users/mark/Library/CloudStorage/OneDrive-Personal"
                ),
                ("Dropbox", "/Users/mark/Library/CloudStorage/Dropbox"),
                (
                    "Google Drive (mark@example.com)",
                    "/Users/mark/Library/CloudStorage/GoogleDrive-mark@example.com/My Drive"
                ),
                (
                    "Google Drive (work@example.com)",
                    "/Users/mark/Library/CloudStorage/GoogleDrive-work@example.com/My Drive"
                ),
                (
                    "iCloud Drive",
                    "/Users/mark/Library/Mobile Documents/com~apple~CloudDocs"
                ),
            ]
        );
    }

    #[test]
    fn older_mac_services_and_linux_dropbox_are_found_too() {
        let mut mac = ComputerFacts::new(System::Mac, "/Users/mark");
        mac.volumes.push(("/Volumes/GoogleDrive".into(), "GoogleDrive".into()));
        mac.files.insert(
            "/Users/mark/.dropbox/info.json".into(),
            r#"{"personal": {"path": "/Users/mark/Dropbox"}}"#.into(),
        );
        exists(
            &mut mac,
            &[
                "/Volumes/GoogleDrive/My Drive",
                "/Users/mark/Dropbox",
                "/Users/mark/OneDrive",
            ],
        );
        assert_eq!(
            names(&synced_folders(&mac)),
            [
                ("OneDrive", "/Users/mark/OneDrive"),
                ("Dropbox", "/Users/mark/Dropbox"),
                ("Google Drive", "/Volumes/GoogleDrive/My Drive")
            ]
        );

        let mut linux = ComputerFacts::new(System::Other, "/home/mark");
        exists(&mut linux, &["/home/mark/Dropbox"]);
        assert_eq!(names(&synced_folders(&linux)), [("Dropbox", "/home/mark/Dropbox")]);
        // Case tells two folders apart on Linux.
        let mut other = ComputerFacts::new(System::Other, "/home/mark");
        exists(&mut other, &["/home/mark/dropbox"]);
        assert!(synced_folders(&other).is_empty());
    }

    #[test]
    fn a_computer_with_no_cloud_service_offers_nothing() {
        assert!(synced_folders(&windows()).is_empty());
        assert!(synced_folders(&ComputerFacts::new(System::Mac, "/Users/mark")).is_empty());
    }

    #[test]
    fn the_service_is_known_from_a_folder_inside_it() {
        let cases = [
            (r"C:\Users\Mark\OneDrive\Pictures", Some(Service::OneDrive)),
            (r"C:\Users\Mark\OneDrive - Contoso", Some(Service::OneDrive)),
            (
                "/Users/mark/Library/CloudStorage/OneDrive-Personal/Photos",
                Some(Service::OneDrive),
            ),
            ("/Users/mark/Dropbox (Acme)/Camera Uploads", Some(Service::Dropbox)),
            (
                "/Users/mark/Library/CloudStorage/GoogleDrive-a@b.c/My Drive/x",
                Some(Service::GoogleDrive),
            ),
            (
                "/Users/mark/Library/Mobile Documents/com~apple~CloudDocs",
                Some(Service::ICloud),
            ),
            ("/home/mark/Pictures", None),
        ];
        for (path, service) in cases {
            // A Windows path read on another system is one part; the
            // separators are only a Windows computer's own.
            if path.contains('\\') && !cfg!(windows) {
                continue;
            }
            assert_eq!(service_of(Path::new(path)), service, "{path}");
        }
    }
}
