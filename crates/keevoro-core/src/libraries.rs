//! Apple Photos, iPhoto and Aperture libraries.
//!
//! On a Mac these look like single files, but they are folders, and old
//! backup drives are full of them. Inside is one copy of each original
//! photo plus several times as many thumbnails, previews, face crops and
//! database files. Scanning the whole thing would bury the user in
//! near-identical small copies.
//!
//! So a library is treated as one source of originals: the scanner only goes
//! into the folder that holds the originals and ignores the rest. This
//! matters on Windows too, because these libraries often sit on old backup
//! disks.

/// The kinds of library keevoro recognises by folder name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhotoLibrary {
    /// `Photos Library.photoslibrary`, macOS 10.10 onwards.
    ApplePhotos,
    /// `iPhoto Library` or `iPhoto Library.photolibrary`.
    IPhoto,
    /// `Aperture Library.aplibrary`.
    Aperture,
}

/// Is a folder with this name a photo library package?
pub fn photo_library(folder_name: &str) -> Option<PhotoLibrary> {
    let lower = folder_name.trim().to_lowercase();
    if lower.ends_with(".photoslibrary") {
        Some(PhotoLibrary::ApplePhotos)
    } else if lower.ends_with(".photolibrary")
        || lower.ends_with(".migratedphotolibrary")
        || lower == "iphoto library"
        || lower.ends_with(".iphoto library")
    {
        Some(PhotoLibrary::IPhoto)
    } else if lower.ends_with(".aplibrary") || lower.ends_with(".migratedaplibrary") {
        Some(PhotoLibrary::Aperture)
    } else {
        None
    }
}

/// Is this top-level folder inside a library the one holding the originals?
///
/// Photos uses `originals` (and `Masters` before macOS 10.15). iPhoto used
/// `Originals` up to iPhoto '08 and `Masters` afterwards. Everything else
/// (`resources`, `Thumbnails`, `Previews`, `Data`, `database`, `Modified`)
/// is derived from the originals and is skipped.
pub fn holds_originals(folder_name: &str) -> bool {
    matches!(folder_name.trim().to_lowercase().as_str(), "originals" | "masters")
}

/// Removes the storage folders a library invents, leaving only names a
/// person chose. `segments` are the folders below the originals folder.
///
/// Apple Photos spreads files across sixteen folders named `0` to `F`, and
/// iPhoto names import batches by timestamp. Old iPhoto event names such as
/// `Cornwall Trip` are the user's own and are kept.
pub fn library_album_segments<S: AsRef<str>>(segments: &[S]) -> Vec<String> {
    segments
        .iter()
        .map(|s| s.as_ref().trim())
        .filter(|s| !is_storage_folder(s))
        .map(str::to_string)
        .collect()
}

fn is_storage_folder(name: &str) -> bool {
    let single_hex = name.len() == 1 && name.chars().all(|c| c.is_ascii_hexdigit());
    // iPhoto import batches: "20110304-123456" or "Roll 12".
    let import_batch = name.len() == 15
        && name.as_bytes().get(8) == Some(&b'-')
        && name.chars().enumerate().all(|(i, c)| i == 8 || c.is_ascii_digit());
    let roll = name
        .to_lowercase()
        .strip_prefix("roll ")
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    // Month and day folders under a year, as in "Masters/2011/03/04".
    let month_or_day = name.len() <= 2 && !name.is_empty() && name.chars().all(|c| c.is_ascii_digit());
    single_hex || month_or_day || import_batch || roll
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_library_packages_by_name() {
        assert_eq!(
            photo_library("Photos Library.photoslibrary"),
            Some(PhotoLibrary::ApplePhotos)
        );
        assert_eq!(photo_library("iPhoto Library"), Some(PhotoLibrary::IPhoto));
        assert_eq!(photo_library("iPhoto Library.photolibrary"), Some(PhotoLibrary::IPhoto));
        assert_eq!(photo_library("Old.migratedphotolibrary"), Some(PhotoLibrary::IPhoto));
        assert_eq!(
            photo_library("Aperture Library.aplibrary"),
            Some(PhotoLibrary::Aperture)
        );
        assert_eq!(photo_library("Holiday photos"), None);
        assert_eq!(photo_library("Library"), None);
    }

    #[test]
    fn only_the_originals_folders_are_entered() {
        assert!(holds_originals("originals"));
        assert!(holds_originals("Masters"));
        assert!(holds_originals("Originals"));
        assert!(!holds_originals("resources"));
        assert!(!holds_originals("Thumbnails"));
        assert!(!holds_originals("Previews"));
        assert!(!holds_originals("Modified"));
    }

    #[test]
    fn storage_folders_are_dropped_and_event_names_kept() {
        assert!(library_album_segments(&["A"]).is_empty());
        assert_eq!(
            library_album_segments(&["2011", "03", "04", "20110304-123456"]),
            vec!["2011"]
        );
        assert_eq!(library_album_segments(&["2008", "Roll 12"]), vec!["2008"]);
        assert_eq!(
            library_album_segments(&["2008", "Cornwall Trip"]),
            vec!["2008", "Cornwall Trip"]
        );
    }
}
