//! The Discover view: everything found so far, shaped for showing.
//!
//! The Discover screen is the heart of the app: big photos grouped into
//! collections a person recognises ("Cornwall, August 2008"), each with one
//! button to keep it. This module turns the catalog into exactly the data
//! that screen draws, so the screen itself contains no decisions.
//!
//! A collection is identified by its [`CollectionView::key`], made from its
//! album path. That is what the "Keep collection" button sends back.

use crate::{summary, wanted};
use anyhow::{bail, Result};
use keevoro_catalog::Catalog;
use keevoro_core::MediaKind;
use keevoro_index::{rendition_path, Rendition};
use keevoro_scan::FoundItem;
use serde::Serialize;
use std::path::Path;

/// How many pictures to offer for a collection's collage.
const PREVIEWS_PER_COLLECTION: usize = 5;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceView {
    pub id: i64,
    /// The friendly name, like "Dad's old Dell".
    pub name: String,
    pub root: String,
    pub items: u64,
}

/// One photo or video, with enough for the app to show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhotoView {
    /// The short content id.
    pub id: String,
    pub is_video: bool,
    pub name: String,
    /// Where it is on the old drive. The app asks for a preview by `id`,
    /// never by this path; it is here for showing, not for reading.
    pub source_path: String,
    /// The thumbnail in the library, once gathered, relative to the
    /// library folder. `None` before then.
    pub thumbnail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionView {
    pub key: String,
    /// "Cornwall", "August 2008" or "Undated".
    pub title: String,
    /// "August 2008 · 83 photos · 6 videos".
    pub summary: String,
    /// Outer album names, for a quiet line like "Holidays".
    pub parents: Vec<String>,
    pub photos: usize,
    pub videos: usize,
    /// False when the user has skipped every file in it.
    pub kept: bool,
    /// How many of its files are safely in the library already.
    pub gathered: usize,
    pub cover: PhotoView,
    /// The first few files, cover first, for a collage.
    pub previews: Vec<PhotoView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Discover {
    pub photos: usize,
    pub videos: usize,
    pub bytes: u64,
    /// Extra copies that will be kept once.
    pub duplicates: usize,
    /// Pictures set aside as probably not memories.
    pub junk: usize,
    /// Files being kept that are not yet in the library.
    pub waiting_to_gather: usize,
    pub sources: Vec<SourceView>,
    /// Newest first; the first is the one to show large.
    pub collections: Vec<CollectionView>,
}

/// The key a collection is known by: its album path, joined.
fn key_of(parents: &[String], title: &str) -> String {
    parents
        .iter()
        .map(String::as_str)
        .chain([title])
        .collect::<Vec<_>>()
        .join(" / ")
}

fn photo_view(library: &Path, catalog: &Catalog, item: &FoundItem) -> Result<PhotoView> {
    let hash = item.blake3.as_deref().unwrap_or_default();
    let id = hash[..hash.len().min(16)].to_string();
    let is_video = item.kind == MediaKind::Video;
    // A thumbnail exists once the file is gathered and previewed.
    let kind = if is_video { Rendition::Poster } else { Rendition::Thumb };
    let relative = rendition_path(kind, &id);
    let on_disk = relative.split('/').fold(library.to_path_buf(), |p, s| p.join(s));
    let thumbnail =
        (!id.is_empty() && catalog.collected_path(hash)?.is_some() && on_disk.is_file()).then_some(relative);
    Ok(PhotoView {
        id,
        is_video,
        name: item
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        source_path: item.path.display().to_string(),
        thumbnail,
    })
}

/// Builds the Discover view from everything the library remembers.
pub fn discover(library: &Path, catalog: &Catalog) -> Result<Discover> {
    let reports = catalog.load_reports()?;
    let rescued = catalog.kept_hashes()?;
    let (summary, items) = summary::summarise_with(&reports, &rescued);

    let mut waiting_to_gather = 0;
    let mut collections = Vec::new();
    for c in &summary.collections {
        let members: Vec<&FoundItem> = c.members.iter().filter_map(|&m| items.get(m).copied()).collect();
        let mut kept = false;
        let mut gathered = 0;
        for item in &members {
            let is_wanted = wanted(catalog, item)?;
            kept |= is_wanted;
            let is_in = match &item.blake3 {
                Some(hash) => catalog.collected_path(hash)?.is_some(),
                None => false,
            };
            gathered += usize::from(is_in);
            waiting_to_gather += usize::from(is_wanted && !is_in);
        }
        let Some(cover) = items.get(c.cover).or(members.first()) else {
            continue;
        };
        let cover = photo_view(library, catalog, cover)?;
        let mut previews = vec![cover.clone()];
        for item in &members {
            if previews.len() >= PREVIEWS_PER_COLLECTION {
                break;
            }
            let view = photo_view(library, catalog, item)?;
            if view.id != cover.id {
                previews.push(view);
            }
        }
        collections.push(CollectionView {
            key: key_of(&c.parents, &c.title),
            title: c.title.clone(),
            summary: c.summary(),
            parents: c.parents.clone(),
            photos: c.photos,
            videos: c.videos,
            kept,
            gathered,
            cover,
            previews,
        });
    }

    let sources = catalog
        .sources()?
        .into_iter()
        .map(|s| SourceView {
            id: s.id,
            name: s.name,
            root: s.root.display().to_string(),
            items: s.items,
        })
        .collect();
    Ok(Discover {
        photos: summary.photos,
        videos: summary.videos,
        bytes: summary.bytes,
        duplicates: summary.duplicates.as_ref().map_or(0, |d| d.copies),
        junk: summary.junk.values().sum(),
        waiting_to_gather,
        sources,
        collections,
    })
}

/// Keeps or skips every file in the collection with this key. Returns how
/// many files the choice was recorded for.
pub fn set_collection_kept(catalog: &Catalog, key: &str, keep: bool) -> Result<usize> {
    let reports = catalog.load_reports()?;
    let rescued = catalog.kept_hashes()?;
    let (summary, items) = summary::summarise_with(&reports, &rescued);
    let Some(collection) = summary.collections.iter().find(|c| key_of(&c.parents, &c.title) == key) else {
        bail!("That collection isn't there any more. It may have changed since the screen was drawn.");
    };
    let mut changed = 0;
    for hash in collection
        .members
        .iter()
        .filter_map(|&m| items.get(m)?.blake3.as_deref())
    {
        catalog.set_choice(hash, keep)?;
        changed += 1;
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{gather_everything, scan_into};
    use std::fs;

    fn real_jpeg(shade: u8) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(640, 480, image::Rgb([shade, 120, 60]));
        let mut bytes = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
            .unwrap();
        bytes
    }

    /// A drive with one album, one loose dated photo and a duplicate.
    fn world() -> (tempfile::TempDir, std::path::PathBuf, Catalog) {
        let dir = tempfile::tempdir().unwrap();
        let drive = dir.path().join("Dads old Dell");
        for (rel, shade) in [
            ("Holidays/Cornwall/IMG_20080812_101500.jpg", 10),
            ("Holidays/Cornwall/IMG_20080813_090000.jpg", 90),
            ("DCIM/100CANON/IMG_20120304_153012.jpg", 170),
            ("Backup/IMG_20080812_101500.jpg", 10),
        ] {
            let p = drive.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, real_jpeg(shade)).unwrap();
        }
        let library = dir.path().join("Keevoro Library");
        let mut catalog = Catalog::open_in_library(&library).unwrap();
        let mut ticks = 0;
        scan_into(&mut catalog, &drive, Some("Dad's old Dell"), &mut |_| ticks += 1).unwrap();
        assert_eq!(ticks, 4);
        (dir, library, catalog)
    }

    #[test]
    fn discover_describes_what_was_found_in_screen_ready_form() {
        let (_dir, library, catalog) = world();
        let d = discover(&library, &catalog).unwrap();
        assert_eq!((d.photos, d.videos, d.duplicates, d.junk), (3, 0, 1, 0));
        assert_eq!(d.waiting_to_gather, 3);
        assert_eq!(d.sources.len(), 1);
        assert_eq!((d.sources[0].name.as_str(), d.sources[0].items), ("Dad's old Dell", 4));

        let titles: Vec<&str> = d.collections.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, ["March 2012", "Cornwall"]);
        let cornwall = &d.collections[1];
        assert_eq!(cornwall.key, "Holidays / Cornwall");
        assert_eq!(cornwall.summary, "August 2008 \u{b7} 2 photos");
        assert_eq!((cornwall.photos, cornwall.kept, cornwall.gathered), (2, true, 0));
        assert_eq!(cornwall.previews.len(), 2);
        assert_eq!(cornwall.previews[0], cornwall.cover, "the cover leads the collage");
        assert_eq!(cornwall.cover.id.len(), 16);
        assert_eq!(cornwall.cover.thumbnail, None, "nothing is gathered yet");
        assert!(
            cornwall.cover.source_path.contains("Cornwall"),
            "the album copy, not the backup one"
        );

        // It is what the app receives: camelCase JSON.
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["waitingToGather"], 3);
        assert_eq!(json["collections"][1]["cover"]["isVideo"], false);
    }

    #[test]
    fn keeping_and_skipping_a_collection_is_one_call() {
        let (_dir, library, catalog) = world();
        assert_eq!(set_collection_kept(&catalog, "Holidays / Cornwall", false).unwrap(), 2);
        let d = discover(&library, &catalog).unwrap();
        assert!(!d.collections[1].kept);
        assert!(d.collections[0].kept, "other collections are untouched");
        assert_eq!(d.waiting_to_gather, 1);

        set_collection_kept(&catalog, "Holidays / Cornwall", true).unwrap();
        assert_eq!(discover(&library, &catalog).unwrap().waiting_to_gather, 3);
        let err = set_collection_kept(&catalog, "Narnia", true).unwrap_err();
        assert!(err.to_string().starts_with("That collection isn't there any more."));
    }

    #[test]
    fn gathering_fills_in_thumbnails_and_counts() {
        let (_dir, library, catalog) = world();
        set_collection_kept(&catalog, "March 2012", false).unwrap();
        let mut steps = Vec::new();
        let outcome = gather_everything(&library, &catalog, false, None, &mut |s| steps.push(s))
            .unwrap()
            .unwrap();
        assert_eq!((outcome.copied.copied, outcome.previews.made), (2, 2));
        assert!(outcome.videos.is_none() && !outcome.videos_need_ffmpeg);
        assert!(steps.contains(&crate::GatherStep::Copying { done: 2, total: 2 }));
        assert!(steps.contains(&crate::GatherStep::MakingPreviews { done: 2, total: 2 }));

        let d = discover(&library, &catalog).unwrap();
        assert_eq!(d.waiting_to_gather, 0);
        let cornwall = &d.collections[1];
        assert_eq!(cornwall.gathered, 2);
        let thumb = cornwall.cover.thumbnail.as_deref().unwrap();
        assert_eq!(thumb, format!("renditions/thumb/{}.webp", cornwall.cover.id));
        assert!(library.join(thumb).is_file());
        assert_eq!(d.collections[0].gathered, 0);

        // Nothing kept means nothing to do.
        set_collection_kept(&catalog, "Holidays / Cornwall", false).unwrap();
        assert!(gather_everything(&library, &catalog, false, None, &mut |_| {})
            .unwrap()
            .is_none());
    }

    #[test]
    fn a_library_inside_a_source_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let drive = dir.path().join("drive");
        fs::create_dir_all(&drive).unwrap();
        fs::write(drive.join("a.jpg"), real_jpeg(1)).unwrap();
        let library = drive.join("Keevoro Library");
        let mut catalog = Catalog::open_in_library(&library).unwrap();
        scan_into(&mut catalog, &drive, None, &mut |_| {}).unwrap();
        let err = gather_everything(&library, &catalog, false, None, &mut |_| {}).unwrap_err();
        assert!(err.to_string().contains("Keevoro never writes to those"), "{err}");
        assert!(!library.join("originals").exists());
        assert_eq!(
            catalog.sources().unwrap()[0].name,
            "drive",
            "an unnamed drive is called after its folder"
        );
    }
}
