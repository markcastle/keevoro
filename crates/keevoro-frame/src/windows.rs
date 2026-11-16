//! Windows: one frame through Media Foundation's source reader.
//!
//! This follows Microsoft's own "VideoThumbnail" sample. The source reader
//! opens the file, is told to hand back plain 32-bit pictures (it inserts
//! whatever decoder and colour conversion that needs), is moved to the
//! moment wanted, and is asked for one sample.
//!
//! Every call into Windows here is `unsafe` because Rust cannot check what
//! Windows does with the pointers it is given. The rules being relied on:
//! COM objects are reference counted by the `windows` crate's wrappers and
//! released when dropped; a locked buffer is only read between `Lock` and
//! `Unlock`, and only for the length `Lock` reported.

use crate::{moment_for, Frame, FrameError};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows::core::{GUID, PCWSTR};
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFMediaType, IMFSample, IMFSourceReader, MFCreateAttributes, MFCreateMediaType,
    MFCreateSourceReaderFromURL, MFMediaType_Video, MFShutdown, MFStartup, MFVideoFormat_RGB32, MFSTARTUP_LITE,
    MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_PD_DURATION,
    MF_SOURCE_READERF_ENDOFSTREAM, MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
    MF_SOURCE_READER_MEDIASOURCE, MF_VERSION,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

/// Media Foundation counts time in units of 100 nanoseconds.
const TICKS_PER_SECOND: f64 = 10_000_000.0;

fn cannot(what: &str, e: &windows::core::Error) -> FrameError {
    FrameError::CannotDecode(format!("{what}: {}", e.message()))
}

/// Media Foundation must be started on a thread before use and stopped
/// after. This does both, however the work in between ends.
struct Started {
    com: bool,
}

impl Started {
    fn new() -> Result<Self, FrameError> {
        // SAFETY: plain start-up calls with no pointers of ours. COM may
        // already be started on this thread in another mode; that is fine
        // for our purposes, and then it is not ours to stop.
        unsafe {
            let com = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            if let Err(e) = MFStartup(MF_VERSION, MFSTARTUP_LITE) {
                if com {
                    CoUninitialize();
                }
                return Err(cannot("Windows' video support would not start", &e));
            }
            Ok(Self { com })
        }
    }
}

impl Drop for Started {
    fn drop(&mut self) {
        // SAFETY: matches the successful start-up calls in `new`.
        unsafe {
            let _ = MFShutdown();
            if self.com {
                CoUninitialize();
            }
        }
    }
}

pub fn still(video: &Path, _max_edge: u32) -> Result<Frame, FrameError> {
    let _started = Started::new()?;
    // Windows wants the path as 16-bit characters ending in a zero.
    let wide: Vec<u16> = video.as_os_str().encode_wide().chain([0]).collect();

    // SAFETY: see the module notes. `wide` outlives the call that reads it,
    // and every out-pointer is a local that Windows fills in.
    unsafe {
        let mut attributes: Option<IMFAttributes> = None;
        MFCreateAttributes(&mut attributes, 1).map_err(|e| cannot("setting up", &e))?;
        let attributes = attributes.ok_or_else(|| FrameError::CannotDecode("setting up".into()))?;
        // Lets the reader convert whatever the video is into plain pictures.
        attributes
            .SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)
            .map_err(|e| cannot("setting up", &e))?;

        let reader: IMFSourceReader =
            MFCreateSourceReaderFromURL(PCWSTR(wide.as_ptr()), &attributes).map_err(|e| cannot("opening it", &e))?;
        let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

        let wanted: IMFMediaType = MFCreateMediaType().map_err(|e| cannot("setting up", &e))?;
        wanted
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(|e| cannot("setting up", &e))?;
        wanted
            .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)
            .map_err(|e| cannot("setting up", &e))?;
        reader
            .SetCurrentMediaType(stream, None, &wanted)
            .map_err(|e| cannot("no decoder for this kind of video", &e))?;

        let actual = reader
            .GetCurrentMediaType(stream)
            .map_err(|e| cannot("reading its size", &e))?;
        let size = actual
            .GetUINT64(&MF_MT_FRAME_SIZE)
            .map_err(|e| cannot("reading its size", &e))?;
        let (width, height) = ((size >> 32) as u32, size as u32);
        if width == 0 || height == 0 {
            return Err(FrameError::CannotDecode("it has no picture".into()));
        }
        // A negative stride means the rows are stored bottom row first.
        let stride = actual
            .GetUINT32(&MF_MT_DEFAULT_STRIDE)
            .map_or(width as i32 * 4, |s| s as i32);

        let duration_secs = reader
            .GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)
            .ok()
            .and_then(|length| u64::try_from(&length).ok())
            .map(|ticks| (ticks as f64 / TICKS_PER_SECOND) as u64);

        // Jump a little way in. This lands on a key frame, so it is quick
        // however long the video is. A video that cannot be moved through
        // simply gives its first frame.
        let moment = (moment_for(duration_secs) * TICKS_PER_SECOND) as i64;
        if moment > 0 {
            let _ = reader.SetCurrentPosition(&GUID::zeroed(), &PROPVARIANT::from(moment));
        }

        // The first few reads can come back empty while the decoder warms up.
        let mut sample: Option<IMFSample> = None;
        for _ in 0..120 {
            let mut flags = 0u32;
            reader
                .ReadSample(stream, 0, None, Some(&mut flags), None, Some(&mut sample))
                .map_err(|e| cannot("decoding it", &e))?;
            if sample.is_some() || flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                break;
            }
        }
        let sample = sample.ok_or_else(|| FrameError::CannotDecode("it has no picture".into()))?;
        let buffer = sample
            .ConvertToContiguousBuffer()
            .map_err(|e| cannot("decoding it", &e))?;

        let mut data = std::ptr::null_mut();
        let mut length = 0u32;
        buffer
            .Lock(&mut data, None, Some(&mut length))
            .map_err(|e| cannot("decoding it", &e))?;
        // Copy out while locked, then unlock whatever happened.
        let pixels = std::slice::from_raw_parts(data, length as usize);
        let rgba = to_rgba(pixels, width, height, stride);
        let _ = buffer.Unlock();

        let rgba = rgba.ok_or_else(|| FrameError::CannotDecode("the picture came back the wrong size".into()))?;
        Ok(Frame {
            width,
            height,
            rgba,
            duration_secs,
        })
    }
}

/// Windows hands back blue, green, red, unused, possibly bottom row first
/// and possibly with padding at the end of each row. This makes it red,
/// green, blue, alpha, top row first, with no padding.
fn to_rgba(pixels: &[u8], width: u32, height: u32, stride: i32) -> Option<Vec<u8>> {
    let (width, height) = (width as usize, height as usize);
    let row_bytes = stride.unsigned_abs() as usize;
    if row_bytes < width * 4 || pixels.len() < row_bytes * height {
        return None;
    }
    let mut rgba = Vec::with_capacity(width * height * 4);
    for row in 0..height {
        let from = if stride < 0 { height - 1 - row } else { row };
        let line = &pixels[from * row_bytes..from * row_bytes + width * 4];
        for pixel in line.chunks_exact(4) {
            rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
        }
    }
    Some(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_are_put_in_the_usual_order_and_rows_the_right_way_up() {
        // Two rows of one pixel each, with two bytes of padding per row.
        let blue_then_red = [255, 0, 0, 0, 9, 9, 0, 0, 255, 0, 9, 9];
        assert_eq!(
            to_rgba(&blue_then_red, 1, 2, 6).unwrap(),
            [0, 0, 255, 255, 255, 0, 0, 255]
        );
        // The same bytes stored bottom row first.
        assert_eq!(
            to_rgba(&blue_then_red, 1, 2, -6).unwrap(),
            [255, 0, 0, 255, 0, 0, 255, 255]
        );
        assert_eq!(to_rgba(&blue_then_red, 1, 3, 6), None, "too few bytes is refused");
    }
}
