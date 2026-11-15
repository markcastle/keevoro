//! Checking the library is still what it was when it was gathered.
//!
//! Disks fail quietly. A file that copied perfectly five years ago can have
//! rotted since, and nobody finds out until the day they want the photo.
//! This re-reads every original in the library and compares it with the
//! hash recorded when it was gathered, so problems are found while the old
//! drive, or the backup, can still supply a good copy.
//!
//! It only reads. Putting a damaged file right is `gather` (from the old
//! drive) or a restore from the backup; this just says what needs it.

use crate::{hash_file, in_library};
use keevoro_catalog::{Catalog, CatalogError};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Debug, Default, Serialize)]
pub struct VerifyReport {
    /// Files read and found identical to when they were gathered.
    pub intact: usize,
    pub problems: Vec<Problem>,
}

/// Re-reads every original in the library. `on_progress` is called with
/// (done, total) after each file.
pub fn verify_library(
    library: &Path,
    catalog: &Catalog,
    on_progress: &mut dyn FnMut(usize, usize),
) -> Result<VerifyReport, CatalogError> {
    let files = catalog.collected()?;
    let total = files.len();
    let mut report = VerifyReport::default();
    for (done, file) in files.into_iter().enumerate() {
        let path = in_library(library, &file.path);
        let reason = match std::fs::metadata(&path) {
            Err(_) => Some("It is missing from the library".to_string()),
            Ok(meta) if meta.len() != file.bytes => {
                Some("It has changed size, so it is damaged or was edited".to_string())
            }
            Ok(_) => match hash_file(&path) {
                Ok(hash) if hash == file.blake3 => None,
                Ok(_) => Some("Its contents have changed, so it is damaged or was edited".to_string()),
                Err(e) => Some(format!("It couldn't be read ({e})")),
            },
        };
        match reason {
            None => report.intact += 1,
            Some(reason) => report.problems.push(Problem { path, reason }),
        }
        on_progress(done + 1, total);
    }
    Ok(report)
}
