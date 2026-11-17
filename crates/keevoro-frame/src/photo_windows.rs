//! Windows: a photo through the Windows Imaging Component.
//!
//! WIC is the picture decoder Windows itself uses (Explorer, Photos). It
//! is fast: it uses the processor's vector instructions, shrinks a JPEG
//! while decoding it (so a thumbnail of a 48-megapixel photo never builds
//! the full picture in memory), and hands work to the graphics hardware
//! where a codec supports that. It also opens whatever formats that
//! computer has codecs for, such as HEIC and camera raw files, which
//! Keevoro's built-in decoder cannot.
//!
//! The same rules as `windows.rs` are relied on: COM objects are reference
//! counted and released when dropped, and pixels are copied into a buffer
//! of exactly the size asked for.

use crate::{FrameError, Picture};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::Foundation::GENERIC_READ;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppRGBA, IWICImagingFactory, WICBitmapDitherTypeNone,
    WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};

fn cannot(what: &str, e: &windows::core::Error) -> FrameError {
    FrameError::CannotDecode(format!("{what}: {}", e.message()))
}

/// COM must be started on a thread before WIC is used, and stopped after
/// if it was this code that started it.
struct Com(bool);

impl Com {
    fn start() -> Self {
        // SAFETY: a plain start-up call. If COM is already started on this
        // thread in another mode that is fine, and then it is not ours to stop.
        Self(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() })
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: matches the successful start in `start`.
            unsafe { CoUninitialize() };
        }
    }
}

pub fn photo(file: &Path, max_edge: u32) -> Result<Picture, FrameError> {
    let _com = Com::start();
    let wide: Vec<u16> = file.as_os_str().encode_wide().chain([0]).collect();

    // SAFETY: see the module notes. `wide` outlives the call that reads it,
    // and `rgba` is exactly the stride times the height that is asked for.
    unsafe {
        let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| cannot("Windows' picture support would not start", &e))?;
        let decoder = factory
            .CreateDecoderFromFilename(
                PCWSTR(wide.as_ptr()),
                None,
                GENERIC_READ,
                WICDecodeMetadataCacheOnDemand,
            )
            .map_err(|e| cannot("no decoder for this kind of picture", &e))?;
        let frame = decoder.GetFrame(0).map_err(|e| cannot("opening it", &e))?;
        let (mut full_width, mut full_height) = (0u32, 0u32);
        frame
            .GetSize(&mut full_width, &mut full_height)
            .map_err(|e| cannot("reading its size", &e))?;
        if full_width == 0 || full_height == 0 {
            return Err(FrameError::CannotDecode("it has no picture".into()));
        }
        let (width, height) = crate::fit_within(full_width, full_height, max_edge);

        // The scaler asks the decoder to shrink while decoding where it can.
        let scaler = factory.CreateBitmapScaler().map_err(|e| cannot("shrinking it", &e))?;
        scaler
            .Initialize(&frame, width, height, WICBitmapInterpolationModeFant)
            .map_err(|e| cannot("shrinking it", &e))?;
        let converter = factory
            .CreateFormatConverter()
            .map_err(|e| cannot("reading its colours", &e))?;
        converter
            .Initialize(
                &scaler,
                &GUID_WICPixelFormat32bppRGBA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .map_err(|e| cannot("reading its colours", &e))?;

        let stride = width
            .checked_mul(4)
            .ok_or_else(|| FrameError::CannotDecode("it is too large".into()))?;
        let mut rgba = vec![0u8; stride as usize * height as usize];
        converter
            .CopyPixels(std::ptr::null(), stride, &mut rgba)
            .map_err(|e| cannot("decoding it", &e))?;
        Ok(Picture {
            width,
            height,
            full_width,
            full_height,
            rgba,
        })
    }
}
