//! The upload engine.
//!
//! Storage providers only move bytes. Everything that makes an upload safe
//! and restartable lives here, written once and shared by every provider:
//!
//! - **Originals are write-once.** An original is never replaced in the
//!   bucket. If its name is already taken by the same file, that counts as
//!   done; if it is taken by a different file, the upload is refused and
//!   reported. Previews and the index can be replaced, because they can
//!   always be made again.
//! - **Everything resumes.** Each finished object is recorded in the
//!   catalog as it lands. Large files go up in parts, and the provider's
//!   upload id is saved before the first part, so after a crash, a reboot
//!   or a dropped connection only the missing parts are sent.
//! - **One bad file never stops the rest.** Problems are collected into the
//!   report with a plain reason.
//! - **Short network drops are retried** a few times with a growing pause.
//!   Anything else (wrong key, no permission) is reported straight away,
//!   because trying again would not help.
//!
//! The engine knows nothing about S3, R2 or folders. It sees only the
//! traits in `keevoro-storage`, which is what lets one test suite cover
//! every provider.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use keevoro_catalog::{Catalog, CatalogError};
use keevoro_storage::{
    ByteSource, MultipartStore, ObjectKey, ObjectStore, PartReceipt, PutOptions, StorageError, UploadId,
};
use serde::Serialize;
use std::collections::BTreeMap;
use std::future::Future;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// One file to send.
#[derive(Debug, Clone)]
pub struct Upload {
    pub key: ObjectKey,
    pub file: PathBuf,
    pub bytes: u64,
    /// True for originals: never replace what is already stored.
    pub write_once: bool,
    pub content_type: Option<String>,
    /// Stored on the object where the provider supports it, for example
    /// the BLAKE3 hash.
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct PublishOptions {
    /// Files larger than this go up in parts.
    pub multipart_above: u64,
    /// Preferred part size; adjusted to fit the provider's limits.
    pub part_bytes: u64,
    /// Extra attempts after a network drop.
    pub retries: u32,
    /// Pause before the first retry; doubled each time.
    pub retry_pause: Duration,
}

impl Default for PublishOptions {
    fn default() -> Self {
        Self {
            multipart_above: 64 * 1024 * 1024,
            part_bytes: 16 * 1024 * 1024,
            retries: 3,
            retry_pause: Duration::from_secs(2),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FailedUpload {
    pub key: String,
    pub reason: String,
}

#[derive(Debug, Default, Serialize)]
pub struct PublishReport {
    pub uploaded: usize,
    pub uploaded_bytes: u64,
    pub already_there: usize,
    pub failed: Vec<FailedUpload>,
}

/// Counts for "3,412 of 9,870 safely backed up".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PublishProgress {
    pub done: usize,
    pub total: usize,
}

enum Outcome {
    Sent,
    AlreadyThere,
}

/// Sends `uploads` to `store`, skipping whatever the catalog says is
/// already there. `target` is a stable name for this storage place; it
/// keeps the records of two different buckets apart.
pub async fn publish(
    store: &dyn MultipartStore,
    catalog: &Catalog,
    target: &str,
    uploads: &[Upload],
    opts: &PublishOptions,
    on_progress: &mut dyn FnMut(&PublishProgress),
) -> Result<PublishReport, CatalogError> {
    let mut report = PublishReport::default();
    let mut progress = PublishProgress {
        done: 0,
        total: uploads.len(),
    };
    for upload in uploads {
        if catalog.is_uploaded(target, upload.key.as_str())? {
            report.already_there += 1;
        } else {
            match send(store, catalog, target, upload, opts).await {
                Ok(outcome) => {
                    catalog.mark_uploaded(target, upload.key.as_str(), upload.bytes)?;
                    match outcome {
                        Outcome::Sent => {
                            report.uploaded += 1;
                            report.uploaded_bytes += upload.bytes;
                        }
                        Outcome::AlreadyThere => report.already_there += 1,
                    }
                }
                Err(SendError::Catalog(e)) => return Err(e),
                Err(SendError::Storage(e)) => report.failed.push(FailedUpload {
                    key: upload.key.to_string(),
                    reason: plain_reason(&e),
                }),
            }
        }
        progress.done += 1;
        on_progress(&progress);
    }
    Ok(report)
}

/// Stores a small document that may be replaced, such as the index.
pub async fn put_document(
    store: &dyn ObjectStore,
    key: &ObjectKey,
    body: Vec<u8>,
    content_type: &str,
    opts: &PublishOptions,
) -> Result<(), StorageError> {
    let put = PutOptions {
        create_only: false,
        content_type: Some(content_type.to_string()),
        metadata: BTreeMap::new(),
    };
    with_retries(opts, || store.put(key, ByteSource::Bytes(body.clone()), &put)).await?;
    Ok(())
}

/// Reading a file back means holding it in memory. Files larger than this
/// (long videos) are checked by presence and size only.
pub const MAX_READ_BACK_BYTES: u64 = 512 * 1024 * 1024;

/// One original as it should be in the storage.
#[derive(Debug, Clone)]
pub struct Expected {
    pub key: ObjectKey,
    pub bytes: u64,
    /// Full BLAKE3 hash, hex.
    pub blake3: String,
}

#[derive(Debug, Default, Serialize)]
pub struct BackupCheck {
    /// Originals found with the right size.
    pub present: usize,
    /// Of those, how many were also downloaded and matched exactly.
    pub read_back: usize,
    pub problems: Vec<FailedUpload>,
}

/// Checks that originals are really in the storage. Every one is looked up
/// and its size compared. With `read_back`, each is also downloaded and its
/// contents compared with the hash from when it was gathered, which is the
/// only way to be certain and costs a full download.
pub async fn check_backup(
    store: &dyn ObjectStore,
    expected: &[Expected],
    read_back: bool,
    opts: &PublishOptions,
    on_progress: &mut dyn FnMut(&PublishProgress),
) -> BackupCheck {
    let mut check = BackupCheck::default();
    let mut progress = PublishProgress {
        done: 0,
        total: expected.len(),
    };
    for want in expected {
        let problem = match with_retries(opts, || store.head(&want.key)).await {
            Ok(None) => Some("It is missing from the backup".to_string()),
            Ok(Some(found)) if found.bytes != want.bytes => {
                Some("The backup's copy is a different size from the one in your library".to_string())
            }
            Ok(Some(_)) if read_back && want.bytes <= MAX_READ_BACK_BYTES => {
                match with_retries(opts, || store.get(&want.key)).await {
                    Ok(bytes) if blake3_hex(&bytes) == want.blake3 => {
                        check.read_back += 1;
                        None
                    }
                    Ok(_) => Some("The backup's copy does not match the original".to_string()),
                    Err(e) => Some(plain_reason(&e)),
                }
            }
            Ok(Some(_)) => None,
            Err(e) => Some(plain_reason(&e)),
        };
        match problem {
            None => check.present += 1,
            Some(reason) => check.problems.push(FailedUpload {
                key: want.key.to_string(),
                reason,
            }),
        }
        progress.done += 1;
        on_progress(&progress);
    }
    check
}

fn blake3_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// Says a storage problem in words a non-technical person can act on.
pub fn plain_reason(e: &StorageError) -> String {
    match e {
        StorageError::AccessDenied(_) => {
            "The storage refused access. Check that the key is right and is allowed to write".into()
        }
        StorageError::Network(_) => "The connection kept dropping. It will be tried again next time".into(),
        StorageError::AlreadyExists(_) => {
            "A different file is already stored under this name, and Keevoro never replaces an original".into()
        }
        StorageError::Io(io) => format!("Couldn't read it from the library ({io})"),
        // Messages already written for the user, such as a mistyped
        // account ID, are passed on as they are.
        StorageError::Provider(said) if said.starts_with("That doesn't look like") => said.clone(),
        StorageError::Provider(said) if said.starts_with("The storage address") => said.clone(),
        other => format!("The storage reported a problem ({other})"),
    }
}

enum SendError {
    Storage(StorageError),
    Catalog(CatalogError),
}

impl From<StorageError> for SendError {
    fn from(e: StorageError) -> Self {
        Self::Storage(e)
    }
}

impl From<CatalogError> for SendError {
    fn from(e: CatalogError) -> Self {
        Self::Catalog(e)
    }
}

async fn send(
    store: &dyn MultipartStore,
    catalog: &Catalog,
    target: &str,
    upload: &Upload,
    opts: &PublishOptions,
) -> Result<Outcome, SendError> {
    let caps = store.capabilities();
    let in_parts = caps.multipart.is_some() && upload.bytes > opts.multipart_above;
    // Asking first costs one small request. It is needed when the provider
    // cannot refuse an overwrite itself, and before a large upload it saves
    // sending gigabytes only to be told the file is already there.
    if upload.write_once && (in_parts || !caps.conditional_create) {
        if let Some(found) = with_retries(opts, || store.head(&upload.key)).await? {
            return same_or_refuse(upload, found.bytes).map_err(Into::into);
        }
    }
    let put = PutOptions {
        create_only: upload.write_once && caps.conditional_create,
        content_type: upload.content_type.clone(),
        metadata: if caps.custom_metadata {
            upload.metadata.clone()
        } else {
            BTreeMap::new()
        },
    };
    let sent = if in_parts {
        send_in_parts(store, catalog, target, upload, &put, opts).await
    } else {
        with_retries(opts, || {
            store.put(&upload.key, ByteSource::File(upload.file.clone()), &put)
        })
        .await
        .map(|_| ())
        .map_err(SendError::from)
    };
    match sent {
        Ok(()) => Ok(Outcome::Sent),
        Err(SendError::Storage(StorageError::AlreadyExists(_))) => {
            let found = with_retries(opts, || store.head(&upload.key)).await?;
            same_or_refuse(upload, found.map_or(u64::MAX, |f| f.bytes)).map_err(Into::into)
        }
        Err(e) => Err(e),
    }
}

/// An object is already stored under this key. If it is the same size it is
/// taken to be the same file (keys are unique per file in a library);
/// otherwise the upload is refused rather than replace an original.
fn same_or_refuse(upload: &Upload, stored_bytes: u64) -> Result<Outcome, StorageError> {
    if stored_bytes == upload.bytes {
        Ok(Outcome::AlreadyThere)
    } else {
        Err(StorageError::AlreadyExists(upload.key.to_string()))
    }
}

async fn send_in_parts(
    store: &dyn MultipartStore,
    catalog: &Catalog,
    target: &str,
    upload: &Upload,
    put: &PutOptions,
    opts: &PublishOptions,
) -> Result<(), SendError> {
    let key = upload.key.as_str();
    // Carry on an earlier attempt if the provider still knows about it.
    let mut held = Vec::new();
    let mut id = None;
    if let Some(saved) = catalog.multipart(target, key)? {
        let saved = UploadId(saved);
        match with_retries(opts, || store.list_parts(&saved)).await {
            Ok(parts) => {
                held = parts;
                id = Some(saved);
            }
            Err(StorageError::UploadNotFound(_)) => {}
            Err(e) => return Err(e.into()),
        }
    }
    let id = match id {
        Some(id) => id,
        None => {
            let fresh = with_retries(opts, || store.start(&upload.key, put)).await?;
            // Saved before any part is sent, so a crash cannot orphan it.
            catalog.set_multipart(target, key, Some(&fresh.0))?;
            fresh
        }
    };

    let part_bytes = part_size(store, upload.bytes, opts.part_bytes);
    let mut receipts = Vec::new();
    let mut offset = 0u64;
    let mut number: u16 = 1;
    while offset < upload.bytes {
        let len = part_bytes.min(upload.bytes - offset);
        let receipt = match held.iter().find(|p| p.number == number && p.bytes == len) {
            Some(already) => already.clone(),
            None => {
                let body = read_range(&upload.file, offset, len)?;
                with_retries(opts, || store.put_part(&id, number, ByteSource::Bytes(body.clone()))).await?
            }
        };
        receipts.push(receipt);
        offset += len;
        number = number
            .checked_add(1)
            .ok_or_else(|| StorageError::InvalidParts("too many parts".into()))?;
    }
    finish(store, &id, &receipts, opts).await?;
    Ok(())
}

async fn finish(
    store: &dyn MultipartStore,
    id: &UploadId,
    receipts: &[PartReceipt],
    opts: &PublishOptions,
) -> Result<(), StorageError> {
    match with_retries(opts, || store.complete(id, receipts)).await {
        Ok(_) => Ok(()),
        Err(e) => {
            // Tidy up when the provider has refused for good, so abandoned
            // parts do not sit there costing storage.
            if !e.is_retryable() {
                let _ = store.abort(id).await;
            }
            Err(e)
        }
    }
}

/// Picks a part size inside the provider's limits that also keeps the
/// number of parts under its maximum.
fn part_size(store: &dyn MultipartStore, total: u64, preferred: u64) -> u64 {
    let Some(limits) = store.capabilities().multipart else {
        return total.max(1);
    };
    let needed_for_count = total.div_ceil(u64::from(limits.max_parts.max(1)));
    preferred
        .max(needed_for_count)
        .clamp(limits.min_part_bytes.max(1), limits.max_part_bytes.max(1))
}

fn read_range(file: &Path, offset: u64, len: u64) -> Result<Vec<u8>, StorageError> {
    let mut f = std::fs::File::open(file)?;
    f.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0u8; usize::try_from(len).map_err(|_| StorageError::InvalidParts("part too large".into()))?];
    f.read_exact(&mut buf)?;
    Ok(buf)
}

/// Runs `action`, trying again after a pause when the error is the kind
/// that may pass (a dropped connection).
async fn with_retries<T, F, Fut>(opts: &PublishOptions, mut action: F) -> Result<T, StorageError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, StorageError>>,
{
    let mut pause = opts.retry_pause;
    let mut attempts_left = opts.retries;
    loop {
        match action().await {
            Err(e) if e.is_retryable() && attempts_left > 0 => {
                attempts_left -= 1;
                tokio::time::sleep(pause).await;
                pause = pause.saturating_mul(2);
            }
            other => return other,
        }
    }
}

#[cfg(test)]
mod tests;
