//! Surveying a drive: where are the photos and videos?
//!
//! Before Keevoro can look through a source, someone has to know which
//! folders to point it at, and on a big disk that is the hard part. A
//! survey answers it in seconds. It walks the folders reading only their
//! listings, the names and sizes a file manager shows, and never opens a
//! single file. From the names it guesses which files are photos and
//! videos, and from where they cluster it suggests folders worth adding.
//!
//! A survey is a hint, not a search. It can be fooled by a wrong file
//! ending, and it knows nothing of duplicates or dates. Its one job is to
//! say "look here", so that the proper search is pointed at the right
//! places.
//!
//! The work is in two halves, kept apart so each can be tested alone:
//!
//! - [`survey`] walks the disk on several threads and counts what each
//!   folder holds ([`Tally`]).
//! - [`suggest`] is pure arithmetic on those counts: it decides which
//!   folders to offer ([`Hint`]). It never touches the disk.

use keevoro_core::{Kinds, MediaKind, SkipRules};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};

/// What one folder holds directly (not in its subfolders), by name alone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tally {
    pub folder: PathBuf,
    pub photos: usize,
    pub videos: usize,
    /// The size of those photos and videos together.
    pub bytes: u64,
    /// Files that are neither.
    pub others: usize,
    /// Of the photos and videos, those only in online storage, not on this
    /// computer: a search will pass over them unopened.
    pub online_only: usize,
}

/// What the photos in a suggested folder look like from the outside.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Likely {
    /// Sizes typical of a camera or phone.
    Photos,
    /// Many very small pictures and no videos: more likely icons or the
    /// pictures a program keeps for itself.
    AppPictures,
}

/// A folder worth looking through, with everything beneath it counted in.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hint {
    pub folder: PathBuf,
    pub photos: usize,
    pub videos: usize,
    pub bytes: u64,
    pub likely: Likely,
    /// Of those photos and videos, how many are only in online storage, so
    /// that a search will pass over them unopened.
    pub online_only: usize,
}

/// The result of a survey.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Survey {
    /// Folders to offer, the most promising first. No hint is inside another.
    pub hints: Vec<Hint>,
    pub folders_seen: usize,
    pub files_seen: usize,
    /// Folders that could not be listed, usually for want of permission.
    pub unreadable: usize,
    /// Photos and videos only in online storage, everywhere it looked.
    pub online_only: usize,
    /// True when it was stopped before it had seen everything.
    pub stopped: bool,
    /// Folders Keevoro made (a backup, an old library, its own data), left
    /// out and why, in order.
    pub left_out: Vec<(PathBuf, keevoro_core::SkipReason)>,
    /// Every folder that holds photos or videos itself, with what it holds
    /// directly, in order: the outline a look inside is drawn from.
    pub media_folders: Vec<Tally>,
}

/// How far a survey has got.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SurveyProgress {
    pub folders_seen: usize,
    pub files_seen: usize,
    pub photos: usize,
    pub videos: usize,
}

/// Below this average size, with no videos beside them, pictures are more
/// likely icons than photographs.
const SMALL_PICTURE: u64 = 60 * 1024;

#[derive(Default, Clone, Copy)]
struct Sum {
    photos: usize,
    videos: usize,
    bytes: u64,
    files: usize,
    online_only: usize,
}

impl Sum {
    fn media(self) -> usize {
        self.photos + self.videos
    }
    fn add(&mut self, other: Sum) {
        self.photos += other.photos;
        self.videos += other.videos;
        self.bytes += other.bytes;
        self.files += other.files;
        self.online_only += other.online_only;
    }
    /// At least half of what is here is photos and videos.
    fn mostly_media(self) -> bool {
        self.media() > 0 && self.media() * 2 >= self.files
    }
    /// Nine in ten of what is here is photos and videos: the bar for
    /// suggesting a whole drive, which is a much bigger thing to add.
    fn nearly_all_media(self) -> bool {
        self.media() > 0 && self.media() * 10 >= self.files * 9
    }
}

/// Chooses which folders to suggest from what each folder holds.
///
/// The aim is the folder a person would have picked themselves: high
/// enough to take in a whole collection (`Pictures`, not each of its two
/// hundred subfolders), but not so high that it drags in a disk of
/// documents and programs. So, working down from `root`:
///
/// - a folder that is mostly photos and videos, counting everything
///   beneath it, is suggested whole;
/// - so is a folder with photos sitting directly in it, since leaving it
///   out would lose them;
/// - otherwise it is only a container, and its subfolders are considered
///   one by one.
///
/// The root itself is suggested only when nearly everything on it is
/// photos (a memory card); a stray picture at the top of a system disk does not make the
/// whole disk a suggestion. No suggestion is ever inside another, so the
/// same file is never offered twice.
pub fn suggest(root: &Path, tallies: &[Tally]) -> Vec<Hint> {
    let mut direct: HashMap<&Path, Sum> = HashMap::new();
    let mut children: HashMap<&Path, Vec<&Path>> = HashMap::new();
    for t in tallies {
        direct.insert(
            &t.folder,
            Sum {
                photos: t.photos,
                videos: t.videos,
                bytes: t.bytes,
                files: t.photos + t.videos + t.others,
                online_only: t.online_only,
            },
        );
        if t.folder != root {
            if let Some(parent) = t.folder.parent() {
                children.entry(parent).or_default().push(&t.folder);
            }
        }
    }
    // Deepest first, so each folder's total is ready before its parent's.
    let mut by_depth: Vec<&Path> = direct.keys().copied().collect();
    by_depth.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    let mut whole: HashMap<&Path, Sum> = direct.clone();
    for folder in by_depth {
        if folder == root {
            continue;
        }
        let total = whole[folder];
        if let Some(parent) = folder.parent().and_then(|p| whole.get_mut(p)) {
            parent.add(total);
        }
    }

    let mut hints = Vec::new();
    let mut to_visit = vec![root];
    while let Some(folder) = to_visit.pop() {
        let Some(total) = whole.get(folder).copied() else {
            continue;
        };
        if total.media() == 0 {
            continue;
        }
        let holds_some_itself = folder != root && direct.get(folder).is_some_and(|d| d.media() > 0);
        let enough = if folder == root {
            total.nearly_all_media()
        } else {
            total.mostly_media()
        };
        if enough || holds_some_itself {
            let small = total.videos == 0 && total.bytes / (total.photos.max(1) as u64) < SMALL_PICTURE;
            hints.push(Hint {
                folder: folder.to_path_buf(),
                photos: total.photos,
                videos: total.videos,
                bytes: total.bytes,
                likely: if small { Likely::AppPictures } else { Likely::Photos },
                online_only: total.online_only,
            });
        } else if let Some(inside) = children.get(folder) {
            to_visit.extend(inside.iter().copied());
        }
    }
    // Real photos before probable icons, then the biggest first.
    hints.sort_by(|a, b| {
        (a.likely == Likely::AppPictures, std::cmp::Reverse(a.bytes), &a.folder).cmp(&(
            b.likely == Likely::AppPictures,
            std::cmp::Reverse(b.bytes),
            &b.folder,
        ))
    });
    hints
}

/// What the walking threads share.
struct Walk<'a> {
    rules: &'a SkipRules,
    kinds: &'a Kinds,
    online: &'a dyn crate::OnlineCheck,
    stop: Option<&'a AtomicBool>,
    queue: Mutex<VecDeque<PathBuf>>,
    more: Condvar,
    /// Folders queued or being listed. The walk is over when it reaches zero.
    in_hand: AtomicUsize,
    tallies: Mutex<Vec<Tally>>,
    /// Folders Keevoro made, left out, and why.
    left_out: Mutex<Vec<(PathBuf, keevoro_core::SkipReason)>>,
    folders: AtomicUsize,
    files: AtomicUsize,
    photos: AtomicUsize,
    videos: AtomicUsize,
    unreadable: AtomicUsize,
}

impl Walk<'_> {
    fn stopped(&self) -> bool {
        self.stop.is_some_and(|s| s.load(Ordering::SeqCst))
    }

    /// Lists one folder: counts its files and queues its subfolders. Only
    /// the listing is read. No file is opened.
    fn list(&self, folder: PathBuf) {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            self.unreadable.fetch_add(1, Ordering::Relaxed);
            return;
        };
        let entries: Vec<std::fs::DirEntry> = entries.flatten().collect();
        // What Keevoro made is never a source. Looked at closely only when
        // one of the names it leaves is there, so a survey stays quick.
        let telltale = entries
            .iter()
            .any(|e| e.file_name() == ".keevoro" || e.file_name() == keevoro_core::BACKUP_GUIDE);
        let made = self.rules.own_place(&folder).or_else(|| {
            telltale
                .then(|| keevoro_core::folder_made_by_keevoro(&folder))
                .flatten()
        });
        if let Some(reason) = made {
            self.folders.fetch_add(1, Ordering::Relaxed);
            if let Ok(mut left_out) = self.left_out.lock() {
                left_out.push((folder, reason));
            }
            return;
        }
        let mut tally = Tally {
            folder,
            ..Tally::default()
        };
        let mut inside = Vec::new();
        for entry in entries {
            let Ok(kind) = entry.file_type() else { continue };
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if kind.is_dir() {
                if self.rules.skip_folder(&name).is_none() {
                    inside.push(entry.path());
                }
            } else if kind.is_file() {
                if self.rules.skip_file(&name).is_some() {
                    continue;
                }
                let guess = name
                    .rsplit_once('.')
                    .and_then(|(_, ending)| self.kinds.looked_for(ending));
                match guess {
                    Some(found) => {
                        // The size and attributes come with the listing on
                        // Windows, and cost one small question elsewhere.
                        if let Ok(listing) = entry.metadata() {
                            tally.bytes += listing.len();
                            if self.online.online_only(&entry.path(), &listing) {
                                tally.online_only += 1;
                            }
                        }
                        if found == MediaKind::Photo {
                            tally.photos += 1;
                        } else {
                            tally.videos += 1;
                        }
                    }
                    None => tally.others += 1,
                }
            }
            // Links are not followed: what they point at is counted where it is.
        }
        self.folders.fetch_add(1, Ordering::Relaxed);
        self.files
            .fetch_add(tally.photos + tally.videos + tally.others, Ordering::Relaxed);
        self.photos.fetch_add(tally.photos, Ordering::Relaxed);
        self.videos.fetch_add(tally.videos, Ordering::Relaxed);
        if !inside.is_empty() {
            self.in_hand.fetch_add(inside.len(), Ordering::SeqCst);
            if let Ok(mut queue) = self.queue.lock() {
                queue.extend(inside);
            }
            self.more.notify_all();
        }
        if let Ok(mut tallies) = self.tallies.lock() {
            tallies.push(tally);
        }
    }

    fn work(&self) {
        loop {
            let next = {
                let Ok(mut queue) = self.queue.lock() else { return };
                loop {
                    if self.stopped() || self.in_hand.load(Ordering::SeqCst) == 0 {
                        break None;
                    }
                    if let Some(folder) = queue.pop_front() {
                        break Some(folder);
                    }
                    // Another thread is still listing and may queue more.
                    let Ok((waited, _)) = self.more.wait_timeout(queue, std::time::Duration::from_millis(20)) else {
                        return;
                    };
                    queue = waited;
                }
            };
            let Some(folder) = next else {
                self.more.notify_all();
                return;
            };
            self.list(folder);
            self.in_hand.fetch_sub(1, Ordering::SeqCst);
            self.more.notify_all();
        }
    }

    fn progress(&self) -> SurveyProgress {
        SurveyProgress {
            folders_seen: self.folders.load(Ordering::Relaxed),
            files_seen: self.files.load(Ordering::Relaxed),
            photos: self.photos.load(Ordering::Relaxed),
            videos: self.videos.load(Ordering::Relaxed),
        }
    }
}

/// Surveys everything under `root` and suggests the folders worth looking
/// through. Reads folder listings only, on several threads, and opens no
/// file, so it goes by the endings of names: those `kinds` looks for.
/// `on_progress` is called a few times a second from the calling thread.
/// Setting `stop` ends it early with what it has so far.
pub fn survey(
    root: &Path,
    rules: &SkipRules,
    kinds: &Kinds,
    online: &dyn crate::OnlineCheck,
    stop: Option<&AtomicBool>,
    on_progress: &mut dyn FnMut(&SurveyProgress),
) -> Survey {
    let walk = Walk {
        rules,
        kinds,
        online,
        stop,
        queue: Mutex::new(VecDeque::from([root.to_path_buf()])),
        more: Condvar::new(),
        in_hand: AtomicUsize::new(1),
        tallies: Mutex::new(Vec::new()),
        left_out: Mutex::new(Vec::new()),
        folders: AtomicUsize::new(0),
        files: AtomicUsize::new(0),
        photos: AtomicUsize::new(0),
        videos: AtomicUsize::new(0),
        unreadable: AtomicUsize::new(0),
    };
    // Listing folders waits on the disk far more than it computes, so
    // more threads than cores pays off, above all on a network drive.
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().clamp(4, 16));
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads).map(|_| scope.spawn(|| walk.work())).collect();
        while workers.iter().any(|w| !w.is_finished()) {
            std::thread::sleep(std::time::Duration::from_millis(100));
            on_progress(&walk.progress());
        }
    });
    let last = walk.progress();
    on_progress(&last);
    let (stopped, unreadable) = (walk.stopped(), walk.unreadable.load(Ordering::Relaxed));
    let tallies = walk.tallies.into_inner().unwrap_or_default();
    let online_only = tallies.iter().map(|t| t.online_only).sum();
    let mut media_folders: Vec<Tally> = tallies.iter().filter(|t| t.photos + t.videos > 0).cloned().collect();
    media_folders.sort_by(|a, b| a.folder.cmp(&b.folder));
    let mut left_out = walk.left_out.into_inner().unwrap_or_default();
    left_out.sort_by(|a, b| a.0.cmp(&b.0));
    Survey {
        hints: suggest(root, &tallies),
        folders_seen: last.folders_seen,
        files_seen: last.files_seen,
        unreadable,
        online_only,
        stopped,
        left_out,
        media_folders,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tally(folder: &str, photos: usize, videos: usize, others: usize) -> Tally {
        Tally {
            folder: PathBuf::from(folder),
            photos,
            videos,
            // Three megabytes a photo and a hundred a video, as a camera makes them.
            bytes: (photos as u64) * 3_000_000 + (videos as u64) * 100_000_000,
            others,
            online_only: 0,
        }
    }

    fn folders(hints: &[Hint]) -> Vec<String> {
        hints.iter().map(|h| h.folder.display().to_string()).collect()
    }

    #[test]
    fn a_collection_is_suggested_whole_not_folder_by_folder() {
        let hints = suggest(
            Path::new("/d"),
            &[
                tally("/d", 0, 0, 3),
                tally("/d/Documents", 0, 0, 400),
                tally("/d/Pictures", 0, 0, 1),
                tally("/d/Pictures/2019", 0, 0, 0),
                tally("/d/Pictures/2019/Cornwall", 120, 4, 2),
                tally("/d/Pictures/2020", 300, 0, 0),
            ],
        );
        assert_eq!(folders(&hints), ["/d/Pictures"]);
        assert_eq!(
            (hints[0].photos, hints[0].videos),
            (420, 4),
            "everything beneath is counted in"
        );
    }

    #[test]
    fn a_container_of_mixed_things_is_opened_up_and_its_parts_offered() {
        let hints = suggest(
            Path::new("/d"),
            &[
                tally("/d", 0, 0, 0),
                tally("/d/Users", 0, 0, 0),
                tally("/d/Users/Mark", 0, 0, 20),
                tally("/d/Users/Mark/Documents", 0, 0, 5000),
                tally("/d/Users/Mark/Pictures", 800, 0, 3),
                tally("/d/Users/Mark/Videos", 0, 40, 0),
            ],
        );
        // Videos is the bigger, so it comes first.
        assert_eq!(folders(&hints), ["/d/Users/Mark/Videos", "/d/Users/Mark/Pictures"]);
    }

    #[test]
    fn photos_loose_in_a_busy_folder_are_not_lost_and_nothing_is_offered_twice() {
        let hints = suggest(
            Path::new("/d"),
            &[
                tally("/d", 0, 0, 0),
                tally("/d/Desktop", 6, 0, 300),
                tally("/d/Desktop/Scans", 50, 0, 0),
            ],
        );
        // Desktop holds photos itself, so it is offered whole, and Scans
        // inside it is not offered a second time.
        assert_eq!(folders(&hints), ["/d/Desktop"]);
        assert_eq!(hints[0].photos, 56);
    }

    #[test]
    fn a_memory_card_is_suggested_as_itself_but_a_system_disk_is_not() {
        let card = suggest(
            Path::new("/card"),
            &[tally("/card", 0, 0, 1), tally("/card/DCIM", 900, 30, 0)],
        );
        assert_eq!(folders(&card), ["/card"]);
        // One stray picture at the top of a disk full of other things.
        let disk = suggest(
            Path::new("/c"),
            &[
                tally("/c", 1, 0, 10),
                tally("/c/Work", 0, 0, 9000),
                tally("/c/Photos", 200, 0, 0),
            ],
        );
        assert_eq!(folders(&disk), ["/c/Photos"]);
    }

    #[test]
    fn tiny_pictures_with_no_videos_are_marked_as_probably_not_photos_and_listed_last() {
        let mut icons = tally("/d/App/icons", 400, 0, 0);
        icons.bytes = 400 * 4_000;
        let hints = suggest(
            Path::new("/d"),
            &[
                tally("/d", 0, 0, 50),
                tally("/d/App", 0, 0, 900),
                icons,
                tally("/d/Holiday", 10, 0, 0),
            ],
        );
        assert_eq!(folders(&hints), ["/d/Holiday", "/d/App/icons"]);
        assert_eq!(hints[0].likely, Likely::Photos);
        assert_eq!(hints[1].likely, Likely::AppPictures);
    }

    #[test]
    fn a_disk_with_no_photos_suggests_nothing() {
        assert!(suggest(Path::new("/d"), &[tally("/d", 0, 0, 10), tally("/d/Work", 0, 0, 99)]).is_empty());
        assert!(suggest(Path::new("/d"), &[]).is_empty());
    }

    #[test]
    fn a_real_disk_is_surveyed_without_opening_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let put = |rel: &str, bytes: usize| {
            let file = root.join(rel);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, vec![0u8; bytes]).unwrap();
        };
        // Not real pictures: a survey goes by names and sizes alone.
        for n in 0..30 {
            put(&format!("Pictures/2019/IMG_{n}.JPG"), 200_000);
        }
        put("Pictures/2019/clip.mov", 900_000);
        for n in 0..10 {
            put(&format!("Work/report {n}.docx"), 10);
        }
        put("node_modules/pkg/logo.png", 10);
        put(".hidden/secret.jpg", 10);
        put("Pictures/._IMG_1.JPG", 10);

        let mut seen = SurveyProgress::default();
        let found = survey(
            root,
            &SkipRules::default(),
            &Kinds::default(),
            &crate::ByAttributes,
            None,
            &mut |p| seen = *p,
        );
        assert_eq!(found.hints.len(), 1);
        let hint = &found.hints[0];
        assert_eq!(hint.folder, root.join("Pictures"));
        assert_eq!((hint.photos, hint.videos), (30, 1));
        assert_eq!(hint.bytes, 30 * 200_000 + 900_000);
        assert_eq!(hint.likely, Likely::Photos);
        assert_eq!((seen.photos, seen.videos), (30, 1));
        // The outline: only folders holding photos or videos themselves.
        let outline: Vec<(PathBuf, usize, usize)> = found
            .media_folders
            .iter()
            .map(|t| (t.folder.clone(), t.photos, t.videos))
            .collect();
        assert_eq!(outline, [(root.join("Pictures").join("2019"), 30, 1)]);
        // Skipped folders are not even listed.
        assert_eq!(found.files_seen, 41);
        assert!(!found.stopped);
    }

    #[test]
    fn a_survey_counts_the_kinds_a_person_added_and_not_those_switched_off() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for n in 0..20 {
            let file = root.join(format!("Panoramas/P{n}.xyz"));
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, [0u8; 50]).unwrap();
        }
        fs::create_dir_all(root.join("Clips")).unwrap();
        fs::write(root.join("Clips/a.avi"), [0u8; 50]).unwrap();
        // Enough else that the whole place is not one big album.
        fs::create_dir_all(root.join("Work")).unwrap();
        for n in 0..10 {
            fs::write(root.join(format!("Work/{n}.docx")), [0u8; 5]).unwrap();
        }
        let built_in = survey(
            root,
            &SkipRules::default(),
            &Kinds::default(),
            &crate::ByAttributes,
            None,
            &mut |_| {},
        );
        assert_eq!(built_in.hints.len(), 1, "only the clips");

        let mut kinds = keevoro_core::FileKinds::default();
        kinds.add("xyz", MediaKind::Photo).unwrap();
        let off = keevoro_core::KindChange {
            on: Some(false),
            ..Default::default()
        };
        kinds.change("avi", &off).unwrap();
        let found = survey(
            root,
            &SkipRules::default(),
            &kinds.lookup(),
            &crate::ByAttributes,
            None,
            &mut |_| {},
        );
        assert_eq!(found.hints.len(), 1, "{:?}", found.hints);
        assert_eq!(found.hints[0].folder, root.join("Panoramas"));
        assert_eq!((found.hints[0].photos, found.hints[0].videos), (20, 0));
    }

    #[test]
    fn what_keevoro_made_is_left_out_of_a_survey_and_said() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let put = |rel: &str| {
            let file = root.join(rel);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, [0u8; 100]).unwrap();
        };
        for n in 0..30 {
            put(&format!("Pictures/Holiday/IMG_{n}.jpg"));
            // An old library's gathered copies and its previews.
            put(&format!("Pictures/Keevoro Library/originals/Holiday/IMG_{n}.jpg"));
            put(&format!("Pictures/Keevoro Library/renditions/display/{n}.webp"));
            // A backup sent to a folder with an ordinary name.
            put(&format!("Pictures/Copy for Mum/originals/Holiday/IMG_{n}.jpg"));
            // A folder that is a backup's destination, not yet written to.
            put(&format!("Pictures/USB/IMG_{n}.jpg"));
        }
        for n in 0..10 {
            put(&format!("Work/{n}.docx"));
        }
        put("Pictures/Keevoro Library/.keevoro/catalog.sqlite");
        put("Pictures/Copy for Mum/What is in this backup.txt");
        put("Pictures/Copy for Mum/index/gallery.json");
        let rules = SkipRules::default().with_own_places(vec![(
            std::fs::canonicalize(root.join("Pictures/USB")).unwrap(),
            keevoro_core::SkipReason::KeevoroBackup,
        )]);
        let root = std::fs::canonicalize(root).unwrap();
        let found = survey(
            &root,
            &rules,
            &Kinds::default(),
            &crate::ByAttributes,
            None,
            &mut |_| {},
        );
        let offered: Vec<_> = found.hints.iter().map(|h| h.folder.clone()).collect();
        // Pictures is offered whole, counting only the person's own photos;
        // a search of it leaves Keevoro's folders out in the same way.
        assert_eq!(offered, [root.join("Pictures")]);
        assert_eq!((found.hints[0].photos, found.files_seen), (30, 40));
        use keevoro_core::SkipReason::{KeevoroBackup, OldLibrary};
        assert_eq!(
            found.left_out,
            [
                (root.join("Pictures").join("Copy for Mum"), KeevoroBackup),
                (root.join("Pictures").join("Keevoro Library"), OldLibrary),
                (root.join("Pictures").join("USB"), KeevoroBackup),
            ]
        );
    }

    #[test]
    fn a_survey_can_be_stopped() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("a/b/c")).unwrap();
        let stop = AtomicBool::new(true);
        let found = survey(
            dir.path(),
            &SkipRules::default(),
            &Kinds::default(),
            &crate::ByAttributes,
            Some(&stop),
            &mut |_| {},
        );
        assert!(found.stopped);
    }
}
