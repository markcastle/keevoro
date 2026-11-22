//! How a destination folder is reached: is it a disk in this computer, or
//! somewhere across a network?
//!
//! It matters for speed. Writing to a disk, one file at a time is best:
//! several at once only make the disk hop about. Writing across a network,
//! most of the time for a small file goes on waiting for the far end to
//! answer, so several at once is several times quicker. A folder gives no
//! sign of which it is from its name alone: `Z:\Backups` may be a disk in
//! the machine or storage in another country.

use std::path::Path;

/// How many files to send at once to a folder across a network. Fewer than
/// to cloud storage: a file share copes less well with many writers.
pub const AT_ONCE_ACROSS_A_NETWORK: usize = 4;

/// Which disk (volume) a folder is on, as a name to remember and compare,
/// or `None` when the system cannot say. A drive unplugged from under a
/// folder that was its mount point can leave an empty folder behind on the
/// computer's own disk, at the same path: this is how the two are told
/// apart.
///
/// On macOS and Linux it is the folder's device number, which the standard
/// library gives without calling the system directly. On Windows it is the
/// volume's own name (`\\?\Volume{...}\`), which stays the same whatever
/// letter the drive is given (`keevoro_frame::drives::volume_of`). A
/// device number can change when the same drive is plugged in again, so
/// it is only ever used together with what is in the folder: see
/// [`crate::confirm`].
pub fn volume_of(folder: &Path) -> Option<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(folder).ok().map(|m| format!("device {}", m.dev()))
    }
    #[cfg(windows)]
    {
        keevoro_frame::drives::volume_of(folder)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = folder;
        None
    }
}

/// Is a source there to be read: the one rule for "plugged in", used by a
/// search, looking again, a backup and the check for duplicates alike. A
/// source whose root is not a folder now (a card in a drawer, a drive
/// unplugged, a share out of reach) is not, and nothing found on it is
/// forgotten or tried file by file.
pub fn is_there(root: &Path) -> bool {
    root.is_dir()
}

/// Is this folder across a network?
///
/// On Windows the system is asked what the path really is. A mapped drive
/// letter such as `Z:` comes back as the share it stands for
/// (`\\server\share`), which is how a network drive is told from a disk
/// without any special permissions. Elsewhere this cannot yet tell, and
/// says no, which only means files go one at a time as before.
pub fn is_across_a_network(folder: &Path) -> bool {
    let real = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    names_a_share(&real.to_string_lossy()) || names_a_share(&folder.to_string_lossy())
}

/// Does this path, as written, name a network share? `\\server\share\...`
/// does, and so does the long form Windows uses for the same thing,
/// `\\?\UNC\server\share\...`. An ordinary long path, `\\?\C:\...`, does not.
fn names_a_share(path: &str) -> bool {
    let path = path.replace('/', "\\");
    match path.strip_prefix(r"\\?\") {
        Some(long) => long.to_ascii_uppercase().starts_with(r"UNC\"),
        None => path.starts_with(r"\\") && path.len() > 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_and_one_inside_it_are_on_the_same_disk() {
        let dir = tempfile::tempdir().unwrap();
        let inside = dir.path().join("Backup");
        std::fs::create_dir_all(&inside).unwrap();
        assert_eq!(volume_of(dir.path()), volume_of(&inside));
        assert_eq!(volume_of(&dir.path().join("not there")), None);
        if cfg!(any(unix, windows)) {
            assert!(volume_of(dir.path()).is_some());
        }
    }

    #[test]
    fn a_share_is_told_from_a_disk_by_what_the_path_really_is() {
        // What Windows says a mapped network drive really is.
        assert!(names_a_share(r"\\?\UNC\nas.example.net\backup\Keevoro"));
        assert!(names_a_share(r"\\nas\photos"));
        assert!(names_a_share("//nas/photos"));
        // Disks, however they are written.
        assert!(!names_a_share(r"\\?\C:\Users\You\Pictures"));
        assert!(!names_a_share(r"D:\Backups"));
        assert!(!names_a_share("/Volumes/Backup"));
        assert!(!names_a_share(r"\\"));
    }

    #[test]
    fn a_folder_on_this_computer_is_not_across_a_network() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_across_a_network(dir.path()));
        // A folder that is not there yet is judged by its name.
        assert!(!is_across_a_network(&dir.path().join("not made yet")));
    }
}
