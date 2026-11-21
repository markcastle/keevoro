//! Remembered surveys: what "Find where the photos are" saw, kept so that
//! a look inside a folder it suggested, and the map of where the space
//! goes, open at once without walking the disk again.
//!
//! What is kept is the survey's whole map ([`keevoro_scan::map::Map`]):
//! every folder, what it holds by kind and by ending, how much is only in
//! online storage, and the biggest files by name. It is bounded however big
//! the place was (see `keevoro_scan::map::Limits`).
//!
//! Surveys are kept in Keevoro's own cache folder (`surveys/` inside it),
//! never beside a person's files, one file per place, named by a
//! fingerprint of the place so the name says nothing about it. Reset
//! removes the cache, and with it every survey, and a person can forget
//! them all at any time ([`forget_all`]), since a map lists folder names
//! across a whole drive. At most [`KEPT`] are kept, taking at most
//! [`MOST_BYTES`] between them, the oldest removed first.
//!
//! A remembered map is honest about its age. One over an hour old is
//! surveyed again when opened, by itself on a disk of this computer and
//! after asking across a network ([`freshness`]). A search of a source
//! marks every survey covering it as out of date ([`mark_out_of_date`]),
//! with a small file beside it, so the next opening surveys afresh.
//!
//! The file is lines of JSON, compact: a header, the endings, the totals by
//! ending, then one line per folder, then one per file kept by name. A file
//! with another `version` is ignored, which is how the format is changed:
//! a place remembered in an older form is simply surveyed again. A file
//! that does not hold together (a folder whose parent comes after it, say)
//! is ignored too, so nothing read back can point outside the map.
//!
//! The last map read is kept in memory, so going in and out of folders on
//! a map does not read the file again each time.

use keevoro_core::SkipReason;
use keevoro_scan::map::{Ending, EndingSum, FolderNode, Kind, KindSum, Map, NamedFile, Totals, Uncounted};
use serde::{Deserialize, Serialize};
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The version written into each file. Bump it to forget older ones.
const VERSION: u32 = 2;

/// How many places' surveys are kept.
pub const KEPT: usize = 8;

/// How much room the surveys kept may take between them. The newest is kept
/// whatever its size; a map is bounded anyway (`keevoro_scan::map::Limits`).
pub const MOST_BYTES: u64 = 200 * 1024 * 1024;

/// How old a remembered map may be before it is surveyed again on opening.
pub const FRESH_FOR_SECONDS: i64 = 60 * 60;

/// Whether a remembered map should be surveyed again as it is opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// Recent, and nothing has been looked through since: shown as it is.
    Fresh,
    /// Out of date, on a disk of this computer: surveyed again by itself,
    /// the old map shown meanwhile.
    Again,
    /// Out of date, across a network, where a survey can take a long while:
    /// the person is asked first.
    AskFirst,
}

/// Is a map surveyed at `surveyed_at` (seconds since 1970) fresh at `now`?
/// Out of date when it is over an hour old or a source inside it has been
/// looked through since. Pure.
pub fn freshness(surveyed_at: i64, now: i64, searched_since: bool, across_a_network: bool) -> Freshness {
    let old = now.saturating_sub(surveyed_at) > FRESH_FOR_SECONDS;
    match (old || searched_since, across_a_network) {
        (false, _) => Freshness::Fresh,
        (true, false) => Freshness::Again,
        (true, true) => Freshness::AskFirst,
    }
}

/// One place's survey, as remembered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remembered {
    /// Seconds since 1970.
    pub surveyed_at: i64,
    /// False when the survey was stopped before every suggestion was final,
    /// so there may be photos and videos it did not see.
    pub complete: bool,
    /// False when it was stopped before everything was sized for the map.
    pub sized_all: bool,
    /// Everything it saw. Its `root` is the place surveyed, in its full
    /// form (canonical).
    pub map: Map,
}

/// A remembered survey as read back, with each folder's totals worked out.
#[derive(Debug)]
pub struct Loaded {
    pub survey: Remembered,
    pub totals: Vec<Totals>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Header {
    version: u32,
    root: PathBuf,
    surveyed_at: i64,
    complete: bool,
    sized_all: bool,
    folders: usize,
    named: usize,
    endings_dropped: bool,
}

/// One folder: parent, name, `[photo files, photo bytes, video files,
/// video bytes, other files, other bytes, carried files, carried bytes]`,
/// online-only bytes, online-only photos and videos, why it is passed by,
/// flags, more folders, first subfolder, subfolders, endings, and what the
/// suggestions did not count. Other files carried along came last, so a
/// line written before they could be is simply one with none of them.
#[derive(Serialize, Deserialize)]
struct FolderLine(
    u32,
    String,
    Vec<u64>,
    u64,
    u32,
    Option<SkipReason>,
    u8,
    u32,
    u32,
    u32,
    Vec<(u16, u32, u64)>,
    #[serde(default)] Option<Vec<u64>>,
);

/// One file kept by name: folder, name, bytes, kind, only online.
#[derive(Serialize, Deserialize)]
struct NamedLine(u32, String, u64, u8, bool);

fn kind_number(kind: Kind) -> u8 {
    kind.index() as u8
}

fn kind_from(number: u8) -> Kind {
    match number {
        0 => Kind::Photo,
        1 => Kind::Video,
        3 => Kind::Carried,
        _ => Kind::Other,
    }
}

/// The order a folder's sums are written in: the kinds a map has always
/// had, then other files carried along.
const WRITTEN: [Kind; 4] = [Kind::Photo, Kind::Video, Kind::Other, Kind::Carried];

/// What the suggestions did not count, in the order written: photos and
/// videos not looked for, everything never seen, then other files.
const UNCOUNTED: [(bool, Kind); 7] = [
    (true, Kind::Photo),
    (true, Kind::Video),
    (false, Kind::Photo),
    (false, Kind::Video),
    (false, Kind::Other),
    (true, Kind::Carried),
    (false, Kind::Carried),
];

/// Where surveys are kept inside the cache.
fn folder(cache: &Path) -> PathBuf {
    cache.join("surveys")
}

/// The file for a place: a fingerprint of its full path.
fn file_for(cache: &Path, root: &Path) -> PathBuf {
    let hash = blake3::hash(root.to_string_lossy().as_bytes()).to_hex();
    folder(cache).join(format!("{}.jsonl", &hash.as_str()[..16]))
}

/// The mark beside a survey's file that says a source inside it has been
/// looked through since it was made.
fn mark_for(file: &Path) -> PathBuf {
    file.with_extension("searched")
}

fn line<T: Serialize>(text: &mut String, value: &T) -> std::io::Result<()> {
    text.push_str(&serde_json::to_string(value)?);
    text.push('\n');
    Ok(())
}

/// Remembers a survey, replacing any earlier one of the same place, and
/// keeps no more than [`KEPT`]. Written whole beside its place and moved
/// in, so nobody ever reads half of one.
pub fn save(cache: &Path, survey: &Remembered) -> std::io::Result<()> {
    let map = &survey.map;
    let mut text = String::new();
    line(
        &mut text,
        &Header {
            version: VERSION,
            root: map.root.clone(),
            surveyed_at: survey.surveyed_at,
            complete: survey.complete,
            sized_all: survey.sized_all,
            folders: map.nodes.len(),
            named: map.named.len(),
            endings_dropped: map.endings_dropped,
        },
    )?;
    let endings: Vec<(&str, u8)> = map.endings.iter().map(|e| (&*e.name, kind_number(e.kind))).collect();
    line(&mut text, &endings)?;
    let totals: Vec<(u64, u64)> = map.ending_totals.iter().map(|t| (t.files, t.bytes)).collect();
    line(&mut text, &totals)?;
    for n in &map.nodes {
        let d = &n.direct;
        line(
            &mut text,
            &FolderLine(
                n.parent,
                n.name.to_string(),
                WRITTEN
                    .iter()
                    .flat_map(|k| [d[k.index()].files, d[k.index()].bytes])
                    .collect(),
                n.online_only_bytes,
                n.online_only_media,
                n.skipped,
                n.flags,
                n.more_folders,
                n.first_child,
                n.children,
                n.endings.iter().map(|e| (e.ending, e.files, e.bytes)).collect(),
                n.uncounted.as_deref().map(|u| {
                    UNCOUNTED
                        .iter()
                        .flat_map(|(off, k)| {
                            let sum = if *off { u.off[k.index()] } else { u.unseen[k.index()] };
                            [sum.files, sum.bytes]
                        })
                        .collect()
                }),
            ),
        )?;
    }
    for f in &map.named {
        line(
            &mut text,
            &NamedLine(
                f.folder,
                f.name.to_string(),
                f.bytes,
                kind_number(f.kind),
                f.online_only,
            ),
        )?;
    }
    let file = file_for(cache, &map.root);
    // What this program read of the place before is out of date.
    LAST_READ
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take_if(|(kept, _, _)| *kept == file);
    keevoro_media::write_rendition(&file, text.as_bytes())?;
    // Fresh now, whatever was looked through before.
    let _ = std::fs::remove_file(mark_for(&file));
    forget_oldest(cache, KEPT, MOST_BYTES);
    Ok(())
}

/// The `at`th pair of files and bytes written, or none.
fn pair(written: &[u64], at: usize) -> KindSum {
    KindSum {
        files: written.get(at * 2).copied().unwrap_or(0),
        bytes: written.get(at * 2 + 1).copied().unwrap_or(0),
    }
}

/// Reads one remembered survey file; `None` for anything that is not one
/// of this version, or does not hold together.
fn read(file: &Path) -> Option<Remembered> {
    let reader = std::io::BufReader::new(std::fs::File::open(file).ok()?);
    let mut lines = reader.lines();
    let header: Header = serde_json::from_str(&lines.next()?.ok()?).ok()?;
    if header.version != VERSION {
        return None;
    }
    let endings: Vec<(String, u8)> = serde_json::from_str(&lines.next()?.ok()?).ok()?;
    let totals: Vec<(u64, u64)> = serde_json::from_str(&lines.next()?.ok()?).ok()?;
    if totals.len() != endings.len() {
        return None;
    }
    let mut nodes = Vec::with_capacity(header.folders.min(1 << 20));
    for _ in 0..header.folders {
        let FolderLine(parent, name, d, online_bytes, online_media, skipped, flags, more, first, children, ends, u) =
            serde_json::from_str(&lines.next()?.ok()?).ok()?;
        nodes.push(FolderNode {
            parent,
            name: name.into(),
            direct: {
                let mut direct = [KindSum::default(); keevoro_scan::map::KINDS];
                for (at, k) in WRITTEN.iter().enumerate() {
                    direct[k.index()] = pair(&d, at);
                }
                direct
            },
            online_only_bytes: online_bytes,
            online_only_media: online_media,
            skipped,
            flags,
            more_folders: more,
            first_child: first,
            children,
            endings: ends
                .into_iter()
                .map(|(ending, files, bytes)| EndingSum { ending, files, bytes })
                .collect(),
            uncounted: u.map(|u| {
                let mut uncounted = Uncounted::default();
                for (at, (off, k)) in UNCOUNTED.iter().enumerate() {
                    let into = if *off {
                        &mut uncounted.off
                    } else {
                        &mut uncounted.unseen
                    };
                    into[k.index()] = pair(&u, at);
                }
                Box::new(uncounted)
            }),
        });
    }
    let mut named = Vec::with_capacity(header.named.min(1 << 20));
    for _ in 0..header.named {
        let NamedLine(folder, name, bytes, kind, online_only) = serde_json::from_str(&lines.next()?.ok()?).ok()?;
        named.push(NamedFile {
            folder,
            name: name.into(),
            bytes,
            kind: kind_from(kind),
            online_only,
        });
    }
    let map = Map {
        root: header.root,
        nodes,
        endings: endings
            .into_iter()
            .map(|(name, kind)| Ending {
                name: name.into(),
                kind: kind_from(kind),
            })
            .collect(),
        named,
        ending_totals: totals
            .into_iter()
            .map(|(files, bytes)| KindSum { files, bytes })
            .collect(),
        endings_dropped: header.endings_dropped,
    };
    holds_together(&map).then_some(Remembered {
        surveyed_at: header.surveyed_at,
        complete: header.complete,
        sized_all: header.sized_all,
        map,
    })
}

/// Every index in the map points inside it, and every folder comes after
/// the folder it is in, as the map's own code expects.
fn holds_together(map: &Map) -> bool {
    let count = map.nodes.len();
    let Some(first) = map.nodes.first() else { return false };
    if first.parent != keevoro_scan::map::NO_PARENT {
        return false;
    }
    let nodes_hold = map
        .nodes
        .iter()
        .enumerate()
        .skip(1)
        .all(|(i, n)| (n.parent as usize) < i)
        && map.nodes.iter().enumerate().all(|(i, n)| {
            n.children == 0 || ((n.first_child as usize) > i && n.first_child as usize + n.children as usize <= count)
        })
        && map
            .nodes
            .iter()
            .all(|n| n.endings.iter().all(|e| (e.ending as usize) < map.endings.len()));
    let named_hold = map.named.iter().all(|f| (f.folder as usize) < count)
        && map.named.windows(2).all(|w| w[0].folder <= w[1].folder);
    nodes_hold && named_hold
}

/// The place a remembered survey file is of, from its first line alone.
fn root_of(file: &Path) -> Option<PathBuf> {
    let reader = std::io::BufReader::new(std::fs::File::open(file).ok()?);
    let header: Header = serde_json::from_str(&reader.lines().next()?.ok()?).ok()?;
    (header.version == VERSION).then_some(header.root)
}

/// Every remembered survey's file, newest first, with when it was written
/// and how long it is.
fn all(cache: &Path) -> Vec<((std::time::SystemTime, u64), PathBuf)> {
    let Ok(entries) = std::fs::read_dir(folder(cache)) else {
        return Vec::new();
    };
    let mut files: Vec<((std::time::SystemTime, u64), PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
        .filter_map(|e| {
            let listed = e.metadata().ok()?;
            Some(((listed.modified().ok()?, listed.len()), e.path()))
        })
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    files
}

/// The last map read: its file, when that was written and how long it
/// was, and what it held.
type Kept = Option<(PathBuf, (std::time::SystemTime, u64), Arc<Loaded>)>;
static LAST_READ: Mutex<Kept> = Mutex::new(None);

/// Reads a survey file, or gives back the one read last if it is the same
/// file, unchanged.
fn load(file: &Path, written: (std::time::SystemTime, u64)) -> Option<Arc<Loaded>> {
    let mut last = LAST_READ.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((kept, when, loaded)) = last.as_ref() {
        if kept == file && *when == written {
            return Some(loaded.clone());
        }
    }
    let survey = read(file)?;
    let totals = survey.map.totals();
    let loaded = Arc::new(Loaded { survey, totals });
    *last = Some((file.to_path_buf(), written, loaded.clone()));
    Some(loaded)
}

/// The newest remembered survey that covers `place` (a full, canonical
/// path): of that place itself or of a folder it is in. It holds the whole
/// survey: whoever answers from it gives only what is inside the place
/// asked about (`keevoro_scan::map::Map::find`, then what is beneath).
pub fn covering(cache: &Path, place: &Path) -> Option<Arc<Loaded>> {
    all(cache).into_iter().find_map(|(written, file)| {
        let root = root_of(&file)?;
        if !place.starts_with(&root) {
            return None;
        }
        load(&file, written)
    })
}

/// Keeps only the newest `keep` surveys, and only as many as fit in
/// `most_bytes` between them, though always the newest. Only Keevoro's own
/// survey files in its own folder are ever removed.
pub fn forget_oldest(cache: &Path, keep: usize, most_bytes: u64) {
    let mut held = 0u64;
    for (n, ((_, bytes), file)) in all(cache).into_iter().enumerate() {
        held = held.saturating_add(bytes);
        if n >= keep || (n > 0 && held > most_bytes) {
            forget(&file);
        }
    }
}

fn forget(file: &Path) {
    let _ = std::fs::remove_file(file);
    let _ = std::fs::remove_file(mark_for(file));
    LAST_READ
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take_if(|(kept, _, _)| kept == file);
}

/// Forgets every remembered survey, and the one held in memory, and says
/// how many there were. Nothing outside Keevoro's own folder is touched.
pub fn forget_all(cache: &Path) -> usize {
    let surveys = all(cache);
    for (_, file) in &surveys {
        forget(file);
    }
    // Marks left beside surveys already gone.
    if let Ok(entries) = std::fs::read_dir(folder(cache)) {
        for entry in entries.flatten() {
            if entry.path().extension().is_some_and(|x| x == "searched") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    *LAST_READ.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    surveys.len()
}

/// A source at `place` (a full path) has just been looked through: every
/// remembered survey that covers it, or lies inside it, is marked out of
/// date, so the next time its map is opened it is surveyed again.
pub fn mark_out_of_date(cache: &Path, place: &Path) {
    for (_, file) in all(cache) {
        let Some(root) = root_of(&file) else { continue };
        if keevoro_core::places::within(place, &root) || keevoro_core::places::within(&root, place) {
            let _ = std::fs::write(mark_for(&file), b"");
        }
    }
}

/// Has a source inside the survey of `root` been looked through since it
/// was made?
pub fn searched_since(cache: &Path, root: &Path) -> bool {
    mark_for(&file_for(cache, root)).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use keevoro_scan::map::{Builder, CountedIn, FileSeen, Limits, Listing};

    /// A map of `root` with `Users/Mark/Documents` and two years of
    /// pictures, one with a big video.
    fn surveyed(root: &Path) -> Remembered {
        let limits = Limits::default();
        let mut b = Builder::new(root, limits);
        let file = |listing: &mut Listing, name: &str, bytes: u64, listed| {
            listing.file(
                &FileSeen {
                    name,
                    bytes,
                    listed,
                    looked_for: listed.is_some(),
                    online_only: name.contains("online"),
                    system: false,
                },
                &limits,
            );
        };
        b.record(CountedIn::Own(0), Listing::default());
        let users = b.subfolders(CountedIn::Own(0), &[("Users", None)]).unwrap();
        b.record(CountedIn::Own(users), Listing::default());
        let mark = b.subfolders(CountedIn::Own(users), &[("Mark", None)]).unwrap();
        b.record(CountedIn::Own(mark), Listing::default());
        let first = b
            .subfolders(
                CountedIn::Own(mark),
                &[
                    ("Documents", None),
                    ("Pictures", None),
                    ("AppData", Some(SkipReason::AppFolder)),
                ],
            )
            .unwrap();
        let mut docs = Listing::default();
        file(&mut docs, "tax.pdf", 2_000_000, None);
        b.record(CountedIn::Own(first), docs);
        b.record(CountedIn::Own(first + 1), Listing::default());
        b.record(CountedIn::Own(first + 2), Listing::default());
        let years = b
            .subfolders(CountedIn::Own(first + 1), &[("2019", None), ("2020", None)])
            .unwrap();
        let mut y2019 = Listing::default();
        file(&mut y2019, "IMG_1.jpg", 3_000_000, Some(keevoro_core::MediaKind::Photo));
        file(
            &mut y2019,
            "clip online.mp4",
            90_000_000,
            Some(keevoro_core::MediaKind::Video),
        );
        b.record(CountedIn::Own(years), y2019);
        let mut y2020 = Listing::default();
        file(&mut y2020, "IMG_2.jpg", 3_000_000, Some(keevoro_core::MediaKind::Photo));
        b.record(CountedIn::Own(years + 1), y2020);
        Remembered {
            surveyed_at: 1_800_000_000,
            complete: true,
            sized_all: false,
            map: b.finish(),
        }
    }

    #[test]
    fn a_survey_is_remembered_whole_and_read_back_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        let root = dir.path().join("C");
        let survey = surveyed(&root);
        save(&cache, &survey).unwrap();
        let pictures = root.join("Users").join("Mark").join("Pictures");
        let loaded = covering(&cache, &pictures).unwrap();
        assert_eq!(loaded.survey, survey, "every folder, ending and named file as it was");
        assert_eq!(loaded.totals, survey.map.totals());
        let inside = loaded.survey.map.find(&pictures).unwrap();
        let outline: Vec<PathBuf> = loaded
            .survey
            .map
            .media_folders(inside)
            .into_iter()
            .map(|t| t.folder)
            .collect();
        assert_eq!(
            outline,
            [pictures.join("2019"), pictures.join("2020")],
            "nothing outside Pictures"
        );
        assert!(covering(&cache, &dir.path().join("D")).is_none());
        // Only inside Keevoro's cache.
        let written: Vec<PathBuf> = walk(dir.path());
        assert!(written.iter().all(|p| p.starts_with(&cache)), "{written:?}");
    }

    #[test]
    fn the_newest_wins_and_only_a_few_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().to_path_buf();
        for n in 0..(KEPT + 3) {
            let root = dir.path().join(format!("place {n}"));
            save(&cache, &surveyed(&root)).unwrap();
            // Distinct modified times, oldest first.
            let file = file_for(&cache, &root);
            std::fs::File::options()
                .write(true)
                .open(&file)
                .unwrap()
                .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000 + n as u64))
                .unwrap();
        }
        forget_oldest(&cache, KEPT, MOST_BYTES);
        assert_eq!(all(&cache).len(), KEPT);
        assert!(
            covering(&cache, &dir.path().join("place 0")).is_none(),
            "the oldest went first"
        );
        assert!(covering(&cache, &dir.path().join(format!("place {}", KEPT + 2))).is_some());
    }

    #[test]
    fn no_more_room_is_taken_than_allowed_the_oldest_going_first_but_never_the_newest() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().to_path_buf();
        for n in 0..4u64 {
            let root = dir.path().join(format!("place {n}"));
            save(&cache, &surveyed(&root)).unwrap();
            std::fs::File::options()
                .write(true)
                .open(file_for(&cache, &root))
                .unwrap()
                .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000 + n))
                .unwrap();
        }
        let one = all(&cache)[0].0 .1;
        // Room for two and a half: the two newest stay.
        forget_oldest(&cache, KEPT, one * 5 / 2);
        let left: Vec<PathBuf> = all(&cache).into_iter().filter_map(|(_, f)| root_of(&f)).collect();
        assert_eq!(left, [dir.path().join("place 3"), dir.path().join("place 2")]);
        // Room for none: the newest is still kept.
        forget_oldest(&cache, KEPT, 1);
        assert_eq!(all(&cache).len(), 1);
    }

    #[test]
    fn a_search_marks_the_surveys_covering_it_out_of_date_until_surveyed_again() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        let drive = dir.path().join("C");
        let elsewhere = dir.path().join("D");
        save(&cache, &surveyed(&drive)).unwrap();
        save(&cache, &surveyed(&elsewhere)).unwrap();
        assert!(!searched_since(&cache, &drive));
        mark_out_of_date(&cache, &drive.join("Users").join("Mark").join("Pictures"));
        assert!(
            searched_since(&cache, &drive),
            "a source inside the drive was looked through"
        );
        assert!(!searched_since(&cache, &elsewhere), "nothing in D was");
        // A search of a whole drive marks a survey of a folder inside it too.
        mark_out_of_date(&cache, &elsewhere.parent().unwrap().join("D"));
        assert!(searched_since(&cache, &elsewhere));
        save(&cache, &surveyed(&drive)).unwrap();
        assert!(!searched_since(&cache, &drive), "surveyed again, it is fresh");
    }

    #[test]
    fn a_map_is_surveyed_again_when_old_or_searched_since_by_itself_on_a_disk_and_asked_across_a_network() {
        let now = 1_800_000_000;
        let hour = FRESH_FOR_SECONDS;
        assert_eq!(freshness(now - 60, now, false, false), Freshness::Fresh);
        assert_eq!(freshness(now - 60, now, false, true), Freshness::Fresh);
        assert_eq!(freshness(now - hour - 1, now, false, false), Freshness::Again);
        assert_eq!(freshness(now - 3 * 24 * hour, now, false, true), Freshness::AskFirst);
        assert_eq!(freshness(now - 60, now, true, false), Freshness::Again);
        assert_eq!(freshness(now - 60, now, true, true), Freshness::AskFirst);
    }

    #[test]
    fn every_remembered_map_can_be_forgotten_and_nothing_else_is_touched() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("picture.webp"), b"p").unwrap();
        for place in ["C", "D"] {
            save(&cache, &surveyed(&dir.path().join(place))).unwrap();
        }
        mark_out_of_date(&cache, &dir.path().join("C"));
        assert!(
            covering(&cache, &dir.path().join("C")).is_some(),
            "read, so held in memory"
        );
        assert_eq!(forget_all(&cache), 2);
        assert!(covering(&cache, &dir.path().join("C")).is_none());
        assert_eq!(std::fs::read_dir(folder(&cache)).unwrap().count(), 0);
        assert_eq!(std::fs::read(cache.join("picture.webp")).unwrap(), b"p");
        assert_eq!(forget_all(&cache), 0, "forgetting again is harmless");
    }

    #[test]
    fn a_survey_of_an_older_form_or_one_that_does_not_hold_together_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("C");
        std::fs::create_dir_all(folder(dir.path())).unwrap();
        // The first, media-only form: surveyed again rather than misread.
        std::fs::write(
            file_for(dir.path(), &root),
            format!(
                "{{\"version\":1,\"root\":{:?},\"surveyedAt\":1,\"complete\":true}}\n",
                root
            ),
        )
        .unwrap();
        assert!(covering(dir.path(), &root).is_none());
        std::fs::write(folder(dir.path()).join("x.jsonl"), "not json").unwrap();
        assert!(covering(dir.path(), &root).is_none());
        // A folder said to be inside one that comes after it.
        let mut survey = surveyed(&root);
        survey.map.nodes[1].parent = 5;
        save(dir.path(), &survey).unwrap();
        assert!(covering(dir.path(), &root).is_none());
        let mut survey = surveyed(&root);
        survey.map.nodes[0].children = 99;
        save(dir.path(), &survey).unwrap();
        assert!(covering(dir.path(), &root).is_none());
        save(dir.path(), &surveyed(&root)).unwrap();
        assert!(covering(dir.path(), &root).is_some());
    }

    fn walk(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            if entry.path().is_dir() {
                out.extend(walk(&entry.path()));
            } else {
                out.push(entry.path());
            }
        }
        out
    }
}
