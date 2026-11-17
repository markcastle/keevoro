//! Previews for camera RAW files.
//!
//! A RAW file (`.CR2`, `.NEF`, `.ARW`, `.DNG` and many more) holds the
//! sensor's unprocessed data, in a format that differs between camera
//! makers and often between models. Decoding that properly is a large job.
//!
//! Fortunately every camera also tucks a finished JPEG inside the RAW file,
//! the same picture it shows on its own screen, usually at full or near
//! full size. Keevoro's preview is made from that. The RAW file itself is
//! kept untouched as the original; only the thumbnail and display copy come
//! from the embedded picture.
//!
//! Rather than understand each maker's layout, this looks through the file
//! for anything that starts like a JPEG, asks each candidate how big it
//! claims to be, and decodes the biggest one that really is a picture. That
//! is format-agnostic and has no per-camera code to go stale. A file may
//! hold several (a tiny thumbnail, a medium preview, the full one), and some
//! RAW formats store the sensor data itself in a JPEG variant that cannot
//! be decoded as a picture, which is why candidates are tried in turn.

use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use std::io::{BufReader, Cursor};
use std::path::Path;

/// File extensions treated as RAW. Lowercase, without the dot.
const EXTENSIONS: &[&str] = &[
    "dng", "cr2", "cr3", "crw", "nef", "nrw", "arw", "srf", "sr2", "orf", "rw2", "raf", "pef", "srw", "x3f", "3fr",
    "erf", "kdc", "mrw", "raw",
];
/// RAW files are tens of megabytes. Anything far beyond that is not read
/// into memory to be searched.
const MAX_FILE_BYTES: u64 = 400 * 1024 * 1024;
/// How many embedded pictures to consider, and how many to try decoding.
const MAX_CANDIDATES: usize = 64;
const MAX_DECODE_TRIES: usize = 4;

/// Is this file named like a camera RAW file?
pub fn is_raw(path: &Path) -> bool {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|e| EXTENSIONS.contains(&e.as_str()))
}

/// The largest picture embedded in a RAW file, turned upright. `None` when
/// the file is too big, unreadable, or holds no usable picture.
pub fn embedded_preview(path: &Path) -> Option<DynamicImage> {
    if std::fs::metadata(path).ok()?.len() > MAX_FILE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let mut picture = largest_jpeg(&bytes)?;
    // The camera records which way up it was held in the RAW file's own
    // tags; the embedded picture is usually stored unturned.
    if let Some(orientation) = container_orientation(path) {
        picture.apply_orientation(orientation);
    }
    Some(picture)
}

/// Finds every place a JPEG might start and decodes the biggest real one.
fn largest_jpeg(bytes: &[u8]) -> Option<DynamicImage> {
    let mut candidates: Vec<(u64, usize)> = bytes
        .windows(3)
        .enumerate()
        .filter(|(_, w)| w == &[0xFF, 0xD8, 0xFF])
        .take(MAX_CANDIDATES)
        .filter_map(|(at, _)| {
            let (w, h) = ImageReader::with_format(Cursor::new(&bytes[at..]), ImageFormat::Jpeg)
                .into_dimensions()
                .ok()?;
            Some((u64::from(w) * u64::from(h), at))
        })
        .filter(|(pixels, _)| *pixels > 0 && *pixels <= crate::MAX_PIXELS)
        .collect();
    candidates.sort_by_key(|c| std::cmp::Reverse(c.0));
    candidates
        .into_iter()
        .take(MAX_DECODE_TRIES)
        .find_map(|(_, at)| image::load_from_memory_with_format(&bytes[at..], ImageFormat::Jpeg).ok())
}

pub(crate) fn container_orientation(path: &Path) -> Option<image::metadata::Orientation> {
    let file = std::fs::File::open(path).ok()?;
    let tags = exif::Reader::new()
        .read_from_container(&mut BufReader::new(file))
        .ok()?;
    let value = tags
        .get_field(exif::Tag::Orientation, exif::In::PRIMARY)?
        .value
        .get_uint(0)?;
    image::metadata::Orientation::from_exif(u8::try_from(value).ok()?)
}

/// The orientation a decoder reports, for callers that already have one.
pub(crate) fn orientation_of(decoder: &mut impl ImageDecoder) -> image::metadata::Orientation {
    decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms)
}
