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
    /// How many folders' listings are read at the same time by
    /// [`ObjectStore::list_all`]: one on a disk, a few across a network.
    listing_at_once: usize,
}

impl FolderStore {
    /// Opens (and creates if needed) a folder store at `root`.
    pub async fn open(root: impl Into<PathBuf>) -> Result<Self, StorageError> {
        let root = root.into();
        fs::create_dir_all(root.join(WORK_DIR)).await?;
        Ok(Self::at(root))
    }

    /// A folder that is already there, opened only to look at what is in
    /// it: nothing is made, not even Keevoro's working folder. A folder
    /// that is not there is refused, never made, because a backup folder
    /// that has gone is far more often a drive unplugged than a folder
    /// someone wants made again on whatever disk is left.
    pub fn look_at(root: impl Into<PathBuf>) -> Result<Self, StorageError> {
        let root = root.into();
        if !root.is_dir() {
            return Err(StorageError::NotFound(root.display().to_string()));
        }
        Ok(Self::at(root))
    }

    /// Reads up to `folders` folders' listings at the same time when
    /// listing everything: worth it across a network, where most of the
    /// time goes on waiting for the far end, and not on a disk.
    #[must_use]
    pub fn listing_at_once(mut self, folders: usize) -> Self {
        self.listing_at_once = folders.max(1);
        self
    }

    fn at(root: PathBuf) -> Self {
        Self {
            root,
            listing_at_once: 1,
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
        }
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

    async fn list_all(&self, prefix: &str) -> Result<Vec<ObjectInfo>, StorageError> {
        let root = self.root.clone();
        let prefix = prefix.to_string();
        let at_once = self.listing_at_once;
        let found = tokio::task::spawn_blocking(move || list_beneath(&root, &prefix, at_once))
            .await
            .map_err(|e| StorageError::Provider(e.to_string()))??;
        found
            .into_iter()
            .map(|(k, bytes)| {
                Ok(ObjectInfo {
                    key: ObjectKey::new(k)?,
                    bytes,
                    metadata: BTreeMap::new(),
                })
            })
            .collect()
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
/// piece, and gives the copy the original's modified time. The source is
/// only ever read.
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
    // Keep the original's modified time: it is sometimes the only clue to
    // when a photo was taken.
    if let Ok(modified) = input.metadata().and_then(|m| m.modified()) {
        let _ = output.set_modified(modified);
    }
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

/// `(key, bytes)` for every file whose key starts with `prefix`, reading
/// only the folders that can hold one: each folder's listing once, never a
/// file's contents. On Windows a listing carries each file's size; on a
/// Mac or Linux the size is one small question per file, which is what a
/// file manager asks too. With `at_once` above one, that many folders are
/// read at the same time, a level of the tree at a time.
///
/// Any folder that cannot be read is an error for the whole listing, so a
/// drive unplugged part-way is never taken for files gone. Only the folder
/// the prefix names being absent, in a root that is there, means nothing
/// is there.
fn list_beneath(root: &Path, prefix: &str, at_once: usize) -> Result<Vec<(String, u64)>, StorageError> {
    // The deepest folder the prefix names whole: `originals/2019/` is the
    // folder `originals/2019`; `originals/IMG` is inside `originals`.
    let whole = match prefix.rfind('/') {
        Some(at) => &prefix[..at],
        None => "",
    };
    let mut start = root.to_path_buf();
    for seg in whole.split('/').filter(|s| !s.is_empty()) {
        if seg == "." || seg == ".." {
            return Ok(Vec::new());
        }
        start.push(seg);
    }
    let mut level = vec![(start, whole.to_string())];
    let mut out = Vec::new();
    let mut first = true;
    while !level.is_empty() {
        let read: Vec<Result<Listed, std::io::Error>> = if at_once > 1 && level.len() > 1 {
            let chunk = level.len().div_ceil(at_once);
            std::thread::scope(|scope| {
                let handles: Vec<_> = level
                    .chunks(chunk)
                    .map(|dirs| scope.spawn(move || dirs.iter().map(read_one).collect::<Vec<_>>()))
                    .collect();
                handles
                    .into_iter()
                    .flat_map(|h| {
                        h.join()
                            .unwrap_or_else(|_| vec![Err(std::io::Error::other("a listing stopped part-way"))])
                    })
                    .collect()
            })
        } else {
            level.iter().map(read_one).collect()
        };
        let mut next = Vec::new();
        for listed in read {
            let listed = match listed {
                Ok(listed) => listed,
                // The prefix's own folder not being there, with the root
                // there, is simply nothing there.
                Err(e) if first && e.kind() == ErrorKind::NotFound && root.is_dir() => return Ok(out),
                Err(e) => return Err(e.into()),
            };
            for (key, bytes) in listed.files {
                if key.starts_with(prefix) {
                    out.push((key, bytes));
                }
            }
            next.extend(listed.folders);
        }
        first = false;
        level = next;
    }
    Ok(out)
}

/// What one folder holds: its files by key with their sizes, and its
/// folders, to be read next.
struct Listed {
    files: Vec<(String, u64)>,
    folders: Vec<(PathBuf, String)>,
}

fn read_one((dir, prefix): &(PathBuf, String)) -> Result<Listed, std::io::Error> {
    let mut listed = Listed {
        files: Vec::new(),
        folders: Vec::new(),
    };
    for entry in std::fs::read_dir(dir)? {
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
        // Not followed: a link is neither a file nor a folder here.
        let ty = entry.file_type()?;
        if ty.is_dir() {
            listed.folders.push((entry.path(), key));
        } else if ty.is_file() {
            listed.files.push((key, entry.metadata()?.len()));
        }
    }
    Ok(listed)
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

/// How long a working file must have been left untouched before it is
/// taken to be abandoned. A copy in progress writes to its file every few
/// seconds, even over a slow network, so an hour without a change means
/// nobody is writing it: not this program, nor another on this computer or
/// another one using the same folder.
pub const ABANDONED_AFTER: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// What tidying a folder's working files did, or would do.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Swept {
    /// Abandoned working files removed (or that would be), and their size.
    pub files: u64,
    pub bytes: u64,
    /// Left in the working folder because they were written to within
    /// [`ABANDONED_AFTER`]: a backup may be using them right now.
    pub in_use: u64,
    /// Left in the working folder because Keevoro does not recognise them
    /// as its own: anything it did not name, and links.
    pub not_ours: u64,
}

impl Swept {
    /// Everything left in the working folder, whatever the reason.
    pub fn left(&self) -> u64 {
        self.in_use + self.not_ours
    }
}

/// Removes the working files under `root`'s `.keevoro-uploads` folder that
/// nobody can ever use again, and the working folder itself once it is
/// empty. Nothing outside that folder is ever touched: not a backed-up
/// file, not anything the person put there.
///
/// What is there, and what becomes of it:
///
/// - `tmp-…`: a whole file being copied, or one whose copy was stopped,
///   crashed or lost its drive part-way. Nothing resumes a part-copied
///   file (the next backup copies it afresh), so once untouched for
///   [`ABANDONED_AFTER`] it is removed.
/// - a folder named in hexadecimal holding `target` and `part-…` files:
///   a large file sent in parts, which versions of Keevoro before 0.22
///   did for folders. Folders are now always sent whole files, so no
///   version that still runs resumes these; once nothing in one has
///   changed for [`ABANDONED_AFTER`], it is removed.
/// - anything else, or a link: left alone, and counted in [`Swept::not_ours`].
///
/// The age rule is what keeps a live copy safe when another program, or
/// another computer, is backing up to the same folder at this moment: its
/// files are being written, so they are young. Every path is checked to
/// lie inside the working folder, by where it really leads, before it is
/// removed, and links are never followed.
pub fn sweep_working_files(root: &Path, abandoned_after: std::time::Duration) -> std::io::Result<Swept> {
    sweep(root, abandoned_after, true)
}

/// What [`sweep_working_files`] would clear from `root`, by the same rules,
/// without removing anything. It reads one small folder's listing, so it
/// is quick enough to ask before a person decides.
pub fn working_files_to_clear(root: &Path, abandoned_after: std::time::Duration) -> std::io::Result<Swept> {
    sweep(root, abandoned_after, false)
}

fn sweep(root: &Path, abandoned_after: std::time::Duration, clear: bool) -> std::io::Result<Swept> {
    let mut swept = Swept::default();
    let work = root.join(WORK_DIR);
    let meta = match std::fs::symlink_metadata(&work) {
        Ok(meta) => meta,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(swept),
        Err(e) => return Err(e),
    };
    if !meta.is_dir() || meta.file_type().is_symlink() {
        // Not a folder Keevoro made: it is somebody else's.
        swept.not_ours += 1;
        return Ok(swept);
    }
    let inside = std::fs::canonicalize(&work)?;
    let old_enough = |modified: SystemTime| {
        SystemTime::now()
            .duration_since(modified)
            .is_ok_and(|age| age >= abandoned_after)
    };
    for entry in std::fs::read_dir(&work)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let meta = std::fs::symlink_metadata(&path)?;
        let ours = std::fs::canonicalize(&path).is_ok_and(|real| real.parent() == Some(inside.as_path()));
        if meta.file_type().is_symlink() || !ours {
            swept.not_ours += 1;
        } else if meta.is_file() && name.starts_with("tmp-") {
            if old_enough(meta.modified()?) {
                if clear {
                    std::fs::remove_file(&path)?;
                }
                swept.files += 1;
                swept.bytes += meta.len();
            } else {
                swept.in_use += 1;
            }
        } else if meta.is_dir() && !name.is_empty() && name.chars().all(|c| c.is_ascii_hexdigit()) {
            sweep_upload_in_parts(&path, &inside, &old_enough, clear, &mut swept)?;
        } else {
            swept.not_ours += 1;
        }
    }
    if clear {
        // Empty now: the working folder goes too. A copy starting this
        // moment makes it again.
        let _ = std::fs::remove_dir(&work);
    }
    Ok(swept)
}

/// One upload in parts, removed whole once nothing in it has changed for
/// long enough, and only if it holds nothing but its own plain files.
fn sweep_upload_in_parts(
    dir: &Path,
    inside: &Path,
    old_enough: &dyn Fn(SystemTime) -> bool,
    clear: bool,
    swept: &mut Swept,
) -> std::io::Result<()> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let meta = std::fs::symlink_metadata(&path)?;
        let ours = std::fs::canonicalize(&path).is_ok_and(|real| real.parent().and_then(Path::parent) == Some(inside));
        if !meta.is_file() || !ours {
            // A link or a folder: not something Keevoro put there.
            swept.not_ours += 1;
            return Ok(());
        }
        if !old_enough(meta.modified()?) {
            // A part written within the hour.
            swept.in_use += 1;
            return Ok(());
        }
        files.push((path, meta.len()));
    }
    for (path, bytes) in files {
        if clear {
            std::fs::remove_file(&path)?;
        }
        swept.files += 1;
        swept.bytes += bytes;
    }
    if clear {
        std::fs::remove_dir(dir)?;
    }
    Ok(())
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
    async fn a_copy_keeps_the_originals_modified_time() {
        // Sometimes the only clue to when a photo was taken.
        let dir = tempfile::tempdir().unwrap();
        let store = FolderStore::open(dir.path().join("to")).await.unwrap();
        let photo = dir.path().join("photo.jpg");
        std::fs::write(&photo, vec![7u8; 1000]).unwrap();
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_247_605_802);
        std::fs::File::options()
            .write(true)
            .open(&photo)
            .unwrap()
            .set_modified(old)
            .unwrap();
        let key = ObjectKey::new("originals/photo.jpg").unwrap();
        let learn = PutOptions {
            create_only: true,
            learn_blake3: true,
            ..Default::default()
        };
        store.put(&key, ByteSource::File(photo.clone()), &learn).await.unwrap();
        let copied = std::fs::metadata(dir.path().join("to/originals/photo.jpg")).unwrap();
        assert_eq!(copied.modified().unwrap(), old);
        assert_eq!(
            std::fs::metadata(&photo).unwrap().modified().unwrap(),
            old,
            "the source is untouched"
        );
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

    /// Every key and size `list_all` gives, in order.
    async fn all_of(s: &FolderStore, prefix: &str) -> Vec<(String, u64)> {
        let mut all: Vec<(String, u64)> = s
            .list_all(prefix)
            .await
            .unwrap()
            .into_iter()
            .map(|o| (o.key.to_string(), o.bytes))
            .collect();
        all.sort();
        all
    }

    #[tokio::test]
    async fn everything_under_a_prefix_is_listed_with_its_size_one_folder_or_several_at_a_time() {
        let (dir, s) = store().await;
        let root = dir.path().join("library");
        for (k, body) in [
            ("originals/2019/a.jpg", &b"aaaa"[..]),
            ("originals/2019/Spain/b.jpg", b"bb"),
            ("originals/c.jpg", b""),
            ("renditions/thumb/x.webp", b"x"),
            ("What is in this backup.txt", b"note"),
        ] {
            s.put(
                &ObjectKey::new(k).unwrap(),
                ByteSource::Bytes(body.to_vec()),
                &PutOptions::default(),
            )
            .await
            .unwrap();
        }
        // Someone's own file beside the backup, and a link, are listed as
        // what they are: the file by its name, the link not at all.
        std::fs::write(root.join("originals").join("mine.txt"), b"mine").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("renditions"), root.join("originals").join("link")).unwrap();
        let want = vec![
            ("originals/2019/Spain/b.jpg".to_string(), 2),
            ("originals/2019/a.jpg".to_string(), 4),
            ("originals/c.jpg".to_string(), 0),
            ("originals/mine.txt".to_string(), 4),
        ];
        assert_eq!(all_of(&s, "originals/").await, want);
        let several = FolderStore::look_at(&root).unwrap().listing_at_once(4);
        assert_eq!(
            all_of(&several, "originals/").await,
            want,
            "several at a time says the same"
        );
        assert_eq!(all_of(&s, "originals/2019/S").await, [want[0].clone()]);
        // The working folder is never listed, and a prefix with nothing
        // under it lists nothing.
        assert!(all_of(&s, "").await.iter().all(|(k, _)| !k.starts_with(WORK_DIR)));
        assert_eq!(all_of(&s, "").await.len(), 6);
        assert!(all_of(&s, "index/").await.is_empty());
    }

    #[tokio::test]
    async fn a_folder_that_is_not_there_is_looked_at_but_never_made() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("USB stick");
        assert!(matches!(FolderStore::look_at(&gone), Err(StorageError::NotFound(_))));
        assert!(!gone.exists(), "looking makes nothing");
        // One that is there is looked at without anything being made in it.
        std::fs::create_dir(&gone).unwrap();
        let s = FolderStore::look_at(&gone).unwrap();
        assert!(s.list_all("originals/").await.unwrap().is_empty());
        assert_eq!(std::fs::read_dir(&gone).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_folder_that_cannot_be_read_part_way_is_a_problem_never_a_shorter_list() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, s) = store().await;
        for k in ["originals/a/1.jpg", "originals/b/2.jpg"] {
            s.put(
                &ObjectKey::new(k).unwrap(),
                ByteSource::Bytes(b"x".to_vec()),
                &PutOptions::default(),
            )
            .await
            .unwrap();
        }
        let locked = dir.path().join("library").join("originals").join("b");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let listed = s.list_all("originals/").await;
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        // Running as an administrator reads anything, which proves nothing.
        if std::fs::read_dir(&locked).is_ok() && listed.is_ok() && listed.as_ref().unwrap().len() == 2 {
            return;
        }
        assert!(listed.is_err(), "{listed:?}");
    }

    mod sweeping {
        use super::super::*;
        use std::fs;
        use std::time::Duration;

        const HOURS_AGO: Duration = Duration::from_secs(2 * 60 * 60);

        fn write_aged(path: &Path, bytes: &[u8], age: Duration) {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
            let file = fs::File::options().write(true).open(path).unwrap();
            file.set_modified(SystemTime::now() - age).unwrap();
        }

        /// A backup folder with real files, as a backup leaves it.
        fn backup() -> (tempfile::TempDir, PathBuf) {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("Backup");
            write_aged(&root.join("originals/2019/IMG_0001.jpg"), b"a photo", HOURS_AGO);
            write_aged(&root.join("index/gallery.json"), b"{}", HOURS_AGO);
            (dir, root)
        }

        fn backed_up_untouched(root: &Path) {
            assert_eq!(fs::read(root.join("originals/2019/IMG_0001.jpg")).unwrap(), b"a photo");
            assert_eq!(fs::read(root.join("index/gallery.json")).unwrap(), b"{}");
        }

        #[test]
        fn abandoned_copies_and_parts_are_cleared_and_the_working_folder_goes_when_empty() {
            let (_dir, root) = backup();
            let work = root.join(WORK_DIR);
            write_aged(&work.join("tmp-18e8662"), &[1u8; 1000], HOURS_AGO);
            // An upload in parts left by an older version: 2 parts and its record.
            write_aged(
                &work.join("18e866207c01f87097a80/target"),
                b"1\noriginals/big.mov",
                HOURS_AGO,
            );
            write_aged(&work.join("18e866207c01f87097a80/part-00001"), &[2u8; 3000], HOURS_AGO);
            write_aged(
                &work.join("18e866207c01f87097a80/part-00002.tmp"),
                &[3u8; 500],
                HOURS_AGO,
            );

            let swept = sweep_working_files(&root, ABANDONED_AFTER).unwrap();
            assert_eq!(
                swept,
                Swept {
                    files: 4,
                    bytes: 1000 + 3000 + 500 + 19,
                    ..Swept::default()
                }
            );
            assert!(!work.exists(), "the empty working folder is removed");
            backed_up_untouched(&root);
            assert_eq!(
                sweep_working_files(&root, ABANDONED_AFTER).unwrap(),
                Swept::default(),
                "nothing twice"
            );
        }

        #[test]
        fn what_would_be_cleared_is_said_first_and_nothing_is_removed_until_asked() {
            let (_dir, root) = backup();
            let work = root.join(WORK_DIR);
            write_aged(&work.join("tmp-old"), &[1u8; 1000], HOURS_AGO);
            write_aged(&work.join("abc123/part-00001"), &[2u8; 3000], HOURS_AGO);
            write_aged(&work.join("tmp-live"), &[1u8; 10], Duration::ZERO);
            write_aged(&work.join("notes.txt"), b"someone's", HOURS_AGO);

            let first = working_files_to_clear(&root, ABANDONED_AFTER).unwrap();
            assert_eq!(
                first,
                Swept {
                    files: 2,
                    bytes: 4000,
                    in_use: 1,
                    not_ours: 1
                }
            );
            assert!(work.join("tmp-old").is_file() && work.join("abc123/part-00001").is_file());
            assert_eq!(
                working_files_to_clear(&root, ABANDONED_AFTER).unwrap(),
                first,
                "asking twice changes nothing"
            );
            assert_eq!(
                sweep_working_files(&root, ABANDONED_AFTER).unwrap(),
                first,
                "it does what it said"
            );
            assert!(!work.join("tmp-old").exists());
            assert_eq!(first.left(), 2);
            backed_up_untouched(&root);
        }

        #[test]
        fn a_copy_another_program_is_making_right_now_is_left_alone() {
            let (_dir, root) = backup();
            let work = root.join(WORK_DIR);
            // Written a moment ago: live, whoever is writing it.
            write_aged(&work.join("tmp-live"), &[1u8; 100], Duration::ZERO);
            write_aged(&work.join("abc123/target"), b"1\noriginals/big.mov", HOURS_AGO);
            write_aged(&work.join("abc123/part-00001"), &[2u8; 100], HOURS_AGO);
            write_aged(&work.join("abc123/part-00002"), &[2u8; 100], Duration::ZERO);
            write_aged(&work.join("tmp-old"), &[1u8; 50], HOURS_AGO);

            let swept = sweep_working_files(&root, ABANDONED_AFTER).unwrap();
            assert_eq!((swept.files, swept.bytes, swept.in_use, swept.not_ours), (1, 50, 2, 0));
            assert!(work.join("tmp-live").is_file());
            assert!(
                work.join("abc123/part-00001").is_file(),
                "an upload still being sent is kept whole"
            );
            backed_up_untouched(&root);
        }

        #[test]
        fn what_keevoro_does_not_recognise_is_left_where_it_is() {
            let (_dir, root) = backup();
            let work = root.join(WORK_DIR);
            write_aged(&work.join("notes.txt"), b"someone's", HOURS_AGO);
            write_aged(&work.join("Holiday/IMG_1.jpg"), b"someone's", HOURS_AGO);
            let swept = sweep_working_files(&root, ABANDONED_AFTER).unwrap();
            assert_eq!((swept.files, swept.in_use, swept.not_ours), (0, 0, 2));
            assert!(work.join("notes.txt").is_file() && work.join("Holiday/IMG_1.jpg").is_file());
        }

        #[test]
        fn a_folder_with_no_working_files_is_fine() {
            let (_dir, root) = backup();
            assert_eq!(sweep_working_files(&root, ABANDONED_AFTER).unwrap(), Swept::default());
            assert_eq!(
                sweep_working_files(&root.join("not there"), ABANDONED_AFTER).unwrap(),
                Swept::default()
            );
        }

        #[cfg(unix)]
        #[test]
        fn links_are_never_followed_out_of_the_working_folder() {
            let (dir, root) = backup();
            let work = root.join(WORK_DIR);
            let elsewhere = dir.path().join("Elsewhere");
            write_aged(&elsewhere.join("tmp-precious"), b"keep", HOURS_AGO);
            write_aged(&elsewhere.join("part-00001"), b"keep", HOURS_AGO);
            fs::create_dir_all(&work).unwrap();
            std::os::unix::fs::symlink(elsewhere.join("tmp-precious"), work.join("tmp-link")).unwrap();
            std::os::unix::fs::symlink(&elsewhere, work.join("abcdef")).unwrap();
            let swept = sweep_working_files(&root, ABANDONED_AFTER).unwrap();
            assert_eq!((swept.files, swept.left()), (0, 2));
            assert_eq!(fs::read(elsewhere.join("tmp-precious")).unwrap(), b"keep");
            assert_eq!(fs::read(elsewhere.join("part-00001")).unwrap(), b"keep");

            // A working folder that is itself a link leads somewhere else:
            // nothing in it is Keevoro's to remove.
            fs::remove_dir_all(&work).unwrap();
            std::os::unix::fs::symlink(&elsewhere, &work).unwrap();
            let swept = sweep_working_files(&root, ABANDONED_AFTER).unwrap();
            assert_eq!((swept.files, swept.left()), (0, 1));
            assert!(elsewhere.join("tmp-precious").is_file());
        }

        #[cfg(unix)]
        #[test]
        fn a_link_inside_an_upload_keeps_the_whole_upload() {
            let (dir, root) = backup();
            let work = root.join(WORK_DIR);
            let outside = dir.path().join("outside.bin");
            write_aged(&outside, b"keep", HOURS_AGO);
            write_aged(&work.join("abc/target"), b"1\nx", HOURS_AGO);
            std::os::unix::fs::symlink(&outside, work.join("abc/part-00001")).unwrap();
            let swept = sweep_working_files(&root, ABANDONED_AFTER).unwrap();
            assert_eq!((swept.files, swept.left()), (0, 1));
            assert_eq!(fs::read(&outside).unwrap(), b"keep");
            assert!(work.join("abc/target").is_file());
        }
    }
}
