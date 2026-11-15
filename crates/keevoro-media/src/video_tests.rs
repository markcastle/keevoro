//! The rules ([`plan`], [`parse_probe`], [`conversion_args`]) are tested
//! everywhere. The tests that make and convert real videos need ffmpeg and
//! skip themselves, saying so, on a machine without it. CI installs ffmpeg
//! for the coverage job so they do run there.

use crate::video::*;
use crate::MediaError;
use std::path::{Path, PathBuf};
use std::process::Command;

fn info(video: &str, audio: Option<&str>, interlaced: bool) -> VideoInfo {
    VideoInfo {
        duration_secs: 37.4,
        width: 640,
        height: 480,
        video_codec: video.into(),
        audio_codec: audio.map(str::to_string),
        interlaced,
        container: String::new(),
    }
}

#[test]
fn modern_mp4s_are_left_alone() {
    assert_eq!(plan(&info("h264", Some("aac"), false), "mp4"), Conversion::Leave);
    assert_eq!(plan(&info("hevc", Some("aac"), false), "MP4"), Conversion::Leave);
    assert_eq!(plan(&info("h264", None, false), "m4v"), Conversion::Leave);
    // Even an odd sound track is not worth re-encoding a modern file for.
    assert_eq!(plan(&info("h264", Some("ac3"), false), "mp4"), Conversion::Leave);
}

#[test]
fn h264_in_an_old_wrapper_is_repackaged_not_re_encoded() {
    assert_eq!(plan(&info("h264", Some("aac"), false), "mov"), Conversion::Remux);
    assert_eq!(plan(&info("h264", Some("aac"), false), "flv"), Conversion::Remux);
    assert_eq!(plan(&info("h264", None, false), "MTS"), Conversion::Remux);
}

#[test]
fn everything_else_is_re_encoded() {
    let re = Conversion::Reencode { deinterlace: false };
    assert_eq!(plan(&info("mpeg4", Some("mp3"), false), "avi"), re);
    assert_eq!(plan(&info("wmv3", Some("wmav2"), false), "wmv"), re);
    assert_eq!(plan(&info("h263", Some("amr_nb"), false), "3gp"), re);
    assert_eq!(plan(&info("mjpeg", Some("pcm_s16le"), false), "mov"), re);
    // H.264 with sound a browser cannot play still needs the sound redone.
    assert_eq!(plan(&info("h264", Some("ac3"), false), "mts"), re);
    // HEVC outside an MP4 is rare and not widely playable.
    assert_eq!(plan(&info("hevc", Some("aac"), false), "mkv"), re);
}

#[test]
fn interlaced_footage_is_deinterlaced() {
    let de = Conversion::Reencode { deinterlace: true };
    assert_eq!(plan(&info("mpeg2video", Some("ac3"), true), "vob"), de);
    assert_eq!(plan(&info("dvvideo", Some("pcm_s16le"), true), "avi"), de);
    // AVCHD 1080i is H.264, but repackaging would keep the stripes.
    assert_eq!(plan(&info("h264", Some("ac3"), true), "mts"), de);
    assert_eq!(plan(&info("h264", Some("aac"), true), "mov"), de);
}

#[test]
fn conversions_carry_dates_and_start_fast() {
    assert_eq!(conversion_args(Conversion::Leave), None);
    let remux = conversion_args(Conversion::Remux).unwrap().join(" ");
    assert!(remux.ends_with("-c copy"), "{remux}");
    for args in [
        remux,
        conversion_args(Conversion::Reencode { deinterlace: true })
            .unwrap()
            .join(" "),
    ] {
        assert!(args.contains("-map_metadata 0"), "{args}");
        assert!(args.contains("-movflags +faststart"), "{args}");
        assert!(
            args.contains("-map 0:a:0?"),
            "a silent video must still convert: {args}"
        );
    }
    let plain = conversion_args(Conversion::Reencode { deinterlace: false })
        .unwrap()
        .join(" ");
    assert!(plain.contains("libx264") && plain.contains("yuv420p") && plain.contains("-c:a aac"));
    assert!(!plain.contains("yadif"));
    assert!(conversion_args(Conversion::Reencode { deinterlace: true })
        .unwrap()
        .join(" ")
        .contains("yadif,"));
}

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
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("out.mp4");
    let i = info("mpeg4", None, false);
    assert!(matches!(
        convert(&nowhere, Path::new("x.avi"), &i, Conversion::Remux, &dest),
        Err(MediaError::Io(_))
    ));
    assert!(
        std::fs::read_dir(dir.path()).unwrap().next().is_none(),
        "nothing is left behind"
    );
    // Nothing to do is not an error and needs no tools.
    convert(&nowhere, Path::new("x.mp4"), &i, Conversion::Leave, &dest).unwrap();
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
fn an_old_avi_becomes_an_mp4_that_plays_everywhere() {
    let Some(tools) = tools() else { return };
    let dir = tempfile::tempdir().unwrap();
    let avi = make_video(&tools, dir.path(), "holiday.avi", &["-c:v", "mpeg4", "-c:a", "mp3"]);
    let before = std::fs::read(&avi).unwrap();

    let original = probe(&tools, &avi).unwrap();
    assert_eq!((original.width, original.height), (322, 242));
    assert_eq!(original.video_codec, "mpeg4");
    assert!((original.duration_secs - 2.0).abs() < 0.2);
    let action = plan(&original, "avi");
    assert_eq!(action, Conversion::Reencode { deinterlace: false });

    let mp4 = dir.path().join("renditions/video/abc.mp4");
    convert(&tools, &avi, &original, action, &mp4).unwrap();
    let result = probe(&tools, &mp4).unwrap();
    assert_eq!(
        (result.video_codec.as_str(), result.audio_codec.as_deref()),
        ("h264", Some("aac"))
    );
    assert!((result.duration_secs - original.duration_secs).abs() <= DURATION_TOLERANCE_SECS);
    assert_eq!(
        (result.width, result.height),
        (322, 242),
        "odd sizes are evened out by at most a pixel"
    );
    assert_eq!(
        plan(&result, "mp4"),
        Conversion::Leave,
        "the result needs nothing further"
    );

    assert_eq!(std::fs::read(&avi).unwrap(), before, "the original is untouched");
    let left: Vec<_> = std::fs::read_dir(mp4.parent().unwrap()).unwrap().collect();
    assert_eq!(left.len(), 1, "no temporary file is left");
}

#[test]
fn h264_in_a_mov_is_repackaged_bit_for_bit() {
    let Some(tools) = tools() else { return };
    let dir = tempfile::tempdir().unwrap();
    let mov = make_video(
        &tools,
        dir.path(),
        "clip.mov",
        &[
            "-vf",
            "scale=320:240",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
        ],
    );
    let original = probe(&tools, &mov).unwrap();
    assert_eq!(plan(&original, "mov"), Conversion::Remux);
    let mp4 = dir.path().join("clip.mp4");
    convert(&tools, &mov, &original, Conversion::Remux, &mp4).unwrap();
    let result = probe(&tools, &mp4).unwrap();
    assert_eq!(result.video_codec, "h264");
    assert!((result.duration_secs - original.duration_secs).abs() <= DURATION_TOLERANCE_SECS);

    // Repackaging does not touch the pictures: every frame is identical.
    let frames = |file: &Path| {
        let out = Command::new(&tools.ffmpeg)
            .args(["-v", "error", "-i"])
            .arg(file)
            .args(["-map", "0:v:0", "-f", "framemd5", "-"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| l.rsplit(',').next().map(|h| h.trim().to_string()))
            .collect::<Vec<_>>()
    };
    let (a, b) = (frames(&mov), frames(&mp4));
    assert!(a.len() >= 40 && a == b, "{} frames vs {}", a.len(), b.len());
}

#[test]
fn interlaced_camcorder_footage_is_detected_and_converted() {
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
    let action = plan(&original, "mpg");
    assert_eq!(action, Conversion::Reencode { deinterlace: true });
    let mp4 = dir.path().join("tape.mp4");
    convert(&tools, &mpg, &original, action, &mp4).unwrap();
    assert!(!probe(&tools, &mp4).unwrap().interlaced);
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
fn a_conversion_that_comes_out_wrong_is_thrown_away() {
    let Some(tools) = tools() else { return };
    let dir = tempfile::tempdir().unwrap();
    let avi = make_video(&tools, dir.path(), "holiday.avi", &["-c:v", "mpeg4", "-c:a", "mp3"]);
    let mut claimed = probe(&tools, &avi).unwrap();
    // Pretend the original was a minute long: the two-second result must
    // then be rejected as incomplete.
    claimed.duration_secs = 60.0;
    let mp4 = dir.path().join("out/short.mp4");
    let err = convert(
        &tools,
        &avi,
        &claimed,
        Conversion::Reencode { deinterlace: false },
        &mp4,
    )
    .unwrap_err();
    assert!(matches!(err, MediaError::Damaged));
    assert!(!mp4.exists());
    assert!(
        std::fs::read_dir(mp4.parent().unwrap()).unwrap().next().is_none(),
        "nothing is left behind"
    );
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
    let i = info("mpeg4", None, false);
    let dest = dir.path().join("out.mp4");
    assert!(convert(&tools, &junk, &i, Conversion::Remux, &dest).is_err());
    assert!(!dest.exists());
}
