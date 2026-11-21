//! Reading ffprobe's answer ([`parse_probe`]) is tested everywhere. The
//! tests that make real videos need ffmpeg and skip themselves, saying so,
//! on a machine without it. CI installs ffmpeg for the coverage job so they
//! do run there.

use crate::video::*;
use crate::MediaError;
use std::path::{Path, PathBuf};
use std::process::Command;

const PHONE_VIDEO: &str = r#"{
  "streams": [
    {"index": 0, "codec_name": "aac", "codec_type": "audio", "duration": "12.5"},
    {"index": 1, "codec_name": "h264", "codec_type": "video", "width": 1920, "height": 1080,
     "field_order": "progressive", "disposition": {"attached_pic": 0},
     "side_data_list": [{"side_data_type": "Display Matrix", "rotation": -90}]}
  ],
  "format": {"format_name": "mov,mp4,m4a,3gp,3g2,mj2", "duration": "12.533000"}
}"#;

#[test]
fn probe_output_is_understood() {
    let v = parse_probe(PHONE_VIDEO).unwrap();
    assert_eq!(
        (v.width, v.height),
        (1080, 1920),
        "a phone held upright is tall, not wide"
    );
    assert!((v.duration_secs - 12.533).abs() < 1e-9);
    assert_eq!(
        (v.video_codec.as_str(), v.audio_codec.as_deref()),
        ("h264", Some("aac"))
    );
    assert!(!v.interlaced);
    assert!(v.container.starts_with("mov,mp4"));

    let camcorder = r#"{"streams":[{"codec_name":"mpeg2video","codec_type":"video","width":720,"height":576,
        "field_order":"tt","duration":"61.0","tags":{"rotate":"180"}}],"format":{"format_name":"mpeg"}}"#;
    let v = parse_probe(camcorder).unwrap();
    assert!(v.interlaced);
    assert_eq!(
        (v.width, v.height, v.audio_codec),
        (720, 576, None),
        "upside down is still wide"
    );
    assert!(
        (v.duration_secs - 61.0).abs() < 1e-9,
        "falls back to the stream's own length"
    );

    let old_phone = r#"{"streams":[{"codec_name":"h263","codec_type":"video","width":176,"height":144,
        "tags":{"rotate":"90"}}],"format":{"format_name":"3gp","duration":"4.0"}}"#;
    assert_eq!(parse_probe(old_phone).unwrap().width, 144);
}

#[test]
fn files_that_are_not_really_videos_are_refused() {
    // A song with cover art: the picture is not a video.
    let song = r#"{"streams":[{"codec_name":"aac","codec_type":"audio"},
        {"codec_name":"mjpeg","codec_type":"video","width":500,"height":500,"disposition":{"attached_pic":1}}],
        "format":{"format_name":"mov,mp4,m4a","duration":"180.0"}}"#;
    assert!(matches!(parse_probe(song), Err(MediaError::Unsupported)));
    assert!(matches!(
        parse_probe(r#"{"streams":[],"format":{}}"#),
        Err(MediaError::Unsupported)
    ));

    for broken in [
        "not json",
        "{}",
        r#"{"streams":[{"codec_type":"video","codec_name":"h264","width":640,"height":480}],"format":{}}"#,
        r#"{"streams":[{"codec_type":"video","codec_name":"h264","width":640,"height":480}],"format":{"duration":"N/A"}}"#,
        r#"{"streams":[{"codec_type":"video","codec_name":"h264"}],"format":{"duration":"3.0"}}"#,
        r#"{"streams":[{"codec_type":"video","width":640,"height":480}],"format":{"duration":"3.0"}}"#,
    ] {
        assert!(matches!(parse_probe(broken), Err(MediaError::Damaged)), "{broken}");
    }
}

#[test]
fn missing_tools_are_reported_as_missing_not_as_a_crash() {
    let nowhere = VideoTools {
        ffmpeg: PathBuf::from("/definitely/not/ffmpeg"),
        ffprobe: PathBuf::from("/definitely/not/ffprobe"),
    };
    assert!(matches!(probe(&nowhere, Path::new("x.avi")), Err(MediaError::Io(_))));
    assert!(matches!(poster(&nowhere, Path::new("x.avi")), Err(MediaError::Io(_))));
}

/// ffmpeg on this machine, or `None` with a note that the test was skipped.
fn tools() -> Option<VideoTools> {
    let found = VideoTools::find();
    if found.is_none() {
        eprintln!("skipped: ffmpeg is not installed");
    }
    found
}

/// Makes a two-second test video with a moving pattern and a tone.
fn make_video(tools: &VideoTools, dir: &Path, name: &str, codec_args: &[&str]) -> PathBuf {
    let out = dir.join(name);
    let status = Command::new(&tools.ffmpeg)
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=2:size=322x242:rate=25",
        ])
        .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=2"])
        .args(codec_args)
        .arg(&out)
        .status()
        .unwrap();
    assert!(status.success(), "could not make {name}");
    out
}

#[test]
fn an_old_avi_is_understood() {
    let Some(tools) = tools() else { return };
    let dir = tempfile::tempdir().unwrap();
    let avi = make_video(&tools, dir.path(), "holiday.avi", &["-c:v", "mpeg4", "-c:a", "mp3"]);
    let before = std::fs::read(&avi).unwrap();
    let original = probe(&tools, &avi).unwrap();
    assert_eq!((original.width, original.height), (322, 242));
    assert_eq!(original.video_codec, "mpeg4");
    assert!((original.duration_secs - 2.0).abs() < 0.2);
    assert_eq!(std::fs::read(&avi).unwrap(), before, "the original is untouched");
}

#[test]
fn interlaced_camcorder_footage_is_detected() {
    let Some(tools) = tools() else { return };
    let dir = tempfile::tempdir().unwrap();
    let mpg = make_video(
        &tools,
        dir.path(),
        "tape.mpg",
        &[
            "-vf",
            "scale=352:288",
            "-c:v",
            "mpeg2video",
            "-flags",
            "+ilme+ildct",
            "-top",
            "1",
            "-c:a",
            "mp2",
        ],
    );
    let original = probe(&tools, &mpg).unwrap();
    assert!(original.interlaced, "{original:?}");
}

#[test]
fn a_poster_is_a_real_picture_of_the_video() {
    let Some(tools) = tools() else { return };
    let dir = tempfile::tempdir().unwrap();
    let avi = make_video(&tools, dir.path(), "holiday.avi", &["-c:v", "mpeg4", "-c:a", "mp3"]);
    let webp = poster(&tools, &avi).unwrap();
    let picture = image::load_from_memory_with_format(&webp, image::ImageFormat::WebP).unwrap();
    assert_eq!((picture.width(), picture.height()), (322, 242));
    // The test pattern is colourful; a black or blank frame would not be.
    let rgb = picture.to_rgb8();
    let colours: std::collections::HashSet<_> = rgb.pixels().map(|p| (p.0[0] / 64, p.0[1] / 64, p.0[2] / 64)).collect();
    assert!(colours.len() > 8, "only {} colours", colours.len());
}

#[test]
fn damaged_and_missing_videos_give_plain_errors() {
    let Some(tools) = tools() else { return };
    let dir = tempfile::tempdir().unwrap();
    let junk = dir.path().join("broken.avi");
    std::fs::write(&junk, b"RIFF this was never a video").unwrap();
    assert!(matches!(
        probe(&tools, &junk),
        Err(MediaError::Damaged | MediaError::Unsupported)
    ));
    assert!(matches!(poster(&tools, &junk), Err(MediaError::Damaged)));
    assert!(matches!(
        probe(&tools, &dir.path().join("gone.avi")),
        Err(MediaError::Io(_))
    ));
}
