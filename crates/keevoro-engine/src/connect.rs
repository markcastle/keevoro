//! `keevoro connect`: set up a backup destination once.
//!
//! Without this, backing up to a bucket means remembering an account ID, a
//! bucket name and two keys every time. `connect` checks that they work,
//! then remembers them in two places chosen for what they hold:
//!
//! - **The keys go in the computer's own keychain** (Credential Manager on
//!   Windows, Keychain on macOS). They are never written to the library, a
//!   config file or a log, and other users of the computer cannot read
//!   them.
//! - **Where to back up to goes in the library's catalog.** That is not a
//!   secret, and it belongs with the library: a library on an external
//!   drive remembers its own destination.
//!
//! After that, `keevoro publish --library <folder>` is the whole command.
//! Keys set in the environment still win over the keychain, which is what
//! scripts and CI use.

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
/// kept, and how to ask the person at the keyboard for one.
pub struct World<'a> {
    pub secrets: &'a dyn SecretStore,
    /// Asks a question and returns the answer. The flag says whether the
    /// answer should be hidden as it is typed.
    pub ask: &'a dyn Fn(&str, bool) -> Result<String>,
}

/// A backup destination as it is remembered. Holds nothing secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Saved {
    /// `folder`, `r2`, `s3` or `endpoint`.
    pub kind: String,
    /// The folder, Cloudflare account ID, Amazon region or server address.
    pub place: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

/// The destination flags shared by `connect` and `publish`.
#[derive(Debug, Default, Clone)]
pub struct Flags {
    pub to_folder: Option<PathBuf>,
    pub to_r2: Option<String>,
    pub to_s3: Option<String>,
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
        } else {
            ("endpoint", flags.to_endpoint.as_ref()?.trim().to_string())
        };
        Some(Self {
            kind: kind.to_string(),
            place,
            bucket: flags.bucket.clone().filter(|_| kind != "folder"),
            base_url: flags.base_url.clone(),
        })
    }

    /// The destination this library was connected to, if any.
    pub fn load(catalog: &Catalog) -> Result<Option<Self>> {
        match catalog.setting(SETTING)? {
            // `null` is what disconnecting leaves behind.
            Some(json) => serde_json::from_str::<Option<Self>>(&json).map_err(|_| {
                anyhow::anyhow!("The saved backup destination can't be read. Run `keevoro connect` again.")
            }),
            None => Ok(None),
        }
    }

    /// How to say it to the user.
    pub fn describe(&self) -> String {
        let bucket = self.bucket.as_deref().unwrap_or_default();
        match self.kind.as_str() {
            "folder" => format!("the folder {}", self.place),
            "r2" => format!("the Cloudflare R2 bucket {bucket}"),
            "s3" => format!("the Amazon S3 bucket {bucket} ({})", self.place),
            _ => format!("the bucket {bucket} at {}", self.place),
        }
    }

    fn preset(&self) -> Option<Preset> {
        match self.kind.as_str() {
            "r2" => Some(Preset::CloudflareR2 {
                account_id: self.place.clone(),
            }),
            "s3" => Some(Preset::AmazonS3 {
                region: self.place.clone(),
            }),
            "endpoint" => Some(Preset::Custom {
                endpoint: self.place.clone(),
                region: None,
                path_style: true,
            }),
            _ => None,
        }
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

    /// Turns the remembered destination into one that can be opened,
    /// finding its keys in the environment or, failing that, the keychain.
    pub fn open(&self, world: &World<'_>) -> Result<Destination> {
        let Some(preset) = self.preset() else {
            return Ok(Destination::Folder(PathBuf::from(&self.place)));
        };
        let (access_key_id, secret_access_key) = match env_keys() {
            Some(keys) => keys,
            None => match world.secrets.get(&self.secret_name())?.as_deref().and_then(split_keys) {
                Some(keys) => keys,
                None => bail!(
                    "Keevoro needs the storage keys for {}. Run `keevoro connect` once to save them, \
                     or set {ACCESS_KEY_VAR} and {SECRET_KEY_VAR}.",
                    self.describe()
                ),
            },
        };
        Ok(Destination::Bucket {
            preset,
            bucket: self.bucket.clone().unwrap_or_default(),
            access_key_id,
            secret_access_key,
        })
    }
}

fn env_keys() -> Option<(String, String)> {
    let var = |name: &str| {
        std::env::var(name)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    Some((var(ACCESS_KEY_VAR)?, var(SECRET_KEY_VAR)?))
}

/// The two keys are kept as one keychain entry, one per line.
fn join_keys(access: &str, secret: &str) -> String {
    format!("{access}\n{secret}")
}

fn split_keys(stored: &str) -> Option<(String, String)> {
    let (access, secret) = stored.split_once('\n')?;
    (!access.is_empty() && !secret.is_empty()).then(|| (access.to_string(), secret.to_string()))
}

/// Checks a destination works, then remembers it and its keys.
pub fn connect(out: &mut dyn Write, catalog: &Catalog, saved: &Saved, world: &World<'_>) -> Result<()> {
    let destination = match saved.preset() {
        None => Destination::Folder(PathBuf::from(&saved.place)),
        Some(preset) => {
            // Keys already in the environment are used as they are;
            // otherwise the person is asked, and the secret is not echoed.
            let (access_key_id, secret_access_key) = match env_keys() {
                Some(keys) => keys,
                None => (
                    (world.ask)("Access key ID", false)?,
                    (world.ask)("Secret access key (it won't be shown)", true)?,
                ),
            };
            Destination::Bucket {
                preset,
                bucket: saved.bucket.clone().unwrap_or_default(),
                access_key_id,
                secret_access_key,
            }
        }
    };
    let keys = match &destination {
        Destination::Bucket {
            access_key_id,
            secret_access_key,
            ..
        } => Some(join_keys(access_key_id, secret_access_key)),
        Destination::Folder(_) => None,
    };

    // Prove it works before remembering anything.
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    crate::send::open(out, &runtime, destination)?;

    if let Some(keys) = keys {
        world.secrets.set(&saved.secret_name(), &keys)?;
    }
    catalog.set_setting(SETTING, &serde_json::to_string(saved)?)?;
    writeln!(out, "Connected to {}.", saved.describe())?;
    if saved.preset().is_some() {
        writeln!(
            out,
            "The keys are saved in this computer's keychain, not in the library."
        )?;
    }
    writeln!(
        out,
        "From now on, `keevoro publish --library <your library>` backs up there."
    )?;
    Ok(())
}

/// Forgets the saved destination and removes its keys from the keychain.
/// Nothing in the storage itself is touched.
pub fn disconnect(out: &mut dyn Write, catalog: &Catalog, world: &World<'_>) -> Result<()> {
    let Some(saved) = Saved::load(catalog)? else {
        writeln!(out, "This library isn't connected to a backup destination.")?;
        return Ok(());
    };
    if saved.preset().is_some() {
        world.secrets.delete(&saved.secret_name())?;
    }
    // An empty value is "no destination"; settings have no delete.
    catalog.set_setting(SETTING, "null")?;
    writeln!(
        out,
        "Disconnected from {}. Nothing stored there was changed.",
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

    /// Nobody is at the keyboard in a test.
    pub fn nobody_to_ask(_question: &str, _hidden: bool) -> Result<String> {
        bail!("Keevoro needs the storage keys. Set {ACCESS_KEY_VAR} and {SECRET_KEY_VAR}, or run this again and paste them when asked.")
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
    }

    #[test]
    fn keys_are_one_keychain_entry_and_come_back_whole() {
        let stored = join_keys("AKIAEXAMPLE", "s3cr3t/with+odd=chars");
        assert_eq!(
            split_keys(&stored),
            Some(("AKIAEXAMPLE".into(), "s3cr3t/with+odd=chars".into()))
        );
        for broken in ["", "only-one-line", "\nsecret", "access\n"] {
            assert_eq!(split_keys(broken), None, "{broken:?}");
        }
    }

    #[test]
    fn a_folder_destination_needs_no_keys_at_all() {
        let secrets = MemorySecrets::default();
        let world = World {
            secrets: &secrets,
            ask: &nobody_to_ask,
        };
        let saved = Saved::from_flags(&Flags {
            to_folder: Some("backup".into()),
            ..flags()
        })
        .unwrap();
        assert!(matches!(saved.open(&world).unwrap(), Destination::Folder(_)));
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
            Some(("access".into(), "secret".into()))
        );
        Keychain.delete(&name).unwrap();
        assert_eq!(Keychain.get(&name).unwrap(), None);
    }
}
