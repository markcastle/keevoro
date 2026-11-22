//! The drives plugged into this computer.
//!
//! Asking someone to find a drive in a folder dialog is asking them to know
//! what "E:" or "/Volumes" means. Listing the drives by name, with their
//! size, lets them recognise the one they just plugged in and click it.
//!
//! This only lists drives. It reads nothing from them. It is `places.list`.
//!
//! **Noticing a drive arrive or leave.** The first screen updates by itself
//! within about two seconds of a drive arriving or leaving. While it is
//! showing and the window is visible, it asks [`if_changed`] every couple
//! of seconds with the drives it last knew. That asks the system only for
//! the list of drives it has mounted ([`mounted`]: names and places, no
//! sizes, and never a network drive), which is a list the system keeps in
//! memory: on Linux one read of `/proc/self/mounts`, on a Mac the volumes
//! it knows, on Windows its local volumes that have something in them, so a
//! memory card pushed into a reader already plugged in counts as arriving.
//! Only when that list has changed is the fuller listing made, with sizes
//! and the mapped network drives, and the screen told which drives have
//! just arrived. What changed is worked out by [`changed`], which is pure.
//! On Windows the system's own notice of a volume arriving or leaving
//! ([`watch_for_drives`], from `keevoro_frame::drives`) makes the screen
//! look at once, so it does not wait for its next look.

use serde::Serialize;
use sysinfo::{DiskRefreshKind, Disks};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    /// Where the drive is, as given back to look through it.
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
    /// The cloud service that keeps this folder in step with this
    /// computer, for a OneDrive, Dropbox, Google Drive or iCloud Drive
    /// folder. Left out for a drive.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synced: Option<keevoro_core::synced::Service>,
}

impl Place {
    /// A folder a cloud service keeps in step, offered by its name.
    pub fn synced(folder: &keevoro_core::synced::SyncedFolder) -> Self {
        Self {
            path: folder.path.clone(),
            name: folder.name.clone(),
            detail: folder.detail(),
            removable: false,
            // Only photos and videos are opened, and nothing that is only
            // online, so looking through one is quick: no need to ask.
            ask_first: false,
            synced: Some(folder.service),
        }
    }
}

/// Every drive worth offering, removable ones first, and the folders cloud
/// services keep in step on this computer ([`crate::synced`]). A drive that
/// is itself a cloud service's (Google Drive's `G:`) is offered once, as
/// the service.
pub fn plugged_in() -> Vec<Place> {
    let mut places = drives();
    let synced: Vec<Place> = crate::synced::on_this_computer().iter().map(Place::synced).collect();
    let bare = |p: &str| p.trim_end_matches(['\\', '/']).to_lowercase();
    places.retain(|drive| !synced.iter().any(|s| bare(&s.path) == bare(&drive.path)));
    places.extend(synced);
    places
}

/// Each drive the system has mounted, as (where, its label), from its
/// quick list: no sizes asked for, so never a wait on a network share.
pub fn labels() -> Vec<(String, String)> {
    Disks::new_with_refreshed_list_specifics(DiskRefreshKind::nothing())
        .list()
        .iter()
        .map(|disk| {
            (
                disk.mount_point().display().to_string(),
                disk.name().to_string_lossy().to_string(),
            )
        })
        .collect()
}

/// Every drive worth offering, removable ones first.
fn drives() -> Vec<Place> {
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
    places.extend(network_drives());
    places.sort_by(|a, b| b.removable.cmp(&a.removable).then_with(|| a.path.cmp(&b.path)));
    places.dedup_by(|a, b| a.path == b.path);
    places
}

/// The drives the system has mounted, by where each is, sorted: the quick
/// list that says whether anything has arrived or left. No sizes are asked
/// for (on Linux, asking a network share's size can hang), and the
/// computer's own system volumes are left out as [`describe`] leaves them
/// out, apart from size.
pub fn mounted() -> Vec<String> {
    let disks = Disks::new_with_refreshed_list_specifics(DiskRefreshKind::nothing());
    let mut mounts: Vec<String> = disks
        .list()
        .iter()
        .map(|disk| disk.mount_point().display().to_string())
        .filter(|mount| !is_system_mount(mount))
        .collect();
    mounts.sort();
    mounts.dedup();
    mounts
}

/// What arrived and what left between two lists of mounted drives.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Change {
    pub arrived: Vec<String>,
    pub left: Vec<String>,
}

/// What changed from `known` to `now`: `None` when nothing did. Pure.
pub fn changed(known: &[String], now: &[String]) -> Option<Change> {
    let arrived: Vec<String> = now.iter().filter(|m| !known.contains(m)).cloned().collect();
    let left: Vec<String> = known.iter().filter(|m| !now.contains(m)).cloned().collect();
    (!arrived.is_empty() || !left.is_empty()).then_some(Change { arrived, left })
}

/// The drives plugged in, as [`if_changed`] gives them when something
/// changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacesNow {
    /// The quick list, to give back next time.
    pub mounts: Vec<String>,
    /// Every drive worth offering, as [`plugged_in`] gives them.
    pub places: Vec<Place>,
    /// The paths of those that have just arrived, to bring to notice.
    pub arrived: Vec<String>,
}

/// The drives plugged in, if they are not what was `known` from the last
/// time: `None` when the quick list is unchanged, so nothing more is asked.
/// With nothing known (the first look), every drive is given and none is
/// said to have just arrived. `asked` makes it list the drives in full
/// whatever the quick list says: the person pressed Refresh, or the system
/// said something arrived.
pub fn if_changed(known: Option<&[String]>, asked: bool) -> Option<PlacesNow> {
    let now = mounted();
    let change = match known {
        None => Some(Change::default()),
        Some(known) => changed(known, &now).or_else(|| asked.then(Change::default)),
    }?;
    let places = plugged_in();
    let arrived = arrived_places(&places, &change.arrived);
    Some(PlacesNow {
        mounts: now,
        places,
        arrived,
    })
}

/// The places among `places` that are at one of the mounts that arrived.
/// Pure.
pub fn arrived_places(places: &[Place], arrived: &[String]) -> Vec<String> {
    let bare = |p: &str| p.trim_end_matches(['\\', '/']).to_string();
    places
        .iter()
        .filter(|place| arrived.iter().any(|m| bare(m) == bare(&place.path)))
        .map(|place| place.path.clone())
        .collect()
}

/// Asks to be told when a drive arrives or leaves, where the system says
/// (Windows). False where it does not, and looking every couple of seconds
/// is the only way.
pub fn watch_for_drives(notice: impl Fn() + Send + Sync + 'static) -> bool {
    keevoro_frame::drives::on_change(Box::new(notice))
}

/// What to call the drive mounted at `path`, if one is: "KINGSTON (E:)".
/// From the quick list, so it costs next to nothing and never waits on a
/// network share. `None` for a folder that is not a drive of its own.
pub fn drive_name(path: &std::path::Path) -> Option<String> {
    let bare = |p: &str| p.trim_end_matches(['\\', '/']).to_string();
    // A source is kept by its real path, which Windows writes `\\?\E:\`.
    let wanted = bare(&keevoro_engine::friendly_path(path));
    let disks = Disks::new_with_refreshed_list_specifics(DiskRefreshKind::nothing());
    disks
        .list()
        .iter()
        .find(|disk| bare(&disk.mount_point().display().to_string()) == wanted)
        .map(|disk| {
            name_for(
                &disk.mount_point().display().to_string(),
                &disk.name().to_string_lossy(),
            )
        })
}

/// Drive letters that lead to another computer: a mapped network drive,
/// such as `Z:` for a share on a server. The system's list of disks leaves
/// these out, because they are not disks of this computer, but to the
/// person they are a drive like any other and belong on the same screen.
///
/// Each letter is asked on a thread of its own and given a short while to
/// answer: a mapped drive whose server is switched off can otherwise hold
/// the whole list up for many seconds. One that does not answer in time is
/// not offered this time; "Choose a folder" still reaches it. The first
/// screen asks for the list each time it is shown and each time the window
/// is returned to, so the answers are kept for [`KEEP_FOR`], and a letter
/// still being asked is never asked again until it answers (see
/// [`Remembered`]): going back and forth never waits again, and never
/// leaves a pile of threads waiting on a server that has gone.
#[cfg(windows)]
fn network_drives() -> Vec<Place> {
    use std::sync::{mpsc, Mutex, PoisonError};
    use std::time::{Duration, Instant};
    const PATIENCE: Duration = Duration::from_millis(1500);
    static DRIVES: Mutex<Remembered> = Mutex::new(Remembered::new());
    let lock = || DRIVES.lock().unwrap_or_else(PoisonError::into_inner);
    let letters = {
        let mut drives = lock();
        let now = Instant::now();
        if !drives.needs_asking(now) {
            return drives.offered();
        }
        let letters = drives.letters_to_ask();
        drives.asking(&letters, now);
        letters
    };
    let (said, heard) = mpsc::channel();
    for &letter in &letters {
        let said = said.clone();
        std::thread::spawn(move || {
            let root = format!("{letter}:\\");
            let path = std::path::Path::new(&root);
            let there = path.is_dir() && keevoro_engine::reach::is_across_a_network(path);
            // Kept even if it comes too late for this list, for the next.
            DRIVES
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .heard(letter, there);
            let _ = said.send(());
        });
    }
    drop(said);
    let until = Instant::now() + PATIENCE;
    let mut answered = 0;
    while answered < letters.len() {
        let Some(left) = until.checked_duration_since(Instant::now()) else {
            break;
        };
        match heard.recv_timeout(left) {
            Ok(()) => answered += 1,
            Err(_) => break,
        }
    }
    lock().offered()
}

#[cfg(not(windows))]
fn network_drives() -> Vec<Place> {
    Vec::new()
}

/// How long the drive letters' answers are kept before they are asked
/// again. Long enough that going back and forth between screens, or in and
/// out of the window, never waits; short enough that a drive mapped a
/// moment ago soon appears.
pub const KEEP_FOR: std::time::Duration = std::time::Duration::from_secs(30);

/// What the drive letters last said about being network drives, and which
/// are still being asked. Pure, so its rules are tested everywhere; only
/// the asking itself is Windows'.
#[cfg(any(windows, test))]
#[derive(Debug, Default)]
pub(crate) struct Remembered {
    /// Each letter's last answer: true for a network drive.
    answers: std::collections::BTreeMap<char, bool>,
    /// When the letters were last asked.
    asked_at: Option<std::time::Instant>,
    /// Letters asked that have not answered yet.
    in_flight: std::collections::BTreeSet<char>,
}

#[cfg(any(windows, test))]
impl Remembered {
    pub(crate) const fn new() -> Self {
        Self {
            answers: std::collections::BTreeMap::new(),
            asked_at: None,
            in_flight: std::collections::BTreeSet::new(),
        }
    }

    /// True when the answers are missing or older than [`KEEP_FOR`].
    pub(crate) fn needs_asking(&self, now: std::time::Instant) -> bool {
        self.asked_at
            .is_none_or(|at| now.saturating_duration_since(at) >= KEEP_FOR)
    }

    /// Every letter but those still being asked from before.
    pub(crate) fn letters_to_ask(&self) -> Vec<char> {
        ('A'..='Z').filter(|l| !self.in_flight.contains(l)).collect()
    }

    /// `letters` are being asked, as of `now`.
    pub(crate) fn asking(&mut self, letters: &[char], now: std::time::Instant) {
        self.in_flight.extend(letters.iter().copied());
        self.asked_at = Some(now);
    }

    /// A letter has answered.
    pub(crate) fn heard(&mut self, letter: char, there: bool) {
        self.in_flight.remove(&letter);
        self.answers.insert(letter, there);
    }

    /// The network drives to offer: those whose last answer said so.
    pub(crate) fn offered(&self) -> Vec<Place> {
        let answers: Vec<(char, bool)> = self.answers.iter().map(|(l, t)| (*l, *t)).collect();
        across_a_network(&answers)
    }
}

/// The letters that answered as network drives, as places to offer.
#[cfg(any(windows, test))]
fn across_a_network(answers: &[(char, bool)]) -> Vec<Place> {
    let mut letters: Vec<char> = answers.iter().filter(|(_, there)| *there).map(|(l, _)| *l).collect();
    letters.sort_unstable();
    letters
        .into_iter()
        .map(|letter| Place {
            path: format!("{letter}:\\"),
            name: format!("Network drive ({letter}:)"),
            detail: "On another computer or server".to_string(),
            removable: false,
            // It may be vast, and is slower than a disk: always ask first.
            ask_first: true,
            synced: None,
        })
        .collect()
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
    if bytes < SMALLEST || is_system_mount(mount) {
        return None;
    }
    let name = name_for(mount, label);
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
        synced: None,
    })
}

/// Places the system mounts for itself, never a drive a person plugs in.
fn is_system_mount(mount: &str) -> bool {
    [
        "/boot",
        "/snap",
        "/sys",
        "/proc",
        "/dev",
        "/run",
        "/System/Volumes",
        "/private",
    ]
    .iter()
    .any(|m| mount.starts_with(m))
}

/// What a person would call the drive mounted at `mount` with this label.
fn name_for(mount: &str, label: &str) -> String {
    // "E:\" reads better as "E:"; "/Volumes/KINGSTON" as "KINGSTON".
    let letter = mount.trim_end_matches(['\\', '/']);
    let last = letter.rsplit(['/', '\\']).next().unwrap_or(letter);
    let label = label.trim();
    let is_letter = letter.len() == 2 && letter.ends_with(':');
    match (label.is_empty() || label.starts_with("/dev/"), is_letter) {
        (false, true) => format!("{label} ({letter})"),
        (false, false) => label.to_string(),
        (true, true) => format!("Drive {letter}"),
        (true, false) if last.is_empty() => "This computer".to_string(),
        (true, false) => last.to_string(),
    }
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
        let linux = describe("/media/you/SDCARD", "/dev/sdb1", 16 * GB, true).unwrap();
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
    fn a_mapped_network_drive_is_offered_like_any_other_drive() {
        let offered = across_a_network(&[('Y', false), ('Z', true), ('C', false), ('N', true)]);
        let names: Vec<&str> = offered.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            ["Network drive (N:)", "Network drive (Z:)"],
            "in order, and only those that answered"
        );
        assert_eq!(offered[1].path, "Z:\\");
        assert_eq!(offered[1].detail, "On another computer or server");
        assert!(
            !offered[1].removable && offered[1].ask_first,
            "never looked through whole without asking"
        );
        assert!(
            across_a_network(&[]).is_empty(),
            "none that answered in time, none offered"
        );
    }

    #[test]
    fn drive_letters_are_asked_once_in_a_while_and_a_slow_one_is_never_asked_twice_at_once() {
        use std::time::{Duration, Instant};
        let start = Instant::now();
        let mut drives = Remembered::new();
        assert!(drives.needs_asking(start), "never asked");
        assert!(drives.offered().is_empty());

        let letters = drives.letters_to_ask();
        assert_eq!(letters.len(), 26);
        drives.asking(&letters, start);
        for letter in letters.iter().copied().filter(|l| *l != 'Q') {
            drives.heard(letter, letter == 'Z');
        }
        // Q's server is switched off: it has not answered.
        let names: Vec<String> = drives.offered().into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["Network drive (Z:)"]);

        // Shown again soon after, nothing is asked: the answers are kept.
        assert!(!drives.needs_asking(start + Duration::from_secs(5)));
        assert!(!drives.needs_asking(start + KEEP_FOR - Duration::from_millis(1)));

        // Later they are asked again, but never Q while it is still asked.
        let later = start + KEEP_FOR;
        assert!(drives.needs_asking(later));
        let again = drives.letters_to_ask();
        assert_eq!(again.len(), 25);
        assert!(!again.contains(&'Q'), "no second thread waits on the same server");
        drives.asking(&again, later);
        drives.heard('Z', false);
        drives.heard('Q', true);
        let names: Vec<String> = drives.offered().into_iter().map(|p| p.name).collect();
        assert_eq!(
            names,
            ["Network drive (Q:)"],
            "a late answer is kept for the next list; one unmapped since is not offered"
        );
        assert!(
            drives.letters_to_ask().contains(&'Q'),
            "asked again once it has answered"
        );
    }

    #[test]
    fn a_drive_plugged_in_or_taken_out_is_noticed_and_nothing_else_is() {
        let known = vec!["/".to_string(), "/media/you/KINGSTON".to_string()];
        assert_eq!(changed(&known, &known), None, "nothing changed, nothing more is asked");
        let now = vec!["/".to_string(), "/media/you/SANDISK".to_string()];
        assert_eq!(
            changed(&known, &now),
            Some(Change {
                arrived: vec!["/media/you/SANDISK".to_string()],
                left: vec!["/media/you/KINGSTON".to_string()],
            })
        );
        // A card pushed into a reader already plugged in is a volume
        // arriving, though no drive letter is new.
        let reader = vec!["C:\\".to_string()];
        let card = vec!["C:\\".to_string(), "F:\\".to_string()];
        assert_eq!(changed(&reader, &card).unwrap().arrived, ["F:\\"]);
        assert_eq!(changed(&card, &reader).unwrap().left, ["F:\\"]);
    }

    #[test]
    fn the_drives_that_just_arrived_are_picked_out_of_the_full_list() {
        let places = [
            describe("E:\\", "KINGSTON", 32 * GB, true).unwrap(),
            describe("F:\\", "SanDisk", 32 * GB, true).unwrap(),
            describe("/Volumes/Family Backup", "Family Backup", 2000 * GB, true).unwrap(),
        ];
        assert_eq!(arrived_places(&places, &["F:\\".to_string()]), ["F:\\"]);
        assert_eq!(
            arrived_places(&places, &["/Volumes/Family Backup/".to_string()]),
            ["/Volumes/Family Backup"],
            "with or without the last slash"
        );
        assert!(arrived_places(&places, &[]).is_empty());
    }

    #[test]
    fn the_quick_list_is_quick_and_a_look_with_nothing_changed_asks_nothing_more() {
        let started = std::time::Instant::now();
        let mounts = mounted();
        let took = started.elapsed();
        eprintln!("The quick list of {} drives took {took:?}.", mounts.len());
        assert!(mounts.windows(2).all(|w| w[0] < w[1]), "sorted, each once");
        assert!(mounts.iter().all(|m| !is_system_mount(m)));
        // Generous, for a busy test machine; it is microseconds to a few
        // milliseconds in practice.
        assert!(took < std::time::Duration::from_secs(2), "{took:?}");
        let started = std::time::Instant::now();
        let first = if_changed(None, false).expect("a first look always lists");
        eprintln!("The full list took {:?}.", started.elapsed());
        assert!(first.arrived.is_empty(), "nothing has just arrived on a first look");
        assert_eq!(if_changed(Some(&first.mounts), false), None);
        let asked = if_changed(Some(&first.mounts), true).expect("Refresh always lists");
        assert_eq!(asked.places, first.places);
        let gone = if_changed(Some(&["/nowhere".to_string()]), false).expect("a change lists");
        assert!(gone
            .arrived
            .iter()
            .all(|path| gone.places.iter().any(|p| &p.path == path)));
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
