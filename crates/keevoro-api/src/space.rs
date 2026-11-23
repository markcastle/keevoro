//! `places.map`: where the space goes in a surveyed place, in the style of
//! WinDirStat. Everything the survey saw is in it, photos or not, because
//! seeing what takes the room (zip files, screen recordings, an old library)
//! is what makes it useful. What is not a photo or video is shown as such,
//! so the screens can mute it.
//!
//! It is answered from the remembered survey (`keevoro_engine::surveys`),
//! so nothing is walked or opened, and only ever about the folder asked
//! and what is inside it: an agent allowed one folder learns nothing of
//! the rest of a remembered drive.
//!
//! On each folder and file it says what Keevoro would make of it, from the
//! choices of the backup in hand through the one rule that decides what is
//! offered (`keevoro_core::Choices::offered`), in its words: a folder
//! Keevoro made (a backup, an old library), one the person left out, a
//! kind of file switched off, and the backup's or a source's photos or
//! videos. A folder Keevoro never looks through (a system or app folder,
//! a hidden one) says so too.

use crate::backups::Named;
use crate::error::{Error, Result};
use crate::inside::existing_folder;
use crate::keevoro::Keevoro;
use keevoro_core::{FileFacts, LeftOutBy, SkipReason};
use keevoro_engine::friendly_path;
use keevoro_scan::map::{self, Item, Kind, Map, ViewNode};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// `places.map`
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MapRequest {
    /// The place to map, written whole, such as `C:\Users\You\Pictures`:
    /// where the trail back starts. It, or a folder it is in, must have
    /// been surveyed.
    pub place: String,
    /// A folder inside the place to show, written whole. The place itself
    /// when left out.
    #[serde(default)]
    pub folder: Option<String>,
    /// How many levels of folders to open, 1 to 6. 3 when left out.
    #[serde(default = "three")]
    pub depth: u8,
    /// The least room, in bytes, a folder or file must take to be shown on
    /// its own; smaller ones are merged. Chosen to fit when left out.
    #[serde(default)]
    pub smallest: Option<u64>,
}

fn three() -> u8 {
    3
}

impl MapRequest {
    /// The map of a place, to the usual depth.
    pub fn of(place: &Path) -> Self {
        MapRequest {
            place: place.display().to_string(),
            folder: None,
            depth: three(),
            smallest: None,
        }
    }
}

/// What a box on the map is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NodeKind {
    Folder,
    /// One file, among the biggest.
    File,
    /// The rest of a folder's own files of one kind, together.
    Files,
    /// Folders too small to be shown on their own, together.
    Smaller,
}

/// Photo, video or anything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum KindName {
    Photo,
    Video,
    /// Everything Keevoro does not look for.
    Other,
    /// Other files a person put on the list, to be carried along.
    Carried,
    Music,
}

impl From<Kind> for KindName {
    fn from(kind: Kind) -> Self {
        match kind {
            Kind::Photo => KindName::Photo,
            Kind::Video => KindName::Video,
            Kind::Other => KindName::Other,
            Kind::Carried => KindName::Carried,
            Kind::Music => KindName::Music,
        }
    }
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

fn not(b: &bool) -> bool {
    !b
}

/// One box on the map: a folder, a file, or several together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapNode {
    /// A folder's or file's name. For several together, what they are:
    /// "Photos", "Videos", "Other files" or "Smaller folders".
    pub name: String,
    /// A folder, written whole: what `folder` takes to go into it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub node: NodeKind,
    /// For a file, or files together, which kind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<KindName>,
    /// The room it takes, everything inside it counted in.
    pub bytes: u64,
    pub photos: u64,
    pub videos: u64,
    pub others: u64,
    pub photo_bytes: u64,
    pub video_bytes: u64,
    pub other_bytes: u64,
    /// Other files a person put on the list, to be carried along. Never
    /// said when there are none.
    #[serde(skip_serializing_if = "is_zero")]
    pub carried: u64,
    #[serde(skip_serializing_if = "is_zero")]
    pub carried_bytes: u64,
    /// Music, for a backup that is for it. Never said when there is none.
    #[serde(skip_serializing_if = "is_zero")]
    pub music: u64,
    #[serde(skip_serializing_if = "is_zero")]
    pub music_bytes: u64,
    /// Of `bytes`, how many are only in online storage, not on this
    /// computer. Never opened.
    #[serde(skip_serializing_if = "is_zero")]
    pub online_only_bytes: u64,
    /// Folders inside it; for smaller folders together, how many.
    #[serde(skip_serializing_if = "is_zero")]
    pub folders: u64,
    /// Why Keevoro never looks through it, when it does not: "System
    /// folder", "Hidden folder", "A Keevoro backup" and so on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped: Option<String>,
    /// Keevoro made it: a backup, an old library, its own files.
    #[serde(skip_serializing_if = "not")]
    pub made_by_keevoro: bool,
    /// Why its photos and videos would not be backed up, when none of them
    /// would: the choice that leaves them out, in words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left_out: Option<String>,
    /// The folder the person left out of the backup in hand that this
    /// folder is, or is in, as a person would write it: what Put back takes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left_out_folder: Option<String>,
    /// Of its photos and videos, the bytes the backup in hand would take
    /// (all of them with no backup in hand).
    pub would_back_up: u64,
    /// It is a source of the backup in hand.
    #[serde(skip_serializing_if = "not")]
    pub source: bool,
    /// It is inside a source of the backup in hand.
    #[serde(skip_serializing_if = "not")]
    pub in_source: bool,
    /// Of the files the backup in hand keeps from it, everything inside it
    /// counted in, how many are safely at every place it goes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backed_up: Option<BackedUp>,
    /// Its listing could not be read.
    #[serde(skip_serializing_if = "not")]
    pub unreadable: bool,
    /// Something in it was not measured: the survey was stopped first.
    #[serde(skip_serializing_if = "not")]
    pub not_sized: bool,
    /// Folders inside it counted into it, past what a map keeps one by one.
    #[serde(skip_serializing_if = "is_zero")]
    pub more_folders: u64,
    /// There is more inside than the depth asked for shows.
    #[serde(skip_serializing_if = "not")]
    pub deeper: bool,
    /// The endings of the files it holds, everything inside it counted in,
    /// as places in the answer's `kinds`: what lights it when a screen
    /// picks out one kind of file.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub endings: Vec<usize>,
    /// What is inside it, the biggest first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<MapNode>,
}

/// How much of what a backup keeps from a folder is safely backed up:
/// "Backed up: 1,180 of 1,204".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackedUp {
    /// Safely at every place the backup goes.
    pub files: usize,
    /// Kept by the backup.
    pub of: usize,
}

/// One step of the trail from the place to the folder shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrailStep {
    pub name: String,
    pub folder: String,
}

/// One ending at or inside the folder shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KindView {
    /// Lower case, without the dot. Empty for files with no ending, and
    /// "…" for the rarer endings a map does not keep one by one.
    pub ending: String,
    pub kind: KindName,
    pub files: u64,
    pub bytes: u64,
    /// For a photo or video ending, why files with it in this folder would
    /// not be backed up, when they would not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left_out: Option<String>,
}

/// What `places.map` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapView {
    /// The place asked about.
    pub place: String,
    /// The folder shown.
    pub folder: String,
    /// From the place to the folder shown.
    pub trail: Vec<TrailStep>,
    /// When the survey this comes from was made, in seconds since 1970.
    pub surveyed_at: i64,
    /// False when that survey was stopped before it had seen every folder.
    pub complete: bool,
    /// False when it was stopped before everything was measured.
    pub sized_all: bool,
    /// The folder shown, and inside it to the depth asked.
    pub top: MapNode,
    /// Its files by ending, the biggest first.
    pub kinds: Vec<KindView>,
    /// Some folders' endings were not kept, so `kinds` is short of `top`.
    #[serde(skip_serializing_if = "not")]
    pub kinds_short: bool,
    /// The least room a box took to be shown on its own, as it turned out.
    pub smallest: u64,
    /// The backup whose choices are said, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup: Option<String>,
    /// The map is out of date (over an hour old, or a source in it has been
    /// looked through since), and how to map it again: by itself on a disk
    /// of this computer, or after asking across a network. Absent when it
    /// is fresh.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh: Option<Refresh>,
    /// A source in it has been looked through since it was mapped.
    #[serde(skip_serializing_if = "not")]
    pub searched_since: bool,
}

/// How an out of date map is mapped again when it is opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Refresh {
    /// On a disk of this computer: surveyed again by itself.
    ByItself,
    /// Across a network, where it may take a while: asked first.
    AskFirst,
}

/// When `smallest` is not given, what is too small to see on a map of
/// this many boxes is merged.
const BOXES_WHEN_NOT_SAID: u64 = 2_000;

impl Keevoro {
    /// `places.map`: where the space goes in a surveyed place.
    pub fn space_map(&self, ask: &MapRequest, backup: Option<&Named>) -> Result<MapView> {
        self.space_map_if_surveyed(ask, backup)?.ok_or_else(|| {
            Error::not_found(format!(
                "Keevoro hasn't mapped {} yet. Find where the photos are in it first (keevoro survey), then see \
                 where the space goes.",
                friendly_path(Path::new(&ask.place))
            ))
        })
    }

    /// As [`Keevoro::space_map`], but `None` when no remembered survey
    /// covers the place, so the app can survey it first.
    pub fn space_map_if_surveyed(&self, ask: &MapRequest, backup: Option<&Named>) -> Result<Option<MapView>> {
        let place = existing_folder(Path::new(&ask.place))?;
        let folder = match &ask.folder {
            Some(folder) => existing_folder(Path::new(folder))?,
            None => place.clone(),
        };
        if !keevoro_core::places::within(&folder, &place) {
            return Err(Error::not_understood(format!(
                "{} is not inside {}.",
                friendly_path(&folder),
                friendly_path(&place)
            )));
        }
        let Some(loaded) = keevoro_engine::surveys::covering(&self.cache_folder(), &place) else {
            return Ok(None);
        };
        let map = &loaded.survey.map;
        let (Some(top), Some(start)) = (map.find(&folder), map.find(&place)) else {
            return Ok(None);
        };
        let choices = Self::choices_for(backup);
        let kinds = self.kinds_for(backup);
        let judge = Judge {
            choices: &choices,
            kinds: &kinds,
        };
        let total = loaded.totals.get(top as usize).map_or(0, map::Totals::bytes);
        let smallest = ask.smallest.unwrap_or(total / BOXES_WHEN_NOT_SAID);
        let shown = map::view(map, &loaded.totals, top, ask.depth, smallest, map::MOST_BOXES);
        let offered = map.offered(top, |path, kind, ending| judge.offered(path, kind, ending));
        let (endings, kinds_short) = map.endings_beneath(top);
        let endings: Vec<(u16, map::KindSum)> = endings
            .into_iter()
            .filter(|(id, _)| map.endings.get(*id as usize).is_some())
            .collect();
        let in_kinds: std::collections::HashMap<u16, usize> =
            endings.iter().enumerate().map(|(i, (id, _))| (*id, i)).collect();
        let backed_up = backed_up_beneath(map, top, backup)?;
        let said = Said {
            map,
            totals: &loaded.totals,
            judge: &judge,
            offered: &offered,
            in_kinds: &in_kinds,
            backed_up: &backed_up,
        };
        let above = said.passed_by_above(top);
        let folder_path = map.path_of(top);
        let kinds_seen = endings
            .into_iter()
            .filter_map(|(id, sum)| {
                let ending = map.endings.get(id as usize)?;
                let left_out = ending
                    .kind
                    .media()
                    .and(above.is_none().then_some(()))
                    .and_then(|()| judge.offered(&folder_path, ending.kind, &ending.name).err())
                    .map(|by| by.why(kinds.lists_other()));
                Some(KindView {
                    ending: ending.name.to_string(),
                    kind: ending.kind.into(),
                    files: sum.files,
                    bytes: sum.bytes,
                    left_out,
                })
            })
            .collect();
        let mut trail: Vec<TrailStep> = Vec::new();
        let mut at = top;
        loop {
            trail.push(TrailStep {
                name: map.nodes[at as usize].name.to_string(),
                folder: friendly_path(&map.path_of(at)),
            });
            if at == start || map.nodes[at as usize].parent == map::NO_PARENT {
                break;
            }
            at = map.nodes[at as usize].parent;
        }
        trail.reverse();
        let searched_since = keevoro_engine::surveys::searched_since(&self.cache_folder(), &map.root);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
        let refresh = match keevoro_engine::surveys::freshness(
            loaded.survey.surveyed_at,
            now,
            searched_since,
            keevoro_engine::reach::is_across_a_network(&place),
        ) {
            keevoro_engine::surveys::Freshness::Fresh => None,
            keevoro_engine::surveys::Freshness::Again => Some(Refresh::ByItself),
            keevoro_engine::surveys::Freshness::AskFirst => Some(Refresh::AskFirst),
        };
        Ok(Some(MapView {
            place: friendly_path(&place),
            folder: friendly_path(&folder),
            trail,
            surveyed_at: loaded.survey.surveyed_at,
            complete: loaded.survey.complete,
            sized_all: loaded.survey.sized_all,
            top: said.node(&shown.top, None, above),
            kinds: kinds_seen,
            kinds_short,
            smallest: shown.smallest,
            backup: backup.map(|b| b.name.clone()),
            refresh,
            searched_since,
        }))
    }
}

impl Keevoro {
    /// The folder a screen may show in the computer's file manager ("Show
    /// in File Explorer"), in its full form: one inside a remembered survey
    /// (so any folder on a map), or one the backup in hand knows (its own
    /// folder, a source or a folder inside one, a folder it is sent to).
    /// Anything else is refused, so a screen can never be made to open any
    /// place it likes. It only decides; the window does the showing.
    pub fn folder_to_show(&self, folder: &Path, backup: Option<&Named>) -> Result<std::path::PathBuf> {
        let real = existing_folder(folder)?;
        let mapped = keevoro_engine::surveys::covering(&self.cache_folder(), &real)
            .is_some_and(|loaded| loaded.survey.map.find(&real).is_some());
        if mapped {
            return Ok(real);
        }
        if let Some(backup) = backup.and_then(|b| crate::backup::Backup::open_existing(&b.home)) {
            let full = |p: &Path| std::fs::canonicalize(p).ok();
            let catalog = backup.catalog();
            let in_a_source = catalog
                .sources()
                .map_err(Error::from)?
                .iter()
                .any(|s| full(&s.root).is_some_and(|root| keevoro_core::places::within(&real, &root)));
            let sent_to = keevoro_engine::connect::destinations(catalog)
                .map_err(Error::from)?
                .iter()
                .filter(|d| d.kind == "folder")
                .filter_map(|d| full(Path::new(&d.place)))
                .any(|place| keevoro_core::places::same_place(&real, &place));
            let home = full(backup.home()).is_some_and(|home| keevoro_core::places::same_place(&real, &home));
            if in_a_source || sent_to || home {
                return Ok(real);
            }
        }
        Err(Error::refused("Keevoro doesn't know that folder."))
    }
}

/// What `places.forget_maps` answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Forgotten {
    /// How many places' surveys were forgotten.
    pub forgotten: usize,
}

impl Keevoro {
    /// `places.forget_maps`: forgets every remembered survey, since a map
    /// lists folder names across a whole drive. Only Keevoro's own cache is
    /// touched; a place is simply surveyed again when next asked about.
    pub fn forget_maps(&self) -> Forgotten {
        let forgotten = keevoro_engine::surveys::forget_all(&self.cache_folder());
        self.log().say(&format!(
            "Forgot {} remembered {}",
            forgotten,
            if forgotten == 1 { "map" } else { "maps" }
        ));
        Forgotten { forgotten }
    }
}

/// Asks the one rule whether files of a kind and ending in a folder would
/// be offered, with the kinds Keevoro looks for now.
struct Judge<'a> {
    choices: &'a keevoro_core::Choices,
    kinds: &'a keevoro_core::Kinds,
}

impl Judge<'_> {
    fn offered(&self, folder: &Path, kind: Kind, ending: &str) -> std::result::Result<(), LeftOutBy> {
        let Some(media) = kind.media() else {
            return Ok(());
        };
        // A file with this ending in this folder; its name does not matter.
        let (file, looked_for) = if ending.is_empty() || ending == map::OTHER_ENDINGS {
            (folder.join("file"), true)
        } else {
            let on = self.kinds.listed(ending).is_some_and(|(_, on)| on);
            (folder.join(format!("file.{ending}")), on)
        };
        self.choices.offered(&FileFacts {
            path: &file,
            kind: media,
            looked_for,
            made_by_keevoro: false,
        })
    }

    fn file(&self, path: &Path, kind: Kind) -> std::result::Result<(), LeftOutBy> {
        let Some(media) = kind.media() else {
            return Ok(());
        };
        let looked_for = path
            .extension()
            .and_then(|e| self.kinds.listed(&e.to_string_lossy()))
            .is_some_and(|(_, on)| on);
        self.choices.offered(&FileFacts {
            path,
            kind: media,
            looked_for,
            made_by_keevoro: false,
        })
    }
}

/// Why a folder is never looked through, as it is said on the map: the
/// reason, and whether Keevoro made it. A folder left out by the person is
/// not one of these: what they chose now is said instead, from the choices.
#[derive(Debug, Clone, Copy)]
struct PassedBy(SkipReason);

/// Turns a view into what is said about each box.
struct Said<'a> {
    map: &'a Map,
    totals: &'a [map::Totals],
    judge: &'a Judge<'a>,
    offered: &'a map::Offered<LeftOutBy>,
    /// Each ending's place in the answer's `kinds`.
    in_kinds: &'a std::collections::HashMap<u16, usize>,
    /// What the backup keeps and has safely backed up, by folder, everything
    /// inside each counted in.
    backed_up: &'a std::collections::HashMap<u32, BackedUp>,
}

/// What the backup in hand keeps from each folder at or inside `top`, and
/// how much of it is safely backed up, everything inside each folder
/// counted in. Worked out now from the backup's own record, by the rule for
/// what is waiting at each destination (`keevoro_engine::safe_by_folder`),
/// never kept in the survey. A file in a folder the map does not hold on its
/// own (past its bounds, or made since) counts in the nearest folder it does.
fn backed_up_beneath(map: &Map, top: u32, backup: Option<&Named>) -> Result<std::collections::HashMap<u32, BackedUp>> {
    let mut rolled: std::collections::HashMap<u32, BackedUp> = std::collections::HashMap::new();
    let Some(backup) = backup.and_then(|b| crate::backup::Backup::open_existing(&b.home)) else {
        return Ok(rolled);
    };
    let by_folder = keevoro_engine::safe_by_folder(backup.catalog()).map_err(Error::from)?;
    for (folder, (safe, kept)) in by_folder {
        let mut at = folder.as_path();
        let node = loop {
            if let Some(node) = map.find(at) {
                break Some(node);
            }
            match at.parent() {
                Some(up) if up.starts_with(&map.root) => at = up,
                _ => break None,
            }
        };
        let Some(node) = node.filter(|n| map.within(*n, top)) else {
            continue;
        };
        for folder in std::iter::once(node).chain(map.ancestors(node)) {
            let sum = rolled.entry(folder).or_insert(BackedUp { files: 0, of: 0 });
            sum.files += safe;
            sum.of += kept;
            if folder == top {
                break;
            }
        }
    }
    Ok(rolled)
}

impl Said<'_> {
    /// The folder Keevoro never looks through that `node` is inside, or is.
    fn passed_by_above(&self, node: u32) -> Option<PassedBy> {
        let mut at = node;
        loop {
            let n = self.map.nodes.get(at as usize)?;
            if let Some(reason) = n.skipped.filter(|r| *r != SkipReason::LeftOut) {
                return Some(PassedBy(reason));
            }
            if n.parent == map::NO_PARENT {
                return None;
            }
            at = n.parent;
        }
    }

    fn node(&self, shown: &ViewNode, folder: Option<u32>, above: Option<PassedBy>) -> MapNode {
        let k = &shown.kinds;
        let mut node = MapNode {
            name: String::new(),
            path: None,
            node: NodeKind::Smaller,
            kind: None,
            bytes: shown.bytes(),
            photos: k[Kind::Photo.index()].files,
            videos: k[Kind::Video.index()].files,
            others: k[Kind::Other.index()].files,
            photo_bytes: k[Kind::Photo.index()].bytes,
            video_bytes: k[Kind::Video.index()].bytes,
            other_bytes: k[Kind::Other.index()].bytes,
            carried: k[Kind::Carried.index()].files,
            carried_bytes: k[Kind::Carried.index()].bytes,
            music: k[Kind::Music.index()].files,
            music_bytes: k[Kind::Music.index()].bytes,
            online_only_bytes: shown.online_only_bytes,
            folders: shown.folders,
            skipped: None,
            made_by_keevoro: false,
            left_out: None,
            left_out_folder: None,
            would_back_up: 0,
            source: false,
            in_source: false,
            backed_up: None,
            unreadable: false,
            not_sized: false,
            more_folders: 0,
            deeper: shown.deeper,
            endings: shown
                .endings
                .iter()
                .filter_map(|e| self.in_kinds.get(e).copied())
                .collect(),
            children: Vec::new(),
        };
        let mut passed = above;
        match shown.item {
            Item::Folder(at) => {
                let n = &self.map.nodes[at as usize];
                if let Some(reason) = n.skipped.filter(|r| *r != SkipReason::LeftOut) {
                    passed = Some(PassedBy(reason));
                }
                let path = self.map.path_of(at);
                node.name = n.name.to_string();
                node.path = Some(friendly_path(&path));
                node.node = NodeKind::Folder;
                node.unreadable = n.has(map::flag::UNREADABLE);
                node.not_sized = self.totals.get(at as usize).is_some_and(|t| t.not_sized);
                node.more_folders = u64::from(n.more_folders);
                node.would_back_up = self.offered.beneath.get(&at).copied().unwrap_or(0);
                node.backed_up = self.backed_up.get(&at).copied();
                for source in &self.judge.choices.sources {
                    if keevoro_core::places::same_place(&path, &source.root) {
                        node.source = true;
                    } else if keevoro_core::places::within(&path, &source.root) {
                        node.in_source = true;
                    }
                }
                if passed.is_none() {
                    node.left_out_folder = self.judge.choices.left_out_folder(&path).map(friendly_path);
                    node.left_out = node.left_out_folder.as_ref().map(|_| self.why(LeftOutBy::Folder));
                    let wanted = Kind::LISTED.iter().map(|kind| k[kind.index()].files).sum::<u64>();
                    if node.left_out.is_none() && wanted > 0 && node.would_back_up == 0 {
                        // Nothing here would be backed up: say the choice
                        // that leaves out the most.
                        node.left_out = self.why_none(at);
                    }
                }
                node.children = shown.children.iter().map(|c| self.node(c, Some(at), passed)).collect();
            }
            Item::File(i) => {
                let file = &self.map.named[i];
                node.name = file.name.to_string();
                node.node = NodeKind::File;
                node.kind = Some(file.kind.into());
                if passed.is_none() && file.kind != Kind::Other {
                    let path = self.map.path_of(file.folder).join(&*file.name);
                    match self.judge.file(&path, file.kind) {
                        Ok(()) => node.would_back_up = file.bytes,
                        Err(by) => node.left_out = Some(self.why(by)),
                    }
                }
            }
            Item::Files(kind) => {
                node.name = match kind {
                    Kind::Photo => "Photos",
                    Kind::Video => "Videos",
                    // Shown beside other files carried along, everything
                    // else is said as such; with none on the list, as ever.
                    Kind::Other if self.judge.kinds.lists_other() => "Everything else",
                    Kind::Other => "Other files",
                    Kind::Carried => "Other files to carry along",
                    Kind::Music => "Music",
                }
                .to_string();
                node.node = NodeKind::Files;
                node.kind = Some(kind.into());
                if let (None, Some(folder), Some(_)) = (passed, folder, kind.media()) {
                    let i = kind.index();
                    let offer = self.offered.by_folder.get(&folder);
                    // The folder's own offered bytes of this kind, at most
                    // what is in this box.
                    node.would_back_up = offer.map_or(0, |o| o.bytes[i]).min(node.bytes);
                    if node.would_back_up == 0 {
                        node.left_out = offer.and_then(|o| o.why_not[i]).map(|(by, _)| self.why(by));
                    }
                }
            }
            Item::Smaller => {
                node.name = "Smaller folders".to_string();
            }
        }
        if let Some(PassedBy(reason)) = passed {
            node.skipped = Some(reason.describe().to_string());
            node.made_by_keevoro = reason.keevoro_made();
            if reason.keevoro_made() {
                node.left_out = Some(self.why(LeftOutBy::MadeByKeevoro));
            }
            node.would_back_up = 0;
        }
        node
    }

    /// Why none of a folder's photos and videos, everything beneath it
    /// counted in, would be backed up: the choice that leaves out the most.
    fn why_none(&self, top: u32) -> Option<String> {
        let mut most: Option<(LeftOutBy, u64)> = None;
        for at in self.map.beneath(top) {
            let Some(offer) = self.offered.by_folder.get(&at) else {
                continue;
            };
            for kind in Kind::LISTED {
                if let Some((by, bytes)) = offer.why_not[kind.index()] {
                    if most.is_none_or(|(_, m)| bytes > m) {
                        most = Some((by, bytes));
                    }
                }
            }
        }
        most.map(|(by, _)| self.why(by))
    }

    /// Why, in words: other files are spoken of only when some kind of
    /// them is on the list.
    fn why(&self, by: LeftOutBy) -> String {
        by.why(self.judge.kinds.lists_other())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::tests::one_at_a_time;
    use crate::keevoro::tests::keevoro;
    use crate::{call, call_for_agent, Op, Said as Heard};
    use serde_json::{json, Value};
    use std::path::PathBuf;

    /// A typical case: a Pictures folder with Camera Roll, Captures
    /// (screen recordings), Texture packs (zip files), an old library and
    /// Screenshots, beside Documents.
    fn pictures(dir: &Path) -> PathBuf {
        let put = |rel: &str, bytes: usize| {
            let file = dir.join(rel);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, vec![0u8; bytes]).unwrap();
        };
        for n in 0..12 {
            put(&format!("Pictures/Camera Roll/IMG_{n}.jpg"), 30_000);
            put(&format!("Pictures/Screenshots/Screenshot {n}.png"), 5_000);
        }
        put("Pictures/Camera Roll/clip.mp4", 200_000);
        put("Pictures/Captures/Game 2019.mp4", 3_000_000);
        put("Pictures/Captures/Game 2020.mp4", 2_000_000);
        put("Pictures/Texture packs/rocks.zip", 4_000_000);
        put("Pictures/Texture packs/grass.zip", 1_500_000);
        put("Pictures/Texture packs/read me.txt", 100);
        for n in 0..5 {
            put(&format!("Pictures/Old Keevoro/originals/IMG_{n}.jpg"), 40_000);
        }
        put("Pictures/Old Keevoro/.keevoro/catalog.sqlite", 1_000);
        put("Documents/tax.pdf", 70_000);
        put("Documents/secret plans.docx", 1_000);
        dir.join("Pictures")
    }

    fn surveyed(keevoro: &Keevoro, place: &Path) {
        let job = keevoro.survey(place, None, None).unwrap();
        assert!(matches!(keevoro.wait(job.id), Some(Heard::Surveyed(_))));
    }

    fn child<'a>(node: &'a Value, name: &str) -> &'a Value {
        node["children"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == name)
            .unwrap_or_else(|| panic!("no {name} in {node}"))
    }

    #[test]
    fn where_the_space_goes_is_answered_from_the_survey_with_everything_in_it() {
        let _turn = one_at_a_time();
        let dir = tempfile::tempdir().unwrap();
        let k = keevoro(dir.path());
        let place = pictures(dir.path());
        let text = place.display().to_string();
        let e = call(&k, Op::PlacesMap, None, json!({ "place": text })).unwrap_err();
        assert_eq!(e.kind, crate::Kind::NotFound);
        assert!(e.message.contains("Find where the photos are"), "{e}");

        surveyed(&k, &place);
        let map = call(&k, Op::PlacesMap, None, json!({ "place": text, "smallest": 1 })).unwrap();
        let top = &map["top"];
        let all = 12 * 30_000 + 12 * 5_000 + 200_000 + 5_000_000 + 5_500_100 + 5 * 40_000 + 1_000;
        assert_eq!(top["bytes"], all, "every file, photo or not");
        assert_eq!(map["complete"], true);
        assert_eq!(map["sizedAll"], true);
        assert_eq!(map["trail"].as_array().unwrap().len(), 1);
        let packs = child(top, "Texture packs");
        assert_eq!(
            (packs["others"].as_u64(), packs["otherBytes"].as_u64()),
            (Some(3), Some(5_500_100))
        );
        assert_eq!(packs["wouldBackUp"], 0);
        assert_eq!(child(packs, "rocks.zip")["node"], "file");
        let old = child(top, "Old Keevoro");
        assert_eq!(old["madeByKeevoro"], true);
        assert_eq!(old["skipped"], "A library from an older Keevoro");
        assert_eq!(old["photos"], 5, "sized, though never looked through");
        assert_eq!(old["wouldBackUp"], 0);
        assert!(old["leftOut"].as_str().unwrap().contains("Keevoro made this folder"));
        let roll = child(top, "Camera Roll");
        assert_eq!(roll["wouldBackUp"], 12 * 30_000 + 200_000);
        assert_eq!(top["wouldBackUp"], 12 * 30_000 + 200_000 + 5_000_000 + 12 * 5_000);
        let zip = map["kinds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|k| k["ending"] == "zip")
            .unwrap();
        assert_eq!((zip["files"].as_u64(), zip["kind"].as_str()), (Some(2), Some("other")));
        // Each box says which of those endings it holds, to light by kind.
        let zip_at = map["kinds"]
            .as_array()
            .unwrap()
            .iter()
            .position(|k| k["ending"] == "zip");
        let holds_zip = |node: &Value| node["endings"].as_array().unwrap().contains(&json!(zip_at.unwrap()));
        assert!(holds_zip(packs) && holds_zip(top));
        assert!(!holds_zip(child(top, "Captures")));

        // Into a folder, with the trail back.
        let inside = call(
            &k,
            Op::PlacesMap,
            None,
            json!({ "place": text, "folder": place.join("Captures").display().to_string(), "depth": 1 }),
        )
        .unwrap();
        let trail: Vec<&str> = inside["trail"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["name"].as_str().unwrap())
            .collect();
        assert_eq!(trail, ["Pictures", "Captures"]);
        assert_eq!(inside["top"]["videos"], 2);
        let elsewhere = call(
            &k,
            Op::PlacesMap,
            None,
            json!({ "place": place.join("Captures").display().to_string(), "folder": text }),
        )
        .unwrap_err();
        assert_eq!(
            elsewhere.kind,
            crate::Kind::NotUnderstood,
            "the folder must be inside the place"
        );
        k.wait_for_jobs();
    }

    #[test]
    fn the_map_says_what_the_backup_in_hand_would_leave_out_and_why() {
        let _turn = one_at_a_time();
        let dir = tempfile::tempdir().unwrap();
        let k = keevoro(dir.path());
        let named = k.start_backup("Stills").unwrap();
        let backup = crate::Backup::open(&named.home).unwrap();
        k.set_backup_wants(&backup, keevoro_core::Wants::only(keevoro_core::MediaKind::Photo))
            .unwrap();
        let place = pictures(dir.path());
        // A Mac's hidden companion file, which no search ever takes, and a
        // backup Keevoro made inside the place.
        std::fs::write(place.join("Camera Roll").join("._IMG_0.jpg"), vec![0u8; 4_096]).unwrap();
        let made = place.join("Backup drive");
        std::fs::create_dir_all(made.join("originals")).unwrap();
        std::fs::write(made.join("originals").join("IMG_9.jpg"), vec![0u8; 50_000]).unwrap();
        std::fs::write(made.join(keevoro_core::BACKUP_GUIDE), "guide").unwrap();
        std::fs::create_dir_all(made.join("index")).unwrap();
        surveyed(&k, &place);
        backup.leave_out(&place.join("Screenshots"), true).unwrap();
        let mut ask = MapRequest::of(&place);
        ask.smallest = Some(1);
        let map = k.space_map(&ask, Some(&named)).unwrap();
        let find = |name: &str| map.top.children.iter().find(|c| c.name == name).unwrap();
        let captures = find("Captures");
        assert_eq!(captures.would_back_up, 0);
        assert_eq!(captures.left_out.as_deref(), Some("this backup is for photos only"));
        let shots = find("Screenshots");
        assert_eq!(shots.left_out_folder, shots.path, "put back by its own name");
        assert_eq!(
            shots.left_out.as_deref(),
            Some("you left this folder out of this backup")
        );
        let roll = find("Camera Roll");
        assert_eq!(roll.would_back_up, 12 * 30_000, "its photos, not its video");
        let video = roll.children.iter().find(|c| c.kind == Some(KindName::Video)).unwrap();
        assert_eq!(video.left_out.as_deref(), Some("this backup is for photos only"));
        let mp4 = map.kinds.iter().find(|k| k.ending == "mp4").unwrap();
        assert_eq!(mp4.left_out.as_deref(), Some("this backup is for photos only"));
        assert_eq!(map.backup.as_deref(), Some("Stills"));
        // The summary line's figure: this backup takes Camera Roll's photos
        // and nothing else. Not the videos (photos only), not Screenshots
        // (left out), not the old library or the backup inside (Keevoro made
        // them), and not the hidden companion file.
        assert_eq!(map.top.would_back_up, 12 * 30_000);
        assert!(find("Backup drive").made_by_keevoro, "{:?}", find("Backup drive"));
        k.wait_for_jobs();
    }

    #[test]
    fn the_map_says_what_is_a_source_and_how_much_of_it_is_backed_up_worked_out_when_asked() {
        let _turn = one_at_a_time();
        let dir = tempfile::tempdir().unwrap();
        let k = keevoro(dir.path());
        let named = k.start_backup("Card photos").unwrap();
        let card = crate::backup::tests::card_looked_through_into(dir.path(), &named.home);
        let before: Vec<(PathBuf, Vec<u8>)> = walk_files(&card);
        surveyed(&k, dir.path());
        let mut ask = MapRequest::of(dir.path());
        ask.smallest = Some(1);
        ask.depth = 3;
        let map = k.space_map(&ask, Some(&named)).unwrap();
        let at = |node: &MapNode, name: &str| node.children.iter().find(|c| c.name == name).cloned().unwrap();
        let on_card = at(&map.top, "Card");
        assert!(on_card.source && !on_card.in_source, "the source itself");
        let holidays = at(&on_card, "Holidays");
        assert!(holidays.in_source && !holidays.source, "and everything inside it");
        let cornwall = at(&holidays, "Cornwall");
        assert_eq!(
            cornwall.backed_up,
            Some(BackedUp { files: 0, of: 2 }),
            "kept, and not backed up yet"
        );
        let kept: usize = on_card.children.iter().filter_map(|c| c.backed_up.map(|b| b.of)).sum();
        assert_eq!(on_card.backed_up.map(|b| b.of), Some(kept), "rolled up the map");
        assert_eq!(map.top.backed_up.map(|b| b.of), Some(kept));
        assert!(!map.top.source && !map.top.in_source);
        // Without a backup in hand, nothing is said of sources or backing up.
        let plain = k.space_map(&ask, None).unwrap();
        let said = serde_json::to_string(&plain).unwrap();
        assert!(!said.contains("backedUp") && !said.contains("\"source\"") && !said.contains("inSource"));
        // Working it out only read the source.
        assert_eq!(walk_files(&card), before);
        k.wait_for_jobs();
    }

    #[test]
    fn only_a_folder_on_a_map_or_one_keevoro_knows_may_be_shown() {
        let _turn = one_at_a_time();
        let dir = tempfile::tempdir().unwrap();
        let k = keevoro(dir.path());
        let named = k.start_backup("Card photos").unwrap();
        let place = pictures(dir.path());
        let elsewhere = dir.path().join("Elsewhere");
        std::fs::create_dir_all(elsewhere.join("Private")).unwrap();
        // Nothing mapped and nothing known: refused.
        let e = k.folder_to_show(&place, Some(&named)).unwrap_err();
        assert_eq!(e.kind, crate::Kind::Refused);
        surveyed(&k, &place);
        let roll = place.join("Camera Roll");
        assert_eq!(
            k.folder_to_show(&roll, None).unwrap(),
            std::fs::canonicalize(&roll).unwrap()
        );
        // Climbing out of the map by "..", or a folder outside it, is not.
        for outside in [
            place.join("Camera Roll").join("..").join("..").join("Elsewhere"),
            elsewhere.join("Private"),
            dir.path().to_path_buf(),
        ] {
            let e = k.folder_to_show(&outside, Some(&named)).unwrap_err();
            assert_eq!(e.kind, crate::Kind::Refused, "{}", outside.display());
        }
        let e = k.folder_to_show(&dir.path().join("Not there"), None).unwrap_err();
        assert_eq!(e.kind, crate::Kind::NotFound);
        // A source, and a folder inside it, are known.
        let card = crate::backup::tests::card_looked_through_into(dir.path(), &named.home);
        assert!(k.folder_to_show(&card.join("Holidays"), Some(&named)).is_ok());
        assert!(
            k.folder_to_show(&named.home, Some(&named)).is_ok(),
            "the backup's own folder"
        );
        k.wait_for_jobs();
    }

    #[test]
    fn a_map_is_fresh_until_a_source_in_it_is_looked_through_and_can_be_forgotten_by_the_person_only() {
        let _turn = one_at_a_time();
        let dir = tempfile::tempdir().unwrap();
        let k = keevoro(dir.path());
        k.start_backup("Card photos").unwrap();
        let card = crate::backup::tests::card_in(dir.path());
        surveyed(&k, &card);
        let mut ask = MapRequest::of(&card);
        ask.smallest = Some(1);
        let fresh = k.space_map(&ask, None).unwrap();
        assert_eq!((fresh.refresh, fresh.searched_since), (None, false), "just mapped");
        let looked = call(
            &k,
            Op::SourcesLook,
            None,
            json!({ "place": card.join("Holidays").display().to_string() }),
        )
        .unwrap();
        k.wait(serde_json::from_value(looked["id"].clone()).unwrap());
        k.wait_for_jobs();
        let after = k.space_map(&ask, None).unwrap();
        assert!(after.searched_since, "a source inside it was looked through since");
        assert_eq!(
            after.refresh,
            Some(Refresh::ByItself),
            "on a disk, mapped again by itself"
        );
        assert_eq!(serde_json::to_value(&after).unwrap()["refresh"], "byItself");
        // An agent may not forget the person's maps.
        let agent = keevoro(dir.path()).as_front_end(crate::FrontEnd::Agent);
        let e = call_for_agent(&agent, Op::PlacesForgetMaps, None, json!({})).unwrap_err();
        assert_eq!(e.kind, crate::Kind::Refused);
        assert!(k.space_map(&ask, None).is_ok(), "still remembered");
        // The person can.
        let forgot = call(&k, Op::PlacesForgetMaps, None, json!({})).unwrap();
        assert_eq!(forgot["forgotten"], 1);
        assert!(
            k.space_map_if_surveyed(&ask, None).unwrap().is_none(),
            "surveyed afresh when next asked"
        );
        k.wait_for_jobs();
    }

    /// Every file under `dir` with what is in it, in order.
    fn walk_files(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            if entry.path().is_dir() {
                out.extend(walk_files(&entry.path()));
            } else {
                out.push((entry.path(), std::fs::read(entry.path()).unwrap()));
            }
        }
        out.sort();
        out
    }

    #[test]
    fn an_agent_allowed_one_folder_reads_nothing_else_of_a_remembered_drive() {
        let _turn = one_at_a_time();
        let dir = tempfile::tempdir().unwrap();
        let person = keevoro(dir.path());
        let place = pictures(dir.path());
        // The person's backup has a source outside the folder allowed, so
        // the map has sources and backed-up figures to say.
        let named = person.start_backup("Card photos").unwrap();
        crate::backup::tests::card_looked_through_into(dir.path(), &named.home);
        // The person surveyed the whole drive, Documents and all.
        surveyed(&person, dir.path());
        let agent = keevoro(dir.path()).as_front_end(crate::FrontEnd::Agent);
        agent
            .set_agent_access(crate::AgentsAccessRequest {
                look_in: Some(vec![place.display().to_string()]),
                back_up_to: None,
                may_change: Some(false),
            })
            .unwrap();
        let whole = dir.path().display().to_string();
        for ask in [
            json!({ "place": whole }),
            json!({ "place": place.display().to_string(), "folder": whole }),
            json!({ "place": whole, "folder": place.display().to_string() }),
            json!({ "place": dir.path().join("Documents").display().to_string() }),
        ] {
            let e = call_for_agent(&agent, Op::PlacesMap, None, ask.clone()).unwrap_err();
            assert_eq!(e.kind, crate::Kind::Refused, "{ask}");
            assert!(e.message.contains("hasn't allowed that folder"), "{e}");
        }
        let allowed = call_for_agent(
            &agent,
            Op::PlacesMap,
            None,
            json!({ "place": place.display().to_string(), "depth": 6, "smallest": 1 }),
        )
        .unwrap();
        let said = allowed.to_string();
        assert!(!said.contains("Documents") && !said.contains("tax.pdf") && !said.contains("secret plans"));
        assert!(
            !said.contains("Holidays") && !said.contains("100CANON") && !said.contains("\"source\""),
            "nothing of a source outside the folder allowed: {said}"
        );
        assert_eq!(
            allowed["trail"].as_array().unwrap().len(),
            1,
            "the trail starts at the folder allowed"
        );
        assert!(said.contains("Camera Roll"));
        person.wait_for_jobs();
    }
}
