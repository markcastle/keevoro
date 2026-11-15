//! Finds photos and videos under a source folder or drive.
//!
//! The scanner only ever reads. It walks the tree in one pass (one reader per
//! physical drive is kinder to old spinning disks), detects media by content
//! rather than extension, hashes each file with BLAKE3 while reading it, and
//! collects every date clue it can find.
//!
//! Problems never stop a scan. Unreadable files and folders are reported so
//! the user can try again later.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod video;

use keevoro_core::{
    best_date, date_from_filename, folder_hints, holds_originals, junk_hint, kind_from_extension, kind_from_mime,
    library_album_segments, photo_library, DateGuess, DateSource, FolderHints, JunkInput, JunkReason, MediaKind,
    PartialDate, SkipReason, SkipRules,
};
use serde::Serialize;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
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

/// Live counts for a progress display.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScanProgress {
    pub files_seen: u64,
    pub photos: u64,
    pub videos: u64,
}

/// Scans `root`, calling `on_progress` after every file.
pub fn scan(root: &Path, opts: &ScanOptions, on_progress: &mut dyn FnMut(&ScanProgress)) -> ScanReport {
    let mut report = ScanReport {
        root: root.to_path_buf(),
        ..Default::default()
    };
    let mut progress = ScanProgress::default();
    // Sorted, so the same drive always gives the same report.
    let mut walk = WalkDir::new(root).follow_links(false).sort_by_file_name().into_iter();

    while let Some(entry) = walk.next() {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                let path = e.path().map_or_else(|| root.to_path_buf(), Path::to_path_buf);
                report.unreadable.push(Unreadable {
                    path,
                    error: e.to_string(),
                });
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
                    report.skipped.push(SkippedFolder {
                        path: entry.path().to_path_buf(),
                        reason,
                    });
                    walk.skip_current_dir();
                }
            }
            continue;
        }
        if !entry.file_type().is_file() || opts.skip.skip_file(&name).is_some() {
            continue;
        }

        progress.files_seen += 1;
        match examine(root, entry.path(), opts) {
            Ok(Some(item)) => {
                match item.kind {
                    MediaKind::Photo => progress.photos += 1,
                    MediaKind::Video => progress.videos += 1,
                }
                report.items.push(item);
            }
            Ok(None) => report.other_files += 1,
            Err(e) => report.unreadable.push(Unreadable {
                path: entry.path().to_path_buf(),
                error: e.to_string(),
            }),
        }
        on_progress(&progress);
    }
    report
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
