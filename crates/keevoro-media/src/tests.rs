use super::*;
use image::{ImageFormat, Rgb, RgbImage, Rgba, RgbaImage};
use std::path::PathBuf;
use tempfile::TempDir;

/// A picture that is red on the left and blue on the right, so a rotation
/// or a flip is easy to detect.
fn red_left_blue_right(w: u32, h: u32) -> RgbImage {
    RgbImage::from_fn(w, h, |x, _| {
        if x < w / 2 {
            Rgb([220, 20, 20])
        } else {
            Rgb([20, 20, 220])
        }
    })
}

fn save(dir: &TempDir, name: &str, img: &RgbImage, format: ImageFormat) -> PathBuf {
    let p = dir.path().join(name);
    img.save_with_format(&p, format).unwrap();
    p
}

fn decode(webp_bytes: &[u8]) -> RgbImage {
    image::load_from_memory_with_format(webp_bytes, ImageFormat::WebP)
        .unwrap()
        .to_rgb8()
}

fn is_red(p: &Rgb<u8>) -> bool {
    p.0[0] > 150 && p.0[2] < 100
}

fn is_blue(p: &Rgb<u8>) -> bool {
    p.0[2] > 150 && p.0[0] < 100
}

#[test]
fn makes_a_thumbnail_and_a_display_copy_at_the_right_sizes() {
    let dir = tempfile::tempdir().unwrap();
    let path = save(&dir, "big.jpg", &red_left_blue_right(3000, 2000), ImageFormat::Jpeg);
    let r = render_photo(&path).unwrap();
    assert_eq!((r.width, r.height), (3000, 2000));

    let thumb = decode(&r.thumb);
    assert_eq!((thumb.width(), thumb.height()), (400, 267));
    let display = decode(&r.display);
    assert_eq!((display.width(), display.height()), (2048, 1365));
    assert!(is_red(thumb.get_pixel(50, 100)) && is_blue(thumb.get_pixel(350, 100)));
    assert!(r.thumb.len() < r.display.len());
    assert!(r.blurhash.len() >= 20 && r.blurhash.is_ascii(), "{}", r.blurhash);
}

#[test]
fn portrait_pictures_fit_by_their_long_edge() {
    let dir = tempfile::tempdir().unwrap();
    let path = save(&dir, "tall.png", &red_left_blue_right(1000, 4000), ImageFormat::Png);
    let r = render_photo(&path).unwrap();
    let thumb = decode(&r.thumb);
    assert_eq!((thumb.width(), thumb.height()), (100, 400));
    assert_eq!(decode(&r.display).height(), 2048);
}

#[test]
fn small_pictures_are_never_enlarged() {
    let dir = tempfile::tempdir().unwrap();
    let path = save(&dir, "small.png", &red_left_blue_right(320, 240), ImageFormat::Png);
    let r = render_photo(&path).unwrap();
    assert_eq!(decode(&r.thumb).dimensions(), (320, 240));
    assert_eq!(decode(&r.display).dimensions(), (320, 240));
}

/// A JPEG with an EXIF block saying "rotate 90 degrees clockwise to view"
/// and a GPS tag, as a phone held upright would write.
fn sideways_phone_jpeg(dir: &TempDir) -> PathBuf {
    // Stored sideways: 600 wide, 400 tall, red on the left.
    let mut plain = Vec::new();
    red_left_blue_right(600, 400)
        .write_to(&mut std::io::Cursor::new(&mut plain), ImageFormat::Jpeg)
        .unwrap();
    let mut tiff = b"MM\0\x2a\0\0\0\x08".to_vec();
    tiff.extend_from_slice(&[0, 2]); // two entries
    tiff.extend_from_slice(&[0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, 6, 0, 0]); // Orientation = 6
    tiff.extend_from_slice(&[0x88, 0x25, 0, 4, 0, 0, 0, 1, 0, 0, 0, 38]); // GPS IFD pointer
    tiff.extend_from_slice(&[0, 0, 0, 0]);
    tiff.extend_from_slice(&[0, 1]); // GPS IFD: one entry
    tiff.extend_from_slice(&[0, 1, 0, 2, 0, 0, 0, 2, b'N', 0, 0, 0]); // GPSLatitudeRef = N
    tiff.extend_from_slice(&[0, 0, 0, 0]);
    let mut app1 = b"Exif\0\0".to_vec();
    app1.extend_from_slice(&tiff);

    let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE1];
    jpeg.extend_from_slice(&u16::try_from(app1.len() + 2).unwrap().to_be_bytes());
    jpeg.extend_from_slice(&app1);
    jpeg.extend_from_slice(&plain[2..]);
    let p = dir.path().join("phone.jpg");
    std::fs::write(&p, jpeg).unwrap();
    p
}

#[test]
fn sideways_phone_photos_come_out_upright() {
    let dir = tempfile::tempdir().unwrap();
    let r = render_photo(&sideways_phone_jpeg(&dir)).unwrap();
    assert_eq!((r.width, r.height), (400, 600), "the upright size is reported");
    let thumb = decode(&r.thumb);
    assert_eq!(thumb.dimensions(), (267, 400));
    // Turned clockwise, the red left edge is now the top.
    assert!(is_red(thumb.get_pixel(133, 40)), "{:?}", thumb.get_pixel(133, 40));
    assert!(is_blue(thumb.get_pixel(133, 360)), "{:?}", thumb.get_pixel(133, 360));
}

#[test]
fn published_copies_carry_no_location_or_camera_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = sideways_phone_jpeg(&dir);
    assert!(std::fs::read(&path).unwrap().windows(4).any(|w| w == b"Exif"));
    let r = render_photo(&path).unwrap();
    for bytes in [&r.thumb, &r.display] {
        assert!(!bytes.windows(4).any(|w| w == b"Exif" || w == b"EXIF"));
        assert!(!bytes.windows(4).any(|w| w == b"XMP "));
    }
}

#[test]
fn see_through_pictures_keep_their_transparency() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("logo.png");
    RgbaImage::from_fn(500, 500, |x, _| {
        if x < 250 {
            Rgba([0, 0, 0, 0])
        } else {
            Rgba([0, 200, 0, 255])
        }
    })
    .save(&p)
    .unwrap();
    let r = render_photo(&p).unwrap();
    let thumb = image::load_from_memory(&r.thumb).unwrap().to_rgba8();
    assert_eq!(thumb.get_pixel(50, 200).0[3], 0);
    assert_eq!(thumb.get_pixel(350, 200).0[3], 255);
}

#[test]
fn unreadable_pictures_give_plain_reasons() {
    let dir = tempfile::tempdir().unwrap();
    let missing = render_photo(&dir.path().join("gone.jpg")).unwrap_err();
    assert!(matches!(missing, MediaError::Io(_)));
    assert!(missing.to_string().starts_with("Couldn't read the picture"));

    let heic = dir.path().join("IMG_0001.HEIC");
    std::fs::write(&heic, b"\0\0\0\x18ftypheic\0\0\0\0mif1heic").unwrap();
    let e = render_photo(&heic).unwrap_err();
    assert!(matches!(e, MediaError::Unsupported), "{e:?}");
    assert_eq!(e.to_string(), "This kind of picture can't be previewed yet");

    let broken = dir.path().join("broken.jpg");
    std::fs::write(
        &broken,
        [0xFF, 0xD8, 0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F', 0, 1, 2, 3],
    )
    .unwrap();
    let e = render_photo(&broken).unwrap_err();
    assert!(matches!(e, MediaError::Damaged | MediaError::Io(_)), "{e:?}");
}

#[test]
fn a_picture_claiming_an_absurd_size_is_refused_before_decoding() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("bomb.png");
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    png.extend_from_slice(&60_000u32.to_be_bytes());
    png.extend_from_slice(&60_000u32.to_be_bytes());
    png.extend_from_slice(&[8, 2, 0, 0, 0, 0, 0, 0, 0]);
    std::fs::write(&p, png).unwrap();
    assert!(render_photo(&p).is_err());
}

#[test]
fn renditions_are_written_whole_and_can_be_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("renditions/thumb/abc.webp");
    write_rendition(&file, b"first").unwrap();
    write_rendition(&file, b"second version").unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), b"second version");
    assert_eq!(
        std::fs::read_dir(file.parent().unwrap()).unwrap().count(),
        1,
        "no temporary file is left"
    );

    // The destination folder cannot be made because a file is in the way.
    assert!(write_rendition(&file.join("x.webp"), b"no").is_err());
}

mod raw_files {
    use super::*;
    use crate::raw::{embedded_preview, is_raw};

    fn jpeg_bytes(img: &RgbImage) -> Vec<u8> {
        let mut bytes = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Jpeg)
            .unwrap();
        bytes
    }

    /// Something shaped like a RAW file: a TIFF-style header (with an
    /// orientation tag when asked), sensor data that is not a picture, a
    /// small embedded thumbnail and a large embedded preview.
    fn pretend_raw(orientation: Option<u8>) -> Vec<u8> {
        let mut raw = b"II*\0\x08\0\0\0".to_vec();
        match orientation {
            // IFD0 with one entry: Orientation (0x0112), SHORT, count 1.
            Some(o) => raw.extend_from_slice(&[1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, o, 0, 0, 0, 0, 0, 0, 0]),
            None => raw.extend_from_slice(&[0, 0, 0, 0, 0, 0]),
        }
        raw.extend(std::iter::repeat_n(0x5Au8, 4000));
        raw.extend_from_slice(&jpeg_bytes(&red_left_blue_right(160, 120)));
        // Sensor data in a JPEG-like wrapper that is not a decodable
        // picture, claiming to be enormous.
        raw.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xC3, 0, 11, 14, 0x40, 0, 0x60, 0, 1, 1, 0x11, 0]);
        raw.extend(std::iter::repeat_n(0xA5u8, 3000));
        raw.extend_from_slice(&jpeg_bytes(&red_left_blue_right(1600, 1200)));
        raw.extend(std::iter::repeat_n(0x11u8, 500));
        raw
    }

    #[test]
    fn raw_files_are_known_by_their_names() {
        for name in ["IMG_0001.CR2", "dsc_0042.nef", "a.ARW", "scan.dng", "x.Cr3"] {
            assert!(is_raw(Path::new(name)), "{name}");
        }
        for name in ["photo.jpg", "notes.raw.txt", "CR2", "clip.mov"] {
            assert!(!is_raw(Path::new(name)), "{name}");
        }
    }

    #[test]
    fn a_raw_file_is_previewed_from_the_largest_picture_inside_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("IMG_0001.CR2");
        std::fs::write(&path, pretend_raw(None)).unwrap();
        let before = std::fs::read(&path).unwrap();

        let r = render_photo(&path).unwrap();
        assert_eq!(
            (r.width, r.height),
            (1600, 1200),
            "the full preview, not the 160 px thumbnail"
        );
        let thumb = decode(&r.thumb);
        assert_eq!(thumb.dimensions(), (400, 300));
        assert!(is_red(thumb.get_pixel(50, 150)) && is_blue(thumb.get_pixel(350, 150)));
        assert_eq!(std::fs::read(&path).unwrap(), before, "the RAW file is only read");
    }

    #[test]
    fn a_raw_photo_taken_sideways_comes_out_upright() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("portrait.NEF");
        // 6 = "turn 90 degrees clockwise to view".
        std::fs::write(&path, pretend_raw(Some(6))).unwrap();
        let r = render_photo(&path).unwrap();
        assert_eq!((r.width, r.height), (1200, 1600));
        let thumb = decode(&r.thumb);
        assert_eq!(thumb.dimensions(), (300, 400));
        assert!(is_red(thumb.get_pixel(150, 40)) && is_blue(thumb.get_pixel(150, 360)));
    }

    #[test]
    fn a_raw_file_with_nothing_usable_inside_has_no_preview() {
        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("empty.ARW");
        std::fs::write(&empty, b"II*\0\x08\0\0\0\0\0\0\0\0\0 only sensor data here").unwrap();
        assert!(render_photo(&empty).is_err());
        assert!(embedded_preview(&empty).is_none());

        // Only the undecodable kind of JPEG inside.
        let lossless = dir.path().join("lossless.DNG");
        let mut bytes = b"II*\0\x08\0\0\0\0\0\0\0\0\0".to_vec();
        bytes.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xC3, 0, 11, 14, 0x10, 0, 0x10, 0, 1, 1, 0x11, 0]);
        std::fs::write(&lossless, bytes).unwrap();
        assert!(render_photo(&lossless).is_err());

        let missing = render_photo(&dir.path().join("gone.CR2")).unwrap_err();
        assert!(
            matches!(missing, MediaError::Io(_)),
            "a missing file is not a format problem"
        );
        assert!(embedded_preview(&dir.path().join("gone.CR2")).is_none());
    }

    #[test]
    fn the_same_bytes_under_an_ordinary_name_are_not_dug_through() {
        // The embedded-picture search is for RAW files only. A damaged TIFF
        // should be reported as damaged, not previewed from whatever
        // happens to be inside it.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scan.tif");
        std::fs::write(&path, pretend_raw(None)).unwrap();
        assert!(render_photo(&path).is_err());
    }
}
