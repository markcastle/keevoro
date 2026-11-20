//! Files that are only in online storage, not on this computer.
//!
//! OneDrive's "files on demand" (and iCloud Drive's "optimise storage" on a
//! Mac) leave a placeholder on the disk that looks like the file, with its
//! name and size, but whose contents are online. Opening one downloads it,
//! so a search that opened every file would quietly download a whole
//! OneDrive. Keevoro tells them apart by what the folder listing already
//! says about each file, its attributes, and never by opening it, then
//! passes them over and says how many on screen.
//!
//! The question is asked through [`OnlineCheck`], so a test can say which
//! files are online on any computer; the real answer ([`ByAttributes`]) is
//! read from Windows' attributes or the Mac's file flags, and on other
//! systems no file is ever online only.

use std::fs::Metadata;
use std::path::Path;

/// Is a file only in online storage? Answered from what listing its folder
/// gave (`listing`), never by opening the file.
pub trait OnlineCheck: Send + Sync + std::fmt::Debug {
    fn online_only(&self, file: &Path, listing: &Metadata) -> bool;
}

/// The real answer, from the file's attributes as the system lists them.
#[derive(Debug, Clone, Copy, Default)]
pub struct ByAttributes;

impl OnlineCheck for ByAttributes {
    fn online_only(&self, _file: &Path, listing: &Metadata) -> bool {
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            windows_online_only(listing.file_attributes())
        }
        #[cfg(target_os = "macos")]
        {
            use std::os::macos::fs::MetadataExt;
            mac_dataless(listing.st_flags())
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let _ = listing;
            false
        }
    }
}

/// Windows marks a file whose contents are in online storage, not on the
/// disk, with these attributes: recall on data access, recall on open, and
/// offline.
const WINDOWS_ONLINE_ONLY: u32 = 0x0040_0000 | 0x0004_0000 | 0x0000_1000;

/// True for Windows attributes that say a file is only in online storage.
pub fn windows_online_only(attributes: u32) -> bool {
    attributes & WINDOWS_ONLINE_ONLY != 0
}

/// A Mac marks a file whose contents have been moved to online storage
/// (iCloud Drive, or another file provider) as "dataless".
const SF_DATALESS: u32 = 0x4000_0000;

/// True for Mac file flags that say a file is only in online storage.
pub fn mac_dataless(flags: u32) -> bool {
    flags & SF_DATALESS != 0
}

/// The online storage a person on this computer would know by name.
pub const SERVICE: &str = if cfg!(target_os = "macos") {
    "iCloud"
} else if cfg!(windows) {
    "OneDrive"
} else {
    "online storage"
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_attributes_say_which_files_are_only_online() {
        // A OneDrive placeholder: recall on data access, as Windows lists it.
        assert!(windows_online_only(0x0040_0000 | 0x20));
        assert!(windows_online_only(0x1000), "offline");
        assert!(windows_online_only(0x0004_0000), "recall on open");
        assert!(!windows_online_only(0x20), "an ordinary file");
        assert!(!windows_online_only(0x0008_0000 | 0x20), "pinned, so on this computer");
    }

    #[test]
    fn mac_flags_say_which_files_are_only_online() {
        assert!(mac_dataless(0x4000_0000));
        assert!(mac_dataless(0x4000_0000 | 0x20), "with other flags");
        assert!(!mac_dataless(0), "an ordinary file");
        assert!(!mac_dataless(0x8000), "hidden is not online");
    }

    #[test]
    fn an_ordinary_file_here_is_on_this_computer() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("IMG_1.jpg");
        std::fs::write(&file, b"photo").unwrap();
        let listing = std::fs::symlink_metadata(&file).unwrap();
        assert!(!ByAttributes.online_only(&file, &listing));
    }
}
