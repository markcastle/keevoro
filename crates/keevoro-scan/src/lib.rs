//! Finds photos and videos under a source folder or drive.
//!
//! The scanner only ever reads. It walks the tree once, passes over files
//! that are plainly programs, documents or music without opening them, and
//! opens the rest a few at a time: it detects media by content rather than
//! extension, hashes each file with BLAKE3 while reading it, and collects
//! every date clue it can find.
//!
//! Problems never stop a scan. Unreadable files and folders are reported so
//! the user can try again later.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod online;
pub mod survey;
mod video;

pub use online::{ByAttributes, OnlineCheck};

use keevoro_core::{
    best_date, date_from_filename, folder_hints, folder_made_by_keevoro, holds_originals, is_raw_extension, junk_hint,
    kind_from_mime, library_album_segments, never_media_extension, photo_library, DateGuess, DateSource, FolderHints,
    JunkInput, JunkReason, Kinds, MediaKind, PartialDate, SkipReason, SkipRules,
};
use serde::Serialize;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

/// Bytes read up front for content detection.
const SNIFF_BYTES: usize = 8 * 1024;
const READ_CHUNK: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub skip: SkipRules,
    /// Hash every media file. Needed for duplicates; can be turned off for a
    /// quick first look.
    pub hash: bool,
    /// Fingerprint large files from a few samples instead of reading them
    /// to the end, so a search takes seconds where it took minutes. Such an
    /// item is marked `hashed: false` until [`whole_hash`] has been run on
    /// it, or it is fingerprinted as it is backed up.
    pub quick: bool,
    /// Set to true from anywhere to stop the search. It ends within a
    /// moment, and its report says it was stopped.
    pub stop: Option<Arc<AtomicBool>>,
    /// The kinds of file looked for, read once from the settings before
    /// the search starts. The built-in list unless told otherwise.
    pub kinds: Kinds,
    /// How a file only in online storage is told apart, so it is never
    /// opened. The file's own attributes unless told otherwise.
    pub online: Arc<dyn OnlineCheck>,
}

impl ScanOptions {
    fn stopped(&self) -> bool {
        self.stop.as_ref().is_some_and(|stop| stop.load(Ordering::SeqCst))
    }
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            skip: SkipRules::default(),
            hash: true,
            quick: false,
            stop: None,
            kinds: Kinds::default(),
            online: Arc::new(ByAttributes),
        }
    }
}

/// One photo or video found on a source.
#[derive(Debug, Clone, Serialize)]
pub struct FoundItem {
    pub path: PathBuf,
    pub kind: MediaKind,
    pub mime: String,
    pub bytes: u64,
    /// The file's fingerprint, hex. `None` when hashing is off. When
    /// `hashed` is true this is the BLAKE3 hash of the whole file; when it
    /// is false it is a quick fingerprint (see [`ScanOptions::quick`]).
    pub blake3: Option<String>,
    /// True when `blake3` covers every byte of the file. Only then may the
    /// file be copied, or called a duplicate for certain.
    pub hashed: bool,
    pub date: Option<DateGuess>,
    pub folder: FolderHints,
    /// Pixel size of a photo, when it could be read from the file header.
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Set when the file is probably not a memory (an app icon, a web page
    /// leftover). It is still listed, just left unticked.
    pub junk: Option<JunkReason>,
}

impl FoundItem {
    /// The short content id used in the index: the first 16 hex characters
    /// (64 bits) of the BLAKE3 hash.
    pub fn id(&self) -> Option<&str> {
        self.blake3.as_deref().map(|h| &h[..16])
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SkippedFolder {
    pub path: PathBuf,
    pub reason: SkipReason,
}

#[derive(Debug, Clone, Serialize)]
pub struct Unreadable {
    pub path: PathBuf,
    pub error: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ScanReport {
    pub root: PathBuf,
    pub items: Vec<FoundItem>,
    pub skipped: Vec<SkippedFolder>,
    pub unreadable: Vec<Unreadable>,
    /// Files looked at that were not photos or videos.
    pub other_files: u64,
    /// Files that could be photos or videos but are only in online storage
    /// (OneDrive's "files on demand", say), not on this computer. They are
    /// passed over unopened, as opening one downloads it.
    pub online_only: Vec<PathBuf>,
    /// True when the search was stopped before it had looked everywhere,
    /// so this is only part of what is there.
    #[serde(skip)]
    pub stopped: bool,
}

/// Live counts for a progress display, and where the search has got to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanProgress {
    pub files_seen: u64,
    pub photos: u64,
    pub videos: u64,
    /// Bytes of photos and videos read so far.
    pub bytes: u64,
    /// The folder of the file most recently looked at.
    pub folder: PathBuf,
    /// A large file being read right now, and its size. Reading one can
    /// take minutes, during which nothing else here changes.
    pub reading: Option<(PathBuf, u64)>,
}

/// Files at least this big are announced before they are read.
const WORTH_ANNOUNCING: u64 = 64 * 1024 * 1024;

/// What became of one thing the walk came across.
enum Looked {
    Media(Box<FoundItem>),
    Other,
    /// Only in online storage: not opened.
    OnlineOnly,
    Unreadable(String),
    Skipped(SkipReason),
    /// A reader is about to start on a large file of this many bytes.
    Reading(u64),
}

/// How many files to look at at once. More than one because most of a
/// search is waiting for the drive; not many, because an old spinning disk
/// gets slower, not faster, when asked for too much at the same time.
fn readers() -> usize {
    std::thread::available_parallelism().map_or(2, |n| n.get().clamp(2, 4))
}

/// Scans `root`, calling `on_progress` after every file.
///
/// One thread walks the folders and hands likely files to a few readers.
/// The report comes out in the same order whatever order they finish in.
pub fn scan(root: &Path, opts: &ScanOptions, on_progress: &mut dyn FnMut(&ScanProgress)) -> ScanReport {
    let mut report = ScanReport {
        root: root.to_path_buf(),
        ..Default::default()
    };
    let mut progress = ScanProgress {
        folder: root.to_path_buf(),
        ..Default::default()
    };
    // A short queue, so the walk cannot run far ahead of the reading.
    let (to_read, waiting) = mpsc::sync_channel::<(u64, PathBuf)>(256);
    let waiting = Mutex::new(waiting);
    let (done, results) = mpsc::channel::<(u64, PathBuf, Looked)>();
    let mut found: Vec<(u64, FoundItem)> = Vec::new();
    let mut skipped: Vec<(u64, SkippedFolder)> = Vec::new();
    let mut unreadable: Vec<(u64, Unreadable)> = Vec::new();
    let mut online: Vec<(u64, PathBuf)> = Vec::new();

    std::thread::scope(|threads| {
        for _ in 0..readers() {
            let done = done.clone();
            let waiting = &waiting;
            threads.spawn(move || loop {
                let next = match waiting.lock() {
                    Ok(queue) => queue.recv(),
                    Err(_) => break,
                };
                let Ok((order, path)) = next else { break };
                keevoro_core::pause::wait_while_paused();
                // Asked to stop: let the queue empty without opening anything more.
                if opts.stopped() {
                    continue;
                }
                if let Some(bytes) = std::fs::metadata(&path)
                    .ok()
                    .map(|m| m.len())
                    .filter(|b| *b >= WORTH_ANNOUNCING)
                {
                    let _ = done.send((order, path.clone(), Looked::Reading(bytes)));
                }
                let looked = match examine(root, &path, opts) {
                    Ok(Some(item)) => Looked::Media(Box::new(item)),
                    Ok(None) => Looked::Other,
                    Err(e) => Looked::Unreadable(e.to_string()),
                };
                if done.send((order, path, looked)).is_err() {
                    break;
                }
            });
        }
        threads.spawn(move || walk(root, opts, &to_read, &done));

        for (order, path, looked) in results {
            if progress.reading.as_ref().is_some_and(|(reading, _)| *reading == path) {
                progress.reading = None;
            }
            match looked {
                Looked::Reading(bytes) => {
                    progress.reading = Some((path, bytes));
                    on_progress(&progress);
                    continue;
                }
                Looked::Skipped(reason) => {
                    skipped.push((order, SkippedFolder { path, reason }));
                    continue;
                }
                Looked::Unreadable(error) if !path.is_file() => {
                    // A folder that could not be opened, not a file.
                    unreadable.push((order, Unreadable { path, error }));
                    continue;
                }
                Looked::Unreadable(error) => unreadable.push((
                    order,
                    Unreadable {
                        path: path.clone(),
                        error,
                    },
                )),
                Looked::Other => report.other_files += 1,
                Looked::OnlineOnly => online.push((order, path.clone())),
                Looked::Media(item) => {
                    match item.kind {
                        MediaKind::Photo => progress.photos += 1,
                        MediaKind::Video => progress.videos += 1,
                    }
                    progress.bytes += item.bytes;
                    found.push((order, *item));
                }
            }
            progress.files_seen += 1;
            if let Some(folder) = path.parent() {
                if folder != progress.folder {
                    progress.folder = folder.to_path_buf();
                }
            }
            on_progress(&progress);
        }
    });

    // Sorted, so the same drive always gives the same report.
    found.sort_by_key(|(order, _)| *order);
    skipped.sort_by_key(|(order, _)| *order);
    unreadable.sort_by_key(|(order, _)| *order);
    report.items = found.into_iter().map(|(_, item)| item).collect();
    report.skipped = skipped.into_iter().map(|(_, s)| s).collect();
    report.unreadable = unreadable.into_iter().map(|(_, u)| u).collect();
    online.sort_by_key(|(order, _)| *order);
    report.online_only = online.into_iter().map(|(_, p)| p).collect();
    report.stopped = opts.stopped();
    report
}

/// Walks the folders under `root` in name order. Files worth opening go to
/// the readers; everything already decided goes straight to the results.
fn walk(
    root: &Path,
    opts: &ScanOptions,
    to_read: &mpsc::SyncSender<(u64, PathBuf)>,
    done: &mpsc::Sender<(u64, PathBuf, Looked)>,
) {
    let mut order = 0u64;
    let mut walk = WalkDir::new(root).follow_links(false).sort_by_file_name().into_iter();
    while let Some(entry) = walk.next() {
        keevoro_core::pause::wait_while_paused();
        if opts.stopped() {
            break;
        }
        order += 1;
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                let path = e.path().map_or_else(|| root.to_path_buf(), Path::to_path_buf);
                let _ = done.send((order, path, Looked::Unreadable(e.to_string())));
                continue;
            }
        };
        let name = entry.file_name().to_string_lossy();
        if entry.file_type().is_dir() {
            // What Keevoro made is never a source: a backup, a backup's own
            // folder, an old library, Keevoro's own data. Judged by what is
            // in it, or by being one of Keevoro's places on this computer.
            let made = opts
                .skip
                .own_place(entry.path())
                .or_else(|| folder_made_by_keevoro(entry.path()));
            if let Some(reason) = made {
                let _ = done.send((order, entry.path().to_path_buf(), Looked::Skipped(reason)));
                walk.skip_current_dir();
                continue;
            }
            if inside_library_package(entry.path()) && !holds_originals(&name) {
                // Thumbnails, previews and databases: copies of what is in
                // the originals folder, so not worth reporting as skipped.
                walk.skip_current_dir();
                continue;
            }
            if entry.depth() > 0 {
                if let Some(reason) = opts.skip.skip_folder(&name) {
                    let _ = done.send((order, entry.path().to_path_buf(), Looked::Skipped(reason)));
                    walk.skip_current_dir();
                }
            }
            continue;
        }
        if !entry.file_type().is_file() || opts.skip.skip_file(&name).is_some() {
            continue;
        }
        let passed_over = entry.path().extension().is_some_and(|ext| {
            let ext = ext.to_string_lossy();
            match opts.kinds.looked_for(&ext) {
                Some(_) => false,
                None => opts.kinds.switched_off(&ext) || never_media_extension(&ext),
            }
        });
        // Only in online storage: opening it would download it. Asked of
        // what the listing says, and only of files that could be photos.
        let online = !passed_over
            && entry
                .metadata()
                .is_ok_and(|listing| opts.online.online_only(entry.path(), &listing));
        let path = entry.into_path();
        let sent = if passed_over {
            // Programs, documents and music, and kinds the person switched
            // off, are not opened at all.
            done.send((order, path, Looked::Other)).is_ok()
        } else if online {
            done.send((order, path, Looked::OnlineOnly)).is_ok()
        } else {
            to_read.send((order, path)).is_ok()
        };
        if !sent {
            break;
        }
    }
}

/// Looks at one file. `Ok(None)` means it is not a photo or video.
fn examine(root: &Path, path: &Path, opts: &ScanOptions) -> std::io::Result<Option<FoundItem>> {
    let mut file = File::open(path)?;
    let mut head = vec![0u8; SNIFF_BYTES];
    let n = read_up_to(&mut file, &mut head)?;
    head.truncate(n);

    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default();
    // What is inside decides, where Keevoro recognises it. A kind known
    // only by its name (most camera raw, and every kind a person added)
    // is taken at its word when the inside says nothing, or says
    // something Keevoro does not count as a photo.
    let inside = infer::get(&head).map(|t| t.mime_type());
    let (kind, mime) = match (inside.and_then(kind_from_mime), opts.kinds.by_name(&ext)) {
        (Some(k), _) => (k, inside.unwrap_or_default().to_string()),
        (None, Some(k)) => (
            k,
            inside.map_or_else(|| format!("application/x-{}", ext.to_lowercase()), str::to_string),
        ),
        (None, None) => return Ok(None),
    };

    let meta = file.metadata()?;
    let (blake3, hashed) = if !opts.hash {
        (None, false)
    } else if opts.quick && meta.len() > QUICK_ABOVE {
        (Some(quick_fingerprint(&mut file, meta.len())?), false)
    } else {
        (Some(hash_rest(&mut file, &head)?), true)
    };

    let rel_dirs: Vec<String> = path
        .strip_prefix(root)
        .ok()
        .and_then(Path::parent)
        .map(|p| {
            p.components()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    let folder = match rel_dirs.iter().position(|d| photo_library(d).is_some()) {
        // Inside a Photos or iPhoto library only the folders below the
        // originals folder can be names the user chose.
        Some(lib) => folder_hints(&library_album_segments(rel_dirs.get(lib + 2..).unwrap_or_default())),
        None => folder_hints(&rel_dirs),
    };

    let mut guesses = Vec::new();
    let mut size = None;
    match kind {
        MediaKind::Photo => {
            if let Some(d) = exif_date(path) {
                guesses.push(DateGuess {
                    date: d,
                    source: DateSource::Exif,
                });
            }
            // A raw file's header gives the size of its small built-in
            // preview, not of the photo, so it is not read at all: better
            // not to know than to be fooled into calling it tiny.
            if !is_raw_extension(&ext) {
                size = pixel_size(&mut file);
            }
        }
        MediaKind::Video => {
            if let Some(d) = video::recording_date(&mut file) {
                guesses.push(DateGuess {
                    date: d,
                    source: DateSource::Video,
                });
            }
        }
    }
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    if let Some(d) = date_from_filename(&file_name) {
        guesses.push(DateGuess {
            date: d,
            source: DateSource::Filename,
        });
    }
    if let Some(d) = folder.date {
        guesses.push(DateGuess {
            date: d,
            source: DateSource::Folder,
        });
    }
    if let Some(d) = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .and_then(PartialDate::from_unix_seconds)
    {
        guesses.push(DateGuess {
            date: d,
            source: DateSource::Modified,
        });
    }

    Ok(Some(FoundItem {
        path: path.to_path_buf(),
        kind,
        mime,
        bytes: meta.len(),
        blake3,
        hashed,
        date: best_date(&guesses),
        junk: junk_hint(&JunkInput {
            kind,
            size,
            folders: &rel_dirs,
            file_name: &file_name,
        }),
        folder,
        width: size.map(|s| s.0),
        height: size.map(|s| s.1),
    }))
}

/// True when `dir` sits directly inside a Photos, iPhoto or Aperture library.
fn inside_library_package(dir: &Path) -> bool {
    dir.parent()
        .and_then(Path::file_name)
        .is_some_and(|n| photo_library(&n.to_string_lossy()).is_some())
}

/// Reads a photo's pixel size from its header without decoding the picture.
fn pixel_size(file: &mut File) -> Option<(u32, u32)> {
    file.seek(SeekFrom::Start(0)).ok()?;
    let size = imagesize::reader_size(BufReader::new(file)).ok()?;
    Some((u32::try_from(size.width).ok()?, u32::try_from(size.height).ok()?))
}

fn read_up_to(r: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match r.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

/// Hashes the bytes already read plus the rest of the file, in one pass.
fn hash_rest(file: &mut File, head: &[u8]) -> std::io::Result<String> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(head);
    let mut buf = vec![0u8; READ_CHUNK];
    loop {
        match file.read(&mut buf)? {
            0 => break,
            n => {
                hasher.update(&buf[..n]);
            }
        }
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// Files this size or smaller are simply read whole: the samples below
/// would cover them anyway.
const QUICK_ABOVE: u64 = 256 * 1024;
const SAMPLE: usize = 64 * 1024;

/// A fingerprint from the file's size and three samples: its start, its
/// middle and its end. Two files with the same quick fingerprint are almost
/// always the same file, but only [`whole_hash`] can say so for certain, so
/// nothing is copied or dropped on the strength of this alone.
///
/// It can never equal a whole-file hash, because the label below is mixed in.
fn quick_fingerprint(file: &mut File, size: u64) -> std::io::Result<String> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"keevoro quick fingerprint 1");
    hasher.update(&size.to_le_bytes());
    let mut buf = vec![0u8; SAMPLE];
    let sample = SAMPLE as u64;
    for start in [0, (size / 2).saturating_sub(sample / 2), size.saturating_sub(sample)] {
        file.seek(SeekFrom::Start(start))?;
        let n = read_up_to(file, &mut buf)?;
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// The BLAKE3 hash of every byte of a file: the fingerprint that proves two
/// files are the same, and that a copy is exact.
pub fn whole_hash(path: &Path) -> std::io::Result<String> {
    hash_rest(&mut File::open(path)?, &[])
}

/// Reads the capture date from EXIF, preferring the original capture time.
fn exif_date(path: &Path) -> Option<PartialDate> {
    let file = File::open(path).ok()?;
    let exif = exif::Reader::new()
        .read_from_container(&mut BufReader::new(file))
        .ok()?;
    for tag in [
        exif::Tag::DateTimeOriginal,
        exif::Tag::DateTimeDigitized,
        exif::Tag::DateTime,
    ] {
        if let Some(field) = exif.get_field(tag, exif::In::PRIMARY) {
            if let exif::Value::Ascii(ref parts) = field.value {
                if let Some(d) = parts
                    .first()
                    .and_then(|b| std::str::from_utf8(b).ok())
                    .and_then(PartialDate::from_exif)
                {
                    return Some(d);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests;
