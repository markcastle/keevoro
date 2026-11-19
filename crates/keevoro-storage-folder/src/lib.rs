//! A storage provider that writes to a local folder, external drive or NAS
//! share.
//!
//! It is a real option for people who want their archive on a second disk,
//! and it is the reference provider for the conformance suite.
//!
//! Every write goes to a temporary file under `.keevoro-uploads/` inside the
//! root first, then moves into place, so a crash never leaves a half-written
//! file under its final name.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use async_trait::async_trait;
use keevoro_storage::{
    ByteSource, Capabilities, ConnectionReport, ListPage, MultipartLimits, MultipartStore, ObjectInfo, ObjectKey,
    ObjectStore, PartReceipt, PutOptions, PutReceipt, StorageError, UploadId,
};
use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::io::AsyncWriteExt;

/// Working area inside the root. Never listed.
const WORK_DIR: &str = ".keevoro-uploads";
const PAGE_SIZE: usize = 1000;

/// Told how many bytes have just been written, as a file is copied.
pub type Watcher = Arc<dyn Fn(u64) + Send + Sync>;

/// How much of a file is read and written at a time. Large enough that a
/// network drive is not asked for thousands of small writes, small enough
/// that progress moves several times a second.
const COPY_BUFFER: usize = 4 * 1024 * 1024;

pub struct FolderStore {
    root: PathBuf,
    caps: Capabilities,
    watcher: Option<Watcher>,
}

impl FolderStore {
    /// Opens (and creates if needed) a folder store at `root`.
    pub async fn open(root: impl Into<PathBuf>) -> Result<Self, StorageError> {
        let root = root.into();
        fs::create_dir_all(root.join(WORK_DIR)).await?;
        Ok(Self {
            root,
            caps: Capabilities {
                multipart: Some(MultipartLimits {
                    min_part_bytes: 1024 * 1024,
                    max_part_bytes: 5 * 1024 * 1024 * 1024,
                    max_parts: 10_000,
                }),
                conditional_create: true,
                server_checksums: false,
                custom_metadata: false,
                whole_files_preferred: true,
                counts_its_own_bytes: false,
            },
            watcher: None,
        })
    }

    /// Has `watcher` told about every few megabytes of a file as they are
    /// written, so that a large file shows steady progress instead of
    /// nothing and then everything.
    #[must_use]
    pub fn watched_by(mut self, watcher: Watcher) -> Self {
        self.watcher = Some(watcher);
        self.caps.counts_its_own_bytes = true;
        self
    }

    fn work(&self) -> PathBuf {
        self.root.join(WORK_DIR)
    }

    fn path_for(&self, key: &ObjectKey) -> Result<PathBuf, StorageError> {
        let mut path = self.root.clone();
        for (i, seg) in key.segments().enumerate() {
            if i == 0 && seg == WORK_DIR {
                return Err(invalid(key, "uses a reserved folder name"));
            }
            check_portable_segment(key, seg)?;
            path.push(seg);
        }
        Ok(path)
    }

    fn upload_dir(&self, id: &UploadId) -> Result<PathBuf, StorageError> {
        if id.0.is_empty() || !id.0.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(StorageError::UploadNotFound(id.0.clone()));
        }
        Ok(self.work().join(&id.0))
    }

    async fn temp_path(&self) -> PathBuf {
        self.work().join(format!("tmp-{}", unique_id()))
    }

    /// Moves a finished temp file into place.
    async fn publish(&self, tmp: &Path, dest: &Path, key: &ObjectKey, create_only: bool) -> Result<(), StorageError> {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).await?;
        }
        if !create_only {
            fs::rename(tmp, dest).await?;
            return Ok(());
        }
        // A hard link fails atomically if the destination exists.
        match fs::hard_link(tmp, dest).await {
            Ok(()) => {
                fs::remove_file(tmp).await?;
                Ok(())
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                let _ = fs::remove_file(tmp).await;
                Err(StorageError::AlreadyExists(key.to_string()))
            }
            Err(_) => {
                // FAT and exFAT drives have no hard links. Check, then move.
                if fs::try_exists(dest).await? {
                    let _ = fs::remove_file(tmp).await;
                    return Err(StorageError::AlreadyExists(key.to_string()));
                }
                fs::rename(tmp, dest).await?;
                Ok(())
            }
        }
    }

    /// Removes folders left empty by a delete, stopping at the root.
    async fn prune_empty_parents(&self, mut dir: PathBuf) {
        while dir != self.root && dir.starts_with(&self.root) {
            if fs::remove_dir(&dir).await.is_err() {
                break; // Not empty, or already gone.
            }
            if !dir.pop() {
                break;
            }
        }
    }
}

#[async_trait]
impl ObjectStore for FolderStore {
    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    async fn check_connection(&self) -> Result<ConnectionReport, StorageError> {
        let probe = self.temp_path().await;
        let can_write = fs::write(&probe, b"keevoro").await.is_ok();
        let can_read = fs::read(&probe).await.map(|b| b == b"keevoro").unwrap_or(false);
        let _ = fs::remove_file(&probe).await;
        Ok(ConnectionReport {
            can_write,
            can_read,
            publicly_listable: None,
        })
    }

    async fn head(&self, key: &ObjectKey) -> Result<Option<ObjectInfo>, StorageError> {
        match fs::metadata(self.path_for(key)?).await {
            Ok(m) if m.is_file() => Ok(Some(ObjectInfo {
                key: key.clone(),
                bytes: m.len(),
                metadata: BTreeMap::new(),
            })),
            Ok(_) => Ok(None),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn holds_exactly(&self, key: &ObjectKey, blake3: &str) -> Result<Option<bool>, StorageError> {
        let path = self.path_for(key)?;
        let expected = blake3.to_string();
        // Read in pieces, so a long video is never held in memory whole.
        tokio::task::spawn_blocking(move || {
            use std::io::Read;
            let mut file = match std::fs::File::open(&path) {
                Ok(file) => file,
                Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Some(false)),
                Err(e) => return Err(StorageError::from(e)),
            };
            let mut hasher = blake3::Hasher::new();
            let mut buffer = vec![0u8; COPY_BUFFER];
            loop {
                match file.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => {
                        hasher.update(&buffer[..n]);
                    }
                    Err(e) if e.kind() == ErrorKind::Interrupted => {}
                    Err(e) => return Err(e.into()),
                }
            }
            Ok(Some(hasher.finalize().to_hex().as_str() == expected))
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
    }

    async fn get(&self, key: &ObjectKey) -> Result<Vec<u8>, StorageError> {
        fs::read(self.path_for(key)?).await.map_err(|e| not_found_or(e, key))
    }

    async fn put(&self, key: &ObjectKey, body: ByteSource, opts: &PutOptions) -> Result<PutReceipt, StorageError> {
        let dest = self.path_for(key)?;
        let tmp = self.temp_path().await;
        let (bytes, blake3) = write_source(
            &tmp,
            body,
            self.watcher.clone(),
            opts.expect_blake3.clone(),
            opts.learn_blake3,
        )
        .await?;
        self.publish(&tmp, &dest, key, opts.create_only).await?;
        Ok(PutReceipt {
            key: key.clone(),
            bytes,
            etag: None,
            blake3,
        })
    }

    async fn list_page(&self, prefix: &str, continuation: Option<String>) -> Result<ListPage, StorageError> {
        let root = self.root.clone();
        let prefix = prefix.to_string();
        let mut keys = tokio::task::spawn_blocking(move || collect_keys(&root))
            .await
            .map_err(|e| StorageError::Provider(e.to_string()))??;
        keys.retain(|(k, _)| k.starts_with(&prefix) && continuation.as_deref().is_none_or(|c| k.as_str() > c));
        keys.sort();
        let more = keys.len() > PAGE_SIZE;
        keys.truncate(PAGE_SIZE);
        let next = if more {
            keys.last().map(|(k, _)| k.clone())
        } else {
            None
        };
        let objects = keys
            .into_iter()
            .map(|(k, bytes)| {
                Ok(ObjectInfo {
                    key: ObjectKey::new(k)?,
                    bytes,
                    metadata: BTreeMap::new(),
                })
            })
            .collect::<Result<_, StorageError>>()?;
        Ok(ListPage { objects, next })
    }

    async fn delete(&self, key: &ObjectKey) -> Result<(), StorageError> {
        let path = self.path_for(key)?;
        match fs::remove_file(&path).await {
            Ok(()) => {
                if let Some(parent) = path.parent() {
                    self.prune_empty_parents(parent.to_path_buf()).await;
                }
                Ok(())
            }
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

#[async_trait]
impl MultipartStore for FolderStore {
    async fn start(&self, key: &ObjectKey, opts: &PutOptions) -> Result<UploadId, StorageError> {
        self.path_for(key)?; // Validate before accepting any parts.
        let id = UploadId(unique_id());
        let dir = self.upload_dir(&id)?;
        fs::create_dir_all(&dir).await?;
        let target = format!("{}\n{}", u8::from(opts.create_only), key);
        fs::write(dir.join("target"), target).await?;
        Ok(id)
    }

    async fn put_part(&self, id: &UploadId, number: u16, body: ByteSource) -> Result<PartReceipt, StorageError> {
        let dir = self.existing_upload(id).await?;
        if number == 0 {
            return Err(StorageError::InvalidParts("part numbers start at 1".into()));
        }
        let tmp = dir.join(format!("part-{number:05}.tmp"));
        let (bytes, _) = write_source(&tmp, body, None, None, false).await?;
        fs::rename(&tmp, dir.join(part_name(number))).await?;
        Ok(PartReceipt {
            number,
            bytes,
            etag: etag(number, bytes),
        })
    }

    async fn complete(&self, id: &UploadId, parts: &[PartReceipt]) -> Result<PutReceipt, StorageError> {
        let dir = self.existing_upload(id).await?;
        if parts.is_empty() {
            return Err(StorageError::InvalidParts("no parts".into()));
        }
        let held = self.list_parts(id).await?;
        for (i, p) in parts.iter().enumerate() {
            let want = u16::try_from(i + 1).map_err(|_| StorageError::InvalidParts("too many parts".into()))?;
            if p.number != want {
                return Err(StorageError::InvalidParts(format!(
                    "expected part {want}, got {}",
                    p.number
                )));
            }
            if !held.contains(p) {
                return Err(StorageError::InvalidParts(format!(
                    "part {} does not match what was uploaded",
                    p.number
                )));
            }
        }
        let target = fs::read_to_string(dir.join("target")).await?;
        let (flag, key) = target
            .split_once('\n')
            .ok_or_else(|| StorageError::Provider("corrupt upload record".into()))?;
        let key = ObjectKey::new(key)?;
        let dest = self.path_for(&key)?;

        let tmp = self.temp_path().await;
        let mut out = fs::File::create(&tmp).await?;
        let mut total = 0u64;
        for p in parts {
            let mut part = fs::File::open(dir.join(part_name(p.number))).await?;
            total += tokio::io::copy(&mut part, &mut out).await?;
        }
        out.flush().await?;
        out.sync_all().await?;
        drop(out);

        self.publish(&tmp, &dest, &key, flag == "1").await?;
        fs::remove_dir_all(&dir).await?;
        Ok(PutReceipt {
            key,
            bytes: total,
            etag: None,
            blake3: None,
        })
    }

    async fn abort(&self, id: &UploadId) -> Result<(), StorageError> {
        match fs::remove_dir_all(self.upload_dir(id)?).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    async fn list_parts(&self, id: &UploadId) -> Result<Vec<PartReceipt>, StorageError> {
        let dir = self.existing_upload(id).await?;
        let mut parts = Vec::new();
        let mut entries = fs::read_dir(&dir).await?;
        while let Some(e) = entries.next_entry().await? {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(n) = name.strip_prefix("part-").and_then(|s| s.parse::<u16>().ok()) {
                let bytes = e.metadata().await?.len();
                parts.push(PartReceipt {
                    number: n,
                    bytes,
                    etag: etag(n, bytes),
                });
            }
        }
        parts.sort_by_key(|p| p.number);
        Ok(parts)
    }
}

impl FolderStore {
    async fn existing_upload(&self, id: &UploadId) -> Result<PathBuf, StorageError> {
        let dir = self.upload_dir(id)?;
        if fs::try_exists(dir.join("target")).await? {
            Ok(dir)
        } else {
            Err(StorageError::UploadNotFound(id.0.clone()))
        }
    }
}

async fn write_source(
    dest: &Path,
    body: ByteSource,
    watcher: Option<Watcher>,
    expect: Option<String>,
    learn: bool,
) -> Result<(u64, Option<String>), StorageError> {
    match body {
        ByteSource::Bytes(b) => {
            let mut f = fs::File::create(dest).await?;
            f.write_all(&b).await?;
            f.sync_all().await?;
            Ok((b.len() as u64, None))
        }
        ByteSource::File(src) => {
            let dest = dest.to_path_buf();
            tokio::task::spawn_blocking(move || copy_file(&src, &dest, watcher.as_ref(), expect.as_deref(), learn))
                .await
                .map_err(|e| StorageError::Provider(e.to_string()))?
        }
    }
}

/// Copies `src` to `dest` in large pieces, telling `watcher` after each
/// piece. The source is only ever read.
///
/// When the file's fingerprint is known (`expect`), the copy is proved
/// exact in two steps, because a backup nobody has checked is only a hope:
///
/// 1. What is read from the source is fingerprinted as it goes by. If that
///    is not the expected fingerprint, the file has changed since it was
///    looked at or the source is not reading back what it holds.
/// 2. What was written is flushed to the disk, read back from start to
///    finish and fingerprinted again. If that differs, the destination did
///    not keep what it was given.
///
/// Either way the copy is removed and the failure reported; nothing wrong
/// is ever left under a real name.
fn copy_file(
    src: &Path,
    dest: &Path,
    watcher: Option<&Watcher>,
    expect: Option<&str>,
    learn: bool,
) -> Result<(u64, Option<String>), StorageError> {
    use std::io::{Read, Write};
    let mut input = std::fs::File::open(src)?;
    let mut output = std::fs::File::create(dest)?;
    let mut buffer = vec![0u8; COPY_BUFFER];
    let mut total = 0u64;
    let prove = expect.is_some() || learn;
    let mut read = blake3::Hasher::new();
    loop {
        let n = match input.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        };
        if prove {
            read.update(&buffer[..n]);
        }
        output.write_all(&buffer[..n])?;
        total += n as u64;
        if let Some(tell) = watcher {
            tell(n as u64);
        }
    }
    output.flush()?;
    if !prove {
        return Ok((total, None));
    }
    let refuse = |why: &str| {
        let _ = std::fs::remove_file(dest);
        Err(StorageError::NotTheSame(why.to_string()))
    };
    // What was read is the file: either it matches the fingerprint already
    // known, or (when none is known yet) it becomes the fingerprint.
    let fingerprint = read.finalize().to_hex().to_string();
    if expect.is_some_and(|expected| expected != fingerprint) {
        return refuse("the file is not as it was when it was looked at");
    }
    // Past the system's own memory and onto the disk before it is read back.
    output.sync_all()?;
    drop(output);
    let mut written = blake3::Hasher::new();
    let mut back = std::fs::File::open(dest)?;
    loop {
        match back.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                written.update(&buffer[..n]);
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
    if written.finalize().to_hex().as_str() != fingerprint {
        return refuse("what was written did not read back the same");
    }
    Ok((total, Some(fingerprint)))
}

/// Walks the root and returns `(key, bytes)` for every file outside the work area.
fn collect_keys(root: &Path) -> Result<Vec<(String, u64)>, StorageError> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if prefix.is_empty() && name == WORK_DIR {
                continue;
            }
            let key = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let ty = entry.file_type()?;
            if ty.is_dir() {
                stack.push((entry.path(), key));
            } else if ty.is_file() {
                out.push((key, entry.metadata()?.len()));
            }
        }
    }
    Ok(out)
}

/// Names that are valid object keys but cannot be files on Windows, which
/// matters because a library folder may be moved to a Windows machine.
fn check_portable_segment(key: &ObjectKey, seg: &str) -> Result<(), StorageError> {
    if seg
        .chars()
        .any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
    {
        return Err(invalid(
            key,
            "contains a character Windows does not allow in file names",
        ));
    }
    if seg.ends_with('.') || seg.ends_with(' ') {
        return Err(invalid(key, "has a folder or file name ending in a dot or space"));
    }
    let stem = seg.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0');
    if reserved {
        return Err(invalid(key, "uses a name Windows reserves"));
    }
    Ok(())
}

fn invalid(key: &ObjectKey, reason: &'static str) -> StorageError {
    StorageError::InvalidKey {
        key: key.to_string(),
        reason,
    }
}

fn not_found_or(e: std::io::Error, key: &ObjectKey) -> StorageError {
    if e.kind() == ErrorKind::NotFound {
        StorageError::NotFound(key.to_string())
    } else {
        e.into()
    }
}

fn part_name(n: u16) -> String {
    format!("part-{n:05}")
}

fn etag(number: u16, bytes: u64) -> String {
    format!("{number}-{bytes}")
}

/// Unique enough for temp names and upload ids within one machine: time plus
/// a process-wide counter, as hex.
fn unique_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:x}{:x}{n:x}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn store() -> (tempfile::TempDir, FolderStore) {
        let dir = tempfile::tempdir().unwrap();
        let s = FolderStore::open(dir.path().join("library")).await.unwrap();
        (dir, s)
    }

    #[tokio::test]
    async fn passes_the_conformance_suite() {
        let (_dir, s) = store().await;
        keevoro_storage::conformance::run_all(&s).await;
    }

    #[tokio::test]
    async fn leaves_no_temp_files_or_empty_folders_behind() {
        let (_dir, s) = store().await;
        keevoro_storage::conformance::run_all(&s).await;
        let work: Vec<_> = std::fs::read_dir(s.work()).unwrap().collect();
        assert!(work.is_empty(), "work area should be empty, found {work:?}");
        let top: Vec<_> = std::fs::read_dir(&s.root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .filter(|n| n != WORK_DIR)
            .collect();
        assert!(top.is_empty(), "root should be empty after cleanup, found {top:?}");
    }

    #[tokio::test]
    async fn a_watched_store_reports_every_byte_of_a_file_as_it_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let seen = Arc::new(AtomicU64::new(0));
        let calls = Arc::new(AtomicU64::new(0));
        let (s, c) = (seen.clone(), calls.clone());
        let store = FolderStore::open(dir.path().join("to"))
            .await
            .unwrap()
            .watched_by(Arc::new(move |n| {
                s.fetch_add(n, Ordering::SeqCst);
                c.fetch_add(1, Ordering::SeqCst);
            }));
        assert!(store.capabilities().counts_its_own_bytes);
        let big = dir.path().join("big.mov");
        let bytes = vec![7u8; COPY_BUFFER * 2 + 123];
        std::fs::write(&big, &bytes).unwrap();
        let key = ObjectKey::new("originals/big.mov").unwrap();
        let receipt = store
            .put(&key, ByteSource::File(big.clone()), &PutOptions::default())
            .await
            .unwrap();
        assert_eq!(receipt.bytes, bytes.len() as u64);
        assert_eq!(seen.load(Ordering::SeqCst), bytes.len() as u64);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            3,
            "told piece by piece, not once at the end"
        );
        assert!(store.get(&key).await.unwrap() == bytes);
        assert_eq!(
            std::fs::read(&big).unwrap().len(),
            bytes.len(),
            "the source is untouched"
        );
    }

    #[tokio::test]
    async fn a_copy_is_proved_exact_or_it_is_not_kept() {
        let dir = tempfile::tempdir().unwrap();
        let store = FolderStore::open(dir.path().join("to")).await.unwrap();
        let photo = dir.path().join("photo.jpg");
        let bytes = vec![42u8; 300_000];
        std::fs::write(&photo, &bytes).unwrap();
        let fingerprint = blake3::hash(&bytes).to_hex().to_string();

        // The right fingerprint: stored, and the same as the original.
        let good = ObjectKey::new("originals/photo.jpg").unwrap();
        let known = PutOptions {
            expect_blake3: Some(fingerprint.clone()),
            ..Default::default()
        };
        store.put(&good, ByteSource::File(photo.clone()), &known).await.unwrap();
        assert!(store.get(&good).await.unwrap() == bytes);

        // The file has changed since it was fingerprinted: refused, and
        // nothing is left under its name or in the work area.
        std::fs::write(&photo, vec![43u8; 300_000]).unwrap();
        let bad = ObjectKey::new("originals/changed.jpg").unwrap();
        let refused = store.put(&bad, ByteSource::File(photo.clone()), &known).await;
        assert!(matches!(refused, Err(StorageError::NotTheSame(_))), "{refused:?}");
        assert!(store.head(&bad).await.unwrap().is_none());
        assert!(
            std::fs::read_dir(store.work()).unwrap().next().is_none(),
            "no half-made copy is left"
        );
        // The source was only read.
        assert_eq!(std::fs::read(&photo).unwrap(), vec![43u8; 300_000]);
    }

    #[tokio::test]
    async fn a_file_with_no_known_fingerprint_is_fingerprinted_as_it_is_copied() {
        let dir = tempfile::tempdir().unwrap();
        let store = FolderStore::open(dir.path().join("to")).await.unwrap();
        let video = dir.path().join("flight.mp4");
        let bytes = vec![9u8; COPY_BUFFER + 77];
        std::fs::write(&video, &bytes).unwrap();
        let key = ObjectKey::new("originals/flight.mp4").unwrap();
        let learn = PutOptions {
            learn_blake3: true,
            ..Default::default()
        };
        let receipt = store.put(&key, ByteSource::File(video), &learn).await.unwrap();
        // One pass over the source gave both the copy and its fingerprint.
        assert_eq!(receipt.blake3.as_deref(), Some(blake3::hash(&bytes).to_hex().as_str()));
        assert!(store.get(&key).await.unwrap() == bytes);
        // Not asked, not given.
        let plain = store
            .put(
                &ObjectKey::new("renditions/a.webp").unwrap(),
                ByteSource::Bytes(vec![1, 2, 3]),
                &PutOptions::default(),
            )
            .await
            .unwrap();
        assert_eq!(plain.blake3, None);
    }

    #[tokio::test]
    async fn a_folder_asks_for_files_whole_because_parts_would_be_written_twice() {
        let (_dir, s) = store().await;
        assert!(s.capabilities().whole_files_preferred);
        assert!(!s.capabilities().counts_its_own_bytes, "only when someone is watching");
    }

    #[tokio::test]
    async fn keys_map_onto_nested_folders() {
        let (_dir, s) = store().await;
        let k = ObjectKey::new("originals/Holidays/Spain 2009/IMG_0001.jpg").unwrap();
        s.put(&k, ByteSource::Bytes(b"x".to_vec()), &PutOptions::default())
            .await
            .unwrap();
        assert!(s
            .root
            .join("originals")
            .join("Holidays")
            .join("Spain 2009")
            .join("IMG_0001.jpg")
            .is_file());
    }

    #[tokio::test]
    async fn rejects_names_windows_cannot_store() {
        let (_dir, s) = store().await;
        for key in [
            "a/what?.jpg",
            "a/b:c.jpg",
            "CON/x.jpg",
            "a/com1.txt",
            "a/trailing.",
            "a/space ",
            ".keevoro-uploads/x",
        ] {
            let k = ObjectKey::new(key).unwrap();
            let r = s
                .put(&k, ByteSource::Bytes(b"x".to_vec()), &PutOptions::default())
                .await;
            assert!(matches!(r, Err(StorageError::InvalidKey { .. })), "{key}");
        }
        for key in ["a/COM0.txt", "a/console.jpg", "a/LPT.jpg"] {
            let k = ObjectKey::new(key).unwrap();
            assert!(
                s.put(&k, ByteSource::Bytes(b"x".to_vec()), &PutOptions::default())
                    .await
                    .is_ok(),
                "{key}"
            );
        }
    }

    #[tokio::test]
    async fn unknown_or_malicious_upload_ids_are_refused() {
        let (_dir, s) = store().await;
        for id in ["", "../../etc", "abc123", "zz"] {
            assert!(s.list_parts(&UploadId(id.into())).await.is_err(), "{id}");
        }
        assert!(s
            .put_part(&UploadId("abc".into()), 1, ByteSource::Bytes(vec![1]))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn part_zero_is_invalid() {
        let (_dir, s) = store().await;
        let id = s
            .start(&ObjectKey::new("v.mov").unwrap(), &PutOptions::default())
            .await
            .unwrap();
        assert!(matches!(
            s.put_part(&id, 0, ByteSource::Bytes(vec![1])).await,
            Err(StorageError::InvalidParts(_))
        ));
    }

    #[tokio::test]
    async fn create_only_multipart_refuses_overwrite() {
        let (_dir, s) = store().await;
        let k = ObjectKey::new("v.mov").unwrap();
        s.put(&k, ByteSource::Bytes(b"original".to_vec()), &PutOptions::default())
            .await
            .unwrap();
        let once = PutOptions {
            create_only: true,
            ..Default::default()
        };
        let id = s.start(&k, &once).await.unwrap();
        let p = s.put_part(&id, 1, ByteSource::Bytes(vec![7; 4])).await.unwrap();
        assert!(matches!(
            s.complete(&id, &[p]).await,
            Err(StorageError::AlreadyExists(_))
        ));
        assert_eq!(s.get(&k).await.unwrap(), b"original");
    }

    #[tokio::test]
    async fn lists_in_pages() {
        let (_dir, s) = store().await;
        for i in 0..(PAGE_SIZE + 5) {
            let k = ObjectKey::new(format!("p/{i:05}.jpg")).unwrap();
            s.put(&k, ByteSource::Bytes(vec![]), &PutOptions::default())
                .await
                .unwrap();
        }
        let first = s.list_page("p/", None).await.unwrap();
        assert_eq!(first.objects.len(), PAGE_SIZE);
        let second = s.list_page("p/", first.next).await.unwrap();
        assert_eq!(second.objects.len(), 5);
        assert!(second.next.is_none());
    }
}
