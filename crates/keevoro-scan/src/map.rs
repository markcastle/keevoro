//! The map of a surveyed place: where the space goes.
//!
//! A survey reads every folder's listing anyway, so it writes down what it
//! saw: for each folder, how many photos, videos and other files it holds
//! directly and how much room each takes, which file endings they have,
//! how much is only in online storage, and the biggest files by name. That
//! is enough to draw a map of a whole drive, in the style of WinDirStat,
//! and to answer "what is taking the room?" without walking it again.
//!
//! Everything is shown, photos or not, because that is what makes a map
//! useful for finding things: a folder of zip files or an old library is
//! exactly what a person is looking for. Folders Keevoro never looks
//! through (system and app folders, its own, those left out) are sized too,
//! after the suggestions are final, and marked.
//!
//! Memory is bounded whatever the drive (see [`Limits`]). Past each bound
//! only the identity of small things is lost, never their size: totals are
//! always exact.
//!
//! - **Folders.** One [`FolderNode`] each, holding its name and its
//!   parent's index, not a whole path. Past the bound, the folders listed
//!   next are counted into the folder above them, which says how many more
//!   folders it holds.
//! - **Files.** Never kept one by one, only summed by kind and by ending,
//!   except the biggest ([`NamedFile`]): at most a few dozen per folder and
//!   a bounded number in all, the smallest given up first. A file given up
//!   is still in its folder's sums.
//! - **Endings.** Each ending is written once, in a table of bounded size
//!   (the rest are "other endings"), and each folder keeps a short list of
//!   the endings in it, up to a bound for the whole map, after which only
//!   the totals for the whole place are kept.
//!
//! This module is pure: the walk in `survey.rs` fills a [`Builder`] with
//! what each listing held, and everything here is tested with made-up
//! listings. [`view`] is the level of detail the screens and the command
//! line ask for: a folder's contents to a depth, with anything too small
//! to see merged.

use keevoro_core::{MediaKind, SkipReason};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::path::{Path, PathBuf};

/// How many kinds a map tells apart.
pub const KINDS: usize = 4;

/// The kinds a map tells apart: the three a person can ask Keevoro to back
/// up, and everything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Photo,
    Video,
    /// Everything that is not on Keevoro's list of kinds.
    Other,
    /// Other files a person put on the list, to be carried along.
    Carried,
}

impl Kind {
    pub const ALL: [Kind; KINDS] = [Kind::Photo, Kind::Video, Kind::Other, Kind::Carried];
    /// What a backup can take: photos, videos and other files carried.
    pub const LISTED: [Kind; 3] = [Kind::Photo, Kind::Video, Kind::Carried];

    /// The kind of a file that is on Keevoro's list as `media`, or is not.
    pub fn of(media: Option<MediaKind>) -> Kind {
        match media {
            Some(MediaKind::Photo) => Kind::Photo,
            Some(MediaKind::Video) => Kind::Video,
            Some(MediaKind::Other) => Kind::Carried,
            None => Kind::Other,
        }
    }

    pub const fn index(self) -> usize {
        self as usize
    }

    /// The kind on Keevoro's list it is, if it is one.
    pub fn media(self) -> Option<MediaKind> {
        match self {
            Kind::Photo => Some(MediaKind::Photo),
            Kind::Video => Some(MediaKind::Video),
            Kind::Carried => Some(MediaKind::Other),
            Kind::Other => None,
        }
    }
}

/// How many files, and how much room they take.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KindSum {
    pub files: u64,
    pub bytes: u64,
}

impl KindSum {
    pub fn add(&mut self, other: KindSum) {
        self.files += other.files;
        self.bytes += other.bytes;
    }

    fn one(bytes: u64) -> KindSum {
        KindSum { files: 1, bytes }
    }

    fn less(self, other: KindSum) -> KindSum {
        KindSum {
            files: self.files.saturating_sub(other.files),
            bytes: self.bytes.saturating_sub(other.bytes),
        }
    }
}

/// The sums of a folder or a box, every kind, added up.
pub fn all_of(sums: &[KindSum; KINDS]) -> KindSum {
    let mut all = KindSum::default();
    for sum in sums {
        all.add(*sum);
    }
    all
}

/// How much a map may hold. The defaults keep a survey of the largest
/// drive to some tens of megabytes; tests use small ones to prove the
/// totals stay exact past each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Folders kept one by one.
    pub folders: usize,
    /// Files kept by name, the biggest, across the whole map.
    pub named_files: usize,
    /// Files kept by name in any one folder.
    pub named_per_folder: usize,
    /// The least a file must take to be kept by name.
    pub named_at_least: u64,
    /// Different endings kept by name; the rest are "other endings".
    pub endings: usize,
    /// (folder, ending) pairs kept across the whole map.
    pub ending_pairs: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            folders: 100_000,
            named_files: 50_000,
            named_per_folder: 64,
            named_at_least: 1024 * 1024,
            endings: 2_000,
            ending_pairs: 500_000,
        }
    }
}

/// What is true of a folder in a map, as bits of [`FolderNode::flags`].
pub mod flag {
    /// Its listing was read. A folder still waiting when the survey was
    /// stopped has none, and is "not sized yet".
    pub const LISTED: u8 = 1;
    /// Its listing could not be read, usually for want of permission.
    pub const UNREADABLE: u8 = 2;
    /// It is inside a folder Keevoro never looks through.
    pub const BENEATH_SKIPPED: u8 = 4;
    /// Its endings were not kept, because the map had kept as many as it
    /// may; its sums are still exact.
    pub const ENDINGS_DROPPED: u8 = 8;
    /// Its files that are not photos or videos looked for are counted but
    /// not sized yet: that waits until the suggestions are final.
    pub const REST_UNSIZED: u8 = 16;
}

/// How much of one ending a folder holds directly.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EndingSum {
    /// Its place in [`Map::endings`].
    pub ending: u16,
    pub files: u32,
    pub bytes: u64,
}

/// What a folder holds that the suggestions never counted, kept so they are
/// worked out exactly as before the map existed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Uncounted {
    /// Photos and videos of a kind this survey was not looking for (switched
    /// off, or the other kind for a backup that wants only one): counted
    /// as other files by the suggestions.
    pub off: [KindSum; KINDS],
    /// Files the suggestions do not count at all: system files such as
    /// `Thumbs.db`, and anything in a folder Keevoro never looks through that
    /// was counted into this one past the folder bound.
    pub unseen: [KindSum; KINDS],
}

/// One folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderNode {
    /// The folder it is in; [`NO_PARENT`] for the place surveyed.
    pub parent: u32,
    pub name: Box<str>,
    /// What it holds directly, by [`Kind`], including what was counted into
    /// it from folders past the folder bound.
    pub direct: [KindSum; KINDS],
    /// Of those bytes, how many are only in online storage.
    pub online_only_bytes: u64,
    /// Of the photos and videos looked for, how many are only online.
    pub online_only_media: u32,
    /// Why Keevoro never looks through it, when it does not. Only on the
    /// folder itself; those inside it carry [`flag::BENEATH_SKIPPED`].
    pub skipped: Option<SkipReason>,
    pub flags: u8,
    /// Folders inside it counted into it, past the folder bound.
    pub more_folders: u32,
    /// Its subfolders are the nodes `first_child..first_child + children`.
    pub first_child: u32,
    pub children: u32,
    /// What it holds directly, by ending.
    pub endings: Box<[EndingSum]>,
    pub uncounted: Option<Box<Uncounted>>,
}

/// The parent of the place surveyed.
pub const NO_PARENT: u32 = u32::MAX;

impl FolderNode {
    fn new(parent: u32, name: &str) -> FolderNode {
        FolderNode {
            parent,
            name: name.into(),
            direct: [KindSum::default(); KINDS],
            online_only_bytes: 0,
            online_only_media: 0,
            skipped: None,
            flags: 0,
            more_folders: 0,
            first_child: 0,
            children: 0,
            endings: Box::new([]),
            uncounted: None,
        }
    }

    pub fn has(&self, bit: u8) -> bool {
        self.flags & bit != 0
    }

    /// Never looked through by Keevoro: skipped itself, or inside a folder
    /// that is.
    pub fn passed_by(&self) -> bool {
        self.skipped.is_some() || self.has(flag::BENEATH_SKIPPED)
    }

    /// Its subfolders' indexes.
    pub fn subfolders(&self) -> std::ops::Range<u32> {
        self.first_child..self.first_child + self.children
    }
}

/// One ending, as kept in [`Map::endings`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ending {
    /// Lower case, without the dot. Empty for files with no ending, and
    /// [`OTHER_ENDINGS`] for those past the bound.
    pub name: Box<str>,
    /// Photo or video when it is on Keevoro's list, switched on or not.
    pub kind: Kind,
}

/// The name kept for endings past the bound.
pub const OTHER_ENDINGS: &str = "…";

/// A file kept by name: one of the biggest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedFile {
    pub folder: u32,
    pub name: Box<str>,
    pub bytes: u64,
    pub kind: Kind,
    pub online_only: bool,
}

/// What a survey saw, folder by folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Map {
    /// The place surveyed, in its full form.
    pub root: PathBuf,
    /// Every folder; the place surveyed is the first. A folder always comes
    /// after the folder it is in.
    pub nodes: Vec<FolderNode>,
    pub endings: Vec<Ending>,
    /// By folder, then the biggest first.
    pub named: Vec<NamedFile>,
    /// Every file in the place by ending, by its place in `endings`.
    pub ending_totals: Vec<KindSum>,
    /// Some folders' endings were not kept (see [`flag::ENDINGS_DROPPED`]).
    pub endings_dropped: bool,
}

/// One file as the walk saw it in a listing.
#[derive(Debug, Clone, Copy)]
pub struct FileSeen<'a> {
    pub name: &'a str,
    pub bytes: u64,
    /// On Keevoro's list as a photo or video, switched on or not.
    pub listed: Option<MediaKind>,
    /// On the list and looked for by this survey.
    pub looked_for: bool,
    pub online_only: bool,
    /// A system file such as `Thumbs.db`, never counted by suggestions.
    pub system: bool,
}

/// What one folder's listing held, gathered by the walk without a lock and
/// then handed to the [`Builder`] in one go.
#[derive(Debug, Clone, Default)]
pub struct Listing {
    sums: [KindSum; KINDS],
    uncounted: Uncounted,
    online_only_bytes: u64,
    online_only_media: u32,
    endings: Vec<(String, Kind, KindSum)>,
    big: Vec<(Box<str>, u64, Kind, bool)>,
    /// Files counted without their size, to be sized later.
    not_sized: u64,
}

impl Listing {
    /// Counts one file in whose size is not asked yet: one the suggestions
    /// do not need, sized once they are final ([`Builder::sized`]).
    pub fn file_unsized(&mut self, file: &FileSeen<'_>) {
        let limits = Limits {
            named_at_least: u64::MAX,
            ..Limits::default()
        };
        self.file(&FileSeen { bytes: 0, ..*file }, &limits);
        self.not_sized += 1;
    }

    /// Counts one file in.
    pub fn file(&mut self, file: &FileSeen<'_>, limits: &Limits) {
        let kind = Kind::of(file.listed);
        let one = KindSum::one(file.bytes);
        self.sums[kind.index()].add(one);
        if file.system {
            self.uncounted.unseen[kind.index()].add(one);
        } else if kind != Kind::Other && !file.looked_for {
            self.uncounted.off[kind.index()].add(one);
        }
        if file.online_only {
            self.online_only_bytes += file.bytes;
            if file.looked_for && !file.system {
                self.online_only_media += 1;
            }
        }
        // Most folders hold a few endings, so a look along them, without
        // making a lower-case copy of each name, is quickest.
        let ending = ending_slice(file.name);
        match self
            .endings
            .iter_mut()
            .find(|(name, _, _)| name.eq_ignore_ascii_case(ending))
        {
            Some((_, _, sum)) => sum.add(one),
            None => self.endings.push((ending.to_ascii_lowercase(), kind, one)),
        }
        if file.bytes >= limits.named_at_least {
            self.big.push((file.name.into(), file.bytes, kind, file.online_only));
        }
    }

    /// How many files it held.
    pub fn files(&self) -> u64 {
        all_of(&self.sums).files
    }
}

/// A file's ending as the map keeps it: lower case, without the dot,
/// letters and numbers only and at most ten of them; empty otherwise, so
/// names like `notes.2019-05-01 copy` do not fill the table.
pub fn ending_of(name: &str) -> String {
    ending_slice(name).to_ascii_lowercase()
}

/// [`ending_of`], as it is written in the name.
fn ending_slice(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((stem, ending))
            if !stem.is_empty()
                && (1..=10).contains(&ending.len())
                && ending.bytes().all(|b| b.is_ascii_alphanumeric()) =>
        {
            ending
        }
        _ => "",
    }
}

/// Where a listing's contents go: into the folder's own node, or counted
/// into a folder above it, past the folder bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CountedIn {
    /// The folder's own node.
    Own(u32),
    /// A folder above it. `passed_by` when the folder listed is one Keevoro
    /// never looks through, so its files are kept out of the suggestions.
    Above { node: u32, passed_by: bool },
}

impl CountedIn {
    pub fn node(self) -> u32 {
        match self {
            CountedIn::Own(node) | CountedIn::Above { node, .. } => node,
        }
    }
}

/// A named file, ordered by size so the smallest is let go first.
#[derive(Debug, PartialEq, Eq)]
struct BySize(NamedFile);

impl Ord for BySize {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.0.bytes, self.0.folder, &self.0.name).cmp(&(other.0.bytes, other.0.folder, &other.0.name))
    }
}

impl PartialOrd for BySize {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// What one box at the top of a map holds so far: a folder in the place
/// surveyed, the files directly in it, or the smaller folders together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopBox {
    pub name: Box<str>,
    pub item: TopItem,
    pub kinds: [KindSum; KINDS],
}

/// What a [`TopBox`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopItem {
    Folder,
    /// The files directly in the place surveyed.
    FilesHere,
    /// This many smaller folders, together.
    Smaller(u32),
}

impl TopBox {
    pub fn bytes(&self) -> u64 {
        all_of(&self.kinds).bytes
    }
}

/// The most boxes [`Builder::top`] gives while a survey walks.
pub const MOST_TOP_BOXES: usize = 48;

/// Fills a [`Map`] as the walk goes, within its [`Limits`].
pub struct Builder {
    limits: Limits,
    map: Map,
    ids: HashMap<Box<str>, u16>,
    named: BinaryHeap<Reverse<BySize>>,
    pairs: usize,
    /// What each folder in the place surveyed holds so far, everything
    /// inside it counted in, by its place among the place's subfolders;
    /// kept as the walk goes so the map can be shown growing.
    top: Vec<[KindSum; KINDS]>,
}

impl Builder {
    /// A map of `root` with nothing in it yet but the place itself.
    pub fn new(root: &Path, limits: Limits) -> Builder {
        let name = root
            .file_name()
            .map_or_else(|| root.display().to_string(), |n| n.to_string_lossy().into_owned());
        let mut builder = Builder {
            limits,
            map: Map {
                root: root.to_path_buf(),
                nodes: vec![FolderNode::new(NO_PARENT, &name)],
                endings: Vec::new(),
                named: Vec::new(),
                ending_totals: Vec::new(),
                endings_dropped: false,
            },
            ids: HashMap::new(),
            named: BinaryHeap::new(),
            pairs: 0,
            top: Vec::new(),
        };
        builder.intern("", Kind::Other);
        builder.intern(OTHER_ENDINGS, Kind::Other);
        builder
    }

    fn intern(&mut self, ending: &str, kind: Kind) -> u16 {
        if let Some(id) = self.ids.get(ending) {
            return *id;
        }
        if self.map.endings.len() >= self.limits.endings.max(2) {
            return 1;
        }
        let id = u16::try_from(self.map.endings.len()).unwrap_or(1);
        self.map.endings.push(Ending {
            name: ending.into(),
            kind,
        });
        self.map.ending_totals.push(KindSum::default());
        self.ids.insert(ending.into(), id);
        id
    }

    /// Adds the subfolders found in a listing, each with why Keevoro never
    /// looks through it, if it does not. They get nodes of their own, the
    /// first of which is given back, when the folder listed has one and
    /// there is room for them all. Otherwise they are counted into the
    /// folder their contents go to, which says how many more it holds.
    pub fn subfolders(&mut self, listed: CountedIn, found: &[(&str, Option<SkipReason>)]) -> Option<u32> {
        let parent = listed.node();
        let at = self.map.nodes.get(parent as usize)?;
        if found.is_empty() {
            return None;
        }
        let beneath = at.passed_by();
        if matches!(listed, CountedIn::Above { .. }) || self.map.nodes.len() + found.len() > self.limits.folders {
            let count = u32::try_from(found.len()).unwrap_or(u32::MAX);
            if let Some(at) = self.map.nodes.get_mut(parent as usize) {
                at.more_folders = at.more_folders.saturating_add(count);
            }
            return None;
        }
        let first = u32::try_from(self.map.nodes.len()).ok()?;
        for (name, skipped) in found {
            let mut node = FolderNode::new(parent, name);
            node.skipped = *skipped;
            if beneath {
                node.flags |= flag::BENEATH_SKIPPED;
            }
            self.map.nodes.push(node);
        }
        if let Some(at) = self.map.nodes.get_mut(parent as usize) {
            at.first_child = first;
            at.children = u32::try_from(found.len()).unwrap_or(0);
        }
        if parent == 0 {
            self.top = vec![[KindSum::default(); KINDS]; found.len()];
        }
        Some(first)
    }

    /// Which of the place's subfolders `node` is, or is inside: its place
    /// among them. `None` for the place itself.
    fn top_of(&self, node: u32) -> Option<usize> {
        let mut at = node;
        loop {
            let parent = self.map.nodes.get(at as usize)?.parent;
            if parent == NO_PARENT {
                return None;
            }
            if parent == 0 {
                return at.checked_sub(self.map.nodes[0].first_child).map(|i| i as usize);
            }
            at = parent;
        }
    }

    /// Adds what was found in `node` to the box at the top it is in.
    fn add_to_top(&mut self, node: u32, sums: &[KindSum; KINDS], bytes_only: bool) {
        let Some(at) = self.top_of(node).and_then(|i| self.top.get_mut(i)) else {
            return;
        };
        for (kept, more) in at.iter_mut().zip(sums) {
            if bytes_only {
                kept.bytes += more.bytes;
            } else {
                kept.add(*more);
            }
        }
    }

    /// The boxes at the top of the map so far, the biggest first, for
    /// showing it grow while the walk goes on: each of the place's
    /// subfolders with everything inside it, the files directly in it, and
    /// past `most` boxes, the smallest folders together. Folders with nothing
    /// found in them yet are left out.
    pub fn top(&self, most: usize) -> Vec<TopBox> {
        let root = &self.map.nodes[0];
        let mut boxes: Vec<TopBox> = self
            .top
            .iter()
            .enumerate()
            .filter(|(_, kinds)| all_of(kinds).files > 0)
            .filter_map(|(i, kinds)| {
                let node = self.map.nodes.get(root.first_child as usize + i)?;
                Some(TopBox {
                    name: node.name.clone(),
                    item: TopItem::Folder,
                    kinds: *kinds,
                })
            })
            .collect();
        if all_of(&root.direct).files > 0 {
            boxes.push(TopBox {
                name: root.name.clone(),
                item: TopItem::FilesHere,
                kinds: root.direct,
            });
        }
        boxes.sort_by(|a, b| b.bytes().cmp(&a.bytes()).then_with(|| a.name.cmp(&b.name)));
        let most = most.max(2);
        if boxes.len() > most {
            let mut smaller = TopBox {
                name: "Smaller folders".into(),
                item: TopItem::Smaller(0),
                kinds: [KindSum::default(); KINDS],
            };
            for gone in boxes.split_off(most - 1) {
                for (kept, more) in smaller.kinds.iter_mut().zip(gone.kinds) {
                    kept.add(more);
                }
                if let TopItem::Smaller(n) = &mut smaller.item {
                    *n += u32::from(gone.item == TopItem::Folder);
                }
            }
            boxes.push(smaller);
        }
        boxes
    }

    /// Marks a folder as one Keevoro never looks through, found by what is
    /// in it or where it is rather than by its name.
    pub fn passed_by(&mut self, node: u32, reason: SkipReason) {
        if let Some(at) = self.map.nodes.get_mut(node as usize) {
            at.skipped = Some(reason);
        }
    }

    /// A folder whose listing could not be read.
    pub fn unreadable(&mut self, into: CountedIn) {
        if let CountedIn::Own(node) = into {
            if let Some(at) = self.map.nodes.get_mut(node as usize) {
                at.flags |= flag::UNREADABLE | flag::LISTED;
            }
        }
    }

    /// Adds what one folder's listing held.
    pub fn record(&mut self, into: CountedIn, listing: Listing) {
        let node = into.node() as usize;
        if node >= self.map.nodes.len() {
            return;
        }
        let mut endings: Vec<EndingSum> = Vec::with_capacity(listing.endings.len());
        for (name, kind, sum) in &listing.endings {
            let id = self.intern(name, *kind);
            if let Some(total) = self.map.ending_totals.get_mut(id as usize) {
                total.add(*sum);
            }
            let files = u32::try_from(sum.files).unwrap_or(u32::MAX);
            match endings.iter_mut().find(|e| e.ending == id) {
                Some(e) => {
                    e.files = e.files.saturating_add(files);
                    e.bytes += sum.bytes;
                }
                None => endings.push(EndingSum {
                    ending: id,
                    files,
                    bytes: sum.bytes,
                }),
            }
        }
        if let CountedIn::Own(_) = into {
            self.keep_named(into.node(), listing.big);
        }
        self.add_to_top(into.node(), &listing.sums, false);
        let limit = self.limits.ending_pairs;
        let at = &mut self.map.nodes[node];
        if let CountedIn::Own(_) = into {
            at.flags |= flag::LISTED;
            if listing.not_sized > 0 {
                at.flags |= flag::REST_UNSIZED;
            }
        }
        for kind in Kind::ALL {
            at.direct[kind.index()].add(listing.sums[kind.index()]);
        }
        at.online_only_bytes += listing.online_only_bytes;
        let mut uncounted = listing.uncounted;
        match into {
            CountedIn::Above { passed_by: true, .. } => {
                // None of it was looked through, so none of it is counted.
                uncounted = Uncounted {
                    unseen: listing.sums,
                    ..Uncounted::default()
                };
            }
            _ => at.online_only_media = at.online_only_media.saturating_add(listing.online_only_media),
        }
        if uncounted != Uncounted::default() {
            let kept = at.uncounted.get_or_insert_with(Box::default);
            for i in 0..KINDS {
                kept.off[i].add(uncounted.off[i]);
                kept.unseen[i].add(uncounted.unseen[i]);
            }
        }
        if at.has(flag::ENDINGS_DROPPED) {
            return;
        }
        let mut merged: Vec<EndingSum> = at.endings.to_vec();
        let before = merged.len();
        for e in endings {
            match merged.iter_mut().find(|m| m.ending == e.ending) {
                Some(m) => {
                    m.files = m.files.saturating_add(e.files);
                    m.bytes += e.bytes;
                }
                None => merged.push(e),
            }
        }
        let added = merged.len() - before;
        if self.pairs + added > limit {
            // Past the bound: this folder keeps no endings of its own. The
            // totals for the whole place above still have them.
            self.pairs -= before;
            at.endings = Box::new([]);
            at.flags |= flag::ENDINGS_DROPPED;
            self.map.endings_dropped = true;
        } else {
            self.pairs += added;
            at.endings = merged.into_boxed_slice();
        }
    }

    /// Adds the sizes of a folder's files that were counted without them,
    /// from a second listing that held only those: their bytes, nothing
    /// counted twice. A file gone since is simply not sized.
    pub fn sized(&mut self, node: u32, sizes: Listing) {
        if node as usize >= self.map.nodes.len() {
            return;
        }
        let mut endings = Vec::with_capacity(sizes.endings.len());
        for (name, kind, sum) in &sizes.endings {
            let id = self.intern(name, *kind);
            if let Some(total) = self.map.ending_totals.get_mut(id as usize) {
                total.bytes += sum.bytes;
            }
            endings.push((id, sum.bytes));
        }
        self.keep_named(node, sizes.big);
        self.add_to_top(node, &sizes.sums, true);
        let at = &mut self.map.nodes[node as usize];
        at.flags &= !flag::REST_UNSIZED;
        for kind in Kind::ALL {
            at.direct[kind.index()].bytes += sizes.sums[kind.index()].bytes;
        }
        at.online_only_bytes += sizes.online_only_bytes;
        if let Some(kept) = at.uncounted.as_deref_mut() {
            for i in 0..KINDS {
                kept.off[i].bytes += sizes.uncounted.off[i].bytes;
                kept.unseen[i].bytes += sizes.uncounted.unseen[i].bytes;
            }
        }
        if at.has(flag::ENDINGS_DROPPED) {
            return;
        }
        let mut merged = at.endings.to_vec();
        for (id, bytes) in endings {
            if let Some(e) = merged.iter_mut().find(|e| e.ending == id) {
                e.bytes += bytes;
            }
        }
        at.endings = merged.into_boxed_slice();
    }

    /// The next folder at or after `from` whose other files wait to be
    /// sized, with its path.
    pub fn next_unsized(&self, from: usize) -> Option<(u32, PathBuf)> {
        let found = self.map.nodes[from.min(self.map.nodes.len())..]
            .iter()
            .position(|n| n.has(flag::REST_UNSIZED))?;
        let node = u32::try_from(from + found).ok()?;
        Some((node, self.map.path_of(node)))
    }

    /// Keeps the biggest of a folder's big files by name, within the bounds.
    fn keep_named(&mut self, folder: u32, mut big: Vec<(Box<str>, u64, Kind, bool)>) {
        if self.limits.named_files == 0 || self.limits.named_per_folder == 0 {
            return;
        }
        big.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        big.truncate(self.limits.named_per_folder);
        for (name, bytes, kind, online_only) in big {
            self.named.push(Reverse(BySize(NamedFile {
                folder,
                name,
                bytes,
                kind,
                online_only,
            })));
            if self.named.len() > self.limits.named_files {
                // The smallest is let go; its size stays in its folder's sums.
                self.named.pop();
            }
        }
    }

    /// The map, finished.
    pub fn finish(mut self) -> Map {
        let mut named: Vec<NamedFile> = self.named.into_iter().map(|Reverse(BySize(f))| f).collect();
        named.sort_by(|a, b| {
            a.folder
                .cmp(&b.folder)
                .then(b.bytes.cmp(&a.bytes))
                .then_with(|| a.name.cmp(&b.name))
        });
        self.map.named = named;
        self.map
    }
}

/// What a folder holds, everything beneath it counted in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Totals {
    pub kinds: [KindSum; KINDS],
    pub online_only_bytes: u64,
    /// Folders beneath it, including those counted into others.
    pub folders: u64,
    /// Some folder beneath it, or it, was not sized: the survey was
    /// stopped first.
    pub not_sized: bool,
}

impl Totals {
    pub fn bytes(&self) -> u64 {
        all_of(&self.kinds).bytes
    }
}

impl Map {
    /// Every folder's totals, by its index. Worked out from the bottom up,
    /// which needs one pass because a folder always comes after its parent.
    pub fn totals(&self) -> Vec<Totals> {
        let mut totals: Vec<Totals> = self
            .nodes
            .iter()
            .map(|n| Totals {
                kinds: n.direct,
                online_only_bytes: n.online_only_bytes,
                folders: u64::from(n.more_folders),
                not_sized: !n.has(flag::LISTED) || n.has(flag::REST_UNSIZED),
            })
            .collect();
        for i in (1..self.nodes.len()).rev() {
            let parent = self.nodes[i].parent as usize;
            if parent >= i {
                continue;
            }
            let child = totals[i];
            let up = &mut totals[parent];
            for k in 0..KINDS {
                up.kinds[k].add(child.kinds[k]);
            }
            up.online_only_bytes += child.online_only_bytes;
            up.folders += child.folders + 1;
            up.not_sized |= child.not_sized;
        }
        totals
    }

    /// The folder's full path.
    pub fn path_of(&self, node: u32) -> PathBuf {
        let mut names = Vec::new();
        let mut at = node;
        while at != 0 && (at as usize) < self.nodes.len() {
            names.push(&*self.nodes[at as usize].name);
            at = self.nodes[at as usize].parent;
        }
        let mut path = self.root.clone();
        for name in names.into_iter().rev() {
            path.push(name);
        }
        path
    }

    /// The folder at `path`, a full path from the computer (never one as
    /// typed), if it is in the map. `None` for anything outside the place
    /// surveyed, and for a folder counted into another past the bound.
    pub fn find(&self, path: &Path) -> Option<u32> {
        let inside = path.strip_prefix(&self.root).ok()?;
        let mut at = 0u32;
        for part in inside.components() {
            let name = part.as_os_str().to_string_lossy();
            let node = self.nodes.get(at as usize)?;
            at = node.subfolders().find(|c| *self.nodes[*c as usize].name == *name)?;
        }
        Some(at)
    }

    /// Is `node` the folder `top` or inside it?
    pub fn within(&self, node: u32, top: u32) -> bool {
        let mut at = node;
        loop {
            if at == top {
                return true;
            }
            match self.nodes.get(at as usize) {
                Some(n) if n.parent != NO_PARENT => at = n.parent,
                _ => return false,
            }
        }
    }

    /// The folders `top` and every folder inside it, parents before their
    /// subfolders.
    pub fn beneath(&self, top: u32) -> Vec<u32> {
        let mut out = Vec::new();
        let mut to_visit = vec![top];
        while let Some(at) = to_visit.pop() {
            let Some(node) = self.nodes.get(at as usize) else {
                continue;
            };
            out.push(at);
            to_visit.extend(node.subfolders().rev());
        }
        out
    }

    /// The folders `node` is inside, the nearest first.
    pub fn ancestors(&self, node: u32) -> impl Iterator<Item = u32> + '_ {
        std::iter::successors(self.nodes.get(node as usize).map(|n| n.parent), |at| {
            self.nodes.get(*at as usize).map(|n| n.parent)
        })
        .take_while(|at| *at != NO_PARENT)
    }

    /// The files kept by name in one folder, biggest first.
    pub fn named_in(&self, folder: u32) -> &[NamedFile] {
        let from = self.named.partition_point(|f| f.folder < folder);
        let to = self.named.partition_point(|f| f.folder <= folder);
        &self.named[from..to]
    }

    /// What each folder holds directly as the suggestions count it, the
    /// way the survey always counted: every folder Keevoro looks through,
    /// with photos and videos it was looking for, and other files.
    pub fn tallies(&self) -> Vec<crate::survey::Tally> {
        let mut paths: Vec<PathBuf> = Vec::with_capacity(self.nodes.len());
        let mut tallies = Vec::new();
        for (i, node) in self.nodes.iter().enumerate() {
            let path = match node.parent {
                NO_PARENT => self.root.clone(),
                parent => paths
                    .get(parent as usize)
                    .map_or_else(PathBuf::new, |p| p.join(&*node.name)),
            };
            if !node.passed_by() && node.has(flag::LISTED) && !node.has(flag::UNREADABLE) {
                tallies.push(self.tally(i, path.clone()));
            }
            paths.push(path);
        }
        tallies
    }

    fn tally(&self, node: usize, folder: PathBuf) -> crate::survey::Tally {
        let n = &self.nodes[node];
        let none = Uncounted::default();
        let u = n.uncounted.as_deref().unwrap_or(&none);
        let wanted = |kind: Kind| {
            let i = kind.index();
            n.direct[i].less(u.off[i]).less(u.unseen[i])
        };
        let (photos, videos, carried) = (wanted(Kind::Photo), wanted(Kind::Video), wanted(Kind::Carried));
        let rest = Kind::Other.index();
        let others = n.direct[rest].files.saturating_sub(u.unseen[rest].files)
            + Kind::LISTED.iter().map(|k| u.off[k.index()].files).sum::<u64>();
        let count = |n: u64| usize::try_from(n).unwrap_or(usize::MAX);
        crate::survey::Tally {
            folder,
            photos: count(photos.files),
            videos: count(videos.files),
            carried: count(carried.files),
            bytes: photos.bytes + videos.bytes + carried.bytes,
            others: count(others),
            online_only: n.online_only_media as usize,
        }
    }

    /// The folders at or inside `top` that hold photos or videos Keevoro
    /// looked for directly, with what they hold, in path order: the outline
    /// a look inside is drawn from. Folders Keevoro never looks through are
    /// not among them.
    pub fn media_folders(&self, top: u32) -> Vec<crate::survey::Tally> {
        let mut found: Vec<crate::survey::Tally> = self
            .beneath(top)
            .into_iter()
            .filter(|i| {
                let n = &self.nodes[*i as usize];
                !n.passed_by() && n.has(flag::LISTED)
            })
            .map(|i| self.tally(i as usize, self.path_of(i)))
            .filter(|t| t.photos + t.videos + t.carried > 0)
            .collect();
        found.sort_by(|a, b| a.folder.cmp(&b.folder));
        found
    }

    /// The files by ending at or inside `top`, biggest first. When some
    /// folders' endings were not kept, what is given is short of the folder's
    /// sums, and the second value is true.
    pub fn endings_beneath(&self, top: u32) -> (Vec<(u16, KindSum)>, bool) {
        let mut sums: HashMap<u16, KindSum> = HashMap::new();
        let mut short = false;
        for i in self.beneath(top) {
            let node = &self.nodes[i as usize];
            short |= node.has(flag::ENDINGS_DROPPED);
            for e in node.endings.iter() {
                sums.entry(e.ending).or_default().add(KindSum {
                    files: u64::from(e.files),
                    bytes: e.bytes,
                });
            }
        }
        let mut sums: Vec<(u16, KindSum)> = sums.into_iter().collect();
        sums.sort_by(|a, b| b.1.bytes.cmp(&a.1.bytes).then(a.0.cmp(&b.0)));
        (sums, short)
    }

    /// The photos and videos at or inside `top` that a backup would take,
    /// judged by `offered` for each folder and ending: everything beneath
    /// `top` in bytes, and for each folder in it, by its index. Folders
    /// Keevoro never looks through take nothing. `offered` is given the
    /// folder's path, the kind and the ending (empty when the folder's
    /// endings were not kept), and answers whether files there would be
    /// offered, or the choice that leaves them out.
    pub fn offered<J, E>(&self, top: u32, offered: J) -> Offered<E>
    where
        J: Fn(&Path, Kind, &str) -> Result<(), E>,
        E: Copy,
    {
        let order = self.beneath(top);
        let mut by_folder: HashMap<u32, FolderOffer<E>> = HashMap::new();
        let mut paths: HashMap<u32, PathBuf> = HashMap::new();
        // Never looked through: a folder Keevoro passes by for a reason of
        // its own, or one inside it. A folder passed by only because the
        // person left it out is judged like any other, so a folder put back
        // since the survey counts, and one still left out says so.
        let mut never: HashMap<u32, bool> = HashMap::new();
        let own_reason = |n: &FolderNode| n.skipped.is_some_and(|r| r != SkipReason::LeftOut);
        for at in &order {
            let node = &self.nodes[*at as usize];
            let path = match paths.get(&node.parent) {
                Some(parent) if *at != top => parent.join(&*node.name),
                _ => self.path_of(*at),
            };
            let above = match never.get(&node.parent) {
                Some(above) if *at != top => *above,
                _ => self.ancestors(*at).any(|a| own_reason(&self.nodes[a as usize])),
            };
            let passed = above || own_reason(node);
            never.insert(*at, passed);
            let mut offer = FolderOffer::default();
            if !passed {
                let media = Kind::LISTED.iter().any(|k| node.direct[k.index()].files > 0);
                if media && node.has(flag::ENDINGS_DROPPED) {
                    for kind in Kind::LISTED {
                        offer.judge(kind, node.direct[kind.index()].bytes, offered(&path, kind, ""));
                    }
                } else if media {
                    for e in node.endings.iter() {
                        let Some(ending) = self.endings.get(e.ending as usize) else {
                            continue;
                        };
                        if ending.kind == Kind::Other {
                            continue;
                        }
                        offer.judge(ending.kind, e.bytes, offered(&path, ending.kind, &ending.name));
                    }
                }
                // Endings are kept for every file, but a search never takes a
                // system file such as a Mac's `._IMG_1.jpg`, nor anything
                // counted in from a folder Keevoro passes by: never more than
                // what is left once those are taken away.
                if let Some(u) = node.uncounted.as_deref() {
                    for kind in Kind::LISTED {
                        let i = kind.index();
                        let most = node.direct[i].bytes.saturating_sub(u.unseen[i].bytes);
                        offer.bytes[i] = offer.bytes[i].min(most);
                    }
                }
            }
            by_folder.insert(*at, offer);
            if node.children > 0 {
                paths.insert(*at, path);
            }
        }
        // From the bottom up, each folder's own plus everything beneath.
        let mut beneath: HashMap<u32, u64> = HashMap::new();
        for at in order.iter().rev() {
            let own = by_folder.get(at).map_or(0, |o| o.bytes.iter().sum::<u64>());
            let total = own + beneath.get(at).copied().unwrap_or(0);
            beneath.insert(*at, total);
            if *at != top {
                let parent = self.nodes[*at as usize].parent;
                *beneath.entry(parent).or_default() += total;
            }
        }
        Offered {
            bytes: beneath.get(&top).copied().unwrap_or(0),
            by_folder,
            beneath,
        }
    }
}

/// What a backup would take of one folder's own photos, videos and other
/// files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FolderOffer<E> {
    /// Bytes of each kind offered, by [`Kind::index`]; never any of
    /// everything else.
    pub bytes: [u64; KINDS],
    /// Why some of each kind are not: the choice that left out the most.
    pub why_not: [Option<(E, u64)>; KINDS],
}

impl<E> Default for FolderOffer<E> {
    fn default() -> Self {
        FolderOffer {
            bytes: [0; KINDS],
            why_not: [None, None, None, None],
        }
    }
}

impl<E: Copy> FolderOffer<E> {
    fn judge(&mut self, kind: Kind, bytes: u64, said: Result<(), E>) {
        let i = kind.index();
        match said {
            Ok(()) => self.bytes[i] += bytes,
            Err(why) => {
                if self.why_not[i].is_none_or(|(_, most)| bytes > most) {
                    self.why_not[i] = Some((why, bytes));
                }
            }
        }
    }
}

/// What [`Map::offered`] answers.
#[derive(Debug, Clone)]
pub struct Offered<E> {
    /// Bytes beneath the folder asked about that a backup would take.
    pub bytes: u64,
    /// Each folder's own, by its index.
    pub by_folder: HashMap<u32, FolderOffer<E>>,
    /// Each folder's with everything beneath it, by its index.
    pub beneath: HashMap<u32, u64>,
}

/// What a box in a [`View`] stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    /// A folder, by its index.
    Folder(u32),
    /// A file kept by name, by its place in [`Map::named`].
    File(usize),
    /// The rest of a folder's own files of one kind: those too small to be
    /// shown, or not kept, by name.
    Files(Kind),
    /// Subfolders too small to be shown, merged.
    Smaller,
}

/// One box of a [`View`], with its contents when it is opened up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewNode {
    pub item: Item,
    pub kinds: [KindSum; KINDS],
    pub online_only_bytes: u64,
    /// For a folder, the folders beneath it; for merged subfolders, how many.
    pub folders: u64,
    /// The boxes inside it, biggest first, when it was opened up.
    pub children: Vec<ViewNode>,
    /// A folder with more inside it than the depth asked for shows.
    pub deeper: bool,
    /// The endings of the files it holds, everything inside it counted in,
    /// by their place in [`Map::endings`], in order. Folders whose endings
    /// were not kept add none.
    pub endings: Vec<u16>,
}

impl ViewNode {
    pub fn bytes(&self) -> u64 {
        all_of(&self.kinds).bytes
    }

    fn count(&self) -> usize {
        1 + self.children.iter().map(ViewNode::count).sum::<usize>()
    }
}

/// What [`view`] answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub top: ViewNode,
    /// The least a box takes to be shown, as it ended up.
    pub smallest: u64,
    /// How many boxes in all.
    pub boxes: usize,
}

/// The deepest a view may go.
pub const DEEPEST: u8 = 6;

/// The most boxes a view may hold.
pub const MOST_BOXES: usize = 8_000;

/// A folder's contents to `depth` levels, for the screens and the command
/// line: its subfolders and the biggest files by name, and the rest of its
/// files of each kind as one box each. Subfolders and named files smaller
/// than `smallest` bytes are merged (subfolders into one box, files into
/// their kind's), and `smallest` is doubled until the view has no more than
/// `most` boxes. Pure.
pub fn view(map: &Map, totals: &[Totals], folder: u32, depth: u8, smallest: u64, most: usize) -> View {
    let depth = depth.clamp(1, DEEPEST);
    let mut smallest = smallest.max(1);
    loop {
        let top = open(map, totals, folder, depth, smallest);
        let boxes = top.count();
        if boxes <= most.max(1) || smallest >= totals.get(folder as usize).map_or(0, Totals::bytes) {
            let mut top = top;
            fill_endings(map, folder, &mut top);
            return View { top, smallest, boxes };
        }
        smallest = smallest.saturating_mul(2);
    }
}

/// Says on each box of a view which endings it holds, so a screen can
/// light the boxes that hold one kind of file. Every folder beneath `top`
/// is given to the box it is shown in: its own, its parent's merged smaller
/// folders, or the folder at the depth where the view stops.
fn fill_endings(map: &Map, top: u32, view: &mut ViewNode) {
    use std::collections::BTreeSet;
    // The boxes in order, each with its parent, and for a folder whose
    // contents are shown, its merged smaller folders.
    struct Seen {
        item: Item,
        parent: Option<usize>,
        opened: bool,
        smaller: Option<usize>,
    }
    let mut seen: Vec<Seen> = Vec::new();
    fn list(node: &ViewNode, parent: Option<usize>, seen: &mut Vec<Seen>) {
        let me = seen.len();
        seen.push(Seen {
            item: node.item,
            parent,
            opened: !node.children.is_empty(),
            smaller: None,
        });
        for child in &node.children {
            if child.item == Item::Smaller {
                seen[me].smaller = Some(seen.len());
            }
            list(child, Some(me), seen);
        }
    }
    list(view, None, &mut seen);
    let mut folder_box: HashMap<u32, usize> = HashMap::new();
    for (i, s) in seen.iter().enumerate() {
        if let Item::Folder(f) = s.item {
            folder_box.insert(f, i);
        }
    }
    let files_box: HashMap<(usize, Kind), usize> = seen
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match s.item {
            Item::Files(kind) => Some(((s.parent?, kind), i)),
            _ => None,
        })
        .collect();
    let mut sets: Vec<BTreeSet<u16>> = vec![BTreeSet::new(); seen.len()];
    let mut box_of: HashMap<u32, usize> = HashMap::new();
    for at in map.beneath(top) {
        let node = &map.nodes[at as usize];
        let shown_in = match folder_box.get(&at) {
            Some(own) => *own,
            None => {
                let Some(up) = box_of.get(&node.parent).copied() else {
                    continue;
                };
                match seen[up] {
                    Seen {
                        item: Item::Folder(_),
                        opened: true,
                        smaller: Some(smaller),
                        ..
                    } => smaller,
                    _ => up,
                }
            }
        };
        box_of.insert(at, shown_in);
        let own = matches!(seen[shown_in].item, Item::Folder(f) if f == at);
        for e in node.endings.iter() {
            sets[shown_in].insert(e.ending);
            // A folder's own files are in its boxes of files by kind.
            if own {
                let kind = map.endings.get(e.ending as usize).map_or(Kind::Other, |x| x.kind);
                if let Some(files) = files_box.get(&(shown_in, kind)) {
                    sets[*files].insert(e.ending);
                }
            }
        }
    }
    let ids: HashMap<&str, u16> = map
        .endings
        .iter()
        .enumerate()
        .filter_map(|(i, e)| Some((&*e.name, u16::try_from(i).ok()?)))
        .collect();
    for (i, s) in seen.iter().enumerate() {
        if let Item::File(f) = s.item {
            if let Some(id) = map.named.get(f).and_then(|f| ids.get(ending_of(&f.name).as_str())) {
                sets[i].insert(*id);
            }
        }
    }
    // From the bottom up, each box holds what the boxes inside it hold.
    for i in (1..seen.len()).rev() {
        if let Some(parent) = seen[i].parent {
            let inner = std::mem::take(&mut sets[i]);
            sets[parent].extend(inner.iter().copied());
            sets[i] = inner;
        }
    }
    let mut sets = sets.into_iter();
    fn give(node: &mut ViewNode, sets: &mut std::vec::IntoIter<BTreeSet<u16>>) {
        node.endings = sets.next().unwrap_or_default().into_iter().collect();
        for child in &mut node.children {
            give(child, sets);
        }
    }
    give(view, &mut sets);
}

fn open(map: &Map, totals: &[Totals], folder: u32, depth: u8, smallest: u64) -> ViewNode {
    let node = &map.nodes[folder as usize];
    let total = totals.get(folder as usize).copied().unwrap_or_default();
    let mut shown = ViewNode {
        item: Item::Folder(folder),
        kinds: total.kinds,
        online_only_bytes: total.online_only_bytes,
        folders: total.folders,
        children: Vec::new(),
        deeper: false,
        endings: Vec::new(),
    };
    if depth == 0 {
        shown.deeper = node.children > 0 || all_of(&node.direct).files > 0;
        return shown;
    }
    let mut smaller = ViewNode {
        item: Item::Smaller,
        kinds: [KindSum::default(); KINDS],
        online_only_bytes: 0,
        folders: 0,
        children: Vec::new(),
        deeper: false,
        endings: Vec::new(),
    };
    for child in node.subfolders() {
        let t = totals.get(child as usize).copied().unwrap_or_default();
        if t.bytes() >= smallest {
            shown.children.push(open(map, totals, child, depth - 1, smallest));
        } else {
            for k in 0..KINDS {
                smaller.kinds[k].add(t.kinds[k]);
            }
            smaller.online_only_bytes += t.online_only_bytes;
            smaller.folders += 1 + t.folders;
        }
    }
    if smaller.folders > 0 {
        shown.children.push(smaller);
    }
    let mut rest = node.direct;
    let start = map.named.partition_point(|f| f.folder < folder);
    for (i, file) in map.named_in(folder).iter().enumerate() {
        if file.bytes < smallest {
            break;
        }
        let k = file.kind.index();
        rest[k] = rest[k].less(KindSum::one(file.bytes));
        let mut kinds = [KindSum::default(); KINDS];
        kinds[k] = KindSum::one(file.bytes);
        shown.children.push(ViewNode {
            item: Item::File(start + i),
            kinds,
            online_only_bytes: if file.online_only { file.bytes } else { 0 },
            folders: 0,
            children: Vec::new(),
            deeper: false,
            endings: Vec::new(),
        });
    }
    for kind in Kind::ALL {
        let sum = rest[kind.index()];
        if sum.files == 0 {
            continue;
        }
        let mut kinds = [KindSum::default(); KINDS];
        kinds[kind.index()] = sum;
        shown.children.push(ViewNode {
            item: Item::Files(kind),
            kinds,
            // Which of the rest are online is not kept file by file.
            online_only_bytes: 0,
            folders: 0,
            children: Vec::new(),
            deeper: false,
            endings: Vec::new(),
        });
    }
    shown.children.sort_by_key(|c| Reverse(c.bytes()));
    shown
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(name: &str, bytes: u64) -> FileSeen<'_> {
        let ending = ending_of(name);
        let listed = match ending.as_str() {
            "jpg" | "png" => Some(MediaKind::Photo),
            "mp4" | "mov" => Some(MediaKind::Video),
            _ => None,
        };
        FileSeen {
            name,
            bytes,
            listed,
            looked_for: listed.is_some() && ending != "png",
            online_only: false,
            system: name == "Thumbs.db" || name == "desktop.ini",
        }
    }

    fn listing(files: &[(&str, u64)], limits: &Limits) -> Listing {
        let mut listing = Listing::default();
        for (name, bytes) in files {
            listing.file(&seen(name, *bytes), limits);
        }
        listing
    }

    /// Pictures with Camera Roll (photos), Captures (videos), Texture packs
    /// (zip files) and an old library, built as the walk would.
    fn pictures(limits: Limits) -> Map {
        let mut b = Builder::new(Path::new("/Pictures"), limits);
        let top = CountedIn::Own(0);
        b.record(top, listing(&[("desktop.ini", 1), ("loose.jpg", 3_000)], &limits));
        let first = b.subfolders(
            top,
            &[
                ("Camera Roll", None),
                ("Captures", None),
                ("Texture packs", None),
                ("Old library", None),
            ],
        );
        // As the walk does: its own node when it has one, the place's otherwise.
        let each = |n: u32, passed_by: bool| match first {
            Some(first) => CountedIn::Own(first + n),
            None => CountedIn::Above { node: 0, passed_by },
        };
        b.record(
            each(0, false),
            listing(
                &[
                    ("a.jpg", 4_000_000),
                    ("b.jpg", 3_000_000),
                    ("c.png", 500),
                    ("Thumbs.db", 10),
                ],
                &limits,
            ),
        );
        b.record(
            each(1, false),
            listing(&[("one.mp4", 90_000_000), ("two.mp4", 2_000_000)], &limits),
        );
        b.record(
            each(2, false),
            listing(
                &[("rocks.zip", 50_000_000), ("grass.zip", 40_000_000), ("read me.txt", 2)],
                &limits,
            ),
        );
        let library = each(3, true);
        if let CountedIn::Own(node) = library {
            b.passed_by(node, SkipReason::OldLibrary);
        }
        b.record(library, listing(&[], &limits));
        let originals = match b.subfolders(library, &[("originals", None)]) {
            Some(node) => CountedIn::Own(node),
            None => CountedIn::Above {
                node: library.node(),
                passed_by: true,
            },
        };
        b.record(originals, listing(&[("copy.jpg", 7_000_000)], &limits));
        b.finish()
    }

    fn at(map: &Map, path: &str) -> u32 {
        map.find(&Path::new("/Pictures").join(path)).unwrap()
    }

    #[test]
    fn every_file_is_counted_by_kind_and_the_totals_add_up() {
        let map = pictures(Limits::default());
        let totals = map.totals();
        let root = totals[0];
        assert_eq!(root.kinds[0].files, 5, "loose, a, b, c and copy");
        assert_eq!(root.kinds[2].files, 5, "desktop.ini, Thumbs.db, two zips and a note");
        assert_eq!(
            root.bytes(),
            1 + 3_000 + 4_000_000 + 3_000_000 + 500 + 10 + 92_000_000 + 90_000_002 + 7_000_000
        );
        assert_eq!(root.folders, 5);
        let captures = &totals[at(&map, "Captures") as usize];
        assert_eq!((captures.kinds[1].files, captures.kinds[1].bytes), (2, 92_000_000));
        assert_eq!(
            map.path_of(at(&map, "Old library/originals")),
            Path::new("/Pictures/Old library/originals")
        );
        assert_eq!(map.find(Path::new("/Elsewhere")), None);
        assert_eq!(map.find(Path::new("/Pictures/Nowhere")), None);
    }

    #[test]
    fn the_suggestions_count_exactly_as_before_the_map() {
        let map = pictures(Limits::default());
        let tallies = map.tallies();
        let roll = tallies.iter().find(|t| t.folder.ends_with("Camera Roll")).unwrap();
        // The png is switched off: an other file, as the survey always said.
        assert_eq!((roll.photos, roll.videos, roll.others), (2, 0, 1));
        assert_eq!(roll.bytes, 7_000_000);
        let top = tallies.iter().find(|t| t.folder == Path::new("/Pictures")).unwrap();
        assert_eq!((top.photos, top.others), (1, 0), "desktop.ini is not counted");
        assert!(
            tallies.iter().all(|t| !t.folder.starts_with("/Pictures/Old library")),
            "a folder Keevoro never looks through is not suggested"
        );
        let outline: Vec<PathBuf> = map.media_folders(0).into_iter().map(|t| t.folder).collect();
        assert_eq!(
            outline,
            [
                PathBuf::from("/Pictures"),
                PathBuf::from("/Pictures/Camera Roll"),
                PathBuf::from("/Pictures/Captures")
            ]
        );
    }

    #[test]
    fn endings_are_kept_by_folder_and_for_the_whole_place() {
        let map = pictures(Limits::default());
        let (endings, short) = map.endings_beneath(at(&map, "Texture packs"));
        assert!(!short);
        let named: Vec<(&str, u64)> = endings
            .iter()
            .map(|(id, sum)| (&*map.endings[*id as usize].name, sum.files))
            .collect();
        assert_eq!(named, [("zip", 2), ("txt", 1)]);
        let zip = map.endings.iter().position(|e| &*e.name == "zip").unwrap();
        assert_eq!(map.ending_totals[zip].bytes, 90_000_000);
        assert_eq!(ending_of("IMG_1.JPG"), "jpg");
        assert_eq!(ending_of(".bashrc"), "");
        assert_eq!(ending_of("notes.2019-05-01 copy"), "");
        assert_eq!(ending_of("no ending"), "");
    }

    #[test]
    fn totals_stay_exact_when_every_bound_is_passed() {
        let tight = Limits {
            folders: 5,
            named_files: 2,
            named_per_folder: 1,
            named_at_least: 1_000,
            endings: 4,
            ending_pairs: 2,
        };
        let roomy = pictures(Limits::default());
        let tight_map = pictures(tight);
        let (exact, bounded) = (roomy.totals()[0], tight_map.totals()[0]);
        assert_eq!(bounded.kinds, exact.kinds, "not a byte lost");
        assert_eq!(
            bounded.folders, exact.folders,
            "folders past the bound are still counted"
        );
        assert_eq!(tight_map.nodes.len(), 5);
        let library = tight_map.find(Path::new("/Pictures/Old library")).unwrap();
        assert_eq!(
            tight_map.nodes[library as usize].more_folders, 1,
            "and the folder above says so"
        );
        assert_eq!(tight_map.named.len(), 2, "only the biggest named");
        assert_eq!(
            &*tight_map.named.iter().max_by_key(|f| f.bytes).unwrap().name,
            "one.mp4"
        );
        assert!(tight_map.endings.len() <= 4);
        assert!(tight_map.endings_dropped);
        let all_endings: u64 = tight_map.ending_totals.iter().map(|s| s.bytes).sum();
        assert_eq!(
            all_endings,
            exact.bytes(),
            "endings past the bound are other endings, not lost"
        );
    }

    #[test]
    fn a_folder_counted_into_another_past_the_bound_keeps_what_was_passed_by_out_of_the_suggestions() {
        let limits = Limits {
            folders: 1,
            ..Limits::default()
        };
        let mut b = Builder::new(Path::new("/d"), limits);
        b.record(CountedIn::Own(0), listing(&[("a.jpg", 10)], &limits));
        assert_eq!(
            b.subfolders(
                CountedIn::Own(0),
                &[("Holiday", None), (".hidden", Some(SkipReason::HiddenFolder))]
            ),
            None
        );
        b.record(
            CountedIn::Above {
                node: 0,
                passed_by: false,
            },
            listing(&[("b.jpg", 10)], &limits),
        );
        b.record(
            CountedIn::Above {
                node: 0,
                passed_by: true,
            },
            listing(&[("c.jpg", 10)], &limits),
        );
        let map = b.finish();
        assert_eq!(map.totals()[0].kinds[0].files, 3);
        assert_eq!(map.tallies()[0].photos, 2);
        assert_eq!(map.nodes[0].more_folders, 2);
    }

    #[test]
    fn files_sized_later_add_their_bytes_and_nothing_is_counted_twice() {
        let limits = Limits::default();
        let mut b = Builder::new(Path::new("/d"), limits);
        let mut first = Listing::default();
        first.file(&seen("a.jpg", 5_000_000), &limits);
        first.file_unsized(&seen("big.zip", 0));
        first.file_unsized(&seen("c.png", 0));
        b.record(CountedIn::Own(0), first);
        assert!(b.map.totals()[0].not_sized, "not sized until it is");
        let before = b.map.tallies();
        assert_eq!(b.next_unsized(0).map(|(node, _)| node), Some(0));
        let mut sizes = Listing::default();
        sizes.file(&seen("big.zip", 3_000_000), &limits);
        sizes.file(&seen("c.png", 7), &limits);
        b.sized(0, sizes);
        assert_eq!(b.next_unsized(0), None);
        let map = b.finish();
        let totals = map.totals()[0];
        assert!(!totals.not_sized);
        assert_eq!(
            totals.kinds[2],
            KindSum {
                files: 1,
                bytes: 3_000_000
            }
        );
        assert_eq!(
            totals.kinds[0],
            KindSum {
                files: 2,
                bytes: 5_000_007
            }
        );
        assert_eq!(map.tallies(), before, "the suggestions are unchanged by sizing");
        assert_eq!(map.named.len(), 2, "big files sized later are named too");
        let zip = map.endings.iter().position(|e| &*e.name == "zip").unwrap();
        assert_eq!(
            map.ending_totals[zip],
            KindSum {
                files: 1,
                bytes: 3_000_000
            }
        );
    }

    #[test]
    fn a_view_opens_to_the_depth_asked_and_merges_what_is_too_small_to_see() {
        let map = pictures(Limits::default());
        let totals = map.totals();
        let shown = view(&map, &totals, 0, 1, 10_000_000, MOST_BOXES);
        let items: Vec<Item> = shown.top.children.iter().map(|c| c.item).collect();
        let roll = at(&map, "Camera Roll");
        let captures = at(&map, "Captures");
        let packs = at(&map, "Texture packs");
        let old = at(&map, "Old library");
        assert_eq!(
            items[..2],
            [Item::Folder(captures), Item::Folder(packs)],
            "biggest first: {items:?}"
        );
        assert!(
            items.contains(&Item::Smaller),
            "Camera Roll and the library are under 10 MB"
        );
        let smaller = shown.top.children.iter().find(|c| c.item == Item::Smaller).unwrap();
        assert_eq!(smaller.folders, 3, "both, and the folder inside the library");
        assert!(!items.contains(&Item::Folder(roll)) && !items.contains(&Item::Folder(old)));
        assert!(
            shown.top.children.iter().all(|c| c.children.is_empty()),
            "one level only"
        );
        assert!(shown.top.children[0].deeper);
        // Every box adds up to the folder.
        let sum: u64 = shown.top.children.iter().map(ViewNode::bytes).sum();
        assert_eq!(sum, shown.top.bytes());

        let deeper = view(&map, &totals, packs, 2, 1, MOST_BOXES);
        let names: Vec<String> = deeper
            .top
            .children
            .iter()
            .map(|c| match c.item {
                Item::File(i) => map.named[i].name.to_string(),
                other => format!("{other:?}"),
            })
            .collect();
        assert_eq!(names, ["rocks.zip", "grass.zip", "Files(Other)"]);
    }

    #[test]
    fn the_top_of_the_map_is_kept_as_it_grows_and_the_smallest_are_put_together() {
        let limits = Limits::default();
        let mut b = Builder::new(Path::new("/Pictures"), limits);
        assert!(b.top(MOST_TOP_BOXES).is_empty(), "nothing yet");
        b.record(CountedIn::Own(0), listing(&[("loose.jpg", 3_000)], &limits));
        let first = b
            .subfolders(
                CountedIn::Own(0),
                &[
                    ("Camera Roll", None),
                    ("Captures", None),
                    ("Empty", None),
                    ("Notes", None),
                ],
            )
            .unwrap();
        b.record(CountedIn::Own(first), listing(&[("a.jpg", 4_000_000)], &limits));
        let inside = b.subfolders(CountedIn::Own(first + 1), &[("2019", None)]).unwrap();
        b.record(CountedIn::Own(first + 1), Listing::default());
        b.record(CountedIn::Own(inside), listing(&[("one.mp4", 9_000_000)], &limits));
        let mut notes = Listing::default();
        notes.file_unsized(&seen("list.txt", 0));
        b.record(CountedIn::Own(first + 3), notes);
        let top = b.top(MOST_TOP_BOXES);
        let said: Vec<(&str, TopItem, u64)> = top.iter().map(|t| (&*t.name, t.item, t.bytes())).collect();
        assert_eq!(
            said,
            [
                ("Captures", TopItem::Folder, 9_000_000),
                ("Camera Roll", TopItem::Folder, 4_000_000),
                ("Pictures", TopItem::FilesHere, 3_000),
                ("Notes", TopItem::Folder, 0),
            ],
            "everything inside a folder counted in, and nothing for a folder with nothing found yet"
        );
        // Sized later, its bytes come in.
        let mut sizes = Listing::default();
        sizes.file(&seen("list.txt", 70), &limits);
        b.sized(first + 3, sizes);
        assert_eq!(b.top(MOST_TOP_BOXES)[3].bytes(), 70);
        let few = b.top(2);
        assert_eq!(few.len(), 2);
        assert_eq!(few[1].item, TopItem::Smaller(2), "Camera Roll and Notes");
        assert_eq!(
            few.iter().map(TopBox::bytes).sum::<u64>(),
            b.finish().totals()[0].bytes(),
            "nothing lost by putting them together"
        );
    }

    #[test]
    fn each_box_of_a_view_says_which_endings_it_holds() {
        let map = pictures(Limits::default());
        let totals = map.totals();
        let shown = view(&map, &totals, 0, 2, 10_000_000, MOST_BOXES);
        let named = |node: &ViewNode| -> Vec<&str> {
            let mut names: Vec<&str> = node.endings.iter().map(|e| &*map.endings[*e as usize].name).collect();
            names.sort_unstable();
            names
        };
        assert_eq!(
            named(&shown.top),
            ["db", "ini", "jpg", "mp4", "png", "txt", "zip"],
            "everything"
        );
        let find = |item: Item| shown.top.children.iter().find(|c| c.item == item).unwrap();
        let packs = find(Item::Folder(at(&map, "Texture packs")));
        assert_eq!(named(packs), ["txt", "zip"]);
        let rocks = packs
            .children
            .iter()
            .find(|c| matches!(c.item, Item::File(i) if &*map.named[i].name == "rocks.zip"))
            .unwrap();
        assert_eq!(named(rocks), ["zip"]);
        assert_eq!(named(find(Item::Folder(at(&map, "Captures")))), ["mp4"]);
        // Camera Roll, the library and what is inside it are merged, and
        // so are their endings.
        assert_eq!(named(find(Item::Smaller)), ["db", "jpg", "png"]);
    }

    #[test]
    fn a_view_never_holds_more_boxes_than_allowed() {
        let limits = Limits::default();
        let mut b = Builder::new(Path::new("/d"), limits);
        b.record(CountedIn::Own(0), Listing::default());
        let names: Vec<String> = (0..500).map(|n| format!("folder {n}")).collect();
        let found: Vec<(&str, Option<SkipReason>)> = names.iter().map(|n| (n.as_str(), None)).collect();
        let first = b.subfolders(CountedIn::Own(0), &found).unwrap();
        for n in 0..500u32 {
            b.record(
                CountedIn::Own(first + n),
                listing(&[("a.zip", 1_000 + u64::from(n))], &limits),
            );
        }
        let map = b.finish();
        let totals = map.totals();
        let shown = view(&map, &totals, 0, 3, 1, 50);
        assert!(shown.boxes <= 50, "{}", shown.boxes);
        assert!(shown.smallest > 1);
        let sum: u64 = shown.top.children.iter().map(ViewNode::bytes).sum();
        assert_eq!(sum, totals[0].bytes(), "what is merged still adds up");
    }

    #[test]
    fn what_a_backup_would_take_follows_the_judge_and_skips_what_keevoro_passes_by() {
        let map = pictures(Limits::default());
        // A backup for photos only that leaves out Camera Roll's pngs.
        let offered = map.offered(0, |path: &Path, kind, ending: &str| {
            if kind == Kind::Video {
                Err("videos")
            } else if ending == "png" {
                Err("ending")
            } else {
                let _ = path;
                Ok(())
            }
        });
        assert_eq!(
            offered.bytes,
            3_000 + 7_000_000,
            "not the library's copy, nor any video"
        );
        let roll = at(&map, "Camera Roll");
        let own = offered.by_folder[&roll];
        assert_eq!(own.bytes, [7_000_000, 0, 0, 0]);
        assert_eq!(own.why_not[0], Some(("ending", 500)));
        let captures = offered.by_folder[&at(&map, "Captures")];
        assert_eq!(captures.why_not[1], Some(("videos", 92_000_000)));
        assert_eq!(offered.beneath[&roll], 7_000_000);
        // Nothing inside the old library, however deep.
        let originals = at(&map, "Old library/originals");
        assert_eq!(offered.beneath[&originals], 0);
        assert_eq!(map.offered(originals, |_: &Path, _, _: &str| Ok::<(), ()>(())).bytes, 0);
    }

    #[test]
    fn a_backup_is_never_said_to_take_what_no_search_would() {
        let limits = Limits::default();
        let mut b = Builder::new(Path::new("/d"), limits);
        let mut here = listing(&[("a.jpg", 10)], &limits);
        // A Mac's companion file: a photo by its ending, never taken.
        here.file(
            &FileSeen {
                system: true,
                ..seen("._a.jpg", 4_096)
            },
            &limits,
        );
        b.record(CountedIn::Own(0), here);
        // Past the folder bound, a folder Keevoro passes by counted in.
        b.record(
            CountedIn::Above {
                node: 0,
                passed_by: true,
            },
            listing(&[("copy.jpg", 7_000)], &limits),
        );
        let map = b.finish();
        assert_eq!(map.totals()[0].kinds[0].bytes, 10 + 4_096 + 7_000, "all of it is shown");
        let offered = map.offered(0, |_: &Path, _, _: &str| Ok::<(), ()>(()));
        assert_eq!(offered.bytes, 10, "but only the photo a search would take is offered");
    }

    #[test]
    fn a_folder_passed_by_only_because_it_was_left_out_is_judged_as_it_is_now() {
        let limits = Limits::default();
        let mut b = Builder::new(Path::new("/d"), limits);
        b.record(CountedIn::Own(0), Listing::default());
        let first = b.subfolders(CountedIn::Own(0), &[("Games", None)]).unwrap();
        b.passed_by(first, SkipReason::LeftOut);
        b.record(CountedIn::Own(first), listing(&[("a.jpg", 10)], &limits));
        let map = b.finish();
        let put_back = map.offered(0, |_: &Path, _, _: &str| Ok::<(), &str>(()));
        assert_eq!(put_back.bytes, 10, "put back since the survey, it counts");
        let still_out = map.offered(0, |_: &Path, _, _: &str| Err("left out"));
        assert_eq!(still_out.by_folder[&first].why_not[0], Some(("left out", 10)));
    }

    /// How much a map of a million files in a hundred thousand folders
    /// holds, by counting what it allocated.
    #[test]
    fn a_map_of_a_million_files_stays_within_its_bounds() {
        let limits = Limits::default();
        let mut b = Builder::new(Path::new("/d"), limits);
        b.record(CountedIn::Own(0), Listing::default());
        let mut next = vec![0u32];
        let mut folders = 1usize;
        let names: Vec<String> = (0..10).map(|n| format!("Folder number {n}")).collect();
        let found: Vec<(&str, Option<SkipReason>)> = names.iter().map(|n| (n.as_str(), None)).collect();
        while folders < 100_000 {
            let mut deeper = Vec::new();
            for parent in next {
                // Past the folder bound, counted into the folder above, as
                // the walk does.
                let first = b.subfolders(CountedIn::Own(parent), &found);
                for c in 0..10 {
                    let mut files = Listing::default();
                    for n in 0..10u64 {
                        let name = format!("IMG_{n}.{}", ["jpg", "mp4", "zip"][(n % 3) as usize]);
                        files.file(&seen(&name, 2_000_000 + n), &limits);
                    }
                    match first {
                        Some(first) => {
                            b.record(CountedIn::Own(first + c), files);
                            deeper.push(first + c);
                        }
                        None => b.record(
                            CountedIn::Above {
                                node: parent,
                                passed_by: false,
                            },
                            files,
                        ),
                    }
                }
                folders += 10;
                if folders >= 100_000 {
                    break;
                }
            }
            next = deeper;
        }
        let map = b.finish();
        assert_eq!(map.totals()[0].kinds.iter().map(|k| k.files).sum::<u64>(), 1_000_000);
        let bytes = held(&map);
        assert!(bytes < 40 * 1024 * 1024, "{bytes} bytes");
        eprintln!(
            "A map of {} folders and a million files holds about {:.1} MB",
            map.nodes.len(),
            bytes as f64 / 1024.0 / 1024.0
        );
    }

    /// What a map at the folder bound really takes, allocator and all, from
    /// what the process holds before and after building one. Linux only, as
    /// it reads `/proc`; run by hand:
    /// `cargo test -p keevoro-scan how_much -- --ignored --nocapture`.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "a measurement, run by hand"]
    fn how_much_a_map_at_the_folder_bound_holds() {
        fn resident() -> usize {
            let statm = std::fs::read_to_string("/proc/self/statm").unwrap();
            let pages: usize = statm.split_whitespace().nth(1).unwrap().parse().unwrap();
            pages * 4096
        }
        let limits = Limits::default();
        let before = resident();
        let mut b = Builder::new(Path::new("/d"), limits);
        b.record(CountedIn::Own(0), Listing::default());
        // Folders ten wide, with names of a usual length, each holding a few
        // files of two or three endings, until the bound stops it.
        let names: Vec<String> = (0..10).map(|n| format!("Holiday folder {n}")).collect();
        let found: Vec<(&str, Option<SkipReason>)> = names.iter().map(|n| (n.as_str(), None)).collect();
        let mut next = vec![0u32];
        while !next.is_empty() {
            let mut deeper = Vec::new();
            for parent in next {
                let Some(first) = b.subfolders(CountedIn::Own(parent), &found) else {
                    continue;
                };
                for c in 0..10 {
                    let mut files = Listing::default();
                    // Five endings, and one file big enough to be named.
                    for n in 0..6u64 {
                        let name = format!("IMG_{n}.{}", ["jpg", "mp4", "txt", "png", "zip", "jpg"][n as usize]);
                        files.file(&seen(&name, if n == 1 { 2_000_000 } else { 300_000 + n }), &limits);
                    }
                    b.record(CountedIn::Own(first + c), files);
                    deeper.push(first + c);
                }
            }
            next = deeper;
        }
        let map = b.finish();
        let after = resident();
        eprintln!(
            "{} folders: {:.1} MB held by the process, {:.1} MB counted by the map, {} bytes a folder node",
            map.nodes.len(),
            (after.saturating_sub(before)) as f64 / 1024.0 / 1024.0,
            held(&map) as f64 / 1024.0 / 1024.0,
            std::mem::size_of::<FolderNode>()
        );
        assert_eq!(map.nodes.len(), limits.folders.min(map.nodes.len()));
    }

    /// Roughly what a map holds in memory: its parts and what they point to.
    fn held(map: &Map) -> usize {
        use std::mem::size_of;
        let nodes: usize = map
            .nodes
            .iter()
            .map(|n| {
                size_of::<FolderNode>()
                    + n.name.len()
                    + n.endings.len() * size_of::<EndingSum>()
                    + n.uncounted.as_ref().map_or(0, |_| size_of::<Uncounted>())
            })
            .sum();
        let named: usize = map.named.iter().map(|f| size_of::<NamedFile>() + f.name.len()).sum();
        nodes + named + map.endings.len() * (size_of::<Ending>() + 8 + size_of::<KindSum>())
    }
}
