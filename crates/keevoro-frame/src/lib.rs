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

#[cfg(target_os = "macos")]
mod macos;
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
