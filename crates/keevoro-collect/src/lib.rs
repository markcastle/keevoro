//! Gathering: copying the files the user chose into the library.
//!
//! This is the step where a mistake could cost someone their only copy of a
//! photo, so it is deliberately plain and paranoid:
//!
//! 1. The source file is opened read-only and hashed while it is read.
//! 2. The bytes go to a hidden temporary file in the destination folder.
//! 3. The temporary file is flushed to disk, read back and hashed again.
//! 4. Only if both hashes match the hash from the scan is it given its real
//!    name, and only then is it recorded as safely gathered.
//!
//! Nothing in the library is ever overwritten. If the name is taken by a
//! different file the new one becomes `IMG_1234 (2).jpg`; if it is taken by
//! the very same file, there is nothing to do.
//!
//! A problem with one file never stops the rest. It goes in the report, and
//! running again picks up exactly where things stopped, because every
//! finished file is recorded in the catalog as it lands.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod previews;
mod verify;
mod videos;

pub use previews::{make_previews, PreviewReport};
pub use verify::{verify_library, Problem, VerifyReport};
pub use videos::{make_video_previews, VideoReport};

use keevoro_catalog::{Catalog, CatalogError};
use keevoro_core::{library_folder, numbered_name, safe_name};
use keevoro_scan::FoundItem;
use serde::Serialize;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

const READ_CHUNK: usize = 1024 * 1024;
/// How many `name (n).jpg` variants to try before giving up on a file.
const MAX_NAME_TRIES: u32 = 10_000;

/// Why one file could not be gathered. The wording is shown to the user.
#[derive(Debug, thiserror::Error)]
pub enum CopyError {
    #[error("Couldn't read it from the source ({0})")]
    Read(#[source] io::Error),
    #[error("Couldn't write it to the library ({0})")]
    Write(#[source] io::Error),
    #[error("It has changed since the source was scanned. Scan the source again to pick up the new version")]
    SourceChanged,
    #[error("The copy didn't match the original, so it was thrown away. The library disk may be faulty")]
    VerifyFailed,
    #[error("It was scanned without checking for duplicates. Scan the source again without --no-hash")]
    NotHashed,
    #[error("Keevoro could not read the whole file to check it, so it was not copied")]
    NotChecked,
    #[error("Too many different files share this name")]
    NameTaken,
}

/// Why gathering could not start or had to stop.
#[derive(Debug, thiserror::Error)]
pub enum GatherError {
    #[error("There isn't enough room for your library. It needs {needed} bytes and the disk has {free} free.")]
    NotEnoughRoom { needed: u64, free: u64 },
    #[error(transparent)]
    Catalog(#[from] CatalogError),
}

/// Where a file ended up.
#[derive(Debug, PartialEq, Eq)]
pub struct Placed {
    pub path: PathBuf,
    /// True when the library already held this exact file under this name,
    /// so nothing new was written.
    pub already_there: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Failed {
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Debug, Default, Serialize)]
pub struct GatherReport {
    /// Files copied and verified in this run.
    pub copied: usize,
    pub copied_bytes: u64,
    /// Files that were already safely in the library.
    pub already_there: usize,
    pub failed: Vec<Failed>,
}

/// Counts for "3,412 of 9,870 safely copied".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GatherProgress {
    pub done: usize,
    pub total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

/// Free space on the disk holding `path`, if the system will say.
pub fn free_space(path: &Path) -> Option<u64> {
    // The library folder may not exist yet; ask about the nearest folder
    // that does.
    path.ancestors()
        .find(|p| p.is_dir())
        .and_then(|p| fs4::available_space(p).ok())
}

/// Copies `items` into the library at `library`, verifying each one.
///
/// `free` is the space available on the library disk (see [`free_space`]);
/// pass `None` to skip the check. Items should already be unique by content;
/// a repeat is simply reported as already there.
pub fn gather(
    library: &Path,
    catalog: &Catalog,
    items: &[&FoundItem],
    free: Option<u64>,
    on_progress: &mut dyn FnMut(&GatherProgress),
) -> Result<GatherReport, GatherError> {
    let mut report = GatherReport::default();
    let mut todo = Vec::new();
    for item in items {
        if is_gathered(library, catalog, item)? {
            report.already_there += 1;
        } else {
            todo.push(*item);
        }
    }

    let needed: u64 = todo.iter().map(|i| i.bytes).sum();
    if let Some(free) = free {
        if needed > free {
            return Err(GatherError::NotEnoughRoom { needed, free });
        }
    }

    let mut progress = GatherProgress {
        done: report.already_there,
        total: items.len(),
        bytes_done: 0,
        bytes_total: needed,
    };
    on_progress(&progress);
    for item in todo {
        match gather_one(library, item) {
            Ok((placed, relative)) => {
                if let Some(hash) = &item.blake3 {
                    catalog.mark_collected(hash, &relative, item.bytes)?;
                }
                if placed.already_there {
                    report.already_there += 1;
                } else {
                    report.copied += 1;
                    report.copied_bytes += item.bytes;
                }
            }
            Err(e) => report.failed.push(Failed {
                path: item.path.clone(),
                reason: e.to_string(),
            }),
        }
        progress.done += 1;
        progress.bytes_done += item.bytes;
        on_progress(&progress);
    }
    Ok(report)
}

/// Turns a library-relative path (`a/b/c.jpg`) into a real path.
pub(crate) fn in_library(library: &Path, relative: &str) -> PathBuf {
    relative.split('/').fold(library.to_path_buf(), |p, s| p.join(s))
}

/// True when the catalog says this file was gathered and it is still there.
fn is_gathered(library: &Path, catalog: &Catalog, item: &FoundItem) -> Result<bool, CatalogError> {
    let Some(hash) = &item.blake3 else {
        return Ok(false);
    };
    Ok(catalog
        .collected_path(hash)?
        .is_some_and(|rel| in_library(library, &rel).is_file()))
}

fn gather_one(library: &Path, item: &FoundItem) -> Result<(Placed, String), CopyError> {
    let hash = item.blake3.as_deref().ok_or(CopyError::NotHashed)?;
    // A quick fingerprint cannot prove a copy exact, so it never stands in
    // for the real one here.
    if !item.hashed {
        return Err(CopyError::NotChecked);
    }
    let folders = library_folder(&item.folder.album, item.date.as_ref());
    let dir = folders.iter().fold(library.to_path_buf(), |p, s| p.join(s));
    let name = item
        .path
        .file_name()
        .map_or_else(|| "unnamed".to_string(), |n| safe_name(&n.to_string_lossy()));
    let placed = copy_verified(&item.path, &dir, &name, hash)?;
    let final_name = placed
        .path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or(name);
    Ok((placed, format!("{}/{final_name}", folders.join("/"))))
}

/// Copies one file into `dest_dir` as `name` (or a numbered variant) and
/// proves the copy matches `expected`, the BLAKE3 hash from the scan.
pub fn copy_verified(source: &Path, dest_dir: &Path, name: &str, expected: &str) -> Result<Placed, CopyError> {
    fs::create_dir_all(dest_dir).map_err(CopyError::Write)?;
    // Hidden, so a later scan of the library disk ignores a leftover.
    let temp = dest_dir.join(format!(".keevoro-part-{}", &expected[..expected.len().min(16)]));
    let result = copy_to_temp(source, &temp, expected).and_then(|()| place(&temp, dest_dir, name, expected));
    // Whatever happened, the temporary file must not be left behind.
    let _ = fs::remove_file(&temp);
    result
}

fn copy_to_temp(source: &Path, temp: &Path, expected: &str) -> Result<(), CopyError> {
    let mut input = File::open(source).map_err(CopyError::Read)?;
    // A leftover from a run that was cut short is never trusted.
    match fs::remove_file(temp) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(CopyError::Write(e)),
        _ => {}
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp)
        .map_err(CopyError::Write)?;

    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; READ_CHUNK];
    loop {
        let n = input.read(&mut buf).map_err(CopyError::Read)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        output.write_all(&buf[..n]).map_err(CopyError::Write)?;
    }
    if hasher.finalize().to_hex().as_str() != expected {
        return Err(CopyError::SourceChanged);
    }
    // Keep the original's modified time; it is sometimes the only date clue.
    if let Ok(modified) = input.metadata().and_then(|m| m.modified()) {
        let _ = output.set_modified(modified);
    }
    // Make sure the bytes are really on the disk before reading them back.
    output.sync_all().map_err(CopyError::Write)?;
    drop(output);

    if hash_file(temp).map_err(CopyError::Write)? != expected {
        return Err(CopyError::VerifyFailed);
    }
    Ok(())
}

/// Gives the verified temporary file its real name without ever replacing
/// a file that is already in the library.
fn place(temp: &Path, dest_dir: &Path, name: &str, expected: &str) -> Result<Placed, CopyError> {
    for n in 1..=MAX_NAME_TRIES {
        let candidate = dest_dir.join(if n == 1 {
            name.to_string()
        } else {
            numbered_name(name, n)
        });
        match link_new(temp, &candidate) {
            Ok(()) => {
                return Ok(Placed {
                    path: candidate,
                    already_there: false,
                })
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                if hash_file(&candidate).map_err(CopyError::Write)? == expected {
                    return Ok(Placed {
                        path: candidate,
                        already_there: true,
                    });
                }
            }
            Err(e) => return Err(CopyError::Write(e)),
        }
    }
    Err(CopyError::NameTaken)
}

/// Makes `to` refer to the file at `from`, failing if `to` exists.
///
/// A hard link does this in one step that cannot overwrite. FAT and exFAT
/// disks have no hard links, so there the fallback is "check, then rename".
fn link_new(from: &Path, to: &Path) -> io::Result<()> {
    match fs::hard_link(from, to) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(e),
        Err(_) if to.exists() => Err(io::ErrorKind::AlreadyExists.into()),
        Err(_) => fs::rename(from, to),
    }
}

pub(crate) fn hash_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
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

#[cfg(test)]
mod tests;
