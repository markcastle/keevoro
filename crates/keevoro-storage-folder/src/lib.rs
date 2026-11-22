//! A storage provider that writes to a local folder, external drive or NAS
//! share.
//!
//! It is a real option for people who want their archive on a second disk,
//! and it is the reference provider for the conformance suite.
//!
//! Every write goes to a temporary file under `.keevoro-uploads/` inside the
//! root first, then moves into place, so a crash never leaves a half-written
//! file under its final name. Only once it is at its final name is it read
//! back and proved, so what is proved is what a later look will find, even
//! on a network share that dropped out while the file was moved into place.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use async_trait::async_trait;
use keevoro_storage::{
    ByteSource, Capabilities, ConnectionReport, Downloaded, ListPage, MultipartLimits, MultipartStore, ObjectInfo,
    ObjectKey, ObjectStore, PartReceipt, PutOptions, PutReceipt, Step, StorageError, UploadId,
};

mod aside;
mod files;
pub use aside::{note_words, set_aside, set_aside_folder};
pub use files::{Files, RealFiles, Written};
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
    /// What files are written through: the computer's own, or a stand-in
    /// that drops out, in tests.
    files: Arc<dyn Files>,
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
            files: Arc::new(RealFiles),
        }
    }

    /// Writes files through `files` instead of the computer's own: how a
    /// test makes a share that drops out part-way.
    #[must_use]
    pub fn with_files(mut self, files: Arc<dyn Files>) -> Self {
        self.files = files;
        self
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
                modified: m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .and_then(|d| i64::try_from(d.as_secs()).ok()),
                ..ObjectInfo::new(key.clone(), m.len())
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
        let copy = Copy {
            files: self.files.clone(),
            tmp: self.temp_path().await,
            dest: self.path_for(key)?,
            key: key.clone(),
            create_only: opts.create_only,
            watcher: self.watcher.clone(),
            expect: opts.expect_blake3.clone(),
            learn: opts.learn_blake3,
        };
        let (bytes, blake3) = tokio::task::spawn_blocking(move || copy.store(body))
            .await
            .map_err(|e| StorageError::Provider(e.to_string()))??;
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
        keys.retain(|(k, _, _)| k.starts_with(&prefix) && continuation.as_deref().is_none_or(|c| k.as_str() > c));
        keys.sort();
        let more = keys.len() > PAGE_SIZE;
        keys.truncate(PAGE_SIZE);
        let next = if more {
            keys.last().map(|(k, _, _)| k.clone())
        } else {
            None
        };
        let objects = keys
            .into_iter()
            .map(|(k, bytes, modified)| {
                Ok(ObjectInfo {
                    modified,
                    ..ObjectInfo::new(ObjectKey::new(k)?, bytes)
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
            .map(|(k, bytes, modified)| {
                Ok(ObjectInfo {
                    modified,
                    ..ObjectInfo::new(ObjectKey::new(k)?, bytes)
                })
            })
            .collect()
    }

    async fn download_to(&self, key: &ObjectKey, file: &Path) -> Result<Downloaded, StorageError> {
        let from = self.path_for(key)?;
        let to = file.to_path_buf();
        let k = key.clone();
        tokio::task::spawn_blocking(move || -> Result<Downloaded, StorageError> {
            use std::io::{Read, Write};
            let mut input = std::fs::File::open(&from).map_err(|e| not_found_or(e, &k))?;
            // Never over a file already there; only what this made is
            // removed if the copy goes wrong.
            let mut out = std::fs::OpenOptions::new().write(true).create_new(true).open(&to)?;
            let copied = (|| -> Result<Downloaded, StorageError> {
                let mut hasher = blake3::Hasher::new();
                let mut buffer = vec![0u8; COPY_BUFFER];
                let mut bytes = 0u64;
                loop {
                    match input.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(n) => {
                            hasher.update(&buffer[..n]);
                            out.write_all(&buffer[..n])?;
                            bytes += n as u64;
                        }
                        Err(e) if e.kind() == ErrorKind::Interrupted => {}
                        Err(e) => return Err(e.into()),
                    }
                }
                out.sync_all()?;
                Ok(Downloaded {
                    bytes,
                    blake3: hasher.finalize().to_hex().to_string(),
                })
            })();
            if copied.is_err() {
                drop(out);
                let _ = std::fs::remove_file(&to);
            }
            copied
        })
        .await
        .map_err(|e| StorageError::Provider(e.to_string()))?
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
        let bytes = write_part(&tmp, body).await?;
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

/// Writes one part of an upload in parts, which `complete` later joins.
/// Whole files go through [`Copy`], which proves them.
async fn write_part(dest: &Path, body: ByteSource) -> Result<u64, StorageError> {
    let bytes = match body {
        ByteSource::Bytes(b) => b,
        ByteSource::File(src) => fs::read(src).await?,
    };
    let mut f = fs::File::create(dest).await?;
    f.write_all(&bytes).await?;
    f.sync_all().await?;
    Ok(bytes.len() as u64)
}

/// One file being stored: where it is written first, where it goes, and how
/// it is to be proved.
struct Copy {
    files: Arc<dyn Files>,
    tmp: PathBuf,
    dest: PathBuf,
    key: ObjectKey,
    create_only: bool,
    watcher: Option<Watcher>,
    /// The fingerprint the file is known by, which what is read must match.
    expect: Option<String>,
    /// Asked to give back the fingerprint, for a file a search only sampled.
    learn: bool,
}

impl Copy {
    /// Stores `body` under its real name and proves it there, telling the
    /// watcher as it goes. The source is only ever read. Answers its size
    /// and, when it was asked for or known, its fingerprint.
    ///
    /// The proof, because a backup nobody has checked is only a hope:
    ///
    /// 1. What is read from the source is fingerprinted as it goes by. If
    ///    that is not the fingerprint expected, the file has changed since
    ///    it was looked at, and it is refused.
    /// 2. The copy is written to a working file, flushed to the disk or the
    ///    far end, and moved into place without ever replacing a file.
    /// 3. Then, at its real name, its size is asked afresh and it is opened
    ///    afresh and read back in full. Only if that is the full size and the
    ///    same fingerprint does it count. This is after the last write and
    ///    the move, so a share that dropped out and came back while the file
    ///    was moved, leaving it short or not there at all, is caught.
    ///
    /// Any error at any step means it does not count. A working file is
    /// removed when it can be, and otherwise left for the sweep. A copy found
    /// wrong at its real name is left where it is, since nothing in a
    /// backup is ever deleted: it is never counted, and the next backup
    /// finds it and says so.
    fn store(self, body: ByteSource) -> Result<(u64, Option<String>), StorageError> {
        let files = self.files.as_ref();
        let written = self.write(files, body);
        let (total, fingerprint) = match written {
            Ok(done) => done,
            Err(e) => {
                let _ = files.remove(&self.tmp);
                return Err(e);
            }
        };
        let linked = match self.place(files) {
            Ok(linked) => linked,
            Err(e) => {
                let _ = files.remove(&self.tmp);
                return Err(e);
            }
        };
        let proved = self.prove(files, total, &fingerprint);
        if linked {
            // The working name is a second name for the same file. If it
            // cannot be removed now, the sweep removes it later; the copy at
            // its real name is unaffected.
            let _ = files.remove(&self.tmp);
        }
        proved?;
        let prove = self.expect.is_some() || self.learn;
        Ok((total, prove.then_some(fingerprint)))
    }

    /// Writes the working file and flushes it, answering its size and the
    /// fingerprint of what was read.
    fn write(&self, files: &dyn Files, body: ByteSource) -> Result<(u64, String), StorageError> {
        use std::io::Read;
        let mut output = files
            .create_new(&self.tmp)
            .map_err(|e| StorageError::at(Step::Writing, e))?;
        let mut read = blake3::Hasher::new();
        let mut total = 0u64;
        let mut modified = None;
        let mut put = |output: &mut Box<dyn Written>, piece: &[u8], total: &mut u64| -> Result<(), StorageError> {
            read.update(piece);
            output
                .write_all(piece)
                .map_err(|e| StorageError::at(Step::Writing, e))?;
            *total += piece.len() as u64;
            if let Some(tell) = &self.watcher {
                tell(piece.len() as u64);
            }
            Ok(())
        };
        match body {
            ByteSource::Bytes(bytes) => put(&mut output, &bytes, &mut total)?,
            ByteSource::File(src) => {
                let mut input = std::fs::File::open(&src).map_err(|e| StorageError::at(Step::ReadingTheSource, e))?;
                let mut buffer = vec![0u8; COPY_BUFFER];
                loop {
                    let n = match input.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                        Err(e) => return Err(StorageError::at(Step::ReadingTheSource, e)),
                    };
                    put(&mut output, &buffer[..n], &mut total)?;
                }
                modified = input.metadata().and_then(|m| m.modified()).ok();
            }
        }
        output.flush().map_err(|e| StorageError::at(Step::Writing, e))?;
        // Keep the original's modified time: it is sometimes the only clue
        // to when a photo was taken.
        if let Some(modified) = modified {
            let _ = output.set_modified(modified);
        }
        let fingerprint = read.finalize().to_hex().to_string();
        if self.expect.as_deref().is_some_and(|expected| expected != fingerprint) {
            return Err(StorageError::NotTheSame(
                "the file is not as it was when it was looked at".to_string(),
            ));
        }
        output.sync().map_err(|e| StorageError::at(Step::Flushing, e))?;
        drop(output);
        Ok((total, fingerprint))
    }

    /// Gives the working file its real name, never over a file already
    /// there. Answers whether the working name is still a second name for
    /// it (a hard link), to be removed once it is proved.
    fn place(&self, files: &dyn Files) -> Result<bool, StorageError> {
        let moving = |e| StorageError::at(Step::MovingIntoPlace, e);
        if let Some(parent) = self.dest.parent() {
            files.make_folders(parent).map_err(moving)?;
        }
        if !self.create_only {
            files.rename(&self.tmp, &self.dest).map_err(moving)?;
            return Ok(false);
        }
        // A hard link fails, all at once, if the name is taken.
        match files.hard_link(&self.tmp, &self.dest) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => Err(StorageError::AlreadyExists(self.key.to_string())),
            Err(_) => {
                // FAT and exFAT drives, and some shares, have no hard links.
                // Check, then move.
                if files.exists(&self.dest).map_err(moving)? {
                    return Err(StorageError::AlreadyExists(self.key.to_string()));
                }
                files.rename(&self.tmp, &self.dest).map_err(moving)?;
                Ok(false)
            }
        }
    }

    /// Reads the copy back at its real name, afresh, and compares it with
    /// what was written.
    fn prove(&self, files: &dyn Files, total: u64, fingerprint: &str) -> Result<(), StorageError> {
        use std::io::Read;
        let back = |e| StorageError::at(Step::ReadingBack, e);
        match files.size(&self.dest).map_err(back)? {
            None => {
                return Err(StorageError::NotTheSame(
                    "it was not there once it had been moved into place".to_string(),
                ))
            }
            Some(size) if size != total => {
                return Err(StorageError::NotTheSame(format!(
                    "once moved into place it was {size} bytes, not {total}"
                )))
            }
            Some(_) => {}
        }
        let mut input = files.open(&self.dest).map_err(back)?;
        let mut written = blake3::Hasher::new();
        let mut buffer = vec![0u8; COPY_BUFFER];
        let mut read = 0u64;
        loop {
            match input.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    written.update(&buffer[..n]);
                    read += n as u64;
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(back(e)),
            }
        }
        if read != total || written.finalize().to_hex().as_str() != fingerprint {
            return Err(StorageError::NotTheSame(
                "what was written did not read back the same".to_string(),
            ));
        }
        Ok(())
    }
}

/// Walks the root and returns `(key, bytes)` for every file outside the work area.
fn collect_keys(root: &Path) -> Result<Vec<(String, u64, Option<i64>)>, StorageError> {
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
                let meta = entry.metadata()?;
                out.push((key, meta.len(), seconds(&meta)));
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
fn list_beneath(root: &Path, prefix: &str, at_once: usize) -> Result<Vec<(String, u64, Option<i64>)>, StorageError> {
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
            for (key, bytes, modified) in listed.files {
                if key.starts_with(prefix) {
                    out.push((key, bytes, modified));
                }
            }
            next.extend(listed.folders);
        }
        first = false;
        level = next;
    }
    Ok(out)
}

/// When a file was last written, in seconds since 1970, from what its
/// listing already gave.
fn seconds(meta: &std::fs::Metadata) -> Option<i64> {
    let since = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;
    i64::try_from(since.as_secs()).ok()
}

/// What one folder holds: its files by key with their sizes, and its
/// folders, to be read next.
struct Listed {
    files: Vec<(String, u64, Option<i64>)>,
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
            let meta = entry.metadata()?;
            listed.files.push((key, meta.len(), seconds(&meta)));
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
mod dropping;

#[cfg(test)]
mod tests {
    use super::*;

    async fn store() -> (tempfile::TempDir, FolderStore) {
        let dir = tempfile::tempdir().unwrap();
        let s = FolderStore::open(dir.path().join("library")).await.unwrap();
        (dir, s)
    }

    #[tokio::test]
    #[ignore = "run by hand: it copies 600 MB"]
    async fn how_long_copying_200_photos_takes() {
        let dir = tempfile::tempdir().unwrap();
        let store = FolderStore::open(dir.path().join("to")).await.unwrap();
        let mut sources = Vec::new();
        for i in 0..200u32 {
            let p = dir.path().join(format!("src-{i}.jpg"));
            let bytes: Vec<u8> = (0..3_000_000u32).map(|b| (b.wrapping_mul(i + 7) % 251) as u8).collect();
            let hash = blake3::hash(&bytes).to_hex().to_string();
            std::fs::write(&p, &bytes).unwrap();
            sources.push((p, hash));
        }
        let started = std::time::Instant::now();
        for (i, (p, hash)) in sources.iter().enumerate() {
            let key = ObjectKey::new(format!("originals/{i}.jpg")).unwrap();
            let opts = PutOptions {
                create_only: true,
                expect_blake3: Some(hash.clone()),
                ..Default::default()
            };
            store.put(&key, ByteSource::File(p.clone()), &opts).await.unwrap();
        }
        let took = started.elapsed();
        println!(
            "200 photos of 3 MB: {took:?}, {:.0} MB a second",
            600.0 / took.as_secs_f64()
        );
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
