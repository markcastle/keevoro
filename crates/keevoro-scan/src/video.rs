//! Reads the recording date from MP4, MOV and 3GP videos.
//!
//! These formats share one container layout: a list of "boxes", each with a
//! length and a four-letter name. The recording date lives in the movie
//! header (`mvhd`) inside the `moov` box. On files straight from a camera
//! `moov` is often at the very end, after gigabytes of video, so the reader
//! hops from box to box by seeking rather than reading everything.
//!
//! The date is stored as seconds since 1904. Phones usually write it in UTC
//! and older cameras in local time, and the file does not say which. Keevoro
//! takes it as written: being a few hours out is far better than no date.

use keevoro_core::PartialDate;
use std::io::{Read, Seek, SeekFrom};

/// Seconds between the container's 1904 epoch and the Unix epoch.
const EPOCH_1904_TO_1970: i64 = 2_082_844_800;
/// 1 January 1990. Anything earlier is an unset camera clock, not a date.
const EARLIEST_BELIEVABLE: i64 = 631_152_000;
/// A damaged file must not send the reader round in circles.
const MAX_BOXES: usize = 4096;

struct BoxHeader {
    /// Where the box's contents start and where the box ends.
    body: u64,
    end: u64,
}

/// The recording date of a video, or `None` when the file is not this kind
/// of container or its clock was never set.
pub fn recording_date<R: Read + Seek>(r: &mut R) -> Option<PartialDate> {
    let len = r.seek(SeekFrom::End(0)).ok()?;
    let moov = find_box(r, 0, len, b"moov", true)?;
    let mvhd = find_box(r, moov.body, moov.end, b"mvhd", false)?;

    r.seek(SeekFrom::Start(mvhd.body)).ok()?;
    let mut version_and_flags = [0u8; 4];
    r.read_exact(&mut version_and_flags).ok()?;
    let created = if version_and_flags[0] == 1 {
        let mut b = [0u8; 8];
        r.read_exact(&mut b).ok()?;
        i64::try_from(u64::from_be_bytes(b)).ok()?
    } else {
        let mut b = [0u8; 4];
        r.read_exact(&mut b).ok()?;
        i64::from(u32::from_be_bytes(b))
    };
    let unix = created.checked_sub(EPOCH_1904_TO_1970)?;
    if unix < EARLIEST_BELIEVABLE {
        return None;
    }
    PartialDate::from_unix_seconds(unix)
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
        let mut body = vec![version, 0, 0, 0];
        if version == 1 {
            body.extend_from_slice(&created_1904.to_be_bytes());
        } else {
            body.extend_from_slice(&u32::try_from(created_1904).unwrap().to_be_bytes());
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
        recording_date(&mut Cursor::new(bytes)).map(|d| d.to_string())
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
