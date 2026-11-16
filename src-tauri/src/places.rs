//! The drives plugged into this computer.
//!
//! Asking someone to find a drive in a folder dialog is asking them to know
//! what "E:" or "/Volumes" means. Listing the drives by name, with their
//! size, lets them recognise the one they just plugged in and click it.
//!
//! This only lists drives. It reads nothing from them.

use serde::Serialize;
use sysinfo::Disks;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    /// What to pass to `scan_place`.
    pub path: String,
    /// What to call it: the drive's own name where it has one.
    pub name: String,
    /// A line of detail, such as "USB drive · 32 GB".
    pub detail: String,
    /// A USB stick, SD card or external disk: the likeliest place to look.
    pub removable: bool,
    /// A whole disk big enough that looking through all of it can take a
    /// long while, so it is worth asking before starting.
    pub ask_first: bool,
}

/// Every drive worth offering, removable ones first.
pub fn plugged_in() -> Vec<Place> {
    let mut places: Vec<Place> = Disks::new_with_refreshed_list()
        .list()
        .iter()
        .filter_map(|disk| {
            describe(
                &disk.mount_point().display().to_string(),
                &disk.name().to_string_lossy(),
                disk.total_space(),
                disk.is_removable(),
            )
        })
        .collect();
    places.sort_by(|a, b| b.removable.cmp(&a.removable).then_with(|| a.path.cmp(&b.path)));
    places.dedup_by(|a, b| a.path == b.path);
    places
}

/// Removable drives larger than this are external hard disks, not sticks or
/// cards, and are asked about before being looked through whole.
const ASK_ABOVE: u64 = 64_000_000_000;

/// Turns what the system says about a drive into what a person would call
/// it. `None` for things that are not drives a person would recognise.
pub fn describe(mount: &str, label: &str, bytes: u64, removable: bool) -> Option<Place> {
    // Tiny or hidden system volumes (boot partitions, snapshots) only
    // confuse. A real drive holds at least a few hundred megabytes.
    const SMALLEST: u64 = 256 * 1024 * 1024;
    let system_mounts = [
        "/boot",
        "/snap",
        "/sys",
        "/proc",
        "/dev",
        "/run",
        "/System/Volumes",
        "/private",
    ];
    if bytes < SMALLEST || system_mounts.iter().any(|m| mount.starts_with(m)) {
        return None;
    }
    // "E:\" reads better as "E:"; "/Volumes/KINGSTON" as "KINGSTON".
    let letter = mount.trim_end_matches(['\\', '/']);
    let last = letter.rsplit(['/', '\\']).next().unwrap_or(letter);
    let label = label.trim();
    let is_letter = letter.len() == 2 && letter.ends_with(':');
    let name = match (label.is_empty() || label.starts_with("/dev/"), is_letter) {
        (false, true) => format!("{label} ({letter})"),
        (false, false) => label.to_string(),
        (true, true) => format!("Drive {letter}"),
        (true, false) if last.is_empty() => "This computer".to_string(),
        (true, false) => last.to_string(),
    };
    let kind = if removable {
        "USB or external drive"
    } else {
        "This computer’s disk"
    };
    Some(Place {
        path: mount.to_string(),
        name,
        detail: format!("{kind} · {}", size(bytes)),
        removable,
        // A USB stick or memory card is what people come here to look
        // through, and is quick. A computer's own disk, or a big external
        // one, is mostly not photos and can take a long time.
        ask_first: !removable || bytes > ASK_ABOVE,
    })
}

/// Sizes as drives are sold: "32 GB", "1 TB".
fn size(bytes: u64) -> String {
    const GB: f64 = 1_000_000_000.0;
    let gb = bytes as f64 / GB;
    if gb >= 950.0 {
        let tb = gb / 1000.0;
        if (tb - tb.round()).abs() < 0.08 {
            format!("{} TB", tb.round())
        } else {
            format!("{tb:.1} TB")
        }
    } else if gb >= 10.0 {
        format!("{} GB", gb.round())
    } else {
        format!("{gb:.1} GB")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1_000_000_000;

    #[test]
    fn drives_are_named_the_way_a_person_would_recognise_them() {
        let usb = describe("E:\\", "KINGSTON", 32 * GB, true).unwrap();
        assert_eq!((usb.name.as_str(), usb.path.as_str()), ("KINGSTON (E:)", "E:\\"));
        assert_eq!(usb.detail, "USB or external drive · 32 GB");

        let unnamed = describe("F:\\", "", 500 * GB, true).unwrap();
        assert_eq!(unnamed.name, "Drive F:");
        let internal = describe("C:\\", "Windows", 512 * GB, false).unwrap();
        assert_eq!(internal.detail, "This computer’s disk · 512 GB");

        let mac = describe("/Volumes/Family Backup", "Family Backup", 2000 * GB, true).unwrap();
        assert_eq!(
            (mac.name.as_str(), mac.detail.as_str()),
            ("Family Backup", "USB or external drive · 2 TB")
        );
        let linux = describe("/media/mark/SDCARD", "/dev/sdb1", 16 * GB, true).unwrap();
        assert_eq!(linux.name, "SDCARD", "a device path is not a name");
        assert_eq!(describe("/", "", 250 * GB, false).unwrap().name, "This computer");
    }

    #[test]
    fn system_volumes_are_not_offered() {
        assert_eq!(describe("/boot/efi", "EFI", 512 * 1024 * 1024, false), None);
        assert_eq!(describe("/System/Volumes/Preboot", "Preboot", 2 * GB, false), None);
        assert_eq!(
            describe("G:\\", "RECOVERY", 100 * 1024 * 1024, false),
            None,
            "too small to be a real drive"
        );
    }

    #[test]
    fn sizes_read_like_the_label_on_the_box() {
        assert_eq!(size(4 * GB), "4.0 GB");
        assert_eq!(size(31_900_000_000), "32 GB");
        assert_eq!(size(1_000 * GB), "1 TB");
        assert_eq!(size(1_500 * GB), "1.5 TB");
        assert_eq!(size(3_980 * GB), "4 TB");
    }

    #[test]
    fn listing_this_machines_drives_does_not_fail() {
        // Whatever is plugged in here, the list must be well-formed.
        let places = plugged_in();
        assert!(places.iter().all(|p| !p.path.is_empty() && !p.name.is_empty()));
        let removable_then_fixed: Vec<bool> = places.iter().map(|p| p.removable).collect();
        let mut sorted = removable_then_fixed.clone();
        sorted.sort_by(|a, b| b.cmp(a));
        assert_eq!(removable_then_fixed, sorted, "removable drives come first");
    }

    #[test]
    fn big_disks_are_asked_about_and_sticks_are_not() {
        let gb = 1_000_000_000;
        let ask = |bytes, removable| describe("E:\\", "X", bytes, removable).unwrap().ask_first;
        assert!(!ask(32 * gb, true), "a USB stick just starts");
        assert!(ask(2000 * gb, true), "an external hard disk is asked about");
        assert!(ask(512 * gb, false), "so is the computer's own disk");
    }
}
