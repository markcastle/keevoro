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
//! removes the cache, and with it every survey. At most [`KEPT`] are kept,
//! the oldest removed first.
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
/// video bytes, other files, other bytes]`, online-only bytes, online-only
/// photos and videos, why it is passed by, flags, more folders, first
/// subfolder, subfolders, endings, and what the suggestions did not count.
#[derive(Serialize, Deserialize)]
struct FolderLine(
    u32,
    String,
    [u64; 6],
    u64,
    u32,
    Option<SkipReason>,
    u8,
    u32,
    u32,
    u32,
    Vec<(u16, u32, u64)>,
    #[serde(default)] Option<[u64; 10]>,
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
        _ => Kind::Other,
    }
}

/// Where surveys are kept inside the cache.
fn folder(cache: &Path) -> PathBuf {
    cache.join("surveys")
}

/// The file for a place: a fingerprint of its full path.
fn file_for(cache: &Path, root: &Path) -> PathBuf {
    let hash = blake3::hash(root.to_string_lossy().as_bytes()).to_hex();
    folder(cache).join(format!("{}.jsonl", &hash.as_str()[..16]))
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
                [d[0].files, d[0].bytes, d[1].files, d[1].bytes, d[2].files, d[2].bytes],
                n.online_only_bytes,
                n.online_only_media,
                n.skipped,
                n.flags,
                n.more_folders,
                n.first_child,
                n.children,
                n.endings.iter().map(|e| (e.ending, e.files, e.bytes)).collect(),
                n.uncounted.as_deref().map(|u| {
                    [
                        u.off[0].files,
                        u.off[0].bytes,
                        u.off[1].files,
                        u.off[1].bytes,
                        u.unseen[0].files,
                        u.unseen[0].bytes,
                        u.unseen[1].files,
                        u.unseen[1].bytes,
                        u.unseen[2].files,
                        u.unseen[2].bytes,
                    ]
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
    forget_oldest(cache, KEPT);
    Ok(())
}

fn sum(files: u64, bytes: u64) -> KindSum {
    KindSum { files, bytes }
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
            direct: [sum(d[0], d[1]), sum(d[2], d[3]), sum(d[4], d[5])],
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
                Box::new(Uncounted {
                    off: [sum(u[0], u[1]), sum(u[2], u[3])],
                    unseen: [sum(u[4], u[5]), sum(u[6], u[7]), sum(u[8], u[9])],
                })
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
        ending_totals: totals.into_iter().map(|(files, bytes)| sum(files, bytes)).collect(),
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

/// Keeps only the newest `keep` surveys. Only Keevoro's own survey files
/// in its own folder are ever removed.
pub fn forget_oldest(cache: &Path, keep: usize) {
    for (_, old) in all(cache).into_iter().skip(keep) {
        let _ = std::fs::remove_file(old);
    }
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
        forget_oldest(&cache, KEPT);
        assert_eq!(all(&cache).len(), KEPT);
        assert!(
            covering(&cache, &dir.path().join("place 0")).is_none(),
            "the oldest went first"
        );
        assert!(covering(&cache, &dir.path().join(format!("place {}", KEPT + 2))).is_some());
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
