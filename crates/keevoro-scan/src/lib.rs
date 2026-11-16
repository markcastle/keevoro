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

mod video;

use keevoro_core::{
    best_date, date_from_filename, folder_hints, holds_originals, junk_hint, kind_from_extension, kind_from_mime,
    library_album_segments, never_media_extension, photo_library, DateGuess, DateSource, FolderHints, JunkInput,
    JunkReason, MediaKind, PartialDate, SkipReason, SkipRules,
};
use serde::Serialize;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Mutex};
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
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            skip: SkipRules::default(),
            hash: true,
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
    /// Full BLAKE3 hash, hex. `None` when hashing is off.
    pub blake3: Option<String>,
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
        let path = entry.into_path();
        let plainly_not_media = path
            .extension()
            .is_some_and(|ext| never_media_extension(&ext.to_string_lossy()));
        let sent = if plainly_not_media {
            // Programs, documents and music are not opened at all.
            done.send((order, path, Looked::Other)).is_ok()
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
    let (kind, mime) = match infer::get(&head) {
        Some(t) => match kind_from_mime(t.mime_type()) {
            Some(k) => (k, t.mime_type().to_string()),
            None => return Ok(None),
        },
        None => match kind_from_extension(&ext) {
            Some(k) => (k, format!("application/x-{}", ext.to_ascii_lowercase())),
            None => return Ok(None),
        },
    };

    let meta = file.metadata()?;
    let blake3 = if opts.hash {
        Some(hash_rest(&mut file, &head)?)
    } else {
        None
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
            size = pixel_size(&mut file);
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
