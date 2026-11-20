//! Reads the movie header of MP4, MOV, M4V and 3GP videos: when the video
//! was recorded, and how long it lasts.
//!
//! These formats share one container layout: a list of "boxes", each with a
//! length and a four-letter name. Both facts live in the movie header
//! (`mvhd`) inside the `moov` box. On files straight from a camera `moov`
//! is often at the very end, after gigabytes of video, so the reader hops
//! from box to box by seeking rather than reading everything. A handful of
//! small reads, and nothing is decoded.
//!
//! The date is stored as seconds since 1904. Phones usually write it in UTC
//! and older cameras in local time, and the file does not say which. Keevoro
//! takes it as written: being a few hours out is far better than no date.

use keevoro_core::PartialDate;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Seconds between the container's 1904 epoch and the Unix epoch.
const EPOCH_1904_TO_1970: i64 = 2_082_844_800;
/// 1 January 1990. Anything earlier is an unset camera clock, not a date.
const EARLIEST_BELIEVABLE: i64 = 631_152_000;
/// A damaged file must not send the reader round in circles.
const MAX_BOXES: usize = 4096;
/// Longer than any real video: a length past it is a damaged header.
const LONGEST_BELIEVABLE: f64 = 7.0 * 24.0 * 60.0 * 60.0;

struct BoxHeader {
    /// Where the box's contents start and where the box ends.
    body: u64,
    end: u64,
}

/// What a video's movie header says.
#[derive(Debug, Clone, PartialEq)]
pub struct MovieHeader {
    /// When it was recorded, if its clock was set.
    pub recorded: Option<PartialDate>,
    /// How long it lasts, in seconds, if it says.
    pub seconds: Option<f64>,
}

/// The movie header of a video, or `None` when the file is not this kind
/// of container.
pub fn movie_header<R: Read + Seek>(r: &mut R) -> Option<MovieHeader> {
    let len = r.seek(SeekFrom::End(0)).ok()?;
    let moov = find_box(r, 0, len, b"moov", true)?;
    let mvhd = find_box(r, moov.body, moov.end, b"mvhd", false)?;

    r.seek(SeekFrom::Start(mvhd.body)).ok()?;
    let mut version_and_flags = [0u8; 4];
    r.read_exact(&mut version_and_flags).ok()?;
    let long = version_and_flags[0] == 1;
    // Created, then modified, then the time scale and the duration.
    let created = read_number(r, long)?;
    read_number(r, long)?;
    let scale = read_number(r, false)?;
    let duration = read_number(r, long)?;

    let recorded = i64::try_from(created)
        .ok()
        .and_then(|c| c.checked_sub(EPOCH_1904_TO_1970))
        .filter(|unix| *unix >= EARLIEST_BELIEVABLE)
        .and_then(PartialDate::from_unix_seconds);
    // All ones means the length is not known.
    let unknown = if long { u64::MAX } else { u64::from(u32::MAX) };
    #[allow(clippy::cast_precision_loss)]
    let seconds = (scale > 0 && duration != unknown)
        .then(|| duration as f64 / scale as f64)
        .filter(|s| s.is_finite() && *s <= LONGEST_BELIEVABLE);
    Some(MovieHeader { recorded, seconds })
}

/// How long the video at `path` lasts, from its movie header, in seconds.
/// `None` for a file that is not MP4, MOV, M4V or 3GP, or does not say.
/// The file is only read, a few bytes at a time.
pub fn length_of(path: &Path) -> Option<f64> {
    let mut file = std::io::BufReader::with_capacity(4096, std::fs::File::open(path).ok()?);
    movie_header(&mut file)?.seconds
}

/// A big-endian number of 32 bits, or 64 when `long`.
fn read_number<R: Read>(r: &mut R, long: bool) -> Option<u64> {
    if long {
        let mut b = [0u8; 8];
        r.read_exact(&mut b).ok()?;
        Some(u64::from_be_bytes(b))
    } else {
        let mut b = [0u8; 4];
        r.read_exact(&mut b).ok()?;
        Some(u64::from(u32::from_be_bytes(b)))
    }
}

/// Walks the boxes between `start` and `end` looking for `wanted`.
/// At the top level the first box must be a known one, so that files which
/// are not this container at all are rejected straight away.
fn find_box<R: Read + Seek>(r: &mut R, start: u64, end: u64, wanted: &[u8; 4], top_level: bool) -> Option<BoxHeader> {
    const TOP_LEVEL: [&[u8; 4]; 7] = [b"ftyp", b"moov", b"mdat", b"free", b"skip", b"wide", b"pnot"];
    let mut pos = start;
    for i in 0..MAX_BOXES {
        if pos.checked_add(8)? > end {
            return None;
        }
        r.seek(SeekFrom::Start(pos)).ok()?;
        let mut head = [0u8; 8];
        r.read_exact(&mut head).ok()?;
        let name = [head[4], head[5], head[6], head[7]];
        if top_level && i == 0 && !TOP_LEVEL.contains(&&name) {
            return None;
        }
        let short = u64::from(u32::from_be_bytes([head[0], head[1], head[2], head[3]]));
        let (size, header_len) = match short {
            // The box runs to the end of the file.
            0 => (end - pos, 8),
            // The real size follows as 64 bits.
            1 => {
                let mut b = [0u8; 8];
                r.read_exact(&mut b).ok()?;
                (u64::from_be_bytes(b), 16)
            }
            n => (n, 8),
        };
        if size < header_len {
            return None;
        }
        let box_end = pos.checked_add(size)?.min(end);
        if &name == wanted {
            return Some(BoxHeader {
                body: pos + header_len,
                end: box_end,
            });
        }
        pos = pos.checked_add(size)?;
    }
    None
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Cursor;

    fn boxed(name: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut v = u32::try_from(body.len() + 8).unwrap().to_be_bytes().to_vec();
        v.extend_from_slice(name);
        v.extend_from_slice(body);
        v
    }

    fn mvhd(version: u8, created_1904: u64) -> Vec<u8> {
        mvhd_lasting(version, created_1904, 600, 61_200)
    }

    /// A movie header saying the video was made at `created_1904` and
    /// lasts `duration` ticks of `scale` a second.
    fn mvhd_lasting(version: u8, created_1904: u64, scale: u32, duration: u64) -> Vec<u8> {
        let mut body = vec![version, 0, 0, 0];
        if version == 1 {
            body.extend_from_slice(&created_1904.to_be_bytes());
            body.extend_from_slice(&created_1904.to_be_bytes());
            body.extend_from_slice(&scale.to_be_bytes());
            body.extend_from_slice(&duration.to_be_bytes());
        } else {
            body.extend_from_slice(&u32::try_from(created_1904).unwrap().to_be_bytes());
            body.extend_from_slice(&u32::try_from(created_1904).unwrap().to_be_bytes());
            body.extend_from_slice(&scale.to_be_bytes());
            body.extend_from_slice(&u32::try_from(duration).unwrap().to_be_bytes());
        }
        body.extend_from_slice(&[0; 16]);
        boxed(b"mvhd", &body)
    }

    /// A minimal MP4 whose movie header says it was recorded at `unix`.
    /// `moov_last` puts the header after the video data, as cameras do.
    pub(crate) fn mp4_recorded_at(unix: i64, version: u8, moov_last: bool) -> Vec<u8> {
        let created = u64::try_from(unix + EPOCH_1904_TO_1970).unwrap();
        let ftyp = boxed(b"ftyp", b"isom\0\0\x02\0isomiso2");
        let moov = boxed(b"moov", &mvhd(version, created));
        let mdat = boxed(b"mdat", &[7u8; 64]);
        if moov_last {
            [ftyp, mdat, moov].concat()
        } else {
            [ftyp, moov, mdat].concat()
        }
    }

    // 14 July 2009, 21:10:02.
    const SPAIN: i64 = 1_247_605_802;

    fn date(bytes: Vec<u8>) -> Option<String> {
        movie_header(&mut Cursor::new(bytes))
            .and_then(|h| h.recorded)
            .map(|d| d.to_string())
    }

    fn seconds(bytes: Vec<u8>) -> Option<f64> {
        movie_header(&mut Cursor::new(bytes)).and_then(|h| h.seconds)
    }

    #[test]
    fn reads_how_long_a_video_lasts_wherever_the_header_sits() {
        // 61,200 ticks of 1/600 of a second: one minute 42 seconds.
        assert_eq!(seconds(mp4_recorded_at(SPAIN, 0, false)), Some(102.0));
        assert_eq!(seconds(mp4_recorded_at(SPAIN, 0, true)), Some(102.0));
        assert_eq!(seconds(mp4_recorded_at(SPAIN, 1, true)), Some(102.0));
        let created = u64::try_from(SPAIN + EPOCH_1904_TO_1970).unwrap();
        let long = [
            boxed(b"ftyp", b"qt  "),
            boxed(b"moov", &mvhd_lasting(1, created, 90_000, 90_000 * 3_725)),
        ]
        .concat();
        assert_eq!(seconds(long), Some(3_725.0), "a 64-bit header, an hour and more");
    }

    #[test]
    fn a_length_not_known_or_not_believable_is_no_length() {
        let created = u64::try_from(SPAIN + EPOCH_1904_TO_1970).unwrap();
        let with = |scale, duration| {
            [
                boxed(b"ftyp", b"isom"),
                boxed(b"moov", &mvhd_lasting(0, created, scale, duration)),
            ]
            .concat()
        };
        assert_eq!(seconds(with(0, 100)), None, "no time scale");
        assert_eq!(seconds(with(600, u64::from(u32::MAX))), None, "all ones: not known");
        assert_eq!(
            seconds(with(1, 30 * 24 * 60 * 60)),
            None,
            "a month long is a damaged header"
        );
        // A header with no room for its length says no length, and no date either.
        let mut short = vec![0u8, 0, 0, 0];
        short.extend_from_slice(&u32::try_from(created).unwrap().to_be_bytes());
        let cut = [boxed(b"ftyp", b"isom"), boxed(b"moov", &boxed(b"mvhd", &short))].concat();
        assert_eq!(movie_header(&mut Cursor::new(cut)), None);
    }

    #[test]
    fn the_length_of_a_file_is_read_without_changing_it() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("clip.mov");
        let bytes = mp4_recorded_at(SPAIN, 0, true);
        std::fs::write(&file, &bytes).unwrap();
        assert_eq!(length_of(&file), Some(102.0));
        assert_eq!(std::fs::read(&file).unwrap(), bytes);
        let avi = dir.path().join("old.avi");
        std::fs::write(&avi, b"RIFF\0\0\0\0AVI LIST").unwrap();
        assert_eq!(length_of(&avi), None);
        assert_eq!(length_of(&dir.path().join("gone.mp4")), None);
    }

    #[test]
    fn reads_the_date_wherever_the_header_sits() {
        assert_eq!(date(mp4_recorded_at(SPAIN, 0, false)).unwrap(), "2009-07-14T21:10:02");
        assert_eq!(date(mp4_recorded_at(SPAIN, 0, true)).unwrap(), "2009-07-14T21:10:02");
        assert_eq!(date(mp4_recorded_at(SPAIN, 1, true)).unwrap(), "2009-07-14T21:10:02");
    }

    #[test]
    fn an_unset_camera_clock_is_not_a_date() {
        // Zero means 1 January 1904.
        let zero = [boxed(b"ftyp", b"qt  "), boxed(b"moov", &mvhd(0, 0))].concat();
        assert_eq!(date(zero), None);
        assert_eq!(date(mp4_recorded_at(100, 0, false)), None);
    }

    #[test]
    fn follows_64_bit_and_run_to_end_boxes() {
        let created = u64::try_from(SPAIN + EPOCH_1904_TO_1970).unwrap();
        // mdat with a 64-bit size, then a moov whose size of 0 means "to the end".
        let mut big = vec![0, 0, 0, 1];
        big.extend_from_slice(b"mdat");
        big.extend_from_slice(&24u64.to_be_bytes());
        big.extend_from_slice(&[9u8; 8]);
        let mut moov = vec![0, 0, 0, 0];
        moov.extend_from_slice(b"moov");
        moov.extend_from_slice(&mvhd(0, created));
        let file = [boxed(b"ftyp", b"isom"), big, moov].concat();
        assert_eq!(date(file).unwrap(), "2009-07-14T21:10:02");
    }

    #[test]
    fn other_and_damaged_files_give_no_date() {
        assert_eq!(date(Vec::new()), None);
        assert_eq!(date(b"RIFF\0\0\0\0AVI LIST".to_vec()), None);
        // No moov at all.
        assert_eq!(date([boxed(b"ftyp", b"isom"), boxed(b"mdat", &[1; 8])].concat()), None);
        // A box that claims to be smaller than its own header.
        let mut bad = boxed(b"ftyp", b"isom");
        bad.extend_from_slice(&[0, 0, 0, 4]);
        bad.extend_from_slice(b"moov");
        assert_eq!(date(bad), None);
        // moov with no header inside, and a header cut short.
        assert_eq!(
            date([boxed(b"ftyp", b"isom"), boxed(b"moov", &boxed(b"udta", &[0; 4]))].concat()),
            None
        );
        let mut cut = mp4_recorded_at(SPAIN, 0, true);
        cut.truncate(cut.len() - 18);
        assert_eq!(date(cut), None);
    }
}
