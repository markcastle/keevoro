//! `keevoro publish`: back the library up and describe it with an index.
//!
//! The order matters. Originals and previews go first and the index goes
//! last, so the index never points at something that is not there yet. Only
//! files that are confirmed stored are listed in it.

use crate::summary;
use anyhow::{Context, Result};
use keevoro_catalog::{Catalog, RenditionState};
use keevoro_core::{group_into_collections, CollectionInput, DateSource};
use keevoro_index::{build_index, gallery_v1, index_key, rendition_path, AlbumInput, ItemInput, Rendition};
use keevoro_publish::{publish, put_document, PublishOptions, Upload};
use keevoro_scan::FoundItem;
use keevoro_storage::ObjectKey;
use keevoro_storage_folder::FolderStore;
use std::collections::BTreeMap;
use std::io::{IsTerminal, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// A gathered file and everything known about its copies in the library.
struct Kept<'a> {
    item: &'a FoundItem,
    hash: &'a str,
    /// Library-relative path of the original, also its storage key.
    path: String,
    preview: Option<RenditionState>,
}

impl Kept<'_> {
    fn id(&self) -> &str {
        &self.hash[..self.hash.len().min(16)]
    }
    fn renditions(&self) -> Vec<Rendition> {
        match self.preview {
            Some(RenditionState::Made { .. }) => vec![Rendition::Thumb, Rendition::Display],
            _ => Vec::new(),
        }
    }
}

pub fn run(
    out: &mut dyn Write,
    catalog: &Catalog,
    library: &Path,
    folder: &Path,
    base_url: Option<&str>,
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

    let runtime = tokio::runtime::Builder::new_current_thread().enable_time().build()?;
    let store = runtime
        .block_on(FolderStore::open(folder))
        .with_context(|| format!("Couldn't use {} as a backup folder", folder.display()))?;
    let home = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    let target = format!("folder:{}", home.display());
    let base_url = base_url.map_or_else(|| file_url(&home), str::to_string);

    let uploads = uploads_for(library, &kept)?;
    let live = std::io::stderr().is_terminal();
    let n = summary::thousands;
    let opts = PublishOptions::default();
    let report = runtime.block_on(publish(&store, catalog, &target, &uploads, &opts, &mut |p| {
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
    let key = match catalog.setting(&format!("index_key:{target}"))? {
        Some(key) => key,
        None => {
            let mut random = [0u8; 16];
            getrandom::fill(&mut random).map_err(|e| anyhow::anyhow!("Couldn't get random numbers ({e})"))?;
            let key = index_key(random);
            catalog.set_setting(&format!("index_key:{target}"), &key)?;
            key
        }
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
                &store,
                &ObjectKey::new(name)?,
                body,
                "application/json",
                &opts,
            ))
            .map_err(|e| anyhow::anyhow!("Couldn't write the index. {}.", keevoro_publish::plain_reason(&e)))?;
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
                content_type: Some("image/webp".to_string()),
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
