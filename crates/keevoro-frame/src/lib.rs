//! One still picture from a video, without any extra software.
//!
//! Windows and macOS can both already play video, so they can both already
//! decode it. This crate asks the operating system for a single frame
//! (Media Foundation on Windows, AVFoundation on a Mac) so that Keevoro
//! can show a thumbnail for a video without the person having to install
//! anything, and without starting another program.
//!
//! What can be decoded is whatever that computer can play. A format it
//! cannot play gives [`FrameError::CannotDecode`], and the caller shows a
//! plain placeholder (or tries another decoder if it has one).
//!
//! The frame is taken from a little way into the video, not the very
//! start, because cameras often record a black or blurred first frame. The
//! decoder is asked to jump there, which lands on the nearest key frame, so
//! a two-hour video costs the same as a ten-second one.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use std::path::Path;

pub mod drives;
#[cfg(target_os = "macos")]
mod macos;
pub mod network;
#[cfg(windows)]
mod photo_windows;
pub mod quiet;
#[cfg(windows)]
mod windows;

/// One decoded picture: `width * height` pixels, four bytes each, in red,
/// green, blue, alpha order, top row first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// How long the video runs, when the decoder could say.
    pub duration_secs: Option<u64>,
}

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    /// This kind of computer has no built-in decoder Keevoro knows how to ask.
    #[error("This computer has no built-in way to read videos")]
    NotAvailable,
    /// The operating system could not open or decode this particular video.
    #[error("This computer can't play this video ({0})")]
    CannotDecode(String),
}

/// How far into a video to take the picture from: a tenth of the way in,
/// but never more than ten seconds, and the very start when the length is
/// not known.
pub fn moment_for(duration_secs: Option<u64>) -> f64 {
    match duration_secs {
        Some(secs) => (secs as f64 / 10.0).min(10.0),
        None => 0.0,
    }
}

/// Takes one frame from a video, no wider or taller than `max_edge` where
/// the decoder can shrink it cheaply (the caller should still be ready to
/// shrink it). The file is only read.
pub fn still(video: &Path, max_edge: u32) -> Result<Frame, FrameError> {
    #[cfg(windows)]
    {
        windows::still(video, max_edge)
    }
    #[cfg(target_os = "macos")]
    {
        macos::still(video, max_edge)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (video, max_edge);
        Err(FrameError::NotAvailable)
    }
}

/// One decoded photo: `width * height` pixels, four bytes each, in red,
/// green, blue, alpha order, top row first, exactly as stored in the file.
/// It is not turned upright: a photo taken with the camera on its side is
/// still on its side here, and the caller turns it using the note the
/// camera left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    /// The size of the photo in the file, before any shrinking.
    pub full_width: u32,
    pub full_height: u32,
    pub rgba: Vec<u8>,
}

/// The size that fits a `width` by `height` picture inside a square of
/// `max_edge`, keeping its shape and never making it bigger. A `max_edge`
/// of zero means "as it is".
pub fn fit_within(width: u32, height: u32, max_edge: u32) -> (u32, u32) {
    let longest = width.max(height);
    if max_edge == 0 || longest <= max_edge {
        return (width, height);
    }
    let scale =
        |side: u32| ((u64::from(side) * u64::from(max_edge) + u64::from(longest) / 2) / u64::from(longest)).max(1);
    (scale(width) as u32, scale(height) as u32)
}

/// Decodes a photo with the operating system's own picture codecs, shrunk
/// to fit `max_edge` where the codec can do that cheaply. The file is only
/// read.
pub fn photo(file: &Path, max_edge: u32) -> Result<Picture, FrameError> {
    #[cfg(windows)]
    {
        photo_windows::photo(file, max_edge)
    }
    #[cfg(not(windows))]
    {
        let _ = (file, max_edge);
        Err(FrameError::NotAvailable)
    }
}

/// Can photos be decoded by the operating system on this kind of computer?
pub const fn photos_available() -> bool {
    cfg!(windows)
}

/// Is there a built-in decoder to ask on this kind of computer?
pub const fn available() -> bool {
    cfg!(any(windows, target_os = "macos"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_picture_is_taken_a_little_way_in() {
        assert_eq!(moment_for(None), 0.0);
        assert_eq!(moment_for(Some(0)), 0.0);
        assert_eq!(moment_for(Some(30)), 3.0);
        assert_eq!(moment_for(Some(7200)), 10.0, "never far into a long video");
    }

    #[test]
    fn a_picture_is_fitted_without_being_stretched_or_enlarged() {
        assert_eq!(fit_within(4000, 3000, 2048), (2048, 1536));
        assert_eq!(fit_within(3000, 4000, 2048), (1536, 2048));
        assert_eq!(fit_within(800, 600, 2048), (800, 600), "never made bigger");
        assert_eq!(fit_within(8000, 6000, 0), (8000, 6000), "zero means as it is");
        assert_eq!(fit_within(10_000, 3, 100), (100, 1), "never squeezed to nothing");
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn other_computers_say_so_plainly() {
        assert!(!available());
        assert!(matches!(
            still(Path::new("holiday.mp4"), 512),
            Err(FrameError::NotAvailable)
        ));
    }
}
