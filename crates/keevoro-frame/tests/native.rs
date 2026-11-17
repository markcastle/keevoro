//! Asks the real decoder on this computer for a real frame.
//!
//! `fixtures/red.mp4` is three seconds of plain red, 128 by 96, in H.264:
//! the format nearly every phone and camera records, and one both Windows
//! and macOS can play with nothing added.

#![cfg(any(windows, target_os = "macos"))]

use keevoro_frame::{still, FrameError};
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[test]
fn a_frame_comes_back_the_right_size_colour_and_way_up() {
    let frame = still(&fixture("red.mp4"), 1600).expect("this computer can play H.264");
    assert_eq!((frame.width, frame.height), (128, 96));
    assert_eq!(frame.rgba.len(), 128 * 96 * 4);
    assert!(
        matches!(frame.duration_secs, Some(2..=3)),
        "about three seconds long, got {:?}",
        frame.duration_secs
    );
    // Every corner and the middle are red: the colours are in the right
    // order and no row is padding or garbage.
    for (x, y) in [(2, 2), (125, 2), (2, 93), (125, 93), (64, 48)] {
        let at = (y * 128 + x) * 4;
        let (r, g, b, a) = (
            frame.rgba[at],
            frame.rgba[at + 1],
            frame.rgba[at + 2],
            frame.rgba[at + 3],
        );
        assert!(r > 180 && g < 90 && b < 90, "pixel {x},{y} is {r},{g},{b}");
        assert_eq!(a, 255);
    }
}

#[test]
fn a_file_that_is_not_a_video_is_refused_plainly() {
    let not_video = std::env::temp_dir().join(format!("keevoro-not-a-video-{}.mp4", std::process::id()));
    std::fs::write(&not_video, b"this is not a video").unwrap();
    let result = still(&not_video, 512);
    let _ = std::fs::remove_file(&not_video);
    assert!(matches!(result, Err(FrameError::CannotDecode(_))), "{result:?}");
}

#[test]
fn a_missing_file_is_refused_plainly() {
    assert!(matches!(
        still(&fixture("no-such-video.mp4"), 512),
        Err(FrameError::CannotDecode(_))
    ));
}

/// `fixtures/red.jpg` is a plain red JPEG, 640 by 480.
#[cfg(windows)]
#[test]
fn a_photo_is_decoded_by_the_system_at_full_size_and_shrunk_on_request() {
    let whole = keevoro_frame::photo(&fixture("red.jpg"), 0).expect("this computer can open a JPEG");
    assert_eq!((whole.width, whole.height), (640, 480));
    assert_eq!((whole.full_width, whole.full_height), (640, 480));
    assert_eq!(whole.rgba.len(), 640 * 480 * 4);

    let small = keevoro_frame::photo(&fixture("red.jpg"), 160).unwrap();
    assert_eq!(
        (small.width, small.height),
        (160, 120),
        "shrunk to fit, keeping its shape"
    );
    assert_eq!(
        (small.full_width, small.full_height),
        (640, 480),
        "and it still says how big the photo is"
    );
    for (x, y) in [(2, 2), (157, 2), (2, 117), (157, 117), (80, 60)] {
        let at = (y * 160 + x) * 4;
        let (r, g, b, a) = (
            small.rgba[at],
            small.rgba[at + 1],
            small.rgba[at + 2],
            small.rgba[at + 3],
        );
        assert!(r > 180 && g < 70 && b < 70, "pixel {x},{y} is {r},{g},{b}");
        assert_eq!(a, 255);
    }
}

#[cfg(windows)]
#[test]
fn a_file_that_is_not_a_picture_is_refused_plainly() {
    let not_picture = std::env::temp_dir().join(format!("keevoro-not-a-picture-{}.jpg", std::process::id()));
    std::fs::write(&not_picture, b"this is not a picture").unwrap();
    let result = keevoro_frame::photo(&not_picture, 512);
    let _ = std::fs::remove_file(&not_picture);
    assert!(matches!(result, Err(FrameError::CannotDecode(_))), "{result:?}");
}
