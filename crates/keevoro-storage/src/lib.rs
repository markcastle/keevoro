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

pub use error::StorageError;
pub use key::ObjectKey;

use async_trait::async_trait;
use std::collections::BTreeMap;
use std::path::PathBuf;

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PutReceipt {
    pub key: ObjectKey,
    pub bytes: u64,
    pub etag: Option<String>,
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

    async fn get(&self, key: &ObjectKey) -> Result<Vec<u8>, StorageError>;

    async fn put(&self, key: &ObjectKey, body: ByteSource, opts: &PutOptions) -> Result<PutReceipt, StorageError>;

    /// Lists objects whose keys start with `prefix`, in key order.
    async fn list_page(&self, prefix: &str, continuation: Option<String>) -> Result<ListPage, StorageError>;

    async fn delete(&self, key: &ObjectKey) -> Result<(), StorageError>;
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
