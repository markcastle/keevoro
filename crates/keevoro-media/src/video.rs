//! Posters for videos, and converting old videos to MP4.
//!
//! Old camcorder and phone videos (AVI, WMV, MPG, 3GP) often will not play
//! on a modern phone or in a browser. Keevoro can make an MP4 copy that
//! plays everywhere, and for every video it makes a poster: one still frame
//! for the gallery to show.
//!
//! The work is done by ffmpeg, run as a separate program. Keevoro does not
//! contain ffmpeg; it uses the copy found on the computer. That keeps the
//! door open on licensing (ffmpeg's best encoder is GPL) and means a
//! computer without ffmpeg still gathers and backs up videos perfectly
//! well, just without posters or conversion.
//!
//! What matters here:
//!
//! - **The original is never touched or replaced.** The MP4 is an extra
//!   copy under `renditions/video/`.
//! - **Repackage before re-encoding.** Many camcorder files already hold
//!   H.264 video and AAC sound inside an old wrapper. Those are moved into
//!   an MP4 wrapper untouched, which is lossless and takes seconds.
//! - **Every conversion is checked.** The result must be the same length
//!   as the original to within half a second and must play through without
//!   errors, or it is thrown away.
//! - **Deciding what to do is separate from doing it.** [`plan`] is a pure
//!   function over what [`probe`] found, so the rules are tested without
//!   needing ffmpeg at all.

use crate::{shrink, to_webp, MediaError, DISPLAY_EDGE};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// A converted video may differ from its original by this much and no more.
pub const DURATION_TOLERANCE_SECS: f64 = 0.5;

/// Where ffmpeg and its companion ffprobe live on this computer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoTools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

impl VideoTools {
    /// Looks for the tools beside the keevoro program first (where a future
    /// installer will put them), then on the PATH. `None` when either is
    /// missing or will not run.
    pub fn find() -> Option<Self> {
        let beside = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf));
        let ext = std::env::consts::EXE_SUFFIX;
        if let Some(dir) = beside {
            let here = Self {
                ffmpeg: dir.join(format!("ffmpeg{ext}")),
                ffprobe: dir.join(format!("ffprobe{ext}")),
            };
            if here.works() {
                return Some(here);
            }
        }
        let on_path = Self {
            ffmpeg: PathBuf::from("ffmpeg"),
            ffprobe: PathBuf::from("ffprobe"),
        };
        on_path.works().then_some(on_path)
    }

    fn works(&self) -> bool {
        [&self.ffmpeg, &self.ffprobe].iter().all(|tool| {
            Command::new(tool)
                .arg("-version")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        })
    }
}

/// What ffprobe says about a video.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoInfo {
    pub duration_secs: f64,
    /// Size as it should be shown, after any rotation flag.
    pub width: u32,
    pub height: u32,
    /// ffmpeg's names: `h264`, `mpeg4`, `wmv3`, `aac`, `mp3`, ...
    pub video_codec: String,
    pub audio_codec: Option<String>,
    pub interlaced: bool,
    /// The wrapper, as ffmpeg names it: `mov,mp4,m4a,3gp,3g2,mj2`, `avi`,
    /// `mpegts`, `asf`, ...
    pub container: String,
}

/// What to do with a video when conversion is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conversion {
    /// It already plays everywhere.
    Leave,
    /// Move the existing video and sound into an MP4 wrapper, untouched.
    Remux,
    /// Re-encode to H.264 and AAC.
    Reencode { deinterlace: bool },
}

/// Decides what a video needs. `extension` is the file's own, without the
/// dot; it tells a real `.mp4` from a `.mov`, which ffmpeg reports as the
/// same family of container.
pub fn plan(info: &VideoInfo, extension: &str) -> Conversion {
    let video = info.video_codec.as_str();
    let sound_is_fine = matches!(info.audio_codec.as_deref(), None | Some("aac"));
    let is_mp4_file = matches!(extension.to_ascii_lowercase().as_str(), "mp4" | "m4v");
    // A modern MP4 is left alone even with HEVC video: phones made it and
    // phones play it, and re-encoding would only lose quality.
    if is_mp4_file && matches!(video, "h264" | "hevc") {
        return Conversion::Leave;
    }
    // Interlaced footage (camcorders, DVDs) shows comb-like stripes on
    // modern screens unless it is deinterlaced, which means re-encoding.
    if info.interlaced {
        return Conversion::Reencode { deinterlace: true };
    }
    if video == "h264" && sound_is_fine {
        return Conversion::Remux;
    }
    Conversion::Reencode { deinterlace: false }
}

/// Reads ffprobe's JSON output. Separate from running ffprobe so it can be
/// tested with saved output.
pub fn parse_probe(json: &str) -> Result<VideoInfo, MediaError> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|_| MediaError::Damaged)?;
    let streams = v.get("streams").and_then(|s| s.as_array()).ok_or(MediaError::Damaged)?;
    let kind = |s: &serde_json::Value| s.get("codec_type").and_then(|t| t.as_str()).map(str::to_string);
    let text = |s: &serde_json::Value, key: &str| s.get(key).and_then(|x| x.as_str()).map(str::to_string);
    let number = |s: &serde_json::Value, key: &str| s.get(key).and_then(serde_json::Value::as_u64);

    // Cover art in a music-style file is a "video" stream of one picture;
    // it is not the video.
    let video = streams
        .iter()
        .find(|s| {
            kind(s).as_deref() == Some("video")
                && s.pointer("/disposition/attached_pic")
                    .and_then(serde_json::Value::as_u64)
                    != Some(1)
        })
        .ok_or(MediaError::Unsupported)?;
    let audio = streams.iter().find(|s| kind(s).as_deref() == Some("audio"));

    let seconds = |s: Option<String>| {
        s.and_then(|d| d.parse::<f64>().ok())
            .filter(|d| d.is_finite() && *d > 0.0)
    };
    let duration_secs = seconds(
        v.pointer("/format/duration")
            .and_then(|d| d.as_str())
            .map(str::to_string),
    )
    .or_else(|| seconds(text(video, "duration")))
    .ok_or(MediaError::Damaged)?;

    let mut width = number(video, "width")
        .and_then(|n| u32::try_from(n).ok())
        .ok_or(MediaError::Damaged)?;
    let mut height = number(video, "height")
        .and_then(|n| u32::try_from(n).ok())
        .ok_or(MediaError::Damaged)?;
    // Phones record sideways and add a note saying how to turn the picture.
    let rotation = video
        .get("side_data_list")
        .and_then(|l| l.as_array())
        .and_then(|l| {
            l.iter()
                .find_map(|d| d.get("rotation").and_then(serde_json::Value::as_i64))
        })
        .or_else(|| {
            video
                .pointer("/tags/rotate")
                .and_then(|r| r.as_str())
                .and_then(|r| r.parse().ok())
        })
        .unwrap_or(0);
    if rotation.rem_euclid(180) == 90 {
        std::mem::swap(&mut width, &mut height);
    }

    let field_order = text(video, "field_order").unwrap_or_default();
    Ok(VideoInfo {
        duration_secs,
        width,
        height,
        video_codec: text(video, "codec_name").ok_or(MediaError::Damaged)?,
        audio_codec: audio.and_then(|a| text(a, "codec_name")),
        interlaced: matches!(field_order.as_str(), "tt" | "bb" | "tb" | "bt"),
        container: v
            .pointer("/format/format_name")
            .and_then(|f| f.as_str())
            .unwrap_or_default()
            .to_string(),
    })
}

/// Asks ffprobe about a video. The file is only read.
pub fn probe(tools: &VideoTools, video: &Path) -> Result<VideoInfo, MediaError> {
    let out = Command::new(&tools.ffprobe)
        .args(["-v", "error", "-print_format", "json", "-show_format", "-show_streams"])
        .arg(video)
        .stdin(Stdio::null())
        .output()?;
    if !out.status.success() {
        return Err(if video.is_file() {
            MediaError::Damaged
        } else {
            MediaError::Io(std::io::ErrorKind::NotFound.into())
        });
    }
    parse_probe(&String::from_utf8_lossy(&out.stdout))
}

/// Makes a poster for a video: one representative frame, as WebP, no wider
/// or taller than a display copy. ffmpeg's `thumbnail` filter picks a frame
/// that is typical of the opening, which avoids the black first frame many
/// camcorders record.
pub fn poster(tools: &VideoTools, video: &Path) -> Result<Vec<u8>, MediaError> {
    let out = Command::new(&tools.ffmpeg)
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(video)
        .args(["-map", "0:v:0", "-vf", "thumbnail", "-frames:v", "1"])
        .args(["-f", "image2pipe", "-c:v", "png", "pipe:1"])
        .stdin(Stdio::null())
        .output()?;
    if !out.status.success() || out.stdout.is_empty() {
        return Err(MediaError::Damaged);
    }
    let frame =
        image::load_from_memory_with_format(&out.stdout, image::ImageFormat::Png).map_err(|_| MediaError::Damaged)?;
    to_webp(&shrink(&frame, DISPLAY_EDGE))
}

/// The ffmpeg arguments, between input and output, for a conversion.
/// `None` when there is nothing to do.
pub fn conversion_args(action: Conversion) -> Option<Vec<&'static str>> {
    // Video first, the first sound track if there is one, and the original's
    // own tags (which carry the recording date). `faststart` puts the index
    // at the front so playback begins before the whole file has arrived.
    let common = [
        "-map",
        "0:v:0",
        "-map",
        "0:a:0?",
        "-map_metadata",
        "0",
        "-movflags",
        "+faststart",
    ];
    let mut args: Vec<&'static str> = common.to_vec();
    match action {
        Conversion::Leave => return None,
        Conversion::Remux => args.extend(["-c", "copy"]),
        Conversion::Reencode { deinterlace } => {
            // H.264 needs even dimensions; some old formats have odd ones.
            args.extend([
                "-vf",
                if deinterlace {
                    "yadif,scale=trunc(iw/2)*2:trunc(ih/2)*2"
                } else {
                    "scale=trunc(iw/2)*2:trunc(ih/2)*2"
                },
            ]);
            // yuv420p is the one pixel format every player accepts. CRF 20
            // is visually clean for home video without huge files.
            args.extend([
                "-c:v", "libx264", "-preset", "medium", "-crf", "20", "-pix_fmt", "yuv420p",
            ]);
            args.extend(["-c:a", "aac", "-b:a", "160k"]);
        }
    }
    Some(args)
}

/// Converts `video` to an MP4 at `dest` and proves the result is good.
/// On any failure nothing is left at `dest`. `info` is the original's
/// [`probe`] result.
pub fn convert(
    tools: &VideoTools,
    video: &Path,
    info: &VideoInfo,
    action: Conversion,
    dest: &Path,
) -> Result<(), MediaError> {
    let Some(args) = conversion_args(action) else {
        return Ok(());
    };
    let dir = dest.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = dest.file_name().unwrap_or(OsStr::new("video.mp4")).to_string_lossy();
    // Hidden while it is being written, and still ending in .mp4 so ffmpeg
    // knows what to make.
    let temp = dir.join(format!(".keevoro-part-{name}"));
    let _ = std::fs::remove_file(&temp);

    let made = Command::new(&tools.ffmpeg)
        .args(["-v", "error", "-nostdin", "-y", "-i"])
        .arg(video)
        .args(&args)
        .args(["-f", "mp4"])
        .arg(&temp)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let checked = match made {
        Ok(status) if status.success() => check_conversion(tools, info, &temp),
        Ok(_) => Err(MediaError::Damaged),
        Err(e) => Err(MediaError::Io(e)),
    };
    let placed = checked.and_then(|()| std::fs::rename(&temp, dest).map_err(MediaError::Io));
    if placed.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    placed
}

/// The converted file must be as long as the original and must play
/// through to the end without a single decoding error.
fn check_conversion(tools: &VideoTools, original: &VideoInfo, converted: &Path) -> Result<(), MediaError> {
    let result = probe(tools, converted)?;
    if (result.duration_secs - original.duration_secs).abs() > DURATION_TOLERANCE_SECS {
        return Err(MediaError::Damaged);
    }
    let played = Command::new(&tools.ffmpeg)
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(converted)
        .args(["-f", "null", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .output()?;
    if played.status.success() && played.stderr.is_empty() {
        Ok(())
    } else {
        Err(MediaError::Damaged)
    }
}
