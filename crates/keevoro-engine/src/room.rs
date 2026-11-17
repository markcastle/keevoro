//! Is there room where the files are going?
//!
//! Nobody should find out a disk is full halfway through sending their
//! photos. Before anything is sent to a folder, what is waiting is weighed
//! against the free space on that folder's disk.
//!
//! Cloud storage has no disk to measure, so it has no answer here; the
//! person is still told how much is about to be sent.

use std::path::Path;

/// A little more than the files themselves: previews, the index, and the
/// slack a disk needs to keep working.
const HEADROOM: u64 = 200 * 1024 * 1024;

/// How a folder's free space compares with what is about to be sent to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Room {
    /// Bytes waiting to be sent.
    pub needed: u64,
    /// Bytes free on the disk the folder is on.
    pub free: u64,
}

impl Room {
    /// True when the files, and a little headroom, will fit.
    pub fn is_enough(&self) -> bool {
        self.free >= self.needed.saturating_add(if self.needed == 0 { 0 } else { HEADROOM })
    }

    /// The sentence to refuse with when there is not enough room.
    pub fn refusal(&self, folder: &Path) -> String {
        format!(
            "There isn't enough room in {}. These files need about {} and that disk has {} free. \
             Free up some space, or send them somewhere else.",
            crate::friendly_path(folder),
            crate::summary::bytes(self.needed),
            crate::summary::bytes(self.free)
        )
    }
}

/// Weighs `needed` bytes against the free space on the disk holding
/// `folder`. `None` when the system will not say how much is free (some
/// network drives), in which case sending is simply allowed to try.
pub fn room_in(folder: &Path, needed: u64) -> Option<Room> {
    keevoro_collect::free_space(folder).map(|free| Room { needed, free })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn files_fit_when_there_is_room_for_them_and_a_little_more() {
        assert!(Room {
            needed: 4 * GB,
            free: 120 * GB
        }
        .is_enough());
        assert!(!Room {
            needed: 4 * GB,
            free: GB
        }
        .is_enough());
        assert!(
            !Room {
                needed: 4 * GB,
                free: 4 * GB
            }
            .is_enough(),
            "exactly full is not enough"
        );
        assert!(Room { needed: 0, free: 0 }.is_enough(), "nothing to send always fits");
    }

    #[test]
    fn the_refusal_says_how_much_is_needed_and_how_much_there_is() {
        let said = Room {
            needed: 4 * GB,
            free: GB,
        }
        .refusal(Path::new("/mnt/backup"));
        assert!(said.contains("/mnt/backup"), "{said}");
        assert!(said.contains("need about 4.0 GB"), "{said}");
        assert!(said.contains("has 1.0 GB free"), "{said}");
    }

    #[test]
    fn a_real_folder_reports_its_disks_free_space() {
        let dir = tempfile::tempdir().unwrap();
        let room = room_in(dir.path(), 1).unwrap();
        assert!(room.free > 0);
        // A folder that does not exist yet is measured by the disk it will be on.
        assert!(room_in(&dir.path().join("not/made/yet"), 1).is_some());
    }
}
