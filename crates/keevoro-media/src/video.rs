//! Posters for videos, and what ffprobe can say about one.
//!
//! For every video Keevoro makes a poster: one still frame for the gallery
//! to show. The computer's own decoder is asked first (`keevoro-frame`);
//! ffmpeg, if it happens to be on the computer, is a second try, and its
//! companion ffprobe gives a video's length and size. Keevoro does not
//! contain ffmpeg, so a computer without it still finds and backs up videos
//! perfectly well.
//!
//! Making MP4 copies of old videos was here once, for the library model
//! that was removed in 0.30. Git history has it, should it come back as a
//! feature (TASKS.md, "Old video conversion prompt").

use crate::{shrink, to_webp, MediaError, DISPLAY_EDGE};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Starts building a command for one of the video tools, set up so that it
/// never shows a window of its own.
///
/// ffmpeg is a console program. On Windows, a console program started by an
/// app that has no console gets a new black window each time, so making
/// thumbnails for a screen of videos would flash dozens of them, which
/// looks alarming. The flag below asks Windows not to make one.
fn quiet(tool: impl AsRef<OsStr>) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut command = Command::new(tool);
        // CREATE_NO_WINDOW
        command.creation_flags(0x0800_0000);
        command
    }
    #[cfg(not(windows))]
    Command::new(tool)
}

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
    ///
    /// The answer is worked out once and remembered. Finding the tools
    /// means running them, and doing that for every thumbnail would start
    /// several programs per picture.
    pub fn find() -> Option<Self> {
        static FOUND: std::sync::OnceLock<Option<VideoTools>> = std::sync::OnceLock::new();
        FOUND.get_or_init(Self::look).clone()
    }

    fn look() -> Option<Self> {
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
            quiet(tool)
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
    let out = quiet(&tools.ffprobe)
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
    let out = quiet(&tools.ffmpeg)
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

/// A poster made for a video, with what was learnt about the video on the way.
#[derive(Debug, Clone, PartialEq)]
pub struct Still {
    /// The poster, as WebP, no wider or taller than a display copy.
    pub webp: Vec<u8>,
    /// How long the video runs, when it could be told.
    pub duration_secs: Option<f64>,
    /// The size of the frame the poster was made from.
    pub width: u32,
    pub height: u32,
}

/// Can this computer make a poster for a video at all: by its own built-in
/// decoder, or failing that with ffmpeg?
pub fn can_make_posters(tools: Option<&VideoTools>) -> bool {
    keevoro_frame::available() || tools.is_some()
}

/// Makes a poster for a video.
///
/// The computer's own video decoder is asked first: it needs nothing
/// installed and starts no other program. ffmpeg, if it happens to be
/// there, is only a second try for the formats the computer cannot play
/// (and the only way on computers with no decoder of their own).
pub fn still(tools: Option<&VideoTools>, video: &Path) -> Result<Still, MediaError> {
    let native = if keevoro_frame::available() {
        match keevoro_frame::still(video, DISPLAY_EDGE) {
            Ok(frame) => return still_from_frame(frame),
            Err(e) => Some(e.to_string()),
        }
    } else {
        None
    };
    match tools {
        Some(tools) => {
            let webp = poster(tools, video)?;
            let info = probe(tools, video).ok();
            Ok(Still {
                webp,
                duration_secs: info.as_ref().map(|i| i.duration_secs),
                width: info.as_ref().map_or(0, |i| i.width),
                height: info.as_ref().map_or(0, |i| i.height),
            })
        }
        None if !video.is_file() => Err(MediaError::Io(std::io::ErrorKind::NotFound.into())),
        None => Err(MediaError::CannotPlay(native.unwrap_or_else(|| {
            "This computer has no built-in way to read videos".to_string()
        }))),
    }
}

fn still_from_frame(frame: keevoro_frame::Frame) -> Result<Still, MediaError> {
    let (width, height, duration) = (frame.width, frame.height, frame.duration_secs);
    let picture = image::RgbaImage::from_raw(width, height, frame.rgba).ok_or(MediaError::Damaged)?;
    let webp = to_webp(&shrink(&image::DynamicImage::ImageRgba8(picture), DISPLAY_EDGE))?;
    Ok(Still {
        webp,
        duration_secs: duration.map(|secs| secs as f64),
        width,
        height,
    })
}
