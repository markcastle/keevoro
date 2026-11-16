//! macOS: one frame through AVFoundation's image generator.
//!
//! `AVAssetImageGenerator` exists for exactly this: give it a video and a
//! time and it hands back a picture, already turned the right way up for
//! videos shot on a phone held sideways, and already shrunk if asked.
//!
//! The calls are `unsafe` because they go through Objective-C, which Rust
//! cannot check. The rules being relied on: objects are reference counted
//! by the `objc2` wrappers and released when dropped; the bitmap context
//! draws only into the buffer it was given, which is exactly
//! `width * height * 4` bytes and outlives the context.

use crate::{moment_for, Frame, FrameError};
use objc2_av_foundation::{AVAssetImageGenerator, AVURLAsset};
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGColorSpace, CGContext, CGImage, CGImageAlphaInfo};
use objc2_core_media::CMTime;
use objc2_foundation::{NSString, NSURL};
use std::ffi::c_void;
use std::path::Path;
use std::ptr::NonNull;

extern "C-unwind" {
    /// Core Graphics' own way to make a picture to draw into. Declared here
    /// because the bindings leave it out.
    fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits_per_component: usize,
        bytes_per_row: usize,
        space: Option<&CGColorSpace>,
        bitmap_info: u32,
    ) -> Option<NonNull<CGContext>>;
}

pub fn still(video: &Path, max_edge: u32) -> Result<Frame, FrameError> {
    let path = video
        .to_str()
        .ok_or_else(|| FrameError::CannotDecode("its name can't be read".into()))?;

    // SAFETY: see the module notes.
    let (image, duration_secs) = unsafe {
        let url = NSURL::fileURLWithPath(&NSString::from_str(path));
        let asset = AVURLAsset::URLAssetWithURL_options(&url, None);
        let seconds = asset.duration().seconds();
        let duration_secs = (seconds.is_finite() && seconds >= 0.0).then_some(seconds as u64);

        let generator = AVAssetImageGenerator::assetImageGeneratorWithAsset(&asset);
        // Phone videos are stored sideways with a note saying so.
        generator.setAppliesPreferredTrackTransform(true);
        if max_edge > 0 {
            let edge = f64::from(max_edge);
            generator.setMaximumSize(CGSize {
                width: edge,
                height: edge,
            });
        }
        // The generator settles on the nearest key frame by default, so
        // this is quick however long the video is.
        let moment = CMTime::with_seconds(moment_for(duration_secs), 600);
        #[allow(deprecated)]
        let image = generator
            .copyCGImageAtTime_actualTime_error(moment, std::ptr::null_mut())
            .map_err(|e| FrameError::CannotDecode(e.localizedDescription().to_string()))?;
        (image, duration_secs)
    };

    let (width, height) = (CGImage::width(Some(&image)), CGImage::height(Some(&image)));
    if width == 0 || height == 0 {
        return Err(FrameError::CannotDecode("it has no picture".into()));
    }
    let mut rgba = vec![0u8; width * height * 4];
    let space = CGColorSpace::new_device_rgb().ok_or_else(|| FrameError::CannotDecode("no colour space".into()))?;
    // SAFETY: `rgba` is exactly width * height * 4 bytes, which is what
    // eight bits per colour and width * 4 bytes per row describe, and it
    // outlives `context`, which is dropped at the end of this block.
    unsafe {
        let context = CGBitmapContextCreate(
            rgba.as_mut_ptr().cast(),
            width,
            height,
            8,
            width * 4,
            Some(&space),
            // Red, green, blue, then a fourth byte: the order Keevoro uses.
            CGImageAlphaInfo::PremultipliedLast.0,
        )
        .map(|raw| CFRetained::from_raw(raw))
        .ok_or_else(|| FrameError::CannotDecode("the picture couldn't be drawn".into()))?;
        let whole = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: CGSize {
                width: width as f64,
                height: height as f64,
            },
        };
        CGContext::draw_image(Some(&context), whole, Some(&image));
    }

    Ok(Frame {
        width: u32::try_from(width).map_err(|_| FrameError::CannotDecode("it is too large".into()))?,
        height: u32::try_from(height).map_err(|_| FrameError::CannotDecode("it is too large".into()))?,
        rgba,
        duration_secs,
    })
}
