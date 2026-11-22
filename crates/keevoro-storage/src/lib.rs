//! Storage provider traits for keevoro.
//!
//! The upload engine depends only on these traits, never on a provider's SDK.
//! A new backend is a new crate that implements them and passes the shared
//! [`conformance`] suite (behind the `conformance` feature).
//!
//! The traits are deliberately small:
//!
//! - [`ObjectStore`] moves whole objects and lists them.
//! - [`MultipartStore`] adds resumable large uploads. The engine persists the
//!   upload id and part receipts, so an upload survives a crash or reboot.
//!
//! Providers move bytes and nothing else. Choosing what to upload, retrying
//! and tracking progress belong to the engine.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod error;
mod key;

#[cfg(feature = "conformance")]
pub mod conformance;

pub use error::{Step, StorageError};
pub use key::ObjectKey;

use async_trait::async_trait;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// What a provider can do. The engine adapts to these instead of assuming.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    /// Limits for multipart uploads, or `None` if not supported.
    pub multipart: Option<MultipartLimits>,
    /// Supports "create only if absent" (`If-None-Match: *`), used to keep
    /// originals write-once without a race.
    pub conditional_create: bool,
    /// The server verifies an upload checksum we send.
    pub server_checksums: bool,
    /// Custom key/value metadata is stored on objects and returned by `head`.
    pub custom_metadata: bool,
    /// Sending a file whole is cheaper here than sending it in parts. True
    /// for a folder, where parts would be written to the disk and then
    /// written a second time when they are joined.
    pub whole_files_preferred: bool,
    /// The store reports bytes as it writes them (see its own docs for
    /// how), so whoever calls it must not count the file again afterwards.
    pub counts_its_own_bytes: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MultipartLimits {
    pub min_part_bytes: u64,
    pub max_part_bytes: u64,
    pub max_parts: u16,
}

/// Where the bytes for an upload come from.
#[derive(Debug, Clone)]
pub enum ByteSource {
    Bytes(Vec<u8>),
    File(PathBuf),
}

#[derive(Debug, Clone, Default)]
pub struct PutOptions {
    /// Fail with [`StorageError::AlreadyExists`] if the key exists.
    pub create_only: bool,
    /// The BLAKE3 fingerprint the file is known to have, as lower-case hex.
    /// A store that can check its own writes uses it to prove the stored
    /// copy exact, and fails with [`StorageError::NotTheSame`] if it is
    /// not. A store that cannot ignores it.
    pub expect_blake3: Option<String>,
    /// Asks a store that can to fingerprint the file as it is written and
    /// give the answer back in the receipt, for a file whose fingerprint is
    /// not known yet. Such a store also proves what it wrote reads back the
    /// same as what it read. A store that cannot leaves the receipt empty.
    pub learn_blake3: bool,
    pub content_type: Option<String>,
    /// Stored when [`Capabilities::custom_metadata`] is true, for example the
    /// BLAKE3 hash and capture date.
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectInfo {
    pub key: ObjectKey,
    pub bytes: u64,
    pub metadata: BTreeMap<String, String>,
    /// When it was last written, in seconds since 1970, where the listing
    /// says: cloud storage's `LastModified`, a folder's file time.
    pub modified: Option<i64>,
    /// The tag cloud storage gives what it holds under a key. It changes
    /// when the object does, so with the size it says "unchanged since the
    /// last look". It is never a fingerprint of the contents: for an object
    /// sent in parts it is not a hash of them at all.
    pub etag: Option<String>,
    /// The storage class, where the listing gives one (`STANDARD`,
    /// `GLACIER`, `DEEP_ARCHIVE`). Some classes cannot be read until the
    /// object is restored.
    pub class: Option<String>,
}

impl ObjectInfo {
    /// What is known of an object from its key and size alone.
    pub fn new(key: ObjectKey, bytes: u64) -> Self {
        Self {
            key,
            bytes,
            metadata: BTreeMap::new(),
            modified: None,
            etag: None,
            class: None,
        }
    }
}

/// What [`ObjectStore::download_to`] wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Downloaded {
    pub bytes: u64,
    /// The BLAKE3 fingerprint of everything written, as lower-case hex.
    pub blake3: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PutReceipt {
    pub key: ObjectKey,
    pub bytes: u64,
    pub etag: Option<String>,
    /// The BLAKE3 fingerprint of what was written, when the store worked
    /// it out and proved the stored copy matches it.
    pub blake3: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListPage {
    pub objects: Vec<ObjectInfo>,
    /// Pass back to get the next page. `None` means this was the last page.
    pub next: Option<String>,
}

/// A friendly result from [`ObjectStore::check_connection`], shown on the
/// storage setup screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionReport {
    pub can_write: bool,
    pub can_read: bool,
    /// True if anyone can list the bucket's contents. The UI warns about this.
    pub publicly_listable: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UploadId(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartReceipt {
    /// 1-based, as in S3.
    pub number: u16,
    pub bytes: u64,
    pub etag: String,
}

#[async_trait]
pub trait ObjectStore: Send + Sync {
    fn capabilities(&self) -> &Capabilities;

    async fn check_connection(&self) -> Result<ConnectionReport, StorageError>;

    /// `Ok(None)` when the object does not exist.
    async fn head(&self, key: &ObjectKey) -> Result<Option<ObjectInfo>, StorageError>;

    /// Is what is stored under `key` exactly the file with this BLAKE3
    /// fingerprint? `Some(true)` or `Some(false)` from a store that can
    /// read its own contents cheaply and has done so; `None` from one that
    /// cannot tell without downloading the whole object, which is the
    /// default. Used before an object already there is counted as backed
    /// up, so that a different file under the same name is never mistaken
    /// for the original.
    async fn holds_exactly(&self, _key: &ObjectKey, _blake3: &str) -> Result<Option<bool>, StorageError> {
        Ok(None)
    }

    async fn get(&self, key: &ObjectKey) -> Result<Vec<u8>, StorageError>;

    async fn put(&self, key: &ObjectKey, body: ByteSource, opts: &PutOptions) -> Result<PutReceipt, StorageError>;

    /// Lists objects whose keys start with `prefix`, in key order.
    async fn list_page(&self, prefix: &str, continuation: Option<String>) -> Result<ListPage, StorageError>;

    /// Every object whose key starts with `prefix`, with its size, in no
    /// promised order: the quick way to learn what is really there, by
    /// name and size, without reading any file or asking about each one.
    /// The default goes through [`ObjectStore::list_page`] a page at a
    /// time; a store that can do better (a folder, which can read each
    /// folder's listing once) does. Any problem part-way is an error, never
    /// a shorter list, so what could not be listed is never taken for gone.
    async fn list_all(&self, prefix: &str) -> Result<Vec<ObjectInfo>, StorageError> {
        let mut all = Vec::new();
        let mut next = None;
        loop {
            let page = self.list_page(prefix, next).await?;
            all.extend(page.objects);
            match page.next {
                Some(more) => next = Some(more),
                None => return Ok(all),
            }
        }
    }

    async fn delete(&self, key: &ObjectKey) -> Result<(), StorageError>;

    /// Writes the object under `key` into a new file at `file`,
    /// fingerprinting it as it is written, and never holding it whole in
    /// memory where the store can help it. The file must not exist yet: an
    /// existing one is never written over. On any problem the part written
    /// is removed, so nothing is left behind. The default reads the object
    /// whole with [`ObjectStore::get`]; the stores here stream it.
    async fn download_to(&self, key: &ObjectKey, file: &Path) -> Result<Downloaded, StorageError> {
        use std::io::Write;
        let body = self.get(key).await?;
        // Never over a file already there; only what this made is removed.
        let mut out = std::fs::OpenOptions::new().write(true).create_new(true).open(file)?;
        if let Err(e) = out.write_all(&body).and_then(|()| out.sync_all()) {
            drop(out);
            let _ = std::fs::remove_file(file);
            return Err(e.into());
        }
        Ok(Downloaded {
            bytes: body.len() as u64,
            blake3: blake3::hash(&body).to_hex().to_string(),
        })
    }

    /// Can these keys read what is under `prefix`? Asked by listing one
    /// page, so nothing is written: the way to test keys for storage that
    /// is only ever read, a source.
    async fn check_reading(&self, prefix: &str) -> Result<(), StorageError> {
        self.list_page(prefix, None).await.map(|_| ())
    }
}

/// Cloud storage that is a source, so is only ever read. It holds the
/// store privately and offers only listing and reading: nothing given one
/// of these can put, replace or delete anything in it, and a change that
/// tried would not compile.
///
/// ```compile_fail
/// # async fn try_to_write(source: keevoro_storage::ReadOnly, key: keevoro_storage::ObjectKey) {
/// let _ = source.put(&key, keevoro_storage::ByteSource::Bytes(Vec::new()), &Default::default()).await;
/// # }
/// ```
///
/// ```compile_fail
/// # async fn try_to_delete(source: keevoro_storage::ReadOnly, key: keevoro_storage::ObjectKey) {
/// let _ = source.delete(&key).await;
/// # }
/// ```
#[derive(Clone)]
pub struct ReadOnly(std::sync::Arc<dyn ObjectStore>);

impl ReadOnly {
    pub fn new(store: std::sync::Arc<dyn ObjectStore>) -> Self {
        Self(store)
    }

    /// See [`ObjectStore::check_reading`].
    pub async fn check_reading(&self, prefix: &str) -> Result<(), StorageError> {
        self.0.check_reading(prefix).await
    }

    /// See [`ObjectStore::list_page`].
    pub async fn list_page(&self, prefix: &str, continuation: Option<String>) -> Result<ListPage, StorageError> {
        self.0.list_page(prefix, continuation).await
    }

    /// See [`ObjectStore::list_all`].
    pub async fn list_all(&self, prefix: &str) -> Result<Vec<ObjectInfo>, StorageError> {
        self.0.list_all(prefix).await
    }

    /// See [`ObjectStore::head`].
    pub async fn head(&self, key: &ObjectKey) -> Result<Option<ObjectInfo>, StorageError> {
        self.0.head(key).await
    }

    /// See [`ObjectStore::download_to`].
    pub async fn download_to(&self, key: &ObjectKey, file: &Path) -> Result<Downloaded, StorageError> {
        self.0.download_to(key, file).await
    }
}

impl std::fmt::Debug for ReadOnly {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReadOnly")
    }
}

#[async_trait]
pub trait MultipartStore: ObjectStore {
    async fn start(&self, key: &ObjectKey, opts: &PutOptions) -> Result<UploadId, StorageError>;

    async fn put_part(&self, id: &UploadId, number: u16, body: ByteSource) -> Result<PartReceipt, StorageError>;

    /// Parts must be in ascending order with no gaps.
    async fn complete(&self, id: &UploadId, parts: &[PartReceipt]) -> Result<PutReceipt, StorageError>;

    async fn abort(&self, id: &UploadId) -> Result<(), StorageError>;

    /// Parts the provider already holds, so an interrupted upload can resume.
    async fn list_parts(&self, id: &UploadId) -> Result<Vec<PartReceipt>, StorageError>;
}
