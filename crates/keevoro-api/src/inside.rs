//! Looking inside a place before it is added: `places.folders` (which
//! folders in it hold photos and videos, from the remembered survey's map) and
//! `places.files` (the photos and videos in one folder, a page at a time).
//!
//! Both only read. Nothing is added to a backup, nothing on the place is
//! opened beyond its folder listings, and the only thing written anywhere
//! is Keevoro's own cache (by `photos.picture`, not here). They list the
//! kinds of file on Keevoro's list, and when one would not be backed up,
//! say which choice left it out (`keevoro_core::Choices::offered`): a
//! kind switched off, the backup's or the source's photos or videos, an
//! ending left out of a source, or a folder the person left out. A folder
//! Keevoro made (a backup, an old library) is not looked inside as if it
//! were photos to back up.

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
    /// `name` (the default), `newest` or `oldest`. Newest and oldest go by
    /// each file's date as the folder lists it, which is when it was last
    /// changed: often when it was copied, not when it was taken.
    #[serde(default)]
    pub sort: Sort,
    /// `all` (the default), `photos` or `videos`.
    #[serde(default)]
    pub only: Show,
}

/// The order `places.files` gives a folder's files in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Sort {
    /// By name, whatever the case.
    #[default]
    Name,
    /// The file's date, newest first.
    Newest,
    /// The file's date, oldest first.
    Oldest,
}

/// Which of a folder's files `places.files` gives.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Show {
    /// Photos and videos.
    #[default]
    All,
    Photos,
    Videos,
}

impl Show {
    fn takes(self, kind: MediaKind) -> bool {
        match self {
            Show::All => true,
            Show::Photos => kind == MediaKind::Photo,
            Show::Videos => kind == MediaKind::Video,
        }
    }
}

/// The files to show, in order: those `only` takes, sorted by `sort`.
/// Files with the same date keep their order by name. Pure, so the rules
/// are tested without a disk.
fn arranged(
    files: &[keevoro_engine::look::FileEntry],
    sort: Sort,
    only: Show,
) -> Vec<&keevoro_engine::look::FileEntry> {
    // The listing is already by name, and the sort keeps that order for a tie.
    let mut shown: Vec<_> = files.iter().filter(|f| only.takes(f.kind)).collect();
    match sort {
        Sort::Name => {}
        Sort::Newest => shown.sort_by_key(|f| std::cmp::Reverse(f.modified)),
        Sort::Oldest => shown.sort_by_key(|f| f.modified),
    }
    shown
}

fn a_hundred() -> usize {
    100
}

impl FilesRequest {
    /// The first page of a folder, by name, photos and videos alike.
    pub fn of(folder: &Path) -> Self {
        Self {
            place: folder.display().to_string(),
            from: 0,
            count: a_hundred(),
            sort: Sort::default(),
            only: Show::default(),
        }
    }
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
    /// True when the person left it, or a folder it is in, out of the
    /// backup in hand.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub left_out: bool,
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
    /// The folder left out of the backup in hand that the place is in (or
    /// is), if any, as a person would write it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left_out: Option<String>,
}

/// One photo or video in a folder.
#[derive(Debug, Clone, PartialEq, Serialize)]
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
    /// How long a video lasts, in seconds, when its movie header says
    /// (MP4, MOV, M4V and 3GP). Never read for a file only online.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seconds: Option<f64>,
    /// Why it would not be backed up, when it would not: the choice that
    /// left it out, in words (`keevoro_core::Choices::offered`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left_out: Option<String>,
}

/// What `places.files` answers.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesView {
    pub folder: String,
    /// How many of the folder's own files are shown, in all: its photos and
    /// videos, or only one kind when asked. What `from` pages through.
    pub total: usize,
    pub photos: usize,
    pub videos: usize,
    pub online_only: usize,
    /// This page of them, in the order asked for.
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
    /// Every choice the backup in hand made about what is offered; none
    /// when there is no backup, so everything is.
    pub(crate) fn choices_for(backup: Option<&Named>) -> keevoro_core::Choices {
        backup
            .and_then(|b| crate::backup::Backup::open_existing(&b.home))
            .and_then(|b| b.catalog().choices().ok())
            .unwrap_or_default()
    }

    /// `places.folders`: the folders inside `place` that hold photos or
    /// videos, from the newest remembered survey that covers it.
    pub fn folders_inside(&self, place: &Path, backup: Option<&Named>) -> Result<FoldersView> {
        self.folders_inside_if_surveyed(place, backup)?.ok_or_else(|| {
            Error::not_found(format!(
                "Keevoro hasn't surveyed {} yet. Find where the photos are in it first (keevoro survey), then look \
                 inside.",
                friendly_path(place)
            ))
        })
    }

    /// As [`Keevoro::folders_inside`], but `None` when no remembered survey
    /// covers the place, so the app can survey it first and then look.
    pub fn folders_inside_if_surveyed(&self, place: &Path, backup: Option<&Named>) -> Result<Option<FoldersView>> {
        let real = existing_folder(place)?;
        let Some(loaded) = keevoro_engine::surveys::covering(&self.cache_folder(), &real) else {
            return Ok(None);
        };
        // Only what is inside the place asked about is said. A folder the
        // survey did not reach on its own (made since, or past its bounds)
        // is surveyed afresh.
        let map = &loaded.survey.map;
        let Some(top) = map.find(&real) else {
            return Ok(None);
        };
        let choices = Self::choices_for(backup);
        let folders: Vec<FolderView> = map
            .media_folders(top)
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
                photos: f.photos,
                videos: f.videos,
                bytes: f.bytes,
                online_only: f.online_only,
                left_out: choices.left_out_folder(&f.folder).is_some(),
            })
            .collect();
        Ok(Some(FoldersView {
            left_out: choices.left_out_folder(&real).map(friendly_path),
            place: friendly_path(place),
            surveyed_at: loaded.survey.surveyed_at,
            complete: loaded.survey.complete,
            photos: folders.iter().map(|f| f.photos).sum(),
            videos: folders.iter().map(|f| f.videos).sum(),
            bytes: folders.iter().map(|f| f.bytes).sum(),
            online_only: folders.iter().map(|f| f.online_only).sum(),
            folders,
        }))
    }

    /// `places.files`: the photos and videos directly in one folder, a page
    /// at a time, sorted and shown as asked. Only its listing is read, and
    /// for the videos in the page, their movie headers.
    pub fn files_in(&self, ask: &FilesRequest, backup: Option<&Named>) -> Result<FilesView> {
        let place = Path::new(&ask.place);
        let real = existing_folder(place)?;
        if let Some(said) = self.made_by_keevoro(&real) {
            return Err(Error::refused(said));
        }
        let kinds = self.kinds_to_look_for();
        let choices = Self::choices_for(backup);
        let files = self.listing(&real, &kinds)?;
        let shown = arranged(&files, ask.sort, ask.only);
        let page = shown
            .iter()
            .skip(ask.from)
            .take(ask.count.clamp(1, MOST_AT_ONCE))
            .map(|f| FileView {
                path: friendly_path(&f.path),
                name: f.name.clone(),
                is_video: f.kind == MediaKind::Video,
                bytes: f.bytes,
                modified: f.modified,
                online_only: f.online_only,
                seconds: f.length(),
                // A folder Keevoro made was refused above, so only the
                // person's choices can leave a file here out.
                left_out: choices
                    .offered(&keevoro_core::FileFacts {
                        path: &f.path,
                        kind: f.kind,
                        looked_for: f.looked_for,
                        made_by_keevoro: false,
                    })
                    .err()
                    .map(|by| by.why(f.kind).to_string()),
            })
            .collect();
        Ok(FilesView {
            folder: friendly_path(place),
            total: shown.len(),
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
pub(crate) fn existing_folder(place: &Path) -> Result<PathBuf> {
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

    fn entry(name: &str, kind: MediaKind, modified: i64) -> keevoro_engine::look::FileEntry {
        keevoro_engine::look::FileEntry {
            path: PathBuf::from(name),
            name: name.to_string(),
            kind,
            bytes: 1,
            modified,
            online_only: false,
            looked_for: true,
        }
    }

    #[test]
    fn files_are_sorted_by_name_or_by_their_date_and_shown_by_kind() {
        use MediaKind::{Photo, Video};
        // As listed: by name.
        let files = [
            entry("a.jpg", Photo, 300),
            entry("b.mp4", Video, 100),
            entry("c.jpg", Photo, 300),
            entry("d.jpg", Photo, 200),
        ];
        let names = |sort, only| -> Vec<&str> {
            arranged(&files, sort, only)
                .into_iter()
                .map(|f| f.name.as_str())
                .collect()
        };
        assert_eq!(names(Sort::Name, Show::All), ["a.jpg", "b.mp4", "c.jpg", "d.jpg"]);
        assert_eq!(
            names(Sort::Newest, Show::All),
            ["a.jpg", "c.jpg", "d.jpg", "b.mp4"],
            "a tie keeps its order by name"
        );
        assert_eq!(names(Sort::Oldest, Show::All), ["b.mp4", "d.jpg", "a.jpg", "c.jpg"]);
        assert_eq!(names(Sort::Name, Show::Videos), ["b.mp4"]);
        assert_eq!(names(Sort::Oldest, Show::Photos), ["d.jpg", "a.jpg", "c.jpg"]);
    }

    /// The least of an MP4 lasting 1 minute 42 seconds.
    fn mp4_of_102_seconds() -> Vec<u8> {
        let boxed = |name: &[u8; 4], body: &[u8]| {
            let mut v = u32::try_from(body.len() + 8).unwrap().to_be_bytes().to_vec();
            v.extend_from_slice(name);
            v.extend_from_slice(body);
            v
        };
        let mut mvhd = vec![0u8; 12];
        mvhd.extend_from_slice(&600u32.to_be_bytes());
        mvhd.extend_from_slice(&61_200u32.to_be_bytes());
        mvhd.extend_from_slice(&[0; 16]);
        [boxed(b"ftyp", b"isom"), boxed(b"moov", &boxed(b"mvhd", &mvhd))].concat()
    }

    #[test]
    fn a_page_is_sorted_and_shown_as_asked_with_each_videos_length() {
        let _turn = one_at_a_time();
        let dir = tempfile::tempdir().unwrap();
        let k = keevoro(dir.path());
        let folder = pictures(dir.path(), 3).join("2019");
        std::fs::write(folder.join("clip.mp4"), mp4_of_102_seconds()).unwrap();
        let place = folder.display().to_string();
        let page = call(
            &k,
            Op::PlacesFiles,
            None,
            json!({ "place": place, "only": "videos", "sort": "name" }),
        )
        .unwrap();
        assert_eq!(page["total"], 2, "only the videos are paged through");
        assert_eq!(page["photos"], 3, "while the folder's counts are all of it");
        assert_eq!(page["files"][0]["name"], "clip.mp4");
        assert_eq!(page["files"][0]["seconds"], 102.0);
        assert!(
            page["files"][1].get("seconds").is_none(),
            "a video that does not say has none"
        );
        assert!(call(&k, Op::PlacesFiles, None, json!({ "place": place, "sort": "taken" })).is_err());
        let newest = call(&k, Op::PlacesFiles, None, json!({ "place": place, "sort": "newest" })).unwrap();
        assert_eq!(newest["total"], 5);
    }

    #[test]
    fn what_the_backup_leaves_out_is_shown_saying_which_choice_left_it_out() {
        let _turn = one_at_a_time();
        let dir = tempfile::tempdir().unwrap();
        let k = keevoro(dir.path());
        let named = k.start_backup("Stills").unwrap();
        let backup = crate::Backup::open(&named.home).unwrap();
        k.set_backup_wants(&backup, keevoro_core::Wants::Photos).unwrap();
        let place = pictures(dir.path(), 3);
        surveyed(&k, &place);
        let folder = place.join("2019");
        let why = |files: &FilesView, name: &str| {
            files
                .files
                .iter()
                .find(|f| f.name == name)
                .and_then(|f| f.left_out.clone())
        };

        // The videos are there, said to be left out by the backup's choice.
        let files = k.files_in(&FilesRequest::of(&folder), Some(&named)).unwrap();
        assert_eq!((files.total, files.videos), (5, 2));
        assert_eq!(
            why(&files, "clip.mp4").as_deref(),
            Some("this backup is for photos only")
        );
        assert_eq!(why(&files, "IMG_0000.jpg"), None);
        // With no backup in hand, nothing is left out.
        let free = k.files_in(&FilesRequest::of(&folder), None).unwrap();
        assert!(free.files.iter().all(|f| f.left_out.is_none()));

        // A folder left out: it, and every file in it, says so.
        backup.leave_out(&folder, true).unwrap();
        let outline = k.folders_inside(&place, Some(&named)).unwrap();
        assert!(outline.folders[0].left_out && outline.left_out.is_none());
        let inside = k.folders_inside(&folder, Some(&named)).unwrap();
        assert!(
            inside.left_out.is_some(),
            "looked inside itself, the whole place is left out"
        );
        let files = k.files_in(&FilesRequest::of(&folder), Some(&named)).unwrap();
        assert_eq!(
            why(&files, "IMG_0000.jpg").as_deref(),
            Some("you left this folder out of this backup")
        );
        backup.leave_out(&folder, false).unwrap();
        let files = k.files_in(&FilesRequest::of(&folder), Some(&named)).unwrap();
        assert_eq!(why(&files, "IMG_0000.jpg"), None, "put back");
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
        let e = k.files_in(&FilesRequest::of(&backup), None).unwrap_err();
        assert_eq!(e.kind, crate::Kind::Refused);
        assert!(e.message.contains("Copy for Mum"), "{e}");
        // Its originals, chosen on purpose, are a folder like any other.
        assert!(k.files_in(&FilesRequest::of(&backup.join("originals")), None).is_ok());
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
