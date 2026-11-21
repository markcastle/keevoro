//! `sources.add_bucket`: cloud storage, a bucket or a folder in one, as a
//! source that is only ever read.
//!
//! Its keys come in as destinations' do, and are held to the same rules:
//! a [`Key`] that cannot be shown, tested (by listing, which writes
//! nothing) and saved in the computer's keychain, never in an answer, the
//! log or the backup's record, and taken out of the words of any error.
//! Only when none are given are those set in the environment used, and
//! then the answer says so, naming the variables.
//!
//! **AI agents.** Adding one is a change, so it needs changes allowed. An
//! agent's allowed folders cannot hold a bucket, so it may not look
//! through or back up from a bucket source: that waits for a way for a
//! person to name a bucket an agent may read (see `for_agents`).

use crate::backup::Backup;
use crate::destinations::{without_keys, Key};
use crate::error::{Error, Result};
use keevoro_core::bucket::BucketPlace;
use keevoro_engine::connect::{Keys, KeysFrom, SecretStore, World};
use serde::{Deserialize, Serialize};

/// What kind of cloud storage a bucket source is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BucketKind {
    /// Cloudflare R2; the place is the account ID.
    R2,
    /// Amazon S3; the place is the region.
    S3,
    /// Any other S3-compatible storage; the place is its address.
    Endpoint,
}

impl BucketKind {
    fn word(self) -> &'static str {
        match self {
            Self::R2 => "r2",
            Self::S3 => "s3",
            Self::Endpoint => "endpoint",
        }
    }
}

/// `sources.add_bucket`. Its `Debug` form never shows the keys.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AddBucketRequest {
    pub kind: BucketKind,
    /// The Cloudflare account ID, Amazon region or server address.
    pub place: String,
    pub bucket: String,
    /// A folder in the bucket to read from, such as `Photos/2019`; the
    /// whole bucket when left out.
    #[serde(default)]
    pub prefix: Option<String>,
    /// What to call the source; its folder's or the bucket's name when
    /// left out.
    #[serde(default)]
    pub name: Option<String>,
    /// The access key ID. Stored in the computer's keychain, never
    /// anywhere else, and never given back.
    #[serde(default)]
    pub access_key_id: Option<Key>,
    /// The secret access key. Stored in the computer's keychain, never
    /// anywhere else, and never given back.
    #[serde(default)]
    pub secret_access_key: Option<Key>,
}

/// What `sources.add_bucket` answers. Never the keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BucketAdded {
    /// The source's id, to look through it with `sources.look_again`.
    pub id: i64,
    pub name: String,
    /// Where it is as its files' paths start, `s3://family-photos/Photos`:
    /// what `sources.look` is given to look through it.
    pub root: String,
    /// Where it is, in words: "the Amazon S3 bucket family-photos".
    pub place: String,
    /// Said only when no keys were given and those set in the environment
    /// were tested and saved instead: which variables, never the keys.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keys: Option<String>,
}

impl Backup {
    /// `sources.add_bucket`: refuses a bucket this backup itself goes to,
    /// tests the keys by reading, saves them in the keychain, and adds the
    /// bucket as a source with nothing found on it yet. Look through it
    /// next with `sources.look_again`. It keeps its folders as they are
    /// unless the person chooses otherwise.
    pub fn add_bucket_source(
        &self,
        request: &AddBucketRequest,
        secrets: &dyn SecretStore,
        environment: &dyn Fn(&str) -> Option<String>,
    ) -> Result<BucketAdded> {
        let access = request.access_key_id.as_ref().map(Key::reveal).unwrap_or_default();
        let secret = request.secret_access_key.as_ref().map(Key::reveal).unwrap_or_default();
        self.add_bucket_with_keys(request, access, secret, secrets, environment)
            .map_err(|e| without_keys(e, &[access, secret]))
    }

    fn add_bucket_with_keys(
        &self,
        request: &AddBucketRequest,
        access: &str,
        secret: &str,
        secrets: &dyn SecretStore,
        environment: &dyn Fn(&str) -> Option<String>,
    ) -> Result<BucketAdded> {
        let place = BucketPlace::new(
            request.kind.word(),
            &request.place,
            &request.bucket,
            request.prefix.as_deref().unwrap_or_default(),
        )
        .ok_or_else(|| Error::failed("Say where the cloud storage is, and which bucket to read from.".into()))?;
        // Half a pair is a mistake to say, not a reason to use other keys.
        let given = Keys::new(access, secret);
        if given.is_none() && !(access.is_empty() && secret.is_empty()) {
            return Err(Error::failed("Enter both keys for the storage.".into()));
        }
        let world = World { secrets, environment };
        let (id, from) = keevoro_engine::bucket::add(self.catalog(), &place, request.name.as_deref(), given, &world)
            .map_err(|e| {
                if e.downcast_ref::<keevoro_engine::connect::CannotReach>().is_some() {
                    Error::unreachable(format!("{e:#}"))
                } else {
                    Error::from(e)
                }
            })?;
        let name = self
            .catalog()
            .sources()?
            .into_iter()
            .find(|s| s.id == id)
            .map_or_else(|| place.name(), |s| s.name);
        Ok(BucketAdded {
            id,
            name,
            root: place.root(),
            place: place.describe(),
            keys: (from == KeysFrom::Environment).then(|| {
                format!(
                    "No keys were given, so Keevoro used the ones set in {} and {}, and saved them in this \
                     computer's keychain for {}.",
                    keevoro_engine::send::ACCESS_KEY_VAR,
                    keevoro_engine::send::SECRET_KEY_VAR,
                    place.describe()
                )
            }),
        })
    }

    /// The keychain entry of a bucket source, by its id, when it is one.
    pub(crate) fn bucket_secret(&self, source: i64) -> Result<Option<String>> {
        Ok(self
            .catalog()
            .sources()?
            .into_iter()
            .find(|s| s.id == source)
            .and_then(|s| keevoro_engine::bucket::place_of(s.bucket.as_deref()))
            .map(|place| place.secret_name()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::tests::one_backup;
    use keevoro_engine::connect::testing::{no_environment, MemorySecrets};

    fn request(prefix: &str) -> AddBucketRequest {
        AddBucketRequest {
            kind: BucketKind::Endpoint,
            // Nothing listens here, so the storage is out of reach.
            place: "http://127.0.0.1:9".into(),
            bucket: "family".into(),
            prefix: Some(prefix.into()),
            name: None,
            access_key_id: Some(Key::new("AKIAMARKER")),
            secret_access_key: Some(Key::new("secret-marker")),
        }
    }

    #[test]
    fn a_bucket_out_of_reach_is_said_and_nothing_is_kept() {
        let (_dir, _card, backup) = one_backup();
        let secrets = MemorySecrets::default();
        let e = backup
            .add_bucket_source(&request("Photos"), &secrets, &no_environment)
            .unwrap_err();
        assert_eq!(e.kind, crate::error::Kind::Unreachable, "{e:?}");
        assert!(
            e.message
                .starts_with("Keevoro couldn't reach the folder Photos in the bucket family"),
            "{e:?}"
        );
        assert!(!format!("{e:?}").contains("secret-marker"));
        assert!(secrets.0.borrow().is_empty(), "keys that did not work are not kept");
        assert!(backup.catalog().sources().unwrap().iter().all(|s| s.bucket.is_none()));
    }

    #[test]
    fn a_bucket_with_half_a_pair_of_keys_or_no_bucket_is_refused_before_anything_is_tried() {
        let (_dir, _card, backup) = one_backup();
        let secrets = MemorySecrets::default();
        let mut half = request("");
        half.secret_access_key = None;
        let e = backup.add_bucket_source(&half, &secrets, &no_environment).unwrap_err();
        assert_eq!(e.message, "Enter both keys for the storage.");
        let mut none = request("");
        none.bucket = " ".into();
        let e = backup.add_bucket_source(&none, &secrets, &no_environment).unwrap_err();
        assert_eq!(
            e.message,
            "Say where the cloud storage is, and which bucket to read from."
        );
    }
}
