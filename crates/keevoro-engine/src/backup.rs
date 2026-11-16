//! Backing a library up, and checking it afterwards.
//!
//! `run` sends originals and previews to a destination and then writes the
//! index. The order matters: the index goes last, so it never points at
//! something that is not there yet, and it lists only files confirmed
//! stored. `check` re-reads the library and, if asked, the backup.
//!
//! Both say what happened by writing plain sentences to `out`, one per
//! line, and report progress as a short line of text through a callback.
//! The command line prints both as they are; the app shows the progress
//! line while it runs and the sentences when it is done.

use crate::summary;
use anyhow::{Context, Result};
use keevoro_catalog::{Catalog, RenditionState, VideoState};
use keevoro_core::{group_into_collections, CollectionInput, DateSource};
use keevoro_index::{build_index, gallery_v1, index_key, rendition_path, AlbumInput, ItemInput, Rendition};
use keevoro_publish::{publish, put_document, PublishOptions, Upload};
use keevoro_scan::FoundItem;
use keevoro_storage::{MultipartStore, ObjectKey, ObjectStore};
use keevoro_storage_folder::FolderStore;
use keevoro_storage_s3::{Preset, S3Settings, S3Store};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// A gathered file and everything known about its copies in the library.
struct Kept<'a> {
    item: &'a FoundItem,
    hash: &'a str,
    /// Library-relative path of the original, also its storage key.
    path: String,
    preview: Option<RenditionState>,
    video: Option<VideoState>,
}

impl Kept<'_> {
    fn id(&self) -> &str {
        &self.hash[..self.hash.len().min(16)]
    }
    fn renditions(&self) -> Vec<Rendition> {
        let mut made = Vec::new();
        if let Some(RenditionState::Made { .. }) = self.preview {
            made.extend([Rendition::Thumb, Rendition::Display]);
        }
        if let Some(VideoState::Known { poster, mp4, .. }) = self.video {
            if poster {
                made.push(Rendition::Poster);
            }
            if mp4 {
                made.push(Rendition::Video);
            }
        }
        made
    }
}

/// Where a backup goes.
pub enum Destination {
    /// An external drive or a network share.
    Folder(PathBuf),
    /// An S3-compatible bucket, with the keys to it. The keys come from
    /// the environment or the system keychain (see `connect.rs`), never
    /// from the command line, where they would end up in shell history.
    Bucket {
        preset: Preset,
        bucket: String,
        access_key_id: String,
        secret_access_key: String,
    },
}

pub const ACCESS_KEY_VAR: &str = "KEEVORO_ACCESS_KEY_ID";
pub const SECRET_KEY_VAR: &str = "KEEVORO_SECRET_ACCESS_KEY";

/// An opened destination: the store, a stable name for it, and the address
/// to use in the index when the user gives none.
type Opened = (Box<dyn MultipartStore>, String, String);

pub fn open(out: &mut dyn Write, runtime: &tokio::runtime::Runtime, destination: Destination) -> Result<Opened> {
    match destination {
        Destination::Folder(folder) => {
            let store = runtime
                .block_on(FolderStore::open(&folder))
                .with_context(|| format!("Couldn't use {} as a backup folder", folder.display()))?;
            let home = std::fs::canonicalize(&folder).unwrap_or(folder);
            Ok((Box::new(store), format!("folder:{}", home.display()), file_url(&home)))
        }
        Destination::Bucket {
            preset,
            bucket,
            access_key_id,
            secret_access_key,
        } => {
            let store = S3Store::connect(&S3Settings {
                preset,
                bucket,
                access_key_id,
                secret_access_key,
            })
            .map_err(|e| anyhow::anyhow!("{}.", keevoro_publish::plain_reason(&e)))?;
            // Find out now whether the keys work, not 9,000 files in.
            let check = runtime.block_on(store.check_connection()).map_err(|e| {
                anyhow::anyhow!(
                    "Couldn't connect to the storage. {}.",
                    keevoro_publish::plain_reason(&e)
                )
            })?;
            if check.publicly_listable == Some(true) {
                writeln!(
                    out,
                    "Warning: anyone can see the list of your files in this bucket. \
                     Turn off public listing in your storage settings."
                )?;
            }
            let name = store.target_name().to_string();
            // Until a web address is given, links are relative to wherever
            // the bucket is served from.
            Ok((Box::new(store), name, "/".to_string()))
        }
    }
}

pub fn run(
    out: &mut dyn Write,
    catalog: &Catalog,
    library: &Path,
    destination: Destination,
    base_url: Option<&str>,
    new_link: bool,
    on_progress: &mut dyn FnMut(&str),
) -> Result<()> {
    let reports = catalog.load_reports()?;
    let (unique, _) = summary::unique_items(&reports);
    let mut kept = Vec::new();
    for item in unique {
        let Some(hash) = item.blake3.as_deref() else { continue };
        // A file the user has since unticked stays in the library but is
        // not published.
        if !crate::wanted(catalog, item)? {
            continue;
        }
        if let Some(path) = catalog.collected_path(hash)? {
            kept.push(Kept {
                item,
                hash,
                path,
                preview: catalog.rendition(hash)?,
                video: catalog.video(hash)?,
            });
        }
    }
    if kept.is_empty() {
        writeln!(
            out,
            "There is nothing in the library to back up yet. Run `keevoro gather` first."
        )?;
        return Ok(());
    }

    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let (store, target, default_url) = open(out, &runtime, destination)?;
    let store = store.as_ref();
    let base_url = base_url.map_or(default_url, str::to_string);

    let uploads = uploads_for(library, catalog, &kept)?;
    let n = summary::thousands;
    let opts = PublishOptions::default();
    let report = runtime.block_on(publish(store, catalog, &target, &uploads, &opts, &mut |p| {
        on_progress(&format!("{} of {} safely backed up", n(p.done), n(p.total)));
    }))?;

    // The index lists only what is confirmed to be in the storage.
    let mut stored = Vec::new();
    for k in &kept {
        if catalog.is_uploaded(&target, &k.path)? {
            stored.push(k);
        }
    }
    let setting = format!("index_key:{target}");
    let fresh_key = || -> Result<String> {
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).map_err(|e| anyhow::anyhow!("Couldn't get random numbers ({e})"))?;
        Ok(index_key(random))
    };
    let previous = catalog.setting(&setting)?;
    // The old index is only removed once the new one is safely written.
    let (key, retired) = match previous {
        Some(old) if new_link => (fresh_key()?, Some(old)),
        Some(current) => (current, None),
        None => (fresh_key()?, None),
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(0);
    let index = build_index(
        &base_url,
        now,
        &albums_for(&stored),
        stored.iter().map(|k| item_for(k)).collect(),
    );
    let documents = [
        (key.clone(), serde_json::to_vec(&index)?),
        (
            "index/gallery.json".to_string(),
            serde_json::to_vec(&gallery_v1(&index))?,
        ),
    ];
    for (name, body) in documents {
        runtime
            .block_on(put_document(
                store,
                &ObjectKey::new(name)?,
                body,
                "application/json",
                &opts,
            ))
            .map_err(|e| anyhow::anyhow!("Couldn't write the index. {}.", keevoro_publish::plain_reason(&e)))?;
    }

    catalog.set_setting(&setting, &key)?;
    if let Some(old) = retired {
        runtime.block_on(store.delete(&ObjectKey::new(old)?)).map_err(|e| {
            anyhow::anyhow!(
                "The new link works, but the old index couldn't be removed. {}.",
                keevoro_publish::plain_reason(&e)
            )
        })?;
        writeln!(out, "Your private link has changed. The old one no longer works.")?;
    }

    if report.uploaded > 0 {
        writeln!(
            out,
            "Safely backed up {} ({}).",
            summary::count(report.uploaded, "file"),
            summary::bytes(report.uploaded_bytes)
        )?;
    }
    if report.already_there > 0 {
        writeln!(out, "{} already there.", summary::count(report.already_there, "file"))?;
    }
    if !report.failed.is_empty() {
        writeln!(out, "Couldn't back up {}:", summary::count(report.failed.len(), "file"))?;
        for f in &report.failed {
            writeln!(out, "  {}: {}.", f.key, f.reason)?;
        }
        writeln!(out, "Run `keevoro publish` again to retry.")?;
    }
    writeln!(
        out,
        "Index written for {} in {}.",
        summary::count(index.items.len(), "file"),
        summary::count(index.albums.len(), "album")
    )?;
    writeln!(out, "Your private index: {}{key}", index.base_url)?;
    Ok(())
}

/// `keevoro check`: re-read the library, and optionally the backup, and
/// say plainly whether everything is still as it was. Fails (so scripts
/// notice) when anything is wrong.
pub fn check(
    out: &mut dyn Write,
    catalog: &Catalog,
    library: &Path,
    backup: Option<Destination>,
    read_back: bool,
    on_progress: &mut dyn FnMut(&str),
) -> Result<()> {
    let n = summary::thousands;
    let files = catalog.collected()?;
    if files.is_empty() {
        writeln!(
            out,
            "There is nothing in the library to check yet. Run `keevoro gather` first."
        )?;
        return Ok(());
    }

    let report = keevoro_collect::verify_library(library, catalog, &mut |done, total| {
        on_progress(&format!("Checking your library: {} of {}", n(done), n(total)));
    })?;
    let mut wrong = report.problems.len();
    if report.intact == 0 && report.problems.is_empty() {
        // Nothing is kept in this folder: everything was sent straight
        // from its source, so there is no library to speak of.
    } else if report.problems.is_empty() {
        writeln!(
            out,
            "Your library: all {} are intact.",
            summary::count(report.intact, "file")
        )?;
    } else {
        writeln!(
            out,
            "Your library: {} intact, {} with a problem:",
            n(report.intact),
            n(report.problems.len())
        )?;
        for p in &report.problems {
            writeln!(out, "  {}: {}.", p.path.display(), p.reason)?;
        }
        writeln!(
            out,
            "To put these right, plug in the source they came from and run `keevoro gather`, or copy them back from your backup."
        )?;
    }

    if let Some(destination) = backup {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        let (store, _, _) = open(out, &runtime, destination)?;
        let expected = files
            .iter()
            .map(|f| {
                Ok(keevoro_publish::Expected {
                    key: ObjectKey::new(f.path.clone())?,
                    bytes: f.bytes,
                    blake3: f.blake3.clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let opts = PublishOptions::default();
        let result = runtime.block_on(keevoro_publish::check_backup(
            store.as_ref(),
            &expected,
            read_back,
            &opts,
            &mut |p| {
                on_progress(&format!("Checking your backup: {} of {}", n(p.done), n(p.total)));
            },
        ));
        wrong += result.problems.len();
        if result.problems.is_empty() {
            let how = if read_back && result.read_back == result.present {
                ", and each one was read back and matches exactly"
            } else if read_back {
                ", and all but the largest were read back and match exactly"
            } else {
                ""
            };
            writeln!(
                out,
                "Your backup: all {} are there{how}.",
                summary::count(result.present, "original")
            )?;
        } else {
            writeln!(
                out,
                "Your backup: {} fine, {} with a problem:",
                n(result.present),
                n(result.problems.len())
            )?;
            for p in &result.problems {
                writeln!(out, "  {}: {}.", p.key, p.reason)?;
            }
            writeln!(out, "Run `keevoro publish` to send anything that is missing.")?;
        }
    }
    if wrong > 0 {
        anyhow::bail!(
            "{} {} attention.",
            summary::count(wrong, "file"),
            if wrong == 1 { "needs" } else { "need" }
        );
    }
    Ok(())
}

/// Originals first, then previews.
fn uploads_for(library: &Path, catalog: &Catalog, kept: &[Kept<'_>]) -> Result<Vec<Upload>> {
    let local = |relative: &str| relative.split('/').fold(library.to_path_buf(), |p, s| p.join(s));
    let mut uploads = Vec::new();
    for k in kept {
        uploads.push(Upload {
            key: ObjectKey::new(k.path.clone())?,
            // The copy in this library if there is one, otherwise the
            // file where it was found: the destination does not care.
            file: keevoro_collect::bytes_of(library, catalog, k.item)?,
            bytes: k.item.bytes,
            write_once: true,
            content_type: Some(k.item.mime.clone()),
            metadata: BTreeMap::from([("blake3".to_string(), k.hash.to_string())]),
        });
    }
    for k in kept {
        for kind in k.renditions() {
            let relative = rendition_path(kind, k.id());
            let file = local(&relative);
            // A preview deleted from the library is simply left out; the
            // next `gather` makes it again.
            let Ok(meta) = std::fs::metadata(&file) else { continue };
            uploads.push(Upload {
                key: ObjectKey::new(relative)?,
                file,
                bytes: meta.len(),
                write_once: false,
                content_type: Some(
                    if kind == Rendition::Video {
                        "video/mp4"
                    } else {
                        "image/webp"
                    }
                    .to_string(),
                ),
                metadata: BTreeMap::new(),
            });
        }
    }
    Ok(uploads)
}

fn item_for(k: &Kept<'_>) -> ItemInput {
    let (width, height, blurhash) = match &k.preview {
        Some(RenditionState::Made {
            width,
            height,
            blurhash,
        }) => (Some(*width), Some(*height), Some(blurhash.clone())),
        // No preview, but the scan may still have read the size.
        _ => (k.item.width, k.item.height, None),
    };
    // For a video the size and length come from reading the video itself.
    let (width, height, duration) = match &k.video {
        Some(VideoState::Known {
            duration_secs,
            width,
            height,
            ..
        }) => (Some(*width), Some(*height), Some(*duration_secs)),
        _ => (width, height, None),
    };
    ItemInput {
        id: k.id().to_string(),
        kind: k.item.kind,
        path: k.path.clone(),
        date: k.item.date,
        width,
        height,
        bytes: k.item.bytes,
        mime: k.item.mime.clone(),
        blurhash,
        duration,
        renditions: k.renditions(),
    }
}

/// The same collections the Discover screen shows become the albums:
/// the user's named folders, then moments for everything unfiled.
fn albums_for(stored: &[&Kept<'_>]) -> Vec<AlbumInput> {
    let inputs: Vec<CollectionInput> = stored
        .iter()
        .map(|k| CollectionInput {
            album: k.item.folder.album.clone(),
            date: k.item.date.filter(|d| d.source != DateSource::Modified).map(|d| d.date),
            kind: k.item.kind,
            bytes: k.item.bytes,
        })
        .collect();
    group_into_collections(&inputs)
        .into_iter()
        .map(|c| AlbumInput {
            path: c.parents.iter().cloned().chain([c.title.clone()]).collect(),
            items: c
                .members
                .iter()
                .filter_map(|&m| stored.get(m))
                .map(|k| k.id().to_string())
                .collect(),
        })
        .collect()
}

/// A `file://` address for a local folder, so the index's links work when
/// it is opened straight from the backup disk.
fn file_url(folder: &Path) -> String {
    let text = folder.to_string_lossy().replace('\\', "/");
    // Windows gives `\\?\D:\Backup`; a browser wants `file:///D:/Backup/`.
    let text = text.trim_start_matches("//?/");
    let encoded = keevoro_index::url_encode_path(text).replace("%3A", ":");
    format!("file:///{}/", encoded.trim_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_folders_become_file_addresses() {
        assert_eq!(
            file_url(Path::new("/mnt/backup disk/photos")),
            "file:///mnt/backup%20disk/photos/"
        );
        assert_eq!(
            file_url(Path::new(r"\\?\D:\Family Backup")),
            "file:///D:/Family%20Backup/"
        );
    }

    /// A source with two photos, searched quickly into a backup whose
    /// home folder is somewhere else entirely.
    fn one_backup() -> (tempfile::TempDir, PathBuf, PathBuf, Catalog) {
        let dir = tempfile::tempdir().unwrap();
        let card = dir.path().join("Card");
        for (rel, shade) in [
            ("Flights/IMG_20240601_100000.jpg", 10),
            ("Flights/IMG_20240601_100500.jpg", 90),
        ] {
            let p = card.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, crate::discover::tests::real_jpeg(shade)).unwrap();
        }
        let home = dir.path().join("home");
        let mut catalog = Catalog::open_in_library(&home).unwrap();
        crate::scan_into(&mut catalog, &card, Some("Card"), &mut |_| {}).unwrap();
        (dir, card, home, catalog)
    }

    fn send(catalog: &Catalog, home: &Path, to: &Path) -> String {
        crate::prepare_to_send(home, catalog, None, &mut |_| {}).unwrap();
        let mut said = Vec::new();
        run(
            &mut said,
            catalog,
            home,
            Destination::Folder(to.to_path_buf()),
            None,
            false,
            &mut |_| {},
        )
        .unwrap();
        String::from_utf8(said).unwrap()
    }

    #[test]
    fn files_go_straight_from_the_source_to_a_folder_with_no_copy_in_between() {
        let (dir, card, home, catalog) = one_backup();
        let to = dir.path().join("Drone footage");
        let said = send(&catalog, &home, &to);
        assert!(said.contains("Safely backed up"), "{said}");

        // Each original is in the destination, exact, under its album.
        for name in ["IMG_20240601_100000.jpg", "IMG_20240601_100500.jpg"] {
            let sent = to.join("originals").join("Flights").join(name);
            assert_eq!(
                keevoro_scan::whole_hash(&sent).unwrap(),
                keevoro_scan::whole_hash(&card.join("Flights").join(name)).unwrap()
            );
        }
        // And the backup's own folder holds no second copy of them.
        assert!(!home.join("originals").exists());
        // The app counts them as safe.
        assert_eq!(crate::discover(&home, &catalog).unwrap().waiting_to_gather, 0);
    }

    #[test]
    fn sending_again_later_only_sends_what_is_new() {
        let (dir, card, home, mut catalog) = one_backup();
        let to = dir.path().join("Drone footage");
        send(&catalog, &home, &to);
        let again = send(&catalog, &home, &to);
        assert!(!again.contains("Safely backed up"), "nothing new to send: {again}");

        // Another flight, another day: the same backup, looked through again.
        std::fs::write(
            card.join("Flights").join("IMG_20240702_090000.jpg"),
            crate::discover::tests::real_jpeg(200),
        )
        .unwrap();
        crate::scan_into(&mut catalog, &card, Some("Card"), &mut |_| {}).unwrap();
        assert_eq!(crate::discover(&home, &catalog).unwrap().waiting_to_gather, 1);
        let later = send(&catalog, &home, &to);
        // The new photo and its two previews; the six from before are not sent again.
        assert!(later.contains("Safely backed up 3 files"), "{later}");
        assert!(later.contains("6 files already there"), "{later}");
        assert!(to
            .join("originals")
            .join("Flights")
            .join("IMG_20240702_090000.jpg")
            .is_file());
    }
}
