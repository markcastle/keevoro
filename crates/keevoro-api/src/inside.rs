//! Looking inside a place before it is added: `places.folders` (which
//! folders in it hold photos and videos, from the remembered survey) and
//! `places.files` (the photos and videos in one folder, a page at a time).
//!
//! Both only read. Nothing is added to a backup, nothing on the place is
//! opened beyond its folder listings, and the only thing written anywhere
//! is Keevoro's own cache (by `photos.picture`, not here). They follow the
//! kinds of file Keevoro looks for, and, when a backup is in hand, its
//! choice of photos or videos. A folder Keevoro made (a backup, an old
//! library) is not looked inside as if it were photos to back up.

use crate::backups::Named;
use crate::error::{Error, Result};
use crate::keevoro::Keevoro;
use keevoro_core::MediaKind;
use keevoro_engine::friendly_path;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The most files `places.files` gives at once.
pub const MOST_AT_ONCE: usize = 500;

/// `places.files`
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilesRequest {
    /// The folder, written whole, such as `C:\Users\Mark\Pictures\2019`.
    pub place: String,
    /// How many to pass over first, for the next page. 0 when left out.
    #[serde(default)]
    pub from: usize,
    /// How many to give, at most 500. 100 when left out.
    #[serde(default = "a_hundred")]
    pub count: usize,
}

fn a_hundred() -> usize {
    100
}

/// One folder that holds photos or videos itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderView {
    /// The folder, as a person would write it, and as it is handed back.
    pub folder: String,
    /// Where it is inside the place asked about, such as `2019 › Cornwall`;
    /// empty for the place itself.
    pub relative: String,
    pub photos: usize,
    pub videos: usize,
    pub bytes: u64,
    /// Of them, how many are only in online storage, never opened.
    pub online_only: usize,
}

/// What `places.folders` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoldersView {
    pub place: String,
    /// When the survey this comes from was made, in seconds since 1970.
    pub surveyed_at: i64,
    /// False when that survey was stopped part-way, so there may be more.
    pub complete: bool,
    pub folders: Vec<FolderView>,
    pub photos: usize,
    pub videos: usize,
    pub bytes: u64,
    pub online_only: usize,
}

/// One photo or video in a folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    /// The file, written whole: what `photos.picture` takes as `file`.
    pub path: String,
    pub name: String,
    pub is_video: bool,
    pub bytes: u64,
    /// When the file was last changed (not when it was taken), in seconds
    /// since 1970.
    pub modified: i64,
    /// Only in online storage, not on this computer: no picture is made.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub online_only: bool,
}

/// What `places.files` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesView {
    pub folder: String,
    /// How many photos and videos the folder holds directly, in all.
    pub total: usize,
    pub photos: usize,
    pub videos: usize,
    pub online_only: usize,
    /// This page of them, by name.
    pub files: Vec<FileView>,
}

/// The last few folder listings, so paging through a big folder reads its
/// listing once. A folder changed since (its modified time moved) is
/// listed again.
#[derive(Default)]
pub(crate) struct Listings(Mutex<VecDeque<Listing>>);

struct Listing {
    folder: PathBuf,
    changed: Option<std::time::SystemTime>,
    kinds: String,
    files: std::sync::Arc<Vec<keevoro_engine::look::FileEntry>>,
}

/// How many listings are kept.
const LISTINGS_KEPT: usize = 4;

impl Keevoro {
    /// The kinds of file looked for, as the backup in hand wants them: one
    /// for photos only is shown no videos. With the one kind it wants, if
    /// it wants only one.
    pub(crate) fn kinds_for(&self, backup: Option<&Named>) -> (keevoro_core::Kinds, Option<MediaKind>) {
        let kinds = self.kinds_to_look_for();
        let only = backup
            .and_then(|b| crate::backup::Backup::open_existing(&b.home))
            .and_then(|b| b.catalog().wants().ok())
            .and_then(keevoro_core::Wants::only);
        match only {
            Some(kind) => (kinds.only(kind), Some(kind)),
            None => (kinds, None),
        }
    }

    /// `places.folders`: the folders inside `place` that hold photos or
    /// videos, from the newest remembered survey that covers it.
    pub fn folders_inside(&self, place: &Path, backup: Option<&Named>) -> Result<FoldersView> {
        let real = existing_folder(place)?;
        let Some(survey) = keevoro_engine::surveys::covering(&self.cache_folder(), &real) else {
            return Err(Error::not_found(format!(
                "Keevoro hasn't surveyed {} yet. Find where the photos are in it first (keevoro survey), then look \
                 inside.",
                friendly_path(place)
            )));
        };
        let (_, only) = self.kinds_for(backup);
        let folders: Vec<FolderView> = survey
            .folders
            .iter()
            .map(|f| FolderView {
                folder: friendly_path(&f.folder),
                relative: f
                    .folder
                    .strip_prefix(&real)
                    .map(|r| {
                        r.components()
                            .map(|c| c.as_os_str().to_string_lossy().into_owned())
                            .collect::<Vec<_>>()
                            .join(" › ")
                    })
                    .unwrap_or_default(),
                photos: if only == Some(MediaKind::Video) { 0 } else { f.photos },
                videos: if only == Some(MediaKind::Photo) { 0 } else { f.videos },
                // Sizes are not kept by kind; a choice of one kind keeps the whole.
                bytes: f.bytes,
                online_only: f.online_only,
            })
            .filter(|f| f.photos + f.videos > 0)
            .collect();
        Ok(FoldersView {
            place: friendly_path(place),
            surveyed_at: survey.surveyed_at,
            complete: survey.complete,
            photos: folders.iter().map(|f| f.photos).sum(),
            videos: folders.iter().map(|f| f.videos).sum(),
            bytes: folders.iter().map(|f| f.bytes).sum(),
            online_only: folders.iter().map(|f| f.online_only).sum(),
            folders,
        })
    }

    /// `places.files`: the photos and videos directly in one folder, by
    /// name, a page at a time. Only its listing is read.
    pub fn files_in(&self, place: &Path, backup: Option<&Named>, from: usize, count: usize) -> Result<FilesView> {
        let real = existing_folder(place)?;
        if let Some(said) = self.made_by_keevoro(&real) {
            return Err(Error::refused(said));
        }
        let (kinds, _) = self.kinds_for(backup);
        let files = self.listing(&real, &kinds)?;
        let page = files
            .iter()
            .skip(from)
            .take(count.clamp(1, MOST_AT_ONCE))
            .map(|f| FileView {
                path: friendly_path(&f.path),
                name: f.name.clone(),
                is_video: f.kind == MediaKind::Video,
                bytes: f.bytes,
                modified: f.modified,
                online_only: f.online_only,
            })
            .collect();
        Ok(FilesView {
            folder: friendly_path(place),
            total: files.len(),
            photos: files.iter().filter(|f| f.kind == MediaKind::Photo).count(),
            videos: files.iter().filter(|f| f.kind == MediaKind::Video).count(),
            online_only: files.iter().filter(|f| f.online_only).count(),
            files: page,
        })
    }

    /// What is said about a folder Keevoro made, judged by itself alone, as
    /// a search judges the folder it is pointed at; `None` for any other.
    pub(crate) fn made_by_keevoro(&self, folder: &Path) -> Option<String> {
        let reason = self
            .skip_rules()
            .own_place(folder)
            .or_else(|| keevoro_core::folder_made_by_keevoro(folder))?;
        crate::jobs::own::left_out(&[(folder.to_path_buf(), reason)], &[])
            .into_iter()
            .next()
            .map(|left| left.said)
    }

    fn listing(
        &self,
        folder: &Path,
        kinds: &keevoro_core::Kinds,
    ) -> Result<std::sync::Arc<Vec<keevoro_engine::look::FileEntry>>> {
        let changed = std::fs::metadata(folder).and_then(|m| m.modified()).ok();
        let which = format!("{kinds:?}");
        let mut kept = self
            .listings
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(listing) = kept
            .iter()
            .find(|l| l.folder == folder && l.changed == changed && l.kinds == which)
        {
            return Ok(listing.files.clone());
        }
        let files = std::sync::Arc::new(
            keevoro_engine::look::list_folder(folder, kinds, &keevoro_scan::ByAttributes)
                .map_err(|e| Error::failed(format!("Keevoro couldn't list {}: {e}.", friendly_path(folder))))?,
        );
        kept.retain(|l| l.folder != folder);
        kept.push_front(Listing {
            folder: folder.to_path_buf(),
            changed,
            kinds: which,
            files: files.clone(),
        });
        kept.truncate(LISTINGS_KEPT);
        Ok(files)
    }
}

/// The folder in its full form, or a plain sentence if it is not there.
fn existing_folder(place: &Path) -> Result<PathBuf> {
    match std::fs::canonicalize(place) {
        Ok(real) if real.is_dir() => Ok(real),
        _ => Err(Error::not_found(format!(
            "Keevoro can't find the folder {}. Is it plugged in?",
            friendly_path(place)
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::tests::card_in;
    use crate::jobs::tests::one_at_a_time;
    use crate::keevoro::tests::keevoro;
    use crate::{call, call_for_agent, Op, Said};
    use serde_json::json;

    /// A folder of `n` photos and two videos, not yet a source.
    fn pictures(dir: &Path, n: usize) -> PathBuf {
        let folder = dir.join("Pictures").join("2019");
        std::fs::create_dir_all(&folder).unwrap();
        for i in 0..n {
            std::fs::write(folder.join(format!("IMG_{i:04}.jpg")), b"jpeg").unwrap();
        }
        std::fs::write(folder.join("clip.mp4"), b"mp4").unwrap();
        std::fs::write(folder.join("zz.MOV"), b"mov").unwrap();
        std::fs::write(folder.join("notes.txt"), b"text").unwrap();
        dir.join("Pictures")
    }

    fn surveyed(keevoro: &Keevoro, place: &Path) {
        let job = keevoro.survey(place, None, None).unwrap();
        assert!(matches!(keevoro.wait(job.id), Some(Said::Surveyed(_))));
    }

    #[test]
    fn a_surveyed_place_is_looked_inside_without_a_walk_and_a_folder_a_page_at_a_time() {
        let _turn = one_at_a_time();
        let dir = tempfile::tempdir().unwrap();
        let k = keevoro(dir.path());
        let place = pictures(dir.path(), 250);
        let text = |p: &Path| p.display().to_string();

        let e = call(&k, Op::PlacesFolders, None, json!({ "place": text(&place) })).unwrap_err();
        assert_eq!(e.kind, crate::Kind::NotFound);
        assert!(e.message.contains("Find where the photos are"), "{e}");

        surveyed(&k, &place);
        let folders = call(&k, Op::PlacesFolders, None, json!({ "place": text(&place) })).unwrap();
        assert_eq!(folders["photos"], 250);
        assert_eq!(folders["videos"], 2);
        assert_eq!(folders["folders"][0]["relative"], "2019");
        assert_eq!(folders["complete"], true);

        let folder = place.join("2019");
        let first = call(
            &k,
            Op::PlacesFiles,
            None,
            json!({ "place": text(&folder), "count": 100 }),
        )
        .unwrap();
        assert_eq!(
            (
                first["total"].as_u64(),
                first["photos"].as_u64(),
                first["videos"].as_u64()
            ),
            (Some(252), Some(250), Some(2))
        );
        let names: Vec<&str> = first["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["name"].as_str().unwrap())
            .collect();
        assert_eq!(names.len(), 100);
        assert_eq!(names[0], "clip.mp4", "by name, whatever the case");
        assert_eq!(names[1], "IMG_0000.jpg");
        let last = call(
            &k,
            Op::PlacesFiles,
            None,
            json!({ "place": text(&folder), "from": 200, "count": 500 }),
        )
        .unwrap();
        let last: Vec<&Value> = last["files"].as_array().unwrap().iter().collect();
        assert_eq!(last.len(), 52);
        assert_eq!(last[51]["name"], "zz.MOV");
        assert_eq!(last[51]["isVideo"], true);
        let most = call(
            &k,
            Op::PlacesFiles,
            None,
            json!({ "place": text(&folder), "count": 100_000 }),
        )
        .unwrap();
        assert_eq!(most["files"].as_array().unwrap().len(), 252.min(MOST_AT_ONCE));
        k.wait_for_jobs();
    }

    use serde_json::Value;

    #[test]
    fn a_backup_for_photos_only_is_shown_no_videos() {
        let _turn = one_at_a_time();
        let dir = tempfile::tempdir().unwrap();
        let k = keevoro(dir.path());
        let named = k.start_backup("Stills").unwrap();
        k.set_backup_wants(&crate::Backup::open(&named.home).unwrap(), keevoro_core::Wants::Photos)
            .unwrap();
        let place = pictures(dir.path(), 3);
        surveyed(&k, &place);
        let folders = k.folders_inside(&place, Some(&named)).unwrap();
        assert_eq!((folders.photos, folders.videos), (3, 0));
        let files = k.files_in(&place.join("2019"), Some(&named), 0, 100).unwrap();
        assert_eq!((files.total, files.videos), (3, 0));
        k.wait_for_jobs();
    }

    #[test]
    fn a_folder_keevoro_made_is_not_looked_inside_as_photos_to_back_up() {
        let dir = tempfile::tempdir().unwrap();
        let k = keevoro(dir.path());
        let backup = dir.path().join("Copy for Mum");
        std::fs::create_dir_all(backup.join("index")).unwrap();
        std::fs::create_dir_all(backup.join("originals")).unwrap();
        std::fs::write(backup.join(keevoro_core::BACKUP_GUIDE), b"note").unwrap();
        std::fs::write(backup.join("IMG_1.jpg"), b"jpeg").unwrap();
        let e = k.files_in(&backup, None, 0, 10).unwrap_err();
        assert_eq!(e.kind, crate::Kind::Refused);
        assert!(e.message.contains("Copy for Mum"), "{e}");
        // Its originals, chosen on purpose, are a folder like any other.
        assert!(k.files_in(&backup.join("originals"), None, 0, 10).is_ok());
    }

    #[test]
    fn an_agent_looks_inside_only_folders_the_person_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let k = keevoro(dir.path()).as_front_end(crate::FrontEnd::Agent);
        let card = card_in(dir.path());
        let elsewhere = dir.path().join("Elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("a.jpg"), b"jpeg").unwrap();
        k.set_agent_access(crate::AgentsAccessRequest {
            look_in: Some(vec![card.display().to_string()]),
            back_up_to: None,
            may_change: Some(false),
        })
        .unwrap();
        let place = |p: &Path| json!({ "place": p.display().to_string() });
        for op in [Op::PlacesFolders, Op::PlacesFiles] {
            let e = call_for_agent(&k, op, None, place(&elsewhere)).unwrap_err();
            assert_eq!(e.kind, crate::Kind::Refused, "{}", op.name());
            assert!(e.message.contains("hasn't allowed that folder"), "{e}");
        }
        let e = call_for_agent(
            &k,
            Op::PhotosPicture,
            None,
            json!({ "file": elsewhere.join("a.jpg").display().to_string() }),
        )
        .unwrap_err();
        assert_eq!(e.kind, crate::Kind::Refused);
        // Inside the allowed folder it is looked at, and a picture made, changing nothing.
        let files = call_for_agent(
            &k,
            Op::PlacesFiles,
            None,
            place(&card.join("Holidays").join("Cornwall")),
        )
        .unwrap();
        let first = files["files"][0]["path"].as_str().unwrap().to_string();
        let picture = call_for_agent(&k, Op::PhotosPicture, None, json!({ "file": first })).unwrap();
        assert_eq!(picture["format"], "webp");
        let both = call_for_agent(&k, Op::PhotosPicture, None, json!({ "file": first, "photo": "abcd" })).unwrap_err();
        assert_eq!(both.kind, crate::Kind::NotUnderstood);
    }
}
