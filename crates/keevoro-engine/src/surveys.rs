//! Remembered surveys: what "Find where the photos are" saw, kept so that
//! a look inside a folder it suggested opens at once, without walking the
//! disk again.
//!
//! This is the first, media-only form: for each folder that holds photos
//! or videos itself, how many of each, their size, and how many are only
//! in online storage. Nothing else about the place is kept.
//!
//! Surveys are kept in Keevoro's own cache folder (`surveys/` inside it),
//! never beside a person's files, one file per place, named by a
//! fingerprint of the place so the name says nothing about it. Reset
//! removes the cache, and with it every survey. At most [`KEPT`] are kept,
//! the oldest removed first.
//!
//! The file is lines of JSON: a header, then one line per folder. A file
//! with another `version` is ignored, which is how the format is changed.

use serde::{Deserialize, Serialize};
use std::io::BufRead;
use std::path::{Path, PathBuf};

/// The version written into each file. Bump it to forget older ones.
const VERSION: u32 = 1;

/// How many places' surveys are kept.
pub const KEPT: usize = 8;

/// What one folder holds directly, by name alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderCount {
    pub folder: PathBuf,
    pub photos: usize,
    pub videos: usize,
    /// The size of those photos and videos together.
    pub bytes: u64,
    /// Of them, how many are only in online storage.
    pub online_only: usize,
}

/// One place's survey, as remembered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remembered {
    /// The place surveyed, in its full form (canonical).
    pub root: PathBuf,
    /// Seconds since 1970.
    pub surveyed_at: i64,
    /// False when the survey was stopped before it had seen everything.
    pub complete: bool,
    /// Every folder that holds photos or videos itself, in path order.
    pub folders: Vec<FolderCount>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Header {
    version: u32,
    root: PathBuf,
    surveyed_at: i64,
    complete: bool,
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

/// Remembers a survey, replacing any earlier one of the same place, and
/// keeps no more than [`KEPT`]. Written whole beside its place and moved
/// in, so nobody ever reads half of one.
pub fn save(cache: &Path, survey: &Remembered) -> std::io::Result<()> {
    let mut text = serde_json::to_string(&Header {
        version: VERSION,
        root: survey.root.clone(),
        surveyed_at: survey.surveyed_at,
        complete: survey.complete,
    })?;
    text.push('\n');
    for folder in &survey.folders {
        text.push_str(&serde_json::to_string(folder)?);
        text.push('\n');
    }
    keevoro_media::write_rendition(&file_for(cache, &survey.root), text.as_bytes())?;
    forget_oldest(cache, KEPT);
    Ok(())
}

/// Reads one remembered survey file; `None` for anything not one.
fn read(file: &Path) -> Option<Remembered> {
    let reader = std::io::BufReader::new(std::fs::File::open(file).ok()?);
    let mut lines = reader.lines();
    let header: Header = serde_json::from_str(&lines.next()?.ok()?).ok()?;
    if header.version != VERSION {
        return None;
    }
    let folders = lines
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect();
    Some(Remembered {
        root: header.root,
        surveyed_at: header.surveyed_at,
        complete: header.complete,
        folders,
    })
}

/// Every remembered survey's file, newest first.
fn all(cache: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder(cache)) else {
        return Vec::new();
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    files.into_iter().map(|(_, p)| p).collect()
}

/// The newest remembered survey that covers `place` (a full, canonical
/// path): of that place itself or of a folder it is in. Only what is
/// inside `place` is given back, so a survey of a whole drive never says
/// more than was asked about.
pub fn covering(cache: &Path, place: &Path) -> Option<Remembered> {
    all(cache).iter().filter_map(|file| read(file)).find_map(|mut survey| {
        if !place.starts_with(&survey.root) {
            return None;
        }
        survey.folders.retain(|f| f.folder.starts_with(place));
        Some(survey)
    })
}

/// Keeps only the newest `keep` surveys. Only Keevoro's own survey files
/// in its own folder are ever removed.
pub fn forget_oldest(cache: &Path, keep: usize) {
    for old in all(cache).into_iter().skip(keep) {
        let _ = std::fs::remove_file(old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(folder: &Path, photos: usize) -> FolderCount {
        FolderCount {
            folder: folder.to_path_buf(),
            photos,
            videos: 1,
            bytes: 10,
            online_only: 0,
        }
    }

    #[test]
    fn a_survey_is_remembered_and_answers_only_inside_the_place_asked_about() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        let root = dir.path().join("C");
        let pictures = root.join("Users").join("Mark").join("Pictures");
        let survey = Remembered {
            root: root.clone(),
            surveyed_at: 1_800_000_000,
            complete: true,
            folders: vec![
                count(&root.join("Users").join("Mark").join("Documents"), 2),
                count(&pictures.join("2019"), 120),
                count(&pictures.join("2020"), 300),
            ],
        };
        save(&cache, &survey).unwrap();
        assert_eq!(covering(&cache, &root), Some(survey.clone()));
        let inside = covering(&cache, &pictures).unwrap();
        assert_eq!(inside.root, root);
        assert_eq!(inside.folders.len(), 2, "nothing outside Pictures is given");
        assert_eq!(covering(&cache, &dir.path().join("D")), None);
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
            save(
                &cache,
                &Remembered {
                    root: root.clone(),
                    surveyed_at: n as i64,
                    complete: true,
                    folders: vec![count(&root, n)],
                },
            )
            .unwrap();
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
        assert_eq!(
            covering(&cache, &dir.path().join("place 0")),
            None,
            "the oldest went first"
        );
        assert!(covering(&cache, &dir.path().join(format!("place {}", KEPT + 2))).is_some());
    }

    #[test]
    fn a_file_of_another_version_or_not_a_survey_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("C");
        std::fs::create_dir_all(folder(dir.path())).unwrap();
        std::fs::write(
            file_for(dir.path(), &root),
            format!(
                "{{\"version\":99,\"root\":{:?},\"surveyedAt\":1,\"complete\":true}}\n",
                root
            ),
        )
        .unwrap();
        assert_eq!(covering(dir.path(), &root), None);
        std::fs::write(folder(dir.path()).join("x.jsonl"), "not json").unwrap();
        assert_eq!(covering(dir.path(), &root), None);
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
