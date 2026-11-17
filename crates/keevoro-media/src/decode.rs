//! Turning a photo file into pixels, by whichever means suits the computer.
//!
//! There is more than one way to decode a photo, and which is best depends
//! on the machine: the operating system's own codecs are usually fastest
//! and open the most formats, but are not everywhere; Keevoro's built-in
//! decoder works anywhere but knows fewer formats; and a camera raw file
//! carries a finished picture inside it that neither may be needed for.
//!
//! So each way is a [`Decoder`], and a [`Decoders`] is an ordered list of
//! them. To decode a photo the list is tried from the top, and the first
//! that manages it wins. Nothing else in Keevoro knows or cares which one
//! did the work, which is what lets a new way be added (another operating
//! system, a graphics card) without touching anything that makes previews.

use crate::{raw, MediaError, MAX_PIXELS};
use image::{DynamicImage, ImageDecoder, ImageReader};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// A photo decoded and turned the right way up.
pub struct Decoded {
    pub picture: DynamicImage,
    /// The size of the photo in the file, upright, before any shrinking.
    pub full_width: u32,
    pub full_height: u32,
}

/// One way of turning a photo file into an upright picture.
pub trait Decoder: Send + Sync {
    /// What to call it in the log: "Windows codecs", "built-in".
    fn name(&self) -> &'static str;

    /// Decodes `file`, no larger than `max_edge` on its longest side where
    /// that can be done cheaply (the caller shrinks further if need be).
    fn decode(&self, file: &Path, max_edge: u32) -> Result<Decoded, MediaError>;
}

/// The operating system's own picture codecs.
pub struct SystemCodecs;

impl Decoder for SystemCodecs {
    fn name(&self) -> &'static str {
        "this computer's own codecs"
    }

    fn decode(&self, file: &Path, max_edge: u32) -> Result<Decoded, MediaError> {
        let got = keevoro_frame::photo(file, max_edge).map_err(|e| match e {
            _ if !file.is_file() => MediaError::Io(std::io::ErrorKind::NotFound.into()),
            keevoro_frame::FrameError::NotAvailable => MediaError::Unsupported,
            keevoro_frame::FrameError::CannotDecode(_) => MediaError::Unsupported,
        })?;
        if u64::from(got.full_width) * u64::from(got.full_height) > MAX_PIXELS {
            return Err(MediaError::Damaged);
        }
        let (full_width, full_height) = (got.full_width, got.full_height);
        let pixels = image::RgbaImage::from_raw(got.width, got.height, got.rgba).ok_or(MediaError::Damaged)?;
        let mut picture = DynamicImage::ImageRgba8(pixels);
        // The system hands the picture back as stored; the camera's note
        // says which way up it should be.
        let turn = raw::container_orientation(file).unwrap_or(image::metadata::Orientation::NoTransforms);
        picture.apply_orientation(turn);
        let sideways = picture.width() != got.width;
        Ok(Decoded {
            picture,
            full_width: if sideways { full_height } else { full_width },
            full_height: if sideways { full_width } else { full_height },
        })
    }
}

/// Keevoro's own decoder, which works on any computer.
pub struct BuiltIn;

impl Decoder for BuiltIn {
    fn name(&self) -> &'static str {
        "the built-in decoder"
    }

    fn decode(&self, file: &Path, _max_edge: u32) -> Result<Decoded, MediaError> {
        let reader = ImageReader::open(file)?.with_guessed_format()?;
        if reader.format().is_none() {
            return Err(MediaError::Unsupported);
        }
        let mut decoder = reader.into_decoder().map_err(crate::decode_error)?;
        let (w, h) = decoder.dimensions();
        if u64::from(w) * u64::from(h) > MAX_PIXELS || w == 0 || h == 0 {
            return Err(MediaError::Damaged);
        }
        let orientation = raw::orientation_of(&mut decoder);
        let mut picture = DynamicImage::from_decoder(decoder).map_err(crate::decode_error)?;
        picture.apply_orientation(orientation);
        Ok(Decoded {
            full_width: picture.width(),
            full_height: picture.height(),
            picture,
        })
    }
}

/// The finished picture a camera raw file carries inside it.
pub struct EmbeddedPreview;

impl Decoder for EmbeddedPreview {
    fn name(&self) -> &'static str {
        "the picture inside the raw file"
    }

    fn decode(&self, file: &Path, _max_edge: u32) -> Result<Decoded, MediaError> {
        if !raw::is_raw(file) {
            return Err(MediaError::Unsupported);
        }
        if !file.is_file() {
            return Err(MediaError::Io(std::io::ErrorKind::NotFound.into()));
        }
        let picture = raw::embedded_preview(file).ok_or(MediaError::Unsupported)?;
        Ok(Decoded {
            full_width: picture.width(),
            full_height: picture.height(),
            picture,
        })
    }
}

/// The ways of decoding a photo, in the order they are tried.
pub struct Decoders(Vec<Box<dyn Decoder>>);

impl Decoders {
    pub fn new(in_order: Vec<Box<dyn Decoder>>) -> Self {
        Self(in_order)
    }

    /// The right list for this computer. With `use_system` the operating
    /// system's codecs are tried first, where it has any Keevoro can ask;
    /// the built-in ways are always there to fall back on.
    pub fn for_this_computer(use_system: bool) -> Self {
        let mut list: Vec<Box<dyn Decoder>> = Vec::new();
        if use_system && keevoro_frame::photos_available() {
            list.push(Box::new(SystemCodecs));
        }
        list.push(Box::new(BuiltIn));
        list.push(Box::new(EmbeddedPreview));
        Self(list)
    }

    /// The names of the ways that will be tried, in order.
    pub fn names(&self) -> Vec<&'static str> {
        self.0.iter().map(|d| d.name()).collect()
    }

    /// Decodes `file` by the first way that manages it. A file that is not
    /// there stops the search at once: no other way would find it either.
    /// When every way fails, the most telling reason is given: "it is
    /// damaged" from any of them says more than "not a kind I know" from
    /// another, so the first reason that is not simply "unsupported" wins.
    pub fn decode(&self, file: &Path, max_edge: u32) -> Result<Decoded, MediaError> {
        let mut reason = None;
        for way in &self.0 {
            match way.decode(file, max_edge) {
                Ok(decoded) => return Ok(decoded),
                Err(MediaError::Io(e)) => return Err(MediaError::Io(e)),
                Err(why) => {
                    if matches!(reason, None | Some(MediaError::Unsupported)) {
                        reason = Some(why);
                    }
                }
            }
        }
        Err(reason.unwrap_or(MediaError::Unsupported))
    }
}

/// Whether the person lets Keevoro use the computer's own codecs and
/// graphics hardware. On unless they switch it off.
static USE_SYSTEM: AtomicBool = AtomicBool::new(true);

/// Switches the use of the computer's own codecs on or off, program-wide.
pub fn set_use_system(on: bool) {
    USE_SYSTEM.store(on, Ordering::SeqCst);
}

/// Does this kind of computer have system codecs Keevoro can ask?
pub fn system_codecs_available() -> bool {
    keevoro_frame::photos_available()
}

pub fn use_system() -> bool {
    USE_SYSTEM.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    /// A way that always answers the same, and counts how often it is asked.
    struct Fixed {
        name: &'static str,
        answer: fn() -> Result<Decoded, MediaError>,
        asked: Arc<AtomicUsize>,
    }

    impl Decoder for Fixed {
        fn name(&self) -> &'static str {
            self.name
        }
        fn decode(&self, _file: &Path, _max_edge: u32) -> Result<Decoded, MediaError> {
            self.asked.fetch_add(1, Ordering::SeqCst);
            (self.answer)()
        }
    }

    fn a_picture() -> Result<Decoded, MediaError> {
        Ok(Decoded {
            picture: DynamicImage::new_rgba8(4, 3),
            full_width: 4,
            full_height: 3,
        })
    }

    fn way(name: &'static str, answer: fn() -> Result<Decoded, MediaError>) -> (Box<dyn Decoder>, Arc<AtomicUsize>) {
        let asked = Arc::new(AtomicUsize::new(0));
        (
            Box::new(Fixed {
                name,
                answer,
                asked: asked.clone(),
            }),
            asked,
        )
    }

    #[test]
    fn the_first_way_that_works_is_used_and_the_rest_are_not_asked() {
        let (first, first_asked) = way("first", a_picture);
        let (second, second_asked) = way("second", a_picture);
        let decoders = Decoders::new(vec![first, second]);
        assert!(decoders.decode(Path::new("a.jpg"), 0).is_ok());
        assert_eq!(
            (first_asked.load(Ordering::SeqCst), second_asked.load(Ordering::SeqCst)),
            (1, 0)
        );
    }

    #[test]
    fn when_one_way_cannot_the_next_is_tried() {
        let (first, _) = way("first", || Err(MediaError::Unsupported));
        let (second, second_asked) = way("second", a_picture);
        let decoders = Decoders::new(vec![first, second]);
        assert_eq!(decoders.decode(Path::new("a.heic"), 0).unwrap().full_width, 4);
        assert_eq!(second_asked.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_missing_file_stops_the_search_at_once() {
        let (first, _) = way("first", || Err(MediaError::Io(std::io::ErrorKind::NotFound.into())));
        let (second, second_asked) = way("second", a_picture);
        let decoders = Decoders::new(vec![first, second]);
        assert!(matches!(
            decoders.decode(Path::new("gone.jpg"), 0),
            Err(MediaError::Io(_))
        ));
        assert_eq!(
            second_asked.load(Ordering::SeqCst),
            0,
            "no other way would find it either"
        );
    }

    #[test]
    fn when_nothing_works_the_most_telling_reason_is_given() {
        for order in [[true, false], [false, true]] {
            let ways = order
                .into_iter()
                .map(|damaged| {
                    if damaged {
                        way("knows it is damaged", || Err(MediaError::Damaged)).0
                    } else {
                        way("does not know the kind", || Err(MediaError::Unsupported)).0
                    }
                })
                .collect();
            let decoders = Decoders::new(ways);
            assert!(matches!(
                decoders.decode(Path::new("a.jpg"), 0),
                Err(MediaError::Damaged)
            ));
        }
        assert!(matches!(
            Decoders::new(Vec::new()).decode(Path::new("a.jpg"), 0),
            Err(MediaError::Unsupported)
        ));
    }

    #[test]
    fn the_built_in_ways_are_always_there_to_fall_back_on() {
        let without = Decoders::for_this_computer(false).names();
        assert_eq!(without, ["the built-in decoder", "the picture inside the raw file"]);
        let with = Decoders::for_this_computer(true).names();
        assert!(with.ends_with(&without), "{with:?}");
        assert_eq!(with.len() > without.len(), keevoro_frame::photos_available());
    }
}
