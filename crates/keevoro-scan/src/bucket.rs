//! What a bucket source holds, worked out from its listing alone.
//!
//! Looking through a bucket downloads nothing: one paged listing gives each
//! object's name, size, when it was written, its tag and its storage class,
//! and that is all this needs. What a file is comes from its ending, as in
//! a survey; its date from its name or folders when they say, else when it
//! was written; its folders from its key, kept exactly as they are. Its
//! fingerprint is the quick one [`keevoro_core::bucket::quick_fingerprint`]
//! gives, unless Keevoro labelled the object with its whole fingerprint
//! when it backed it up, and the object is read in full when it is backed
//! up, as a folder source's file is. Pure: the engine lists, this decides.

use crate::{FoundItem, ScanReport, SkippedFolder};
use keevoro_core::bucket::{needs_restoring, quick_fingerprint, BucketPlace};
use keevoro_core::{
    best_date, date_from_filename, folder_hints, made_by_keevoro, DateGuess, DateSource, FolderHints, Kinds, MediaKind,
    PartialDate, SkipRules,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// One object, as its listing gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub key: String,
    pub bytes: u64,
    pub modified: Option<i64>,
    pub etag: Option<String>,
    pub class: Option<String>,
    /// The whole fingerprint Keevoro labelled it with when it backed it
    /// up, when it was asked for and is there.
    pub label: Option<String>,
}

/// What a look through `place` finds in `listed`, as a search's report: the
/// files Keevoro looks for, the folders it passes by (one of its own
/// backups, a hidden or system folder, one left out), and objects in
/// archive storage, which cannot be read until restored, as the files not
/// here, with their sizes.
pub fn found_in_listing(place: &BucketPlace, listed: &[Listed], kinds: &Kinds, skip: &SkipRules) -> ScanReport {
    let root = place.root();
    let start = place.listing_prefix();
    let mut report = ScanReport {
        root: PathBuf::from(&root),
        ..ScanReport::default()
    };
    // What each folder holds by name, to know Keevoro's own backups by
    // their note beside an `index` folder, as on a disk.
    let mut files_in: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut folders_in: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for object in listed {
        let Some(rest) = object.key.strip_prefix(&start) else {
            continue;
        };
        let parts: Vec<&str> = rest.split('/').collect();
        for depth in 0..parts.len() {
            let folder = parts[..depth].join("/");
            if depth + 1 == parts.len() {
                files_in.entry(folder).or_default().insert(parts[depth].to_string());
            } else {
                folders_in.entry(folder).or_default().insert(parts[depth].to_string());
            }
        }
    }
    let none = BTreeSet::new();
    let keevoros: Vec<(String, keevoro_core::SkipReason)> = files_in
        .iter()
        .filter_map(|(folder, files)| {
            let folders = folders_in.get(folder).unwrap_or(&none);
            // `.keevoro/catalog.sqlite` is a file two folders down.
            let record = folders.contains(".keevoro")
                && files_in
                    .get(&join(folder, ".keevoro"))
                    .is_some_and(|f| f.contains("catalog.sqlite"));
            let reason = made_by_keevoro(
                &|name| {
                    if name == keevoro_core::RECORD {
                        record
                    } else {
                        files.contains(name)
                    }
                },
                &|name| folders.contains(name),
            )?;
            Some((folder.clone(), reason))
        })
        .collect();

    let mut skipped: BTreeMap<String, keevoro_core::SkipReason> = BTreeMap::new();
    let mut items = Vec::new();
    'objects: for object in listed {
        let Some(rest) = object.key.strip_prefix(&start).filter(|r| !r.is_empty()) else {
            continue;
        };
        let parts: Vec<&str> = rest.split('/').collect();
        let (name, folders) = parts.split_last().unwrap_or((&"", &[]));
        // Keevoro's own backups, folders left out, and folders no search
        // looks in, each said once, by the folder.
        for (folder, reason) in &keevoros {
            if folder.is_empty() || rest.starts_with(&format!("{folder}/")) {
                skipped.entry(folder.clone()).or_insert(*reason);
                continue 'objects;
            }
        }
        for depth in 0..folders.len() {
            let folder = folders[..=depth].join("/");
            let reason = skip
                .skip_folder(folders[depth])
                .or_else(|| skip.own_place(&PathBuf::from(join(&root, &folder))));
            if let Some(reason) = reason {
                skipped.entry(folder).or_insert(reason);
                continue 'objects;
            }
        }
        if skip.skip_file(name).is_some() {
            continue;
        }
        let ending = name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
        let Some(kind) = kinds.looked_for(&ending) else {
            report.other_files += 1;
            continue;
        };
        let path = PathBuf::from(format!("{root}/{rest}"));
        if needs_restoring(object.class.as_deref()) {
            report.online_only.push(path);
            report.online_only_bytes.push(object.bytes);
            continue;
        }
        let modified = object
            .modified
            .and_then(PartialDate::from_unix_seconds)
            .map(|date| DateGuess {
                date,
                source: DateSource::Modified,
            });
        let named = date_from_filename(name).map(|date| DateGuess {
            date,
            source: DateSource::Filename,
        });
        let folder = match kind {
            // Other files keep their folders, starting with the source's.
            MediaKind::Other => FolderHints {
                album: std::iter::once(place.name())
                    .chain(folders.iter().map(|f| (*f).to_string()))
                    .collect(),
                date: None,
                kept: false,
            },
            _ => folder_hints(folders),
        };
        let date = match kind {
            MediaKind::Other => modified,
            _ => best_date(&[named, modified].into_iter().flatten().collect::<Vec<_>>()),
        };
        let (blake3, hashed) = match &object.label {
            Some(label) => (label.clone(), true),
            None => (
                quick_fingerprint(
                    &root,
                    &object.key,
                    object.bytes,
                    object.etag.as_deref(),
                    object.modified,
                ),
                false,
            ),
        };
        items.push(FoundItem {
            path,
            kind,
            mime: mime_of(kind, &ending),
            bytes: object.bytes,
            blake3: Some(blake3),
            hashed,
            date,
            folder,
            width: None,
            height: None,
            junk: None,
        });
    }
    items.sort_by(|a, b| a.path.cmp(&b.path));
    report.items = items;
    report.skipped = skipped
        .into_iter()
        .map(|(folder, reason)| SkippedFolder {
            path: PathBuf::from(join(&root, &folder)),
            reason,
        })
        .collect();
    report
}

fn join(base: &str, rest: &str) -> String {
    match (base.is_empty(), rest.is_empty()) {
        (_, true) => base.to_string(),
        (true, false) => rest.to_string(),
        (false, false) => format!("{base}/{rest}"),
    }
}

/// What kind of file it is, for where it is sent, from its ending: the
/// object is not opened to find out.
fn mime_of(kind: MediaKind, ending: &str) -> String {
    let known = match ending {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "heic" | "heif" => "image/heic",
        "webp" => "image/webp",
        "tif" | "tiff" => "image/tiff",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "avi" => "video/x-msvideo",
        "3gp" => "video/3gpp",
        _ => "",
    };
    match (known, kind) {
        ("", MediaKind::Photo) => "image/octet-stream".to_string(),
        ("", MediaKind::Video) => "video/octet-stream".to_string(),
        ("", MediaKind::Other) => "application/octet-stream".to_string(),
        (known, _) => known.to_string(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use keevoro_core::SkipReason;

    fn listed(key: &str, bytes: u64) -> Listed {
        Listed {
            key: key.into(),
            bytes,
            modified: Some(1_560_000_000),
            etag: Some(format!("\"{bytes}\"")),
            class: None,
            label: None,
        }
    }

    fn place(prefix: &str) -> BucketPlace {
        BucketPlace::new("s3", "eu-west-2", "family", prefix).unwrap()
    }

    #[test]
    fn photos_and_videos_are_known_by_their_endings_and_keep_their_folders() {
        let objects = [
            listed("Photos/Spain 2019/IMG_20190704_101500.jpg", 3_000_000),
            listed("Photos/Spain 2019/clip.mov", 90_000_000),
            listed("Photos/notes.txt", 10),
            listed("Photos/.thumbnails/a.jpg", 5),
            listed("Elsewhere/b.jpg", 5),
        ];
        let r = found_in_listing(&place("Photos"), &objects, &Kinds::default(), &SkipRules::default());
        assert_eq!(r.root, PathBuf::from("s3://family/Photos"));
        let paths: Vec<String> = r.items.iter().map(|i| i.path.display().to_string()).collect();
        assert_eq!(
            paths,
            [
                "s3://family/Photos/Spain 2019/IMG_20190704_101500.jpg",
                "s3://family/Photos/Spain 2019/clip.mov"
            ]
        );
        let photo = &r.items[0];
        assert_eq!(
            (photo.kind, photo.bytes, photo.mime.as_str()),
            (MediaKind::Photo, 3_000_000, "image/jpeg")
        );
        assert_eq!(r.items[1].kind, MediaKind::Video);
        // The date its name gives beats when it was put in the bucket.
        let date = photo.date.unwrap();
        assert_eq!(
            (date.source, date.date.to_string().as_str()),
            (DateSource::Filename, "2019-07-04T10:15:00")
        );
        assert_eq!(r.items[1].date.unwrap().source, DateSource::Modified);
        assert_eq!(photo.folder.album, ["Spain 2019"]);
        assert!(!photo.hashed, "a quick fingerprint until it is read");
        assert_eq!(r.other_files, 1, "the notes are not looked for");
        assert_eq!(r.skipped.len(), 1);
        assert_eq!(r.skipped[0].path, PathBuf::from("s3://family/Photos/.thumbnails"));
        assert_eq!(r.skipped[0].reason, SkipReason::HiddenFolder);
    }

    #[test]
    fn a_keevoro_backup_in_the_bucket_is_passed_by_unless_its_originals_are_chosen() {
        let objects = [
            listed("Old backup/What is in this backup.txt", 10),
            listed("Old backup/index/gallery.json", 10),
            listed("Old backup/originals/2019/a.jpg", 10),
            listed("Phone/b.jpg", 10),
        ];
        let r = found_in_listing(&place(""), &objects, &Kinds::default(), &SkipRules::default());
        assert_eq!(r.items.len(), 1);
        assert_eq!(r.skipped[0].reason, SkipReason::KeevoroBackup);
        assert_eq!(r.skipped[0].path, PathBuf::from("s3://family/Old backup"));
        // Choosing its originals on purpose backs them up.
        let r = found_in_listing(
            &place("Old backup/originals"),
            &objects,
            &Kinds::default(),
            &SkipRules::default(),
        );
        assert_eq!(r.items.len(), 1);
        assert!(r.skipped.is_empty());
    }

    #[test]
    fn objects_in_archive_storage_are_counted_with_their_size_and_never_read() {
        let mut archived = listed("a.jpg", 7_000);
        archived.class = Some("DEEP_ARCHIVE".into());
        let mut instant = listed("b.jpg", 9);
        instant.class = Some("GLACIER_IR".into());
        let r = found_in_listing(
            &place(""),
            &[archived, instant],
            &Kinds::default(),
            &SkipRules::default(),
        );
        assert_eq!(r.items.len(), 1);
        assert_eq!(r.online_only, [PathBuf::from("s3://family/a.jpg")]);
        assert_eq!(r.online_only_bytes, [7_000]);
    }

    #[test]
    fn a_label_keevoro_wrote_is_the_fingerprint_and_every_other_object_has_its_own_quick_one() {
        let mut labelled = listed("originals/a.jpg", 10);
        labelled.label = Some("ab".repeat(32));
        let twin = listed("originals/b.jpg", 10);
        let mut same_tag = listed("originals/c.jpg", 10);
        same_tag.etag = twin.etag.clone();
        let r = found_in_listing(
            &place(""),
            &[labelled, twin, same_tag],
            &Kinds::default(),
            &SkipRules::default(),
        );
        assert!(r.items[0].hashed);
        assert_eq!(r.items[0].blake3.as_deref(), Some("ab".repeat(32).as_str()));
        assert_ne!(
            r.items[1].blake3, r.items[2].blake3,
            "the same size and tag do not make two objects alike"
        );
    }

    #[test]
    fn other_files_keep_the_source_folder_and_theirs() {
        let mut kinds = keevoro_core::FileKinds::default();
        kinds.add("pdf", MediaKind::Other).unwrap();
        let r = found_in_listing(
            &place("Docs"),
            &[listed("Docs/Tax/2019.pdf", 5)],
            &kinds.lookup(),
            &SkipRules::default(),
        );
        assert_eq!(r.items[0].kind, MediaKind::Other);
        assert_eq!(r.items[0].folder.album, ["Docs", "Tax"]);
    }
}
