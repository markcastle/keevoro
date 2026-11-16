//! A small picture of any file Keevoro has found, on demand.
//!
//! The Discover screen shows photos before anything has been gathered, so
//! it cannot rely on the library's thumbnails. This makes one from wherever
//! the file can be read:
//!
//! 1. the library's own thumbnail, if the file has been gathered;
//! 2. a copy made earlier and kept in the app's cache folder;
//! 3. otherwise, by reading the file on the old drive and keeping the
//!    result in the cache for next time.
//!
//! The cache is the only thing written, and it lives in the app's own data
//! folder: never in the library (it is not part of the archive) and never
//! on the old drive. It can be deleted at any time.

use anyhow::{bail, Context, Result};
use keevoro_catalog::Catalog;
use keevoro_core::MediaKind;
use keevoro_index::{rendition_path, Rendition};
use keevoro_media::{render_photo, video, write_rendition, VideoTools};
use std::path::Path;

/// A WebP preview of the file whose content id starts with `id`.
pub fn preview(
    library: &Path,
    catalog: &Catalog,
    cache: &Path,
    id: &str,
    video_tools: Option<&VideoTools>,
) -> Result<Vec<u8>> {
    let Some((source, kind, hash)) = catalog.find_item(id)? else {
        bail!("That photo isn't known to this library.");
    };
    let id = &hash[..hash.len().min(16)];

    if catalog.collected_path(&hash)?.is_some() {
        let rendition = match kind {
            MediaKind::Photo => Rendition::Thumb,
            MediaKind::Video => Rendition::Poster,
        };
        let in_library = rendition_path(rendition, id)
            .split('/')
            .fold(library.to_path_buf(), |p, s| p.join(s));
        if let Ok(bytes) = std::fs::read(&in_library) {
            return Ok(bytes);
        }
    }

    let cached = cache.join(format!("{id}.webp"));
    if let Ok(bytes) = std::fs::read(&cached) {
        return Ok(bytes);
    }

    let bytes = match kind {
        MediaKind::Photo => render_photo(&source).map(|r| r.thumb),
        MediaKind::Video => video::still(video_tools, &source).map(|still| still.webp),
    }
    .with_context(|| {
        if source.exists() {
            "This one can't be previewed"
        } else {
            "The source this is on isn't plugged in"
        }
    })?;
    // Failing to cache is not a reason to withhold the picture.
    let _ = write_rendition(&cached, &bytes);
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{gather_everything, scan_into};
    use std::fs;

    fn real_jpeg(shade: u8) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(1200, 800, image::Rgb([shade, 120, 60]));
        let mut bytes = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
            .unwrap();
        bytes
    }

    fn is_webp(bytes: &[u8]) -> bool {
        bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP"
    }

    #[test]
    fn a_preview_comes_from_the_drive_then_the_cache_then_the_library() {
        let dir = tempfile::tempdir().unwrap();
        let drive = dir.path().join("drive");
        fs::create_dir_all(drive.join("Family")).unwrap();
        fs::write(drive.join("Family/a.jpg"), real_jpeg(40)).unwrap();
        let library = dir.path().join("library");
        let cache = dir.path().join("cache");
        let mut catalog = Catalog::open_in_library(&library).unwrap();
        let report = scan_into(&mut catalog, &drive, None, &mut |_| {}).unwrap();
        let id = report.items[0].id().unwrap().to_string();
        let before = fs::read(drive.join("Family/a.jpg")).unwrap();

        // Straight from the drive, and kept for next time.
        let first = preview(&library, &catalog, &cache, &id, None).unwrap();
        assert!(is_webp(&first));
        let picture = image::load_from_memory(&first).unwrap();
        assert_eq!((picture.width(), picture.height()), (400, 267));
        assert!(cache.join(format!("{id}.webp")).is_file());
        assert_eq!(
            fs::read(drive.join("Family/a.jpg")).unwrap(),
            before,
            "the drive is only read"
        );
        assert!(
            !library.join("renditions").exists(),
            "the cache is not part of the library"
        );

        // With the drive unplugged, the cache still answers.
        fs::rename(&drive, dir.path().join("unplugged")).unwrap();
        assert_eq!(preview(&library, &catalog, &cache, &id, None).unwrap(), first);

        // Once gathered, the library's own thumbnail is used.
        fs::rename(dir.path().join("unplugged"), &drive).unwrap();
        gather_everything(&library, &catalog, false, None, &mut |_| {})
            .unwrap()
            .unwrap();
        fs::remove_dir_all(&cache).unwrap();
        let from_library = preview(&library, &catalog, &cache, &id, None).unwrap();
        assert!(is_webp(&from_library));
        assert!(!cache.exists(), "nothing needed caching");
    }

    #[test]
    fn previews_that_cannot_be_made_say_why() {
        let dir = tempfile::tempdir().unwrap();
        let drive = dir.path().join("drive");
        fs::create_dir_all(&drive).unwrap();
        fs::write(drive.join("a.jpg"), real_jpeg(40)).unwrap();
        fs::write(
            drive.join("clip.mp4"),
            b"\0\0\0\x18ftypisom\0\0\x02\0isomiso2 pretend video",
        )
        .unwrap();
        let library = dir.path().join("library");
        let cache = dir.path().join("cache");
        let mut catalog = Catalog::open_in_library(&library).unwrap();
        let report = scan_into(&mut catalog, &drive, None, &mut |_| {}).unwrap();
        let id_of = |name: &str| {
            report
                .items
                .iter()
                .find(|i| i.path.ends_with(name))
                .unwrap()
                .id()
                .unwrap()
                .to_string()
        };

        let err = preview(&library, &catalog, &cache, "0000", None).unwrap_err();
        assert_eq!(err.to_string(), "That photo isn't known to this library.");
        let err = preview(&library, &catalog, &cache, &id_of("clip.mp4"), None).unwrap_err();
        // Not a real video, so no decoder here or anywhere can show it.
        assert_eq!(err.to_string(), "This one can't be previewed");

        fs::remove_dir_all(&drive).unwrap();
        let err = preview(&library, &catalog, &cache, &id_of("a.jpg"), None).unwrap_err();
        assert_eq!(err.to_string(), "The source this is on isn't plugged in");
    }
}
