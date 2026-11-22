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
//! - [`survey`] walks the disk on several threads and writes down what
//!   each folder holds, every file sized, in a map of the place
//!   ([`crate::map`]), from which what each folder holds as the
//!   suggestions count it is worked out ([`Tally`]). The size of every
//!   file, not only photos and videos, costs one small question per file
//!   on a Mac or Linux (it comes with the listing on Windows), so there it
//!   is asked for the files the suggestions do not need only once they are
//!   final, and the survey suggests as quickly as it did before the map.
//! - [`suggest`] is pure arithmetic on those counts: it decides which
//!   folders to offer ([`Hint`]). It never touches the disk.

use crate::map::{Builder, CountedIn, FileSeen, Limits, Listing, Map};
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
    /// Other files a person put on the list, looked for.
    pub carried: usize,
    /// The size of those photos, videos and other files together.
    pub bytes: u64,
    /// Files that are none of them.
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
    /// Other files a person put on the list. Never said when there are
    /// none, so nothing changes for anyone who never adds a kind of them.
    #[serde(skip_serializing_if = "is_none_of_them")]
    pub others: usize,
    pub bytes: u64,
    pub likely: Likely,
    /// Of those photos and videos, how many are only in online storage, so
    /// that a search will pass over them unopened.
    pub online_only: usize,
}

fn is_none_of_them(n: &usize) -> bool {
    *n == 0
}

/// The result of a survey.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// False when it was stopped before every folder Keevoro never looks
    /// through was sized for the map; the suggestions are final either way
    /// unless `stopped`.
    pub sized_all: bool,
    /// Everything it saw, folder by folder.
    pub map: Map,
    /// How long it took until the suggestions were final: the wait a person
    /// has before they can choose.
    pub suggested_in: std::time::Duration,
}

/// How far a survey has got.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SurveyProgress {
    pub folders_seen: usize,
    pub files_seen: usize,
    pub photos: usize,
    pub videos: usize,
    /// Other files a person put on the list, seen so far.
    pub others: usize,
    /// Every suggestion is final, and the folders Keevoro never looks
    /// through are being sized for the map.
    pub sizing: bool,
    /// The top of the map so far, so it can be shown growing: given at
    /// most every [`TOP_EVERY`], and empty in between.
    pub top: Vec<crate::map::TopBox>,
}

/// How often the top of the map is given with the progress: twice a
/// second, often enough to see it grow, and no more.
pub const TOP_EVERY: std::time::Duration = std::time::Duration::from_millis(500);

/// Below this average size, with no videos beside them, pictures are more
/// likely icons than photographs.
const SMALL_PICTURE: u64 = 60 * 1024;

#[derive(Default, Clone, Copy)]
struct Sum {
    photos: usize,
    videos: usize,
    carried: usize,
    bytes: u64,
    files: usize,
    online_only: usize,
}

impl Sum {
    /// Everything wanted: photos, videos and other files carried along.
    fn media(self) -> usize {
        self.photos + self.videos + self.carried
    }
    fn add(&mut self, other: Sum) {
        self.photos += other.photos;
        self.videos += other.videos;
        self.carried += other.carried;
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
                carried: t.carried,
                bytes: t.bytes,
                files: t.photos + t.videos + t.carried + t.others,
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
            let small =
                total.videos == 0 && total.carried == 0 && total.bytes / (total.photos.max(1) as u64) < SMALL_PICTURE;
            hints.push(Hint {
                folder: folder.to_path_buf(),
                photos: total.photos,
                videos: total.videos,
                others: total.carried,
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

/// A folder waiting to be listed.
struct Pending {
    folder: PathBuf,
    /// Where what it holds is counted.
    into: CountedIn,
    /// It is one Keevoro never looks through, or inside one: it is listed
    /// only to size it, once the suggestions are final.
    passed_by: bool,
}

/// Folders waiting: those the suggestions need first, then those that are
/// only sized for the map.
#[derive(Default)]
struct Queues {
    now: VecDeque<Pending>,
    later: VecDeque<Pending>,
    /// Where the look for folders whose other files wait to be sized has
    /// got to, and whether it is over.
    unsized_from: usize,
    unsized_done: bool,
}

/// One piece of work for a walking thread.
enum Work {
    List(Pending),
    /// Sizes a listed folder's other files.
    Size(u32, PathBuf),
}

/// On Windows the size of every file comes with the folder's listing, so
/// everything is sized as it is listed. Elsewhere each size is a question
/// of its own, so only photos and videos are sized at first: what the
/// suggestions need. The rest of each folder is sized once they are final.
const SIZES_COME_WITH_THE_LISTING: bool = cfg!(windows);

/// What the walking threads share.
struct Walk<'a> {
    rules: &'a SkipRules,
    kinds: &'a Kinds,
    online: &'a dyn crate::OnlineCheck,
    limits: Limits,
    stop: Option<&'a AtomicBool>,
    queues: Mutex<Queues>,
    more: Condvar,
    /// Folders queued or being listed, of each queue. The walk is over when
    /// both reach zero, and the second is only started on when the first is.
    now_in_hand: AtomicUsize,
    later_in_hand: AtomicUsize,
    /// The suggestions are final and the folders Keevoro passes by are
    /// being sized.
    sizing: AtomicBool,
    started: std::time::Instant,
    /// When the suggestions were final.
    suggested: std::sync::OnceLock<std::time::Duration>,
    map: Mutex<Builder>,
    /// Folders left out because of where they are or what is in them
    /// (Keevoro's own, those the person left out), and why.
    left_out: Mutex<Vec<(PathBuf, keevoro_core::SkipReason)>>,
    folders: AtomicUsize,
    files: AtomicUsize,
    photos: AtomicUsize,
    videos: AtomicUsize,
    others: AtomicUsize,
    unreadable: AtomicUsize,
}

impl Walk<'_> {
    fn stopped(&self) -> bool {
        self.stop.is_some_and(|s| s.load(Ordering::SeqCst))
    }

    /// Every suggestion is final: only sizing for the map is left.
    fn now_sizing(&self) {
        self.sizing.store(true, Ordering::SeqCst);
        let _ = self.suggested.set(self.started.elapsed());
    }

    fn builder(&self) -> std::sync::MutexGuard<'_, Builder> {
        self.map.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Queues folders, counting them in hand first, so the walk never
    /// looks finished while they wait.
    fn queue(&self, pending: Vec<Pending>) {
        if pending.is_empty() {
            return;
        }
        let later = pending.iter().filter(|p| p.passed_by).count();
        self.later_in_hand.fetch_add(later, Ordering::SeqCst);
        self.now_in_hand.fetch_add(pending.len() - later, Ordering::SeqCst);
        if let Ok(mut queues) = self.queues.lock() {
            for p in pending {
                if p.passed_by {
                    queues.later.push_back(p);
                } else {
                    queues.now.push_back(p);
                }
            }
        }
        self.more.notify_all();
    }

    /// A folder Keevoro never looks through, found by where it is or what
    /// is in it: said, and sized later.
    fn pass_by(&self, p: Pending, reason: keevoro_core::SkipReason) {
        let into = match p.into {
            CountedIn::Own(node) => {
                self.builder().passed_by(node, reason);
                CountedIn::Own(node)
            }
            CountedIn::Above { node, .. } => CountedIn::Above { node, passed_by: true },
        };
        if let Ok(mut left_out) = self.left_out.lock() {
            left_out.push((p.folder.clone(), reason));
        }
        self.queue(vec![Pending {
            folder: p.folder,
            into,
            passed_by: true,
        }]);
    }

    /// Lists one folder: counts its files and queues its subfolders. Only
    /// the listing is read. No file is opened.
    fn list(&self, p: Pending) {
        // A place left out by where it is (Keevoro's own, or one the
        // person left out) is not even listed until the suggestions are.
        if !p.passed_by {
            if let Some(reason) = self.rules.own_place(&p.folder) {
                self.pass_by(p, reason);
                return;
            }
        }
        let entries: Vec<std::fs::DirEntry> = match std::fs::read_dir(&p.folder) {
            Ok(entries) => entries.flatten().collect(),
            Err(_) => {
                // A folder Keevoro passes by is not one the person is told
                // could not be read; the map marks it.
                if !p.passed_by {
                    self.unreadable.fetch_add(1, Ordering::Relaxed);
                }
                self.builder().unreadable(p.into);
                return;
            }
        };
        // What Keevoro made is never a source. Looked at closely only when
        // one of the names it leaves is there, so a survey stays quick.
        if !p.passed_by {
            let telltale = entries.iter().any(|e| {
                let name = e.file_name();
                name == ".keevoro" || name == keevoro_core::BACKUP_GUIDE || name == keevoro_core::SET_ASIDE_NOTE
            });
            if let Some(reason) = telltale
                .then(|| keevoro_core::folder_made_by_keevoro(&p.folder))
                .flatten()
            {
                self.pass_by(p, reason);
                return;
            }
        }
        let mut listing = Listing::default();
        let mut found: Vec<(String, Option<keevoro_core::SkipReason>, PathBuf)> = Vec::new();
        let (mut photos, mut videos, mut others) = (0, 0, 0);
        for entry in entries {
            let Ok(kind) = entry.file_type() else { continue };
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if kind.is_dir() {
                let reason = if p.passed_by {
                    None
                } else {
                    self.rules.skip_folder(&name)
                };
                found.push((name.into_owned(), reason, entry.path()));
            } else if kind.is_file() {
                let on_list = name.rsplit_once('.').and_then(|(_, ending)| self.kinds.listed(ending));
                let looked_for = on_list.is_some_and(|(_, on)| on);
                let system = self.rules.skip_file(&name).is_some();
                let counted = looked_for && !system;
                if !p.passed_by && counted {
                    match on_list.map(|(kind, _)| kind) {
                        Some(MediaKind::Photo) => photos += 1,
                        Some(MediaKind::Other) => others += 1,
                        _ => videos += 1,
                    }
                }
                let mut file = FileSeen {
                    name: &name,
                    bytes: 0,
                    listed: on_list.map(|(kind, _)| kind),
                    looked_for,
                    online_only: false,
                    system,
                };
                // Only a folder with a node of its own can be come back to.
                let later =
                    !SIZES_COME_WITH_THE_LISTING && !counted && !p.passed_by && matches!(p.into, CountedIn::Own(_));
                if later {
                    listing.file_unsized(&file);
                } else {
                    (file.bytes, file.online_only) = self.size_of(&entry);
                    listing.file(&file, &self.limits);
                }
            }
            // Links are not followed: what they point at is counted where it is.
        }
        self.folders.fetch_add(1, Ordering::Relaxed);
        self.files.fetch_add(
            usize::try_from(listing.files()).unwrap_or(usize::MAX),
            Ordering::Relaxed,
        );
        self.photos.fetch_add(photos, Ordering::Relaxed);
        self.videos.fetch_add(videos, Ordering::Relaxed);
        self.others.fetch_add(others, Ordering::Relaxed);
        let first = {
            let mut map = self.builder();
            let names: Vec<(&str, Option<keevoro_core::SkipReason>)> =
                found.iter().map(|(name, reason, _)| (name.as_str(), *reason)).collect();
            let first = map.subfolders(p.into, &names);
            map.record(p.into, listing);
            first
        };
        let above_passed_by = matches!(p.into, CountedIn::Above { passed_by: true, .. });
        let inside = found
            .into_iter()
            .enumerate()
            .map(|(i, (_, reason, folder))| {
                let passed_by = p.passed_by || reason.is_some();
                let into = match first {
                    Some(first) => CountedIn::Own(first + u32::try_from(i).unwrap_or(0)),
                    None => CountedIn::Above {
                        node: p.into.node(),
                        passed_by: passed_by || above_passed_by,
                    },
                };
                Pending {
                    folder,
                    into,
                    passed_by,
                }
            })
            .collect();
        self.queue(inside);
    }

    /// A file's size, and whether it is only online, from its listing.
    fn size_of(&self, entry: &std::fs::DirEntry) -> (u64, bool) {
        match entry.metadata() {
            Ok(listed_as) => (listed_as.len(), self.online.online_only(&entry.path(), &listed_as)),
            Err(_) => (0, false),
        }
    }

    /// Sizes a listed folder's files that the suggestions did not need: a
    /// second look at its listing, once they are final.
    fn size_rest(&self, node: u32, folder: &Path) {
        let Ok(entries) = std::fs::read_dir(folder) else { return };
        let mut sizes = Listing::default();
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|t| t.is_file()) {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let on_list = name.rsplit_once('.').and_then(|(_, ending)| self.kinds.listed(ending));
            let looked_for = on_list.is_some_and(|(_, on)| on);
            let system = self.rules.skip_file(&name).is_some();
            if looked_for && !system {
                continue;
            }
            let (bytes, online_only) = self.size_of(&entry);
            sizes.file(
                &FileSeen {
                    name: &name,
                    bytes,
                    listed: on_list.map(|(kind, _)| kind),
                    looked_for,
                    online_only,
                    system,
                },
                &self.limits,
            );
        }
        self.builder().sized(node, sizes);
    }

    /// The next piece of work: the folders the suggestions need, and only
    /// when they are all done, sizing what they did not need and the
    /// folders Keevoro passes by.
    fn next(&self) -> Option<Work> {
        let Ok(mut queues) = self.queues.lock() else {
            return None;
        };
        loop {
            if self.stopped() {
                return None;
            }
            if let Some(p) = queues.now.pop_front() {
                return Some(Work::List(p));
            }
            if self.now_in_hand.load(Ordering::SeqCst) == 0 {
                if !queues.unsized_done {
                    match self.builder().next_unsized(queues.unsized_from) {
                        Some((node, folder)) => {
                            queues.unsized_from = node as usize + 1;
                            self.now_sizing();
                            return Some(Work::Size(node, folder));
                        }
                        None => queues.unsized_done = true,
                    }
                }
                if let Some(p) = queues.later.pop_front() {
                    self.now_sizing();
                    return Some(Work::List(p));
                }
                if self.later_in_hand.load(Ordering::SeqCst) == 0 {
                    return None;
                }
            }
            // Another thread is still listing and may queue more.
            let Ok((waited, _)) = self.more.wait_timeout(queues, std::time::Duration::from_millis(20)) else {
                return None;
            };
            queues = waited;
        }
    }

    fn work(&self) {
        // Pause holds a survey too, between one folder and the next.
        keevoro_core::pause::wait_while_paused();
        while let Some(work) = self.next() {
            match work {
                Work::List(p) => {
                    let later = p.passed_by;
                    self.list(p);
                    if later {
                        self.later_in_hand.fetch_sub(1, Ordering::SeqCst);
                    } else {
                        self.now_in_hand.fetch_sub(1, Ordering::SeqCst);
                    }
                }
                Work::Size(node, folder) => self.size_rest(node, &folder),
            }
            self.more.notify_all();
            keevoro_core::pause::wait_while_paused();
        }
        self.more.notify_all();
    }

    /// How far it has got, with the top of the map when `with_top`.
    fn progress(&self, with_top: bool) -> SurveyProgress {
        SurveyProgress {
            folders_seen: self.folders.load(Ordering::Relaxed),
            files_seen: self.files.load(Ordering::Relaxed),
            photos: self.photos.load(Ordering::Relaxed),
            videos: self.videos.load(Ordering::Relaxed),
            others: self.others.load(Ordering::Relaxed),
            sizing: self.sizing.load(Ordering::Relaxed),
            top: if with_top {
                self.builder().top(crate::map::MOST_TOP_BOXES)
            } else {
                Vec::new()
            },
        }
    }
}

/// Surveys everything under `root` and suggests the folders worth looking
/// through. Reads folder listings only, on several threads, and opens no
/// file, so it goes by the endings of names: those `kinds` looks for.
///
/// Everything it sees goes into a map of the place, within `limits`. The
/// folders Keevoro never looks through (system and app folders, hidden
/// ones, its own, those left out) are listed last, only to size them for
/// the map, once every suggestion is final: [`SurveyProgress::sizing`]
/// says when, and stopping then loses no suggestion.
///
/// `on_progress` is called a few times a second from the calling thread.
/// Setting `stop` ends it early with what it has so far.
pub fn survey(
    root: &Path,
    rules: &SkipRules,
    kinds: &Kinds,
    online: &dyn crate::OnlineCheck,
    limits: Limits,
    stop: Option<&AtomicBool>,
    on_progress: &mut dyn FnMut(&SurveyProgress),
) -> Survey {
    let walk = Walk {
        rules,
        kinds,
        online,
        limits,
        stop,
        queues: Mutex::new(Queues {
            now: VecDeque::from([Pending {
                folder: root.to_path_buf(),
                into: CountedIn::Own(0),
                passed_by: false,
            }]),
            later: VecDeque::new(),
            unsized_from: 0,
            unsized_done: false,
        }),
        more: Condvar::new(),
        now_in_hand: AtomicUsize::new(1),
        later_in_hand: AtomicUsize::new(0),
        sizing: AtomicBool::new(false),
        started: std::time::Instant::now(),
        suggested: std::sync::OnceLock::new(),
        map: Mutex::new(Builder::new(root, limits)),
        left_out: Mutex::new(Vec::new()),
        folders: AtomicUsize::new(0),
        files: AtomicUsize::new(0),
        photos: AtomicUsize::new(0),
        videos: AtomicUsize::new(0),
        others: AtomicUsize::new(0),
        unreadable: AtomicUsize::new(0),
    };
    // Listing folders waits on the disk far more than it computes, so
    // more threads than cores pays off, above all on a network drive.
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().clamp(4, 16));
    std::thread::scope(|scope| {
        // Each worker holds a sender until it is done, so the wait ends the
        // moment the last one is, not at the next tick.
        let (done, finished) = std::sync::mpsc::channel::<()>();
        for _ in 0..threads {
            let done = done.clone();
            let walk = &walk;
            scope.spawn(move || {
                walk.work();
                drop(done);
            });
        }
        drop(done);
        let mut top_given: Option<std::time::Instant> = None;
        while let Err(std::sync::mpsc::RecvTimeoutError::Timeout) =
            finished.recv_timeout(std::time::Duration::from_millis(100))
        {
            let with_top = top_given.is_none_or(|at| at.elapsed() >= TOP_EVERY);
            if with_top {
                top_given = Some(std::time::Instant::now());
            }
            on_progress(&walk.progress(with_top));
        }
    });
    let last = walk.progress(true);
    on_progress(&last);
    let asked_to_stop = walk.stopped();
    let undone_now = walk.now_in_hand.load(Ordering::SeqCst) > 0;
    let undone_later = walk.later_in_hand.load(Ordering::SeqCst) > 0;
    let unreadable = walk.unreadable.load(Ordering::Relaxed);
    let map = walk
        .map
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .finish();
    let rest_unsized = map.nodes.iter().any(|n| n.has(crate::map::flag::REST_UNSIZED));
    let tallies = map.tallies();
    let online_only = tallies.iter().map(|t| t.online_only).sum();
    let mut left_out = walk.left_out.into_inner().unwrap_or_default();
    left_out.sort_by(|a, b| a.0.cmp(&b.0));
    Survey {
        hints: suggest(root, &tallies),
        folders_seen: last.folders_seen,
        files_seen: last.files_seen,
        unreadable,
        online_only,
        stopped: asked_to_stop && undone_now,
        sized_all: !(asked_to_stop && (undone_now || undone_later || rest_unsized)),
        left_out,
        map,
        suggested_in: walk.suggested.get().copied().unwrap_or_else(|| walk.started.elapsed()),
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
            carried: 0,
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
            crate::map::Limits::default(),
            None,
            &mut |p| seen = p.clone(),
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
            .map
            .media_folders(0)
            .iter()
            .map(|t| (t.folder.clone(), t.photos, t.videos))
            .collect();
        assert_eq!(outline, [(root.join("Pictures").join("2019"), 30, 1)]);
        // Folders Keevoro never looks through are never suggested, but they
        // are sized for the map once the suggestions are final, and marked.
        assert_eq!(found.files_seen, 44, "every file, the system ones too");
        assert!(!found.stopped && found.sized_all);
        assert!(seen.sizing);
        // The last progress carries the top of the map, as it grew.
        let top: Vec<(&str, u64)> = seen.top.iter().map(|t| (&*t.name, t.bytes())).collect();
        assert_eq!(top[0], ("Pictures", 30 * 200_000 + 900_000 + 10));
        assert_eq!(top.len(), 4, "Pictures, Work, node_modules and .hidden: {top:?}");
        let totals = found.map.totals();
        let developer = found.map.find(&root.join("node_modules")).unwrap();
        assert_eq!(
            found.map.nodes[developer as usize].skipped,
            Some(keevoro_core::SkipReason::DeveloperFolder)
        );
        assert_eq!(
            totals[developer as usize].kinds[0],
            crate::map::KindSum { files: 1, bytes: 10 }
        );
        let hidden = found.map.find(&root.join(".hidden")).unwrap();
        assert_eq!(totals[hidden as usize].bytes(), 10);
        assert_eq!(
            totals[0].bytes(),
            30 * 200_000 + 900_000 + 10 * 10 + 10 + 10 + 10,
            "everything, photos or not"
        );
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
            crate::map::Limits::default(),
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
            crate::map::Limits::default(),
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
            crate::map::Limits::default(),
            None,
            &mut |_| {},
        );
        let offered: Vec<_> = found.hints.iter().map(|h| h.folder.clone()).collect();
        // Pictures is offered whole, counting only the person's own photos;
        // a search of it leaves Keevoro's folders out in the same way.
        assert_eq!(offered, [root.join("Pictures")]);
        use keevoro_core::SkipReason::{KeevoroBackup, OldLibrary};
        assert_eq!(found.hints[0].photos, 30);
        // Their files are still sized for the map, after the suggestions.
        assert_eq!(found.files_seen, 40 + 61 + 32 + 30);
        let library = found.map.find(&root.join("Pictures").join("Keevoro Library")).unwrap();
        assert_eq!(found.map.nodes[library as usize].skipped, Some(OldLibrary));
        assert_eq!(
            found.map.totals()[library as usize].kinds[0].files,
            60,
            "its copies and previews"
        );
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
    fn a_folder_left_out_is_never_listed_or_suggested() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        for n in 0..20 {
            for folder in ["Games", "Games 2"] {
                let file = root.join(folder).join(format!("IMG_{n}.jpg"));
                fs::create_dir_all(file.parent().unwrap()).unwrap();
                fs::write(file, [0u8; 100]).unwrap();
            }
        }
        // Not mostly photos as a whole, so its folders are offered one by one.
        fs::create_dir_all(root.join("Work")).unwrap();
        for n in 0..60 {
            fs::write(root.join("Work").join(format!("{n}.docx")), [0u8; 100]).unwrap();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root.join("Games"), fs::Permissions::from_mode(0o000)).unwrap();
        }
        let rules = SkipRules::default().with_left_out(vec![root.join("Games")]);
        let found = survey(
            &root,
            &rules,
            &Kinds::default(),
            &crate::ByAttributes,
            crate::map::Limits::default(),
            None,
            &mut |_| {},
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root.join("Games"), fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert_eq!(found.unreadable, 0, "it was not even listed");
        let offered: Vec<_> = found.hints.iter().map(|h| h.folder.clone()).collect();
        assert_eq!(offered, [root.join("Games 2")]);
        assert_eq!(
            found.left_out,
            [(root.join("Games"), keevoro_core::SkipReason::LeftOut)]
        );
    }

    /// How long the walk takes over a made-up tree of 200,000 files. Run by
    /// hand: `cargo test -p keevoro-scan how_long -- --ignored --nocapture`.
    #[test]
    #[ignore = "a measurement, run by hand"]
    fn how_long_a_survey_of_200_000_files_takes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let endings = ["jpg", "jpg", "jpg", "jpg", "mp4", "txt", "zip", "dat", "png", "docx"];
        for f in 0..2_000 {
            let folder = root.join(format!("{}", f % 20)).join(format!("folder {f}"));
            fs::create_dir_all(&folder).unwrap();
            for n in 0..100 {
                fs::write(folder.join(format!("file {n}.{}", endings[n % endings.len()])), b"").unwrap();
            }
        }
        let mut times = Vec::new();
        let mut suggested = Vec::new();
        for _ in 0..5 {
            let started = std::time::Instant::now();
            let found = survey(
                root,
                &SkipRules::default(),
                &Kinds::default(),
                &crate::ByAttributes,
                crate::map::Limits::default(),
                None,
                &mut |_| {},
            );
            times.push(started.elapsed().as_secs_f64());
            suggested.push(found.suggested_in.as_secs_f64());
            assert_eq!(found.files_seen, 200_000);
        }
        times.sort_by(f64::total_cmp);
        suggested.sort_by(f64::total_cmp);
        eprintln!(
            "200,000 files: suggestions final in {:.3} s (median of five; fastest {:.3} s); whole map {:.3} s \
             (fastest {:.3} s)",
            suggested[2], suggested[0], times[2], times[0]
        );
    }

    /// Stops the survey the first time it is asked about a file whose path
    /// holds `at`.
    #[derive(Debug)]
    struct StopAt<'a> {
        at: &'static str,
        stop: &'a AtomicBool,
    }

    impl crate::OnlineCheck for StopAt<'_> {
        fn online_only(&self, file: &Path, _: &std::fs::Metadata) -> bool {
            if file.to_string_lossy().contains(self.at) {
                self.stop.store(true, Ordering::SeqCst);
            }
            false
        }
    }

    #[test]
    fn folders_keevoro_passes_by_are_sized_after_the_suggestions_and_stopping_then_loses_none() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let put = |rel: &str, bytes: usize| {
            let file = root.join(rel);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, vec![0u8; bytes]).unwrap();
        };
        for n in 0..20 {
            put(&format!("Pictures/IMG_{n}.jpg"), 1_000);
            put(&format!("Work/{n}.docx"), 10);
            put(&format!("node_modules/pkg/deep/{n}.png"), 10);
        }
        put("node_modules/logo.png", 10);
        let whole = survey(
            &root,
            &SkipRules::default(),
            &Kinds::default(),
            &crate::ByAttributes,
            crate::map::Limits::default(),
            None,
            &mut |_| {},
        );
        assert!(whole.sized_all && !whole.stopped);
        assert!(!whole.map.totals()[0].not_sized);

        let stop = AtomicBool::new(false);
        let stopping = StopAt {
            at: "node_modules",
            stop: &stop,
        };
        let cut_short = survey(
            &root,
            &SkipRules::default(),
            &Kinds::default(),
            &stopping,
            crate::map::Limits::default(),
            Some(&stop),
            &mut |_| {},
        );
        assert!(stop.load(Ordering::SeqCst), "it was stopped while sizing");
        assert!(!cut_short.stopped, "every suggestion was already final");
        assert_eq!(cut_short.hints, whole.hints);
        assert!(!cut_short.sized_all);
        assert!(cut_short.map.totals()[0].not_sized, "the map says what was not sized");
    }

    #[test]
    fn a_link_or_junction_is_not_followed_so_nothing_is_counted_twice() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let photos = root.join("Photos");
        fs::create_dir_all(&photos).unwrap();
        for n in 0..5 {
            fs::write(photos.join(format!("IMG_{n}.jpg")), [0u8; 100]).unwrap();
        }
        let link = root.join("Shortcut to Photos");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&photos, &link).unwrap();
        #[cfg(windows)]
        {
            // A junction, which needs no special permission to make.
            let made = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&link)
                .arg(&photos)
                .output()
                .unwrap();
            assert!(made.status.success(), "{made:?}");
        }
        let found = survey(
            &root,
            &SkipRules::default(),
            &Kinds::default(),
            &crate::ByAttributes,
            crate::map::Limits::default(),
            None,
            &mut |_| {},
        );
        let totals = found.map.totals();
        assert_eq!(totals[0].kinds[0], crate::map::KindSum { files: 5, bytes: 500 });
        assert_eq!(found.map.find(&link), None, "the link is not a folder of the map");
        assert_eq!(found.hints.iter().map(|h| h.photos).sum::<usize>(), 5);
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
            crate::map::Limits::default(),
            Some(&stop),
            &mut |_| {},
        );
        assert!(found.stopped);
    }
}
