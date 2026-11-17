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
    /// The size of everything found on it, in bytes.
    pub bytes: u64,
}

/// A path as a person would write it. Windows gives long paths back as
/// `\\?\C:\Users\...`, which means nothing to most people.
pub fn friendly_path(path: &Path) -> String {
    let shown = path.display().to_string();
    if let Some(share) = shown.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{share}")
    } else if let Some(plain) = shown.strip_prefix(r"\\?\") {
        plain.to_string()
    } else {
        shown
    }
}

/// Stops Keevoro remembering a source: what was found there leaves the list.
///
/// Nothing on the source is touched, and photos already gathered stay in
/// the library. Returns false when there was no such source.
pub fn forget_source(catalog: &mut Catalog, id: i64) -> Result<bool> {
    Ok(catalog.forget_source(id)?)
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
    /// How much room those waiting files will need where they are sent,
    /// in bytes.
    pub waiting_bytes: u64,
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
    let mut waiting_bytes = 0;
    let mut collections = Vec::new();
    for c in &summary.collections {
        let members: Vec<&FoundItem> = c.members.iter().filter_map(|&m| items.get(m).copied()).collect();
        let mut kept = false;
        let mut gathered = 0;
        for item in &members {
            let is_wanted = wanted(catalog, item)?;
            kept |= is_wanted;
            let is_in = match &item.blake3 {
                // Safe means a copy exists somewhere it was sent: in this
                // library's own folder, or in storage it was sent to.
                Some(hash) => match catalog.collected_path(hash)? {
                    Some(place) => catalog.is_here(hash)? || catalog.stored_anywhere(&place)?,
                    None => false,
                },
                None => false,
            };
            gathered += usize::from(is_in);
            waiting_to_gather += usize::from(is_wanted && !is_in);
            if is_wanted && !is_in {
                waiting_bytes += item.bytes;
            }
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
            root: friendly_path(&s.root),
            items: s.items,
            bytes: s.bytes,
        })
        .collect();
    Ok(Discover {
        photos: summary.photos,
        videos: summary.videos,
        bytes: summary.bytes,
        duplicates: summary.duplicates.as_ref().map_or(0, |d| d.copies),
        junk: summary.junk.values().sum(),
        waiting_to_gather,
        waiting_bytes,
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

/// One file inside a collection, for the screen where single photos are
/// ticked and unticked.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhotoChoice {
    #[serde(flatten)]
    pub photo: PhotoView,
    /// When it was taken, as precisely as is known, or `None`.
    pub taken: Option<String>,
    pub kept: bool,
    /// Why it was set aside, for pictures in the junk list.
    pub junk_reason: Option<String>,
}

fn choice(library: &Path, catalog: &Catalog, item: &FoundItem) -> Result<PhotoChoice> {
    Ok(PhotoChoice {
        photo: photo_view(library, catalog, item)?,
        taken: item.date.map(|d| d.date.to_string()),
        kept: wanted(catalog, item)?,
        junk_reason: item.junk.map(|r| r.describe().to_string()),
    })
}

/// Every file in the collection with this key, in date order.
pub fn collection_photos(library: &Path, catalog: &Catalog, key: &str) -> Result<Vec<PhotoChoice>> {
    let reports = catalog.load_reports()?;
    let rescued = catalog.kept_hashes()?;
    let (summary, items) = summary::summarise_with(&reports, &rescued);
    let Some(collection) = summary.collections.iter().find(|c| key_of(&c.parents, &c.title) == key) else {
        bail!("That collection isn't there any more. It may have changed since the screen was drawn.");
    };
    collection
        .members
        .iter()
        .filter_map(|&m| items.get(m))
        .map(|item| choice(library, catalog, item))
        .collect()
}

/// The pictures set aside as probably not memories, each with its reason.
/// One the user has since chosen to keep is no longer listed.
pub fn set_aside(library: &Path, catalog: &Catalog) -> Result<Vec<PhotoChoice>> {
    let reports = catalog.load_reports()?;
    let rescued = catalog.kept_hashes()?;
    let (everything, _) = summary::unique_items(&reports);
    everything
        .into_iter()
        .filter(|i| i.junk.is_some() && !i.blake3.as_ref().is_some_and(|h| rescued.contains(h)))
        .map(|item| choice(library, catalog, item))
        .collect()
}

/// Keeps or skips the one file whose content id starts with `id`.
pub fn set_photo_kept(catalog: &Catalog, id: &str, keep: bool) -> Result<()> {
    let Some((_, _, hash)) = catalog.find_item(id)? else {
        bail!("That photo isn't known to this library.");
    };
    catalog.set_choice(&hash, keep)?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{gather_everything, scan_into};
    use std::fs;

    pub(crate) fn real_jpeg(shade: u8) -> Vec<u8> {
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
    fn forgetting_a_source_never_touches_its_files() {
        let (dir, library, mut catalog) = world();
        let drive = dir.path().join("Dads old Dell");
        let id = discover(&library, &catalog).unwrap().sources[0].id;

        assert!(forget_source(&mut catalog, id).unwrap());
        let after = discover(&library, &catalog).unwrap();
        assert!(after.sources.is_empty());
        assert_eq!((after.photos, after.collections.len()), (0, 0));
        for rel in [
            "Holidays/Cornwall/IMG_20080812_101500.jpg",
            "Holidays/Cornwall/IMG_20080813_090000.jpg",
            "DCIM/100CANON/IMG_20120304_153012.jpg",
            "Backup/IMG_20080812_101500.jpg",
        ] {
            assert!(drive.join(rel).is_file(), "{rel} must still be on the source");
        }
        // Asking twice is harmless.
        assert!(!forget_source(&mut catalog, id).unwrap());
    }

    #[test]
    fn windows_long_paths_are_shown_the_ordinary_way() {
        let shown = |p: &str| friendly_path(Path::new(p));
        assert_eq!(shown(r"\\?\C:\Users\Mark\Pictures"), r"C:\Users\Mark\Pictures");
        assert_eq!(shown(r"\\?\UNC\nas\photos"), r"\\nas\photos");
        assert_eq!(shown("/Volumes/KINGSTON"), "/Volumes/KINGSTON");
    }

    #[test]
    fn discover_describes_what_was_found_in_screen_ready_form() {
        let (_dir, library, catalog) = world();
        let d = discover(&library, &catalog).unwrap();
        assert_eq!((d.photos, d.videos, d.duplicates, d.junk), (3, 0, 1, 0));
        assert_eq!(d.waiting_to_gather, 3);
        // The room they need is the size of the three files kept, the
        // duplicate counted once.
        let kept: u64 = [10, 90, 170].iter().map(|s| real_jpeg(*s).len() as u64).sum();
        assert_eq!(d.waiting_bytes, kept);
        assert_eq!(d.sources.len(), 1);
        assert_eq!((d.sources[0].name.as_str(), d.sources[0].items), ("Dad's old Dell", 4));
        // Its size is every file found on it, the duplicate included.
        let all: u64 = [10, 90, 170, 10].iter().map(|s| real_jpeg(*s).len() as u64).sum();
        assert_eq!(d.sources[0].bytes, all);

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
        assert_eq!(d.waiting_bytes, 0, "nothing more is needed once everything is safe");
        let cornwall = &d.collections[1];
        assert_eq!(cornwall.gathered, 2);
        let thumb = cornwall.cover.thumbnail.as_deref().unwrap();
        assert_eq!(thumb, format!("renditions/thumb/{}.webp", cornwall.cover.id));
        assert!(library.join(thumb).is_file());
        assert_eq!(d.collections[0].gathered, 0);

        let said = outcome.sentences();
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].starts_with("Safely copied 2 files ("), "{said:?}");
        let again = gather_everything(&library, &catalog, false, None, &mut |_| {})
            .unwrap()
            .unwrap();
        assert_eq!(
            again.sentences(),
            ["Everything you're keeping was already safely in your library."]
        );

        // Nothing kept means nothing to do.
        set_collection_kept(&catalog, "Holidays / Cornwall", false).unwrap();
        assert!(gather_everything(&library, &catalog, false, None, &mut |_| {})
            .unwrap()
            .is_none());
    }

    #[test]
    fn the_end_of_a_gather_is_explained_including_what_needs_attention() {
        let dir = tempfile::tempdir().unwrap();
        let drive = dir.path().join("drive");
        fs::create_dir_all(&drive).unwrap();
        fs::write(drive.join("good.jpg"), real_jpeg(10)).unwrap();
        fs::write(drive.join("gone.jpg"), real_jpeg(200)).unwrap();
        // Recognised as a photo by its first bytes, but not a real picture.
        let mut odd = vec![
            0xFF, 0xD8, 0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F', 0, 1, 1, 0, 0, 1, 0, 1, 0, 0,
        ];
        odd.extend([7u8; 64]);
        fs::write(drive.join("odd.jpg"), odd).unwrap();
        fs::write(
            drive.join("clip.mp4"),
            b"\0\0\0\x18ftypisom\0\0\x02\0isomiso2 pretend video",
        )
        .unwrap();
        let library = dir.path().join("library");
        let mut catalog = Catalog::open_in_library(&library).unwrap();
        scan_into(&mut catalog, &drive, None, &mut |_| {}).unwrap();
        fs::remove_file(drive.join("gone.jpg")).unwrap();

        let outcome = gather_everything(&library, &catalog, true, None, &mut |_| {})
            .unwrap()
            .unwrap();
        let said = outcome.sentences();
        assert_eq!(said.len(), 4, "{said:?}");
        assert!(said[0].starts_with("Safely copied 3 files ("), "{said:?}");
        assert!(
            said[1].starts_with("1 file couldn't be copied. Nothing was lost"),
            "{said:?}"
        );
        assert_eq!(
            said[2],
            "1 photo can't be previewed yet. The originals are safe in your library."
        );
        // Whether the computer has no decoder at all or one that cannot play
        // this (pretend) video, the person is told the same plain thing.
        assert!(said[3].contains("no preview picture"), "{said:?}");
    }

    #[test]
    fn single_photos_in_a_collection_can_be_listed_and_chosen() {
        let (_dir, library, catalog) = world();
        let photos = collection_photos(&library, &catalog, "Holidays / Cornwall").unwrap();
        assert_eq!(photos.len(), 2);
        assert!(photos.iter().all(|p| p.kept && p.junk_reason.is_none()));
        assert_eq!(photos[0].taken.as_deref(), Some("2008-08-12T10:15:00"), "in date order");
        let json = serde_json::to_value(&photos[0]).unwrap();
        assert_eq!(
            json["name"], "IMG_20080812_101500.jpg",
            "the photo's own fields sit alongside"
        );
        assert_eq!(json["kept"], true);

        set_photo_kept(&catalog, &photos[1].photo.id, false).unwrap();
        let after = collection_photos(&library, &catalog, "Holidays / Cornwall").unwrap();
        assert_eq!((after[0].kept, after[1].kept), (true, false));
        let d = discover(&library, &catalog).unwrap();
        assert!(d.collections[1].kept, "one photo skipped does not skip the collection");
        assert_eq!(d.waiting_to_gather, 2);

        assert!(collection_photos(&library, &catalog, "Narnia").is_err());
        let err = set_photo_kept(&catalog, "0000", true).unwrap_err();
        assert_eq!(err.to_string(), "That photo isn't known to this library.");
    }

    #[test]
    fn pictures_set_aside_are_listed_with_reasons_and_can_be_kept() {
        let dir = tempfile::tempdir().unwrap();
        let drive = dir.path().join("drive");
        fs::create_dir_all(drive.join("Grandad")).unwrap();
        fs::write(drive.join("Grandad/portrait.jpg"), real_jpeg(30)).unwrap();
        let tiny = image::RgbImage::from_pixel(240, 180, image::Rgb([90, 90, 90]));
        tiny.save(drive.join("Grandad/1952.png")).unwrap();
        let library = dir.path().join("library");
        let mut catalog = Catalog::open_in_library(&library).unwrap();
        scan_into(&mut catalog, &drive, None, &mut |_| {}).unwrap();

        assert_eq!(discover(&library, &catalog).unwrap().junk, 1);
        let aside = set_aside(&library, &catalog).unwrap();
        assert_eq!(aside.len(), 1);
        assert_eq!(aside[0].photo.name, "1952.png");
        assert_eq!(aside[0].junk_reason.as_deref(), Some("Tiny images (under 300 px)"));
        assert!(!aside[0].kept, "set aside means not kept until the user says so");

        set_photo_kept(&catalog, &aside[0].photo.id, true).unwrap();
        assert!(set_aside(&library, &catalog).unwrap().is_empty());
        let d = discover(&library, &catalog).unwrap();
        assert_eq!((d.junk, d.photos), (0, 2), "it is back among the photos");
        assert_eq!(collection_photos(&library, &catalog, "Grandad").unwrap().len(), 2);
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
