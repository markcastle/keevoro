//! Where a backup goes: remembering each destination, and its keys.
//!
//! Adding a destination checks that it works, then remembers it in two
//! places chosen for what they hold:
//!
//! - **The keys go in the computer's own keychain** (Credential Manager on
//!   Windows, Keychain on macOS). They are never written to the backup, a
//!   settings file or a log, and other users of the computer cannot read
//!   them.
//! - **Where to back up to goes in the backup's catalog.** That is not a
//!   secret, and it belongs with the backup.
//!
//! Keys given when a destination is added are the ones tested and saved.
//! Keys set in the environment (`KEEVORO_ACCESS_KEY_ID` and
//! `KEEVORO_SECRET_ACCESS_KEY`, which scripts use) are only a fallback:
//! used when none are given, or none are saved for a destination, and said
//! in words whenever they are, never silently.

use crate::send::{Destination, ACCESS_KEY_VAR, SECRET_KEY_VAR};
use anyhow::{bail, Result};
use keevoro_catalog::Catalog;
use keevoro_storage_s3::Preset;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;

/// The catalog setting that holds the saved destination.
const SETTING: &str = "destination";
/// The name Keevoro's entries appear under in the system keychain.
const KEYCHAIN_SERVICE: &str = "Keevoro storage keys";

/// Somewhere to keep secrets. The real one is the system keychain; tests
/// use one that lives in memory.
pub trait SecretStore {
    fn get(&self, name: &str) -> Result<Option<String>>;
    fn set(&self, name: &str, value: &str) -> Result<()>;
    fn delete(&self, name: &str) -> Result<()>;
}

/// The operating system's keychain.
pub struct Keychain;

impl Keychain {
    fn entry(name: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(KEYCHAIN_SERVICE, name)
            .map_err(|e| anyhow::anyhow!("Couldn't reach your computer's keychain ({e})."))
    }
}

impl SecretStore for Keychain {
    fn get(&self, name: &str) -> Result<Option<String>> {
        match Self::entry(name)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => bail!("Couldn't read the storage keys from your computer's keychain ({e})."),
        }
    }

    fn set(&self, name: &str, value: &str) -> Result<()> {
        Self::entry(name)?.set_password(value).map_err(|e| {
            anyhow::anyhow!(
                "Couldn't save the storage keys in your computer's keychain ({e}). \
                 You can set {ACCESS_KEY_VAR} and {SECRET_KEY_VAR} instead."
            )
        })
    }

    fn delete(&self, name: &str) -> Result<()> {
        match Self::entry(name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => bail!("Couldn't remove the storage keys from your computer's keychain ({e})."),
        }
    }
}

/// The outside world, as far as this module needs it: where secrets are
/// kept, and the environment keys may be set in for scripts.
pub struct World<'a> {
    pub secrets: &'a dyn SecretStore,
    /// Reads one variable of the environment. The running program reads
    /// its own ([`the_environment`]); tests give one of their own.
    pub environment: &'a dyn Fn(&str) -> Option<String>,
}

/// The running program's own environment.
pub fn the_environment(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// A pair of storage keys on their way to being tested and saved. It
/// cannot be shown: its `Debug` form is `[hidden]`.
#[derive(Clone, PartialEq, Eq)]
pub struct Keys {
    access: String,
    secret: String,
}

impl Keys {
    /// Both keys, trimmed. `None` when either is empty.
    pub fn new(access: &str, secret: &str) -> Option<Self> {
        let (access, secret) = (access.trim(), secret.trim());
        (!access.is_empty() && !secret.is_empty()).then(|| Self {
            access: access.to_string(),
            secret: secret.to_string(),
        })
    }

    /// The two keys, for taking them out of anything that might repeat them.
    pub fn both(&self) -> [&str; 2] {
        [&self.access, &self.secret]
    }

    pub(crate) fn access(&self) -> &str {
        &self.access
    }

    pub(crate) fn secret(&self) -> &str {
        &self.secret
    }

    fn from_environment(environment: &dyn Fn(&str) -> Option<String>) -> Option<Self> {
        Self::new(&environment(ACCESS_KEY_VAR)?, &environment(SECRET_KEY_VAR)?)
    }
}

impl std::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[hidden]")
    }
}

/// Where the keys used for a destination came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeysFrom {
    /// A folder needs none.
    NotNeeded,
    /// Given when it was added: by the app, the command line or an agent.
    Given,
    /// Saved in the computer's keychain for this destination.
    Keychain,
    /// Set in the environment, because none were given or saved.
    Environment,
}

/// The one rule for which keys are used: those given or saved for the
/// destination itself, and only when there are none, the environment's.
pub fn keys_to_use(
    own: Option<Keys>,
    own_from: KeysFrom,
    environment: &dyn Fn(&str) -> Option<String>,
) -> Option<(Keys, KeysFrom)> {
    match own {
        Some(keys) => Some((keys, own_from)),
        None => Keys::from_environment(environment).map(|keys| (keys, KeysFrom::Environment)),
    }
}

/// The place `saved` names, to be read before it is added anywhere (as
/// when a backup is opened from it): a folder as it is, cloud storage with
/// the keys given, or only when none are, the environment's.
pub fn to_read(
    saved: &Saved,
    given: Option<Keys>,
    environment: &dyn Fn(&str) -> Option<String>,
) -> Result<Destination> {
    let Some(preset) = saved.preset() else {
        return saved.as_folder();
    };
    let Some((keys, _)) = keys_to_use(given, KeysFrom::Given, environment) else {
        bail!("Enter both keys for the storage.");
    };
    Ok(Destination::Bucket {
        preset,
        bucket: saved.bucket.clone().unwrap_or_default(),
        access_key_id: keys.access,
        secret_access_key: keys.secret,
    })
}

/// What is said when a destination is added with the environment's keys.
/// It names the variables, never the keys.
pub fn environment_keys_saved(saved: &Saved) -> String {
    format!(
        "No keys were given, so Keevoro used the ones set in {ACCESS_KEY_VAR} and {SECRET_KEY_VAR}, \
         and saved them in this computer's keychain for {}.",
        saved.describe()
    )
}

/// What is said when a destination is opened with the environment's keys.
pub fn environment_keys_used(saved: &Saved) -> String {
    format!(
        "Using the storage keys set in {ACCESS_KEY_VAR} and {SECRET_KEY_VAR} for {}, \
         as none are saved in this computer's keychain for it.",
        saved.describe()
    )
}

/// A backup destination as it is remembered. Holds nothing secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Saved {
    /// `folder`, `r2`, `s3`, `gcs` or `endpoint`.
    pub kind: String,
    /// The folder, Cloudflare account ID, Amazon region or server address;
    /// for Google Cloud Storage, always `storage.googleapis.com`.
    pub place: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// The name this destination's records are kept under in the catalog,
    /// learnt the first time it is opened. It is remembered here so that
    /// what has reached a destination can be counted while it is unplugged
    /// or offline. `None` for a destination saved before this was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// For a folder: which disk it was on when it was added and when it was
    /// last reached ([`crate::reach::volume_of`]), so an empty folder left
    /// behind on another disk by a drive unplugged is not taken for it.
    /// `None` for a folder saved before this was kept, until it is next
    /// reached, and for cloud storage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<String>,
}

/// A destination as it is asked for: one of the places, and its bucket.
#[derive(Debug, Default, Clone)]
pub struct Flags {
    pub to_folder: Option<PathBuf>,
    pub to_r2: Option<String>,
    pub to_s3: Option<String>,
    /// Google Cloud Storage, which needs no place: only the bucket.
    pub to_gcs: bool,
    pub to_endpoint: Option<String>,
    pub bucket: Option<String>,
    pub base_url: Option<String>,
}

impl Saved {
    /// The destination the flags describe, or `None` when none was given.
    pub fn from_flags(flags: &Flags) -> Option<Self> {
        let (kind, place) = if let Some(folder) = &flags.to_folder {
            ("folder", folder.to_string_lossy().to_string())
        } else if let Some(account) = &flags.to_r2 {
            ("r2", account.trim().to_string())
        } else if let Some(region) = &flags.to_s3 {
            ("s3", region.trim().to_string())
        } else if flags.to_gcs {
            ("gcs", keevoro_core::bucket::GOOGLE_PLACE.to_string())
        } else {
            ("endpoint", flags.to_endpoint.as_ref()?.trim().to_string())
        };
        Some(Self {
            kind: kind.to_string(),
            place,
            bucket: flags.bucket.clone().filter(|_| kind != "folder"),
            base_url: flags.base_url.clone(),
            target: None,
            volume: None,
        })
    }

    /// The one destination a backup saved before it could have several.
    pub fn load(catalog: &Catalog) -> Result<Option<Self>> {
        match catalog.setting(SETTING)? {
            // `null` is what disconnecting leaves behind.
            Some(json) => serde_json::from_str::<Option<Self>>(&json).map_err(|_| {
                anyhow::anyhow!("The saved backup destination can't be read. Choose where to back up to again.")
            }),
            None => Ok(None),
        }
    }

    /// Is this the same place as `other`, whatever else is remembered about it?
    pub fn same_place(&self, other: &Self) -> bool {
        (&self.kind, &self.place, &self.bucket) == (&other.kind, &other.place, &other.bucket)
    }

    /// How to say it to the user.
    pub fn describe(&self) -> String {
        let bucket = self.bucket.as_deref().unwrap_or_default();
        match self.kind.as_str() {
            "folder" => format!("the folder {}", self.place),
            "r2" => format!("the Cloudflare R2 bucket {bucket}"),
            "s3" => format!("the Amazon S3 bucket {bucket} ({})", self.place),
            "gcs" => format!("the Google Cloud Storage bucket {bucket}"),
            _ => format!("the bucket {bucket} at {}", self.place),
        }
    }

    pub(crate) fn preset(&self) -> Option<Preset> {
        match self.kind.as_str() {
            "r2" => Some(Preset::CloudflareR2 {
                account_id: self.place.clone(),
            }),
            "s3" => Some(Preset::AmazonS3 {
                region: self.place.clone(),
            }),
            "gcs" => Some(Preset::GoogleCloud),
            "endpoint" => Some(Preset::Custom {
                endpoint: self.place.clone(),
                region: None,
                path_style: true,
            }),
            _ => None,
        }
    }

    /// A destination that is not cloud storage Keevoro knows, as the folder
    /// it is. A kind a later Keevoro added is refused, never taken for a
    /// folder named after its place.
    fn as_folder(&self) -> Result<Destination> {
        if self.kind != "folder" {
            bail!(
                "{} was added by a newer version of Keevoro, which knows a kind of storage this one doesn't. \
                 Update Keevoro to back up there.",
                capitalised(&self.describe())
            );
        }
        Ok(Destination::Folder(PathBuf::from(&self.place)))
    }

    /// The keychain entry for this destination's keys: one per bucket, so
    /// two libraries backing up to the same bucket share it.
    fn secret_name(&self) -> String {
        format!(
            "{}:{}/{}",
            self.kind,
            self.place,
            self.bucket.as_deref().unwrap_or_default()
        )
    }

    /// Turns the remembered destination into one that can be opened, with
    /// the keys saved for it in the keychain or, only when there are none,
    /// those set in the environment, which is then said to `out`.
    pub fn open(&self, out: &mut dyn Write, world: &World<'_>) -> Result<Destination> {
        let Some(preset) = self.preset() else {
            return self.as_folder();
        };
        let saved = world.secrets.get(&self.secret_name())?.as_deref().and_then(split_keys);
        let Some((keys, from)) = keys_to_use(saved, KeysFrom::Keychain, world.environment) else {
            bail!(
                "Keevoro needs the storage keys for {}. Add it as a destination again to save them, \
                 or set {ACCESS_KEY_VAR} and {SECRET_KEY_VAR}.",
                self.describe()
            )
        };
        if from == KeysFrom::Environment {
            writeln!(out, "{}", environment_keys_used(self))?;
        }
        Ok(Destination::Bucket {
            preset,
            bucket: self.bucket.clone().unwrap_or_default(),
            access_key_id: keys.access,
            secret_access_key: keys.secret,
        })
    }
}

/// The words with their first letter a capital, for the start of a sentence.
fn capitalised(words: &str) -> String {
    let mut letters = words.chars();
    letters
        .next()
        .map(|first| first.to_uppercase().chain(letters).collect())
        .unwrap_or_default()
}

/// The two keys are kept as one keychain entry, one per line.
pub(crate) fn join_keys(access: &str, secret: &str) -> String {
    format!("{access}\n{secret}")
}

pub(crate) fn split_keys(stored: &str) -> Option<Keys> {
    let (access, secret) = stored.split_once('\n')?;
    Keys::new(access, secret)
}

/// The catalog's name for the list of a backup's destinations.
const LIST: &str = "destinations";

/// Every destination this backup goes to, in the order they were added.
/// A backup saved when there could only be one still has its one.
pub fn destinations(catalog: &Catalog) -> Result<Vec<Saved>> {
    match catalog.setting(LIST)? {
        Some(json) => serde_json::from_str(&json).map_err(|_| {
            anyhow::anyhow!("The saved backup destinations can't be read. Choose where to back up to again.")
        }),
        None => Ok(Saved::load(catalog)?.into_iter().collect()),
    }
}

/// Saves the list. The first is also kept where a single destination used
/// to be, for anything that still asks for "the" destination.
fn remember(catalog: &Catalog, all: &[Saved]) -> Result<()> {
    catalog.set_setting(LIST, &serde_json::to_string(all)?)?;
    catalog.set_setting(SETTING, &serde_json::to_string(&all.first())?)?;
    Ok(())
}

/// Notes the name a destination's records are kept under, once it has
/// been opened, for one saved before that was remembered.
pub fn note_target(catalog: &Catalog, saved: &Saved, target: &str) -> Result<()> {
    let mut all = destinations(catalog)?;
    if let Some(found) = all
        .iter_mut()
        .find(|d| d.same_place(saved) && d.target.as_deref() != Some(target))
    {
        found.target = Some(target.to_string());
        remember(catalog, &all)?;
    }
    Ok(())
}

/// Notes which disk a folder destination is on, once it has been reached
/// there, for one saved before that was remembered or whose drive is now
/// known by another number.
pub fn note_volume(catalog: &Catalog, saved: &Saved, volume: &str) -> Result<()> {
    let mut all = destinations(catalog)?;
    if let Some(found) = all
        .iter_mut()
        .find(|d| d.same_place(saved) && d.volume.as_deref() != Some(volume))
    {
        found.volume = Some(volume.to_string());
        remember(catalog, &all)?;
    }
    Ok(())
}

/// A destination that could not be reached when it was being added, so
/// that it can be told apart from anything else that went wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CannotReach(pub String);

impl std::fmt::Display for CannotReach {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CannotReach {}

/// Checks a destination works, then adds it to those this backup goes to.
/// The ones already there are kept. Adding one that is already there only
/// refreshes its keys.
///
/// Cloud storage is tested, and saved, with the keys `given`. Only when
/// none are given are the environment's used, and then that is said to
/// `out`. Answers where the keys came from.
pub fn add(
    out: &mut dyn Write,
    catalog: &Catalog,
    saved: &Saved,
    given: Option<Keys>,
    world: &World<'_>,
) -> Result<KeysFrom> {
    prove_and_remember(out, catalog, saved, given, world)
}

/// Takes one destination off the list, by its place in it, and removes its
/// keys from the keychain unless another on the list uses the same ones.
/// Nothing in the storage itself is touched.
pub fn remove(catalog: &Catalog, secrets: &dyn SecretStore, at: usize) -> Result<Option<Saved>> {
    let mut all = destinations(catalog)?;
    if at >= all.len() {
        return Ok(None);
    }
    let gone = all.remove(at);
    if gone.preset().is_some() && !all.iter().any(|d| d.secret_name() == gone.secret_name()) {
        secrets.delete(&gone.secret_name())?;
    }
    remember(catalog, &all)?;
    Ok(Some(gone))
}

fn prove_and_remember(
    out: &mut dyn Write,
    catalog: &Catalog,
    saved: &Saved,
    given: Option<Keys>,
    world: &World<'_>,
) -> Result<KeysFrom> {
    let (destination, keys) = match saved.preset() {
        None => (saved.as_folder()?, None),
        Some(preset) => {
            let Some((keys, from)) = keys_to_use(given, KeysFrom::Given, world.environment) else {
                bail!("Enter both keys for the storage.");
            };
            let destination = Destination::Bucket {
                preset,
                bucket: saved.bucket.clone().unwrap_or_default(),
                access_key_id: keys.access.clone(),
                secret_access_key: keys.secret.clone(),
            };
            (destination, Some((keys, from)))
        }
    };

    // Prove it works before remembering anything. Whatever went wrong has
    // the keys taken out of its words, in case the storage repeated them.
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let opened = crate::send::open_or_make(out, &runtime, destination).map_err(|e| {
        let mut said = format!("{e:#}");
        for key in keys.iter().flat_map(|(keys, _)| keys.both()) {
            said = said.replace(key, "[hidden]");
        }
        CannotReach(said)
    })?;

    let (_, target, _) = opened;
    let saved = &Saved {
        target: Some(target),
        // The disk it is on now, as it is added: the one it is known by.
        volume: match saved.preset() {
            None => crate::reach::volume_of(std::path::Path::new(&saved.place)),
            Some(_) => None,
        },
        ..saved.clone()
    };
    if let Some((keys, _)) = &keys {
        world
            .secrets
            .set(&saved.secret_name(), &join_keys(&keys.access, &keys.secret))?;
    }
    let mut all = destinations(catalog)?;
    match all.iter_mut().find(|d| d.same_place(saved)) {
        Some(already) => *already = saved.clone(),
        None => all.push(saved.clone()),
    }
    remember(catalog, &all)?;
    writeln!(out, "Connected to {}.", saved.describe())?;
    let from = keys.map_or(KeysFrom::NotNeeded, |(_, from)| from);
    match from {
        KeysFrom::Environment => writeln!(out, "{}", environment_keys_saved(saved))?,
        KeysFrom::NotNeeded => {}
        _ => writeln!(
            out,
            "The keys are saved in this computer's keychain, not with the backup."
        )?,
    }
    Ok(from)
}

/// The keychain entries this backup's destinations and bucket sources use,
/// each with the
/// destination in words. A folder needs none. Two destinations on one
/// bucket share an entry, which is given once.
pub fn keys_kept_for(catalog: &Catalog) -> Result<Vec<(String, String)>> {
    let mut kept: Vec<(String, String)> = Vec::new();
    for saved in destinations(catalog)? {
        let name = saved.secret_name();
        if saved.preset().is_some() && !kept.iter().any(|(n, _)| *n == name) {
            kept.push((name, saved.describe()));
        }
    }
    // Cloud storage it reads from has keys of its own.
    for source in catalog.sources()? {
        if let Some(place) = crate::bucket::place_of(source.bucket.as_deref()) {
            kept.push((place.secret_name(), place.describe()));
        }
    }
    Ok(kept)
}

/// Removes these entries from the keychain. Nothing in the storage itself
/// is touched.
pub fn forget_keys(secrets: &dyn SecretStore, names: &[String]) -> Result<()> {
    for name in names {
        secrets.delete(name)?;
    }
    Ok(())
}

/// Forgets the saved destination and removes its keys from the keychain.
/// Nothing in the storage itself is touched.
pub fn disconnect(out: &mut dyn Write, catalog: &Catalog, secrets: &dyn SecretStore) -> Result<()> {
    let all = destinations(catalog)?;
    let Some(saved) = all.first() else {
        writeln!(out, "This backup doesn't go anywhere yet.")?;
        return Ok(());
    };
    for each in &all {
        if each.preset().is_some() {
            secrets.delete(&each.secret_name())?;
        }
    }
    remember(catalog, &[])?;
    writeln!(
        out,
        "Disconnected from {}. Nothing that was backed up there was changed.",
        saved.describe()
    )?;
    Ok(())
}

/// Stand-ins for the keychain and the keyboard, for tests here and in the
/// front ends. Not used by the running program.
pub mod testing {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    /// A keychain that lives in memory.
    #[derive(Default)]
    pub struct MemorySecrets(pub RefCell<BTreeMap<String, String>>);

    impl SecretStore for MemorySecrets {
        fn get(&self, name: &str) -> Result<Option<String>> {
            Ok(self.0.borrow().get(name).cloned())
        }
        fn set(&self, name: &str, value: &str) -> Result<()> {
            self.0.borrow_mut().insert(name.to_string(), value.to_string());
            Ok(())
        }
        fn delete(&self, name: &str) -> Result<()> {
            self.0.borrow_mut().remove(name);
            Ok(())
        }
    }

    /// An environment with nothing set in it.
    pub fn no_environment(_name: &str) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    fn flags() -> Flags {
        Flags::default()
    }

    #[test]
    fn flags_become_a_destination_that_holds_no_secrets() {
        assert_eq!(Saved::from_flags(&flags()), None);
        let r2 = Saved::from_flags(&Flags {
            to_r2: Some(" 0123456789abcdef0123456789abcdef ".into()),
            bucket: Some("family-photos".into()),
            base_url: Some("https://photos.example.com".into()),
            ..flags()
        })
        .unwrap();
        assert_eq!((r2.kind.as_str(), r2.place.len()), ("r2", 32));
        assert_eq!(r2.describe(), "the Cloudflare R2 bucket family-photos");
        let json = serde_json::to_string(&r2).unwrap();
        assert!(
            !json.to_lowercase().contains("secret") && !json.contains("key"),
            "{json}"
        );
        assert_eq!(serde_json::from_str::<Saved>(&json).unwrap(), r2);

        let folder = Saved::from_flags(&Flags {
            to_folder: Some("F:/Family Backup".into()),
            bucket: Some("ignored".into()),
            ..flags()
        })
        .unwrap();
        assert_eq!((folder.kind.as_str(), folder.bucket.clone()), ("folder", None));
        assert_eq!(folder.describe(), "the folder F:/Family Backup");

        let s3 = Saved::from_flags(&Flags {
            to_s3: Some("eu-west-2".into()),
            bucket: Some("b".into()),
            ..flags()
        })
        .unwrap();
        assert_eq!(s3.describe(), "the Amazon S3 bucket b (eu-west-2)");
        let nas = Saved::from_flags(&Flags {
            to_endpoint: Some("https://nas.local:9000".into()),
            bucket: Some("b".into()),
            ..flags()
        })
        .unwrap();
        assert_eq!(nas.describe(), "the bucket b at https://nas.local:9000");
        assert_ne!(s3.secret_name(), nas.secret_name());

        let google = Saved::from_flags(&Flags {
            to_gcs: true,
            bucket: Some("family-archive".into()),
            ..flags()
        })
        .unwrap();
        assert_eq!(
            (google.kind.as_str(), google.place.as_str()),
            ("gcs", "storage.googleapis.com")
        );
        assert_eq!(google.describe(), "the Google Cloud Storage bucket family-archive");
        assert_eq!(google.preset(), Some(Preset::GoogleCloud));
        assert_eq!(google.secret_name(), "gcs:storage.googleapis.com/family-archive");
    }

    #[test]
    fn a_kind_of_storage_from_a_newer_keevoro_is_refused_never_taken_for_a_folder() {
        let later = Saved {
            kind: "tape".into(),
            place: "storage.example.com".into(),
            bucket: Some("family".into()),
            base_url: None,
            target: None,
            volume: None,
        };
        let world = World {
            secrets: &MemorySecrets::default(),
            environment: &no_environment,
        };
        let Err(refused) = later.open(&mut Vec::new(), &world) else {
            panic!("a kind this version doesn't know was opened");
        };
        let refused = refused.to_string();
        assert!(refused.contains("newer version of Keevoro"), "{refused}");
        assert!(
            refused.starts_with("The bucket family at storage.example.com"),
            "{refused}"
        );
    }

    #[test]
    fn keys_are_one_keychain_entry_and_come_back_whole() {
        let stored = join_keys("AKIAEXAMPLE", "s3cr3t/with+odd=chars");
        assert_eq!(split_keys(&stored), Keys::new("AKIAEXAMPLE", "s3cr3t/with+odd=chars"));
        for broken in ["", "only-one-line", "\nsecret", "access\n"] {
            assert_eq!(split_keys(broken), None, "{broken:?}");
        }
    }

    const ACCESS: &str = "AKIAENGINEMARKER01";
    const SECRET: &str = "engine-secret-marker/02";
    const ENV_ACCESS: &str = "AKIAENVIRONMENTMARKER03";
    const ENV_SECRET: &str = "environment-secret-marker/04";

    fn keys_in_the_environment(name: &str) -> Option<String> {
        match name {
            ACCESS_KEY_VAR => Some(format!(" {ENV_ACCESS} ")),
            SECRET_KEY_VAR => Some(ENV_SECRET.to_string()),
            _ => None,
        }
    }

    fn bucket() -> Saved {
        Saved::from_flags(&Flags {
            to_endpoint: Some("http://127.0.0.1:9".into()),
            bucket: Some("family".into()),
            ..flags()
        })
        .unwrap()
    }

    #[test]
    fn keys_given_win_and_the_environment_is_only_a_fallback() {
        let given = Keys::new(ACCESS, SECRET);
        let env = &keys_in_the_environment;
        assert_eq!(
            keys_to_use(given.clone(), KeysFrom::Given, env),
            given.clone().map(|k| (k, KeysFrom::Given))
        );
        assert_eq!(
            keys_to_use(given.clone(), KeysFrom::Keychain, env),
            given.map(|k| (k, KeysFrom::Keychain))
        );
        assert_eq!(
            keys_to_use(None, KeysFrom::Given, env),
            Keys::new(ENV_ACCESS, ENV_SECRET).map(|k| (k, KeysFrom::Environment))
        );
        assert_eq!(keys_to_use(None, KeysFrom::Given, &no_environment), None);
        // Half a pair in the environment is no pair at all.
        let half = |name: &str| (name == ACCESS_KEY_VAR).then(|| "AKIA".to_string());
        assert_eq!(keys_to_use(None, KeysFrom::Given, &half), None);
        assert_eq!(Keys::new(" ", "x"), None);
        assert_eq!(format!("{:?}", Keys::new(ACCESS, SECRET)), "Some([hidden])");
    }

    #[test]
    fn opening_uses_the_keys_saved_for_it_before_the_environments_and_says_when_it_does_not() {
        let secrets = MemorySecrets::default();
        let world = World {
            secrets: &secrets,
            environment: &keys_in_the_environment,
        };
        let saved = bucket();
        let mut said = Vec::new();
        let Destination::Bucket { access_key_id, .. } = saved.open(&mut said, &world).unwrap() else {
            panic!("a bucket")
        };
        assert_eq!(access_key_id, ENV_ACCESS);
        let said = String::from_utf8(said).unwrap();
        assert_eq!(said.trim(), environment_keys_used(&saved));
        assert!(said.contains(ACCESS_KEY_VAR) && !said.contains(ENV_ACCESS) && !said.contains(ENV_SECRET));

        secrets.set(&saved.secret_name(), &join_keys(ACCESS, SECRET)).unwrap();
        let mut said = Vec::new();
        let Destination::Bucket {
            access_key_id,
            secret_access_key,
            ..
        } = saved.open(&mut said, &world).unwrap()
        else {
            panic!("a bucket")
        };
        assert_eq!((access_key_id.as_str(), secret_access_key.as_str()), (ACCESS, SECRET));
        assert!(said.is_empty(), "nothing to say when its own keys are used");

        let nothing = World {
            secrets: &MemorySecrets::default(),
            environment: &no_environment,
        };
        let Err(e) = saved.open(&mut Vec::new(), &nothing) else {
            panic!("no keys anywhere")
        };
        assert!(e.to_string().contains("needs the storage keys"), "{e}");
    }

    #[test]
    fn adding_cloud_storage_tests_the_keys_given_or_says_it_used_the_environments() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = Catalog::open(&dir.path().join("c.sqlite")).unwrap();
        let secrets = MemorySecrets::default();
        let world = World {
            secrets: &secrets,
            environment: &keys_in_the_environment,
        };
        // Nothing listens there, so it is refused, and nothing is saved.
        for given in [Keys::new(ACCESS, SECRET), None] {
            let e = add(&mut Vec::new(), &catalog, &bucket(), given, &world).unwrap_err();
            let said = format!("{e} {e:#} {e:?}");
            for key in [ACCESS, SECRET, ENV_ACCESS, ENV_SECRET] {
                assert!(!said.contains(key), "{said}");
            }
            assert!(e.downcast_ref::<CannotReach>().is_some(), "{said}");
        }
        assert!(secrets.0.borrow().is_empty());
        assert!(destinations(&catalog).unwrap().is_empty());
        // With no keys given and none in the environment, it says so.
        let nothing = World {
            secrets: &secrets,
            environment: &no_environment,
        };
        let e = add(&mut Vec::new(), &catalog, &bucket(), None, &nothing).unwrap_err();
        assert_eq!(e.to_string(), "Enter both keys for the storage.");
        assert!(environment_keys_saved(&bucket()).contains(ACCESS_KEY_VAR));
    }

    /// Against a real server, where one is set up (CI starts MinIO): the
    /// keys given are what is tested and saved, even with other keys in the
    /// environment, and the environment's are saved, and said, only when
    /// none are given.
    #[test]
    fn against_a_real_server_the_keys_given_are_the_ones_saved() {
        let var = |name: &str| std::env::var(format!("KEEVORO_S3_TEST_{name}")).ok();
        let (Some(endpoint), Some(bucket), Some(access), Some(secret)) =
            (var("ENDPOINT"), var("BUCKET"), var("ACCESS_KEY"), var("SECRET_KEY"))
        else {
            eprintln!("skipped: no test server set up");
            return;
        };
        let to = Saved::from_flags(&Flags {
            to_endpoint: Some(endpoint),
            bucket: Some(bucket),
            ..flags()
        })
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let catalog = Catalog::open(&dir.path().join("c.sqlite")).unwrap();
        let secrets = MemorySecrets::default();
        let wrong = |name: &str| (name == ACCESS_KEY_VAR || name == SECRET_KEY_VAR).then(|| "wrong".to_string());
        let world = World {
            secrets: &secrets,
            environment: &wrong,
        };
        let mut said = Vec::new();
        let from = add(&mut said, &catalog, &to, Keys::new(&access, &secret), &world).unwrap();
        assert_eq!(from, KeysFrom::Given);
        assert_eq!(
            secrets.get(&to.secret_name()).unwrap(),
            Some(join_keys(&access, &secret))
        );
        let said = String::from_utf8(said).unwrap();
        assert!(!said.contains(ACCESS_KEY_VAR) && !said.contains(&secret), "{said}");

        secrets.delete(&to.secret_name()).unwrap();
        let right = |name: &str| match name {
            ACCESS_KEY_VAR => Some(access.clone()),
            SECRET_KEY_VAR => Some(secret.clone()),
            _ => None,
        };
        let world = World {
            secrets: &secrets,
            environment: &right,
        };
        let mut said = Vec::new();
        assert_eq!(
            add(&mut said, &catalog, &to, None, &world).unwrap(),
            KeysFrom::Environment
        );
        let said = String::from_utf8(said).unwrap();
        assert!(said.contains(&environment_keys_saved(&to)), "{said}");
        assert!(!said.contains(&secret), "{said}");
        assert_eq!(
            secrets.get(&to.secret_name()).unwrap(),
            Some(join_keys(&access, &secret))
        );
    }

    #[test]
    fn a_folder_destination_needs_no_keys_at_all() {
        let secrets = MemorySecrets::default();
        let world = World {
            secrets: &secrets,
            environment: &no_environment,
        };
        let saved = Saved::from_flags(&Flags {
            to_folder: Some("backup".into()),
            ..flags()
        })
        .unwrap();
        assert!(matches!(
            saved.open(&mut Vec::new(), &world).unwrap(),
            Destination::Folder(_)
        ));
        assert!(secrets.0.borrow().is_empty());
    }

    #[test]
    fn the_real_keychain_reports_a_missing_entry_as_missing() {
        // Whatever this machine's keychain is, asking for something that
        // was never saved must be "nothing there" or a plain error, never a
        // crash; and removing it must be harmless.
        let name = format!("keevoro-test-entry-{}", std::process::id());
        match Keychain.get(&name) {
            Ok(found) => assert_eq!(found, None),
            Err(e) => assert!(e.to_string().contains("keychain"), "{e}"),
        }
        if let Err(e) = Keychain.delete(&name) {
            assert!(e.to_string().contains("keychain"), "{e}");
        }
    }

    #[test]
    fn the_real_keychain_keeps_and_gives_back_a_secret_where_there_is_one() {
        let name = format!("keevoro-test-roundtrip-{}", std::process::id());
        if let Err(e) = Keychain.set(&name, &join_keys("access", "secret")) {
            // A build machine with no keychain service. The message must
            // still point at the way round it.
            assert!(e.to_string().contains(ACCESS_KEY_VAR), "{e}");
            eprintln!("skipped: no usable keychain here ({e})");
            return;
        }
        assert_eq!(
            Keychain.get(&name).unwrap().as_deref().and_then(split_keys),
            Keys::new("access", "secret")
        );
        Keychain.delete(&name).unwrap();
        assert_eq!(Keychain.get(&name).unwrap(), None);
    }
}
