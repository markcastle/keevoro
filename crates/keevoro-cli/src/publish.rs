//! `keevoro publish`: back the library up and describe it with an index.
//!
//! The order matters. Originals and previews go first and the index goes
//! last, so the index never points at something that is not there yet. Only
//! files that are confirmed stored are listed in it.

use crate::summary;
use anyhow::{bail, Context, Result};
use keevoro_catalog::{Catalog, RenditionState, VideoState};
use keevoro_core::{group_into_collections, CollectionInput, DateSource};
use keevoro_index::{build_index, gallery_v1, index_key, rendition_path, AlbumInput, ItemInput, Rendition};
use keevoro_publish::{publish, put_document, PublishOptions, Upload};
use keevoro_scan::FoundItem;
use keevoro_storage::{MultipartStore, ObjectKey, ObjectStore};
use keevoro_storage_folder::FolderStore;
use keevoro_storage_s3::{Preset, S3Settings, S3Store};
use std::collections::BTreeMap;
use std::io::{IsTerminal, Write};
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
    /// An S3-compatible bucket. The keys come from the environment so they
    /// never appear in shell history or a process list.
    Bucket { preset: Preset, bucket: String },
}

pub const ACCESS_KEY_VAR: &str = "KEEVORO_ACCESS_KEY_ID";
pub const SECRET_KEY_VAR: &str = "KEEVORO_SECRET_ACCESS_KEY";

/// An opened destination: the store, a stable name for it, and the address
/// to use in the index when the user gives none.
type Opened = (Box<dyn MultipartStore>, String, String);

fn open(out: &mut dyn Write, runtime: &tokio::runtime::Runtime, destination: Destination) -> Result<Opened> {
    match destination {
        Destination::Folder(folder) => {
            let store = runtime
                .block_on(FolderStore::open(&folder))
                .with_context(|| format!("Couldn't use {} as a backup folder", folder.display()))?;
            let home = std::fs::canonicalize(&folder).unwrap_or(folder);
            Ok((Box::new(store), format!("folder:{}", home.display()), file_url(&home)))
        }
        Destination::Bucket { preset, bucket } => {
            let key = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
            let (Some(access_key_id), Some(secret_access_key)) = (key(ACCESS_KEY_VAR), key(SECRET_KEY_VAR)) else {
                bail!(
                    "Keevoro needs the storage keys. Set {ACCESS_KEY_VAR} and {SECRET_KEY_VAR} \
                     to a key that can read and write this one bucket, then run this again."
                );
            };
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

    let uploads = uploads_for(library, &kept)?;
    let live = std::io::stderr().is_terminal();
    let n = summary::thousands;
    let opts = PublishOptions::default();
    let report = runtime.block_on(publish(store, catalog, &target, &uploads, &opts, &mut |p| {
        if live {
            eprint!("\r\x1b[2K  {} of {} safely backed up", n(p.done), n(p.total));
            let _ = std::io::stderr().flush();
        }
    }))?;
    if live {
        eprint!("\r\x1b[2K");
    }

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

/// Originals first, then previews.
fn uploads_for(library: &Path, kept: &[Kept<'_>]) -> Result<Vec<Upload>> {
    let local = |relative: &str| relative.split('/').fold(library.to_path_buf(), |p, s| p.join(s));
    let mut uploads = Vec::new();
    for k in kept {
        uploads.push(Upload {
            key: ObjectKey::new(k.path.clone())?,
            file: local(&k.path),
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
}
