//! How fast things went in earlier backups on this computer, remembered so
//! the next backup's first estimate of the time left is not a guess.
//!
//! What is kept is a handful of numbers: previews made a second, bytes
//! sent a second to each destination, and how many bytes of previews an
//! original makes (see `keevoro_core::estimate::Remembered`). They are
//! kept in one small file in Keevoro's own data folder, never at a source
//! or a destination, shared by every Keevoro program on the computer.
//! Losing the file costs nothing but a rougher first estimate, so a file
//! that cannot be read is simply treated as empty.

use keevoro_core::estimate::Remembered;
use std::path::Path;

/// The file's name in Keevoro's data folder.
pub(crate) const FILE: &str = "measured rates.json";

/// What earlier backups measured, or nothing.
pub(crate) fn remembered(file: &Path) -> Remembered {
    std::fs::read(file)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Folds what this backup measured into what is remembered. Written
/// beside the file and moved into place, so nobody reads half of it.
pub(crate) fn remember(file: &Path, measured: &Remembered) -> std::io::Result<()> {
    if measured.rates.is_empty() && measured.preview_bytes_per_original.is_none() {
        return Ok(());
    }
    let updated = remembered(file).updated_with(measured);
    let dir = file.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let beside = dir.join(format!(".{FILE}.{}", std::process::id()));
    std::fs::write(
        &beside,
        serde_json::to_vec_pretty(&updated).map_err(std::io::Error::other)?,
    )?;
    std::fs::rename(&beside, file).inspect_err(|_| {
        let _ = std::fs::remove_file(&beside);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_was_measured_is_remembered_and_blended_with_what_came_before() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("data").join(FILE);
        assert_eq!(remembered(&file), Remembered::default(), "nothing yet");
        let mut first = Remembered::default();
        first.rates.insert("makingPreviews".into(), 4.0);
        remember(&file, &first).unwrap();
        assert_eq!(remembered(&file).rates["makingPreviews"], 4.0);
        let mut second = Remembered::default();
        second.rates.insert("makingPreviews".into(), 8.0);
        remember(&file, &second).unwrap();
        assert_eq!(remembered(&file).rates["makingPreviews"], 6.0);
        // Nothing measured changes nothing, and leaves nothing beside it.
        remember(&file, &Remembered::default()).unwrap();
        let names: Vec<_> = std::fs::read_dir(file.parent().unwrap()).unwrap().collect();
        assert_eq!(names.len(), 1);
        // A damaged file is as good as none.
        std::fs::write(&file, b"{not json").unwrap();
        assert_eq!(remembered(&file), Remembered::default());
    }
}
