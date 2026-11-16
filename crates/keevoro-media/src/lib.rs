//! Smaller copies of photos for showing in a gallery.
//!
//! An original from a modern camera is 5 to 25 MB. A gallery page showing a
//! hundred of them needs something far lighter, so for every photo keevoro
//! makes:
//!
//! - a **thumbnail**, 400 px on its longest side, for grids;
//! - a **display copy**, 2048 px on its longest side, for viewing;
//! - a **blurhash**, a 30-character smudge of the picture's colours that a
//!   page can show instantly while the real image loads.
//!
//! Three things here are deliberate and easy to break by accident:
//!
//! - **Renditions carry no metadata.** They are built from pixels alone, so
//!   GPS positions and camera serial numbers in the original never reach a
//!   published copy. Do not "helpfully" copy EXIF across.
//! - **Rotation is baked in.** Phones store sideways pixels plus a note
//!   saying "rotate me". Since the note is dropped, the pixels are turned
//!   first, and the reported width and height are the upright ones.
//! - **Nothing is ever enlarged.** A 300 px picture stays 300 px.
//!
//! Camera RAW files are previewed from the finished picture the camera
//! embeds in them (see [`raw`]). Formats the decoder cannot read at all
//! (HEIC) return [`MediaError::Unsupported`]. That is not a failure of the library: the
//! original is still safe, it simply has no preview yet.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod raw;
pub mod video;

pub use video::{Conversion, Still, VideoInfo, VideoTools};

use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageReader};
use std::io::Write;
use std::path::Path;

pub const THUMB_EDGE: u32 = 400;
pub const DISPLAY_EDGE: u32 = 2048;
/// WebP quality, 0 to 100. 80 is visually clean at about a third the size
/// of a similar JPEG.
const QUALITY: f32 = 80.0;
/// Pictures larger than this (in pixels) are refused rather than risk
/// running the machine out of memory on a damaged or hostile file.
pub(crate) const MAX_PIXELS: u64 = 400_000_000;

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("Couldn't read the picture ({0})")]
    Io(#[from] std::io::Error),
    #[error("This kind of picture can't be previewed yet")]
    Unsupported,
    #[error("The picture seems to be damaged")]
    Damaged,
    /// A video this computer has no decoder for.
    #[error("{0}")]
    CannotPlay(String),
}

/// The smaller copies of one photo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhotoRenditions {
    /// Size of the original once turned upright.
    pub width: u32,
    pub height: u32,
    pub blurhash: String,
    /// WebP bytes.
    pub thumb: Vec<u8>,
    pub display: Vec<u8>,
}

/// Makes the thumbnail, display copy and blurhash for the photo at `path`.
/// The file is only read.
pub fn render_photo(path: &Path) -> Result<PhotoRenditions, MediaError> {
    let picture = match decode_upright(path) {
        Ok(picture) => picture,
        // A camera RAW file cannot be decoded directly, but it carries a
        // finished picture inside it. A missing file stays a missing file.
        Err(MediaError::Io(e)) => return Err(MediaError::Io(e)),
        Err(e) if raw::is_raw(path) => raw::embedded_preview(path).ok_or(e)?,
        Err(e) => return Err(e),
    };
    let (width, height) = (picture.width(), picture.height());

    let display = shrink(&picture, DISPLAY_EDGE);
    // The thumbnail is made from the display copy: same result, far less
    // work than shrinking a 24-megapixel original twice.
    let thumb = shrink(&display, THUMB_EDGE);
    let tiny = thumb.resize(32, 32, FilterType::Triangle).to_rgba8();
    let blurhash =
        blurhash::encode(4, 3, tiny.width(), tiny.height(), tiny.as_raw()).map_err(|_| MediaError::Damaged)?;

    Ok(PhotoRenditions {
        width,
        height,
        blurhash,
        thumb: to_webp(&thumb)?,
        display: to_webp(&display)?,
    })
}

/// Decodes an ordinary picture file and turns it the right way up.
fn decode_upright(path: &Path) -> Result<DynamicImage, MediaError> {
    let reader = ImageReader::open(path)?.with_guessed_format()?;
    if reader.format().is_none() {
        return Err(MediaError::Unsupported);
    }
    let mut decoder = reader.into_decoder().map_err(decode_error)?;
    let (w, h) = decoder.dimensions();
    if u64::from(w) * u64::from(h) > MAX_PIXELS || w == 0 || h == 0 {
        return Err(MediaError::Damaged);
    }
    let orientation = raw::orientation_of(&mut decoder);
    let mut picture = DynamicImage::from_decoder(decoder).map_err(decode_error)?;
    picture.apply_orientation(orientation);
    Ok(picture)
}

/// Writes rendition bytes to `file`, replacing any earlier version in one
/// step so a reader never sees half a file. Unlike originals, renditions
/// can always be made again, so replacing one is safe.
pub fn write_rendition(file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = file.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let temp = dir.join(format!(".keevoro-part-{name}"));
    let mut out = std::fs::File::create(&temp)?;
    out.write_all(bytes)?;
    out.sync_all()?;
    drop(out);
    std::fs::rename(&temp, file).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

/// Fits the picture inside `edge` x `edge`, keeping its shape and never
/// making it bigger.
pub(crate) fn shrink(picture: &DynamicImage, edge: u32) -> DynamicImage {
    if picture.width().max(picture.height()) <= edge {
        picture.clone()
    } else {
        picture.resize(edge, edge, FilterType::Lanczos3)
    }
}

pub(crate) fn to_webp(picture: &DynamicImage) -> Result<Vec<u8>, MediaError> {
    // The encoder takes 8-bit RGB or RGBA; anything else is converted.
    let rgba = picture.to_rgba8();
    let has_transparency = rgba.pixels().any(|p| p.0[3] != 255);
    let bytes = if has_transparency {
        webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height())
            .encode(QUALITY)
            .to_vec()
    } else {
        let rgb = picture.to_rgb8();
        webp::Encoder::from_rgb(rgb.as_raw(), rgb.width(), rgb.height())
            .encode(QUALITY)
            .to_vec()
    };
    if bytes.is_empty() {
        return Err(MediaError::Damaged);
    }
    Ok(bytes)
}

fn decode_error(e: image::ImageError) -> MediaError {
    match e {
        image::ImageError::IoError(io) => MediaError::Io(io),
        image::ImageError::Unsupported(_) => MediaError::Unsupported,
        _ => MediaError::Damaged,
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod video_tests;
