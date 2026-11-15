//! Working out when a photo or video was taken.
//!
//! Old files rarely carry a reliable date, so every guess records where it
//! came from. The UI can then say "Roughly 2009" for a weak guess instead of
//! pretending to know the exact second.
//!
//! Times are local wall-clock time with no timezone, because old cameras did
//! not record one and inventing one would be wrong.

use regex::Regex;
use serde::{Serialize, Serializer};
use std::fmt;
use std::sync::LazyLock;

/// Earliest year accepted from file names and folders. Digital photos and
/// scans of older prints can be earlier, but four-digit numbers below this in
/// names are far more often counters than years.
const MIN_YEAR: u16 = 1950;
const MAX_YEAR: u16 = 2099;

/// A date that may only be known to the year or month.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PartialDate {
    pub year: u16,
    pub month: Option<u8>,
    pub day: Option<u8>,
    /// Hour, minute, second. Only present when `day` is.
    pub time: Option<(u8, u8, u8)>,
}

impl PartialDate {
    pub fn year(year: u16) -> Option<Self> {
        (MIN_YEAR..=MAX_YEAR).contains(&year).then_some(Self {
            year,
            month: None,
            day: None,
            time: None,
        })
    }

    pub fn month(year: u16, month: u8) -> Option<Self> {
        let mut d = Self::year(year)?;
        (1..=12).contains(&month).then(|| {
            d.month = Some(month);
            d
        })
    }

    pub fn day(year: u16, month: u8, day: u8) -> Option<Self> {
        let mut d = Self::month(year, month)?;
        (1..=days_in_month(year, month)).contains(&day).then(|| {
            d.day = Some(day);
            d
        })
    }

    /// Adds a time if it is valid; an invalid time is dropped rather than
    /// throwing away a good date.
    pub fn with_time(mut self, h: u8, m: u8, s: u8) -> Self {
        if self.day.is_some() && h < 24 && m < 60 && s < 61 {
            self.time = Some((h, m, s));
        }
        self
    }

    /// Parses the EXIF `DateTimeOriginal` format, `YYYY:MM:DD HH:MM:SS`.
    /// Cameras with an unset clock write zeros, which are rejected.
    pub fn from_exif(s: &str) -> Option<Self> {
        static RE: LazyLock<Regex> =
            LazyLock::new(|| re(r"^\s*(\d{4})[:\-](\d{2})[:\-](\d{2})(?:[ T](\d{2}):(\d{2}):(\d{2}))?"));
        let c = RE.captures(s.trim_end_matches('\0'))?;
        // Exif dates can legitimately be older than MIN_YEAR (scanned film
        // tagged by hand), so only reject the impossible.
        let year: u16 = c[1].parse().ok()?;
        if !(1826..=MAX_YEAR).contains(&year) {
            return None;
        }
        let month: u8 = c[2].parse().ok()?;
        let day: u8 = c[3].parse().ok()?;
        if !(1..=12).contains(&month) || !(1..=days_in_month(year, month)).contains(&day) {
            return None;
        }
        let d = Self {
            year,
            month: Some(month),
            day: Some(day),
            time: None,
        };
        Some(match (c.get(4), c.get(5), c.get(6)) {
            (Some(h), Some(m), Some(s)) => d.with_time(
                h.as_str().parse().ok()?,
                m.as_str().parse().ok()?,
                s.as_str().parse().ok()?,
            ),
            _ => d,
        })
    }

    /// Converts a Unix timestamp (seconds, UTC) to a date. Used for file
    /// modified times, the weakest source.
    pub fn from_unix_seconds(secs: i64) -> Option<Self> {
        let days = secs.div_euclid(86_400);
        let rem = secs.rem_euclid(86_400);
        let (y, m, d) = civil_from_days(days);
        let year = u16::try_from(y).ok()?;
        let d = Self::day(year, m, d)?;
        // rem is in 0..86_400, so these always fit in u8.
        let h = u8::try_from(rem / 3600).ok()?;
        let mi = u8::try_from((rem % 3600) / 60).ok()?;
        let s = u8::try_from(rem % 60).ok()?;
        Some(d.with_time(h, mi, s))
    }
}

impl fmt::Display for PartialDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}", self.year)?;
        if let Some(m) = self.month {
            write!(f, "-{m:02}")?;
        }
        if let Some(d) = self.day {
            write!(f, "-{d:02}")?;
        }
        if let Some((h, m, s)) = self.time {
            write!(f, "T{h:02}:{m:02}:{s:02}")?;
        }
        Ok(())
    }
}

impl Serialize for PartialDate {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

/// Where a date came from, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DateSource {
    /// Camera EXIF data.
    Exif,
    /// Video container metadata.
    Video,
    /// A date in the file name, like `IMG_20120304_153012.jpg`.
    Filename,
    /// A date in a folder name, like `2009-07 Spain`.
    Folder,
    /// The file's modified time. Often the date it was copied, not taken.
    Modified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DateGuess {
    pub date: PartialDate,
    pub source: DateSource,
}

/// Picks the most trustworthy guess.
pub fn best_date(guesses: &[DateGuess]) -> Option<DateGuess> {
    guesses.iter().copied().min_by_key(|g| g.source)
}

/// Finds a date in a file name.
pub fn date_from_filename(name: &str) -> Option<PartialDate> {
    // 20120304, 20120304_153012, 20120304-153012, PXL_20210101_120000123
    static COMPACT: LazyLock<Regex> =
        LazyLock::new(|| re(r"(?:^|\D)((?:19|20)\d{2})(\d{2})(\d{2})(?:[_\-T ]?(\d{2})(\d{2})(\d{2}))?"));
    // 2012-03-04, 2012-03-04 15.30.12, "Screenshot 2015-06-01 at 10.11.12"
    static SEPARATED: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"(?:^|\D)((?:19|20)\d{2})[-_.](\d{2})[-_.](\d{2})(?:(?:[ _T\-]|\sat\s)(\d{2})[.:\-_](\d{2})[.:\-_](\d{2}))?",
        )
    });
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    for rx in [&*SEPARATED, &*COMPACT] {
        for c in rx.captures_iter(stem) {
            if let Some(d) = day_from_caps(&c) {
                return Some(d);
            }
        }
    }
    None
}

/// What one folder name says about dates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FolderDate {
    pub date: PartialDate,
    /// True when the name is nothing but a date (`2009_07_14`), so it is a
    /// camera or import folder rather than something the user named.
    pub date_only: bool,
}

/// Finds a date hint in a single folder name.
pub fn date_from_folder(name: &str) -> Option<FolderDate> {
    let name = name.trim();
    // Full date, separated or compact, anywhere in the name.
    static FULL: LazyLock<Regex> =
        LazyLock::new(|| re(r"^(.*?)(?:^|\D)((?:19|20)\d{2})[-_. ]?(\d{2})[-_. ]?(\d{2})(?:\D(.*)|$)"));
    static YEAR_MONTH: LazyLock<Regex> =
        LazyLock::new(|| re(r"^(.*?)(?:^|\D)((?:19|20)\d{2})[-_. ](\d{2})(?:\D(.*)|$)"));
    static MONTH_NAME_YEAR: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"(?i)^(.*?)\b(january|february|march|april|may|june|july|august|september|october|november|december|jan|feb|mar|apr|jun|jul|aug|sept|sep|oct|nov|dec)\b\.?\s+((?:19|20)\d{2})\b(.*)$",
        )
    });
    static YEAR_MONTH_NAME: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"(?i)^(.*?)\b((?:19|20)\d{2})\s+(january|february|march|april|may|june|july|august|september|october|november|december|jan|feb|mar|apr|jun|jul|aug|sept|sep|oct|nov|dec)\b(.*)$",
        )
    });
    static YEAR: LazyLock<Regex> = LazyLock::new(|| re(r"^(.*?)(?:^|\D)((?:19|20)\d{2})(?:\D(.*)|$)"));

    let only = |before: &str, after: Option<&str>| is_filler(before) && after.is_none_or(is_filler);

    if let Some(c) = FULL.captures(name) {
        if let Some(d) = parse_ymd(&c[2], &c[3], &c[4]) {
            return Some(FolderDate {
                date: d,
                date_only: only(&c[1], c.get(5).map(|m| m.as_str())),
            });
        }
    }
    if let Some(c) = YEAR_MONTH.captures(name) {
        if let (Ok(y), Ok(m)) = (c[2].parse(), c[3].parse()) {
            if let Some(d) = PartialDate::month(y, m) {
                return Some(FolderDate {
                    date: d,
                    date_only: only(&c[1], c.get(4).map(|m| m.as_str())),
                });
            }
        }
    }
    if let Some(c) = MONTH_NAME_YEAR.captures(name) {
        if let (Some(m), Ok(y)) = (month_from_name(&c[2]), c[3].parse()) {
            if let Some(d) = PartialDate::month(y, m) {
                return Some(FolderDate {
                    date: d,
                    date_only: only(&c[1], Some(&c[4])),
                });
            }
        }
    }
    if let Some(c) = YEAR_MONTH_NAME.captures(name) {
        if let (Ok(y), Some(m)) = (c[2].parse(), month_from_name(&c[3])) {
            if let Some(d) = PartialDate::month(y, m) {
                return Some(FolderDate {
                    date: d,
                    date_only: only(&c[1], Some(&c[4])),
                });
            }
        }
    }
    if let Some(c) = YEAR.captures(name) {
        if let Ok(y) = c[2].parse() {
            if let Some(d) = PartialDate::year(y) {
                return Some(FolderDate {
                    date: d,
                    date_only: only(&c[1], c.get(3).map(|m| m.as_str())),
                });
            }
        }
    }
    None
}

fn day_from_caps(c: &regex::Captures<'_>) -> Option<PartialDate> {
    let d = parse_ymd(&c[1], &c[2], &c[3])?;
    Some(match (c.get(4), c.get(5), c.get(6)) {
        (Some(h), Some(m), Some(s)) => match (h.as_str().parse(), m.as_str().parse(), s.as_str().parse()) {
            (Ok(h), Ok(m), Ok(s)) => d.with_time(h, m, s),
            _ => d,
        },
        _ => d,
    })
}

fn parse_ymd(y: &str, m: &str, d: &str) -> Option<PartialDate> {
    PartialDate::day(y.parse().ok()?, m.parse().ok()?, d.parse().ok()?)
}

/// Separators and spacing that do not count as a name.
fn is_filler(s: &str) -> bool {
    s.chars()
        .all(|c| c.is_whitespace() || matches!(c, '-' | '_' | '.' | '(' | ')' | '[' | ']'))
}

fn month_from_name(s: &str) -> Option<u8> {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let lower = s.to_ascii_lowercase();
    MONTHS
        .iter()
        .position(|m| lower.starts_with(m))
        .and_then(|i| u8::try_from(i + 1).ok())
}

fn re(pattern: &str) -> Regex {
    // Patterns are compile-time constants covered by tests.
    #[allow(clippy::expect_used)]
    Regex::new(pattern).expect("built-in date pattern must compile")
}

fn is_leap(y: u16) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: u16, m: u8) -> u8 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(y) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Howard Hinnant's days-from-epoch to civil date algorithm.
fn civil_from_days(z: i64) -> (i64, u8, u8) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    // d is 1..=31 and m is 1..=12 by construction.
    (y, u8::try_from(m).unwrap_or(1), u8::try_from(d).unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: u16, m: u8, d: u8) -> PartialDate {
        PartialDate::day(y, m, d).unwrap()
    }

    #[test]
    fn validates_calendar_dates() {
        assert!(PartialDate::day(2012, 2, 29).is_some());
        assert!(PartialDate::day(2011, 2, 29).is_none());
        assert!(PartialDate::day(1900, 2, 29).is_none());
        assert!(PartialDate::day(2000, 2, 29).is_some());
        assert!(PartialDate::day(2012, 4, 31).is_none());
        assert!(PartialDate::month(2012, 13).is_none());
        assert!(PartialDate::year(1949).is_none());
        assert!(PartialDate::year(2100).is_none());
    }

    #[test]
    fn invalid_time_is_dropped_but_date_kept() {
        let d = day(2012, 3, 4).with_time(25, 0, 0);
        assert_eq!(d.time, None);
        assert_eq!(d.to_string(), "2012-03-04");
    }

    #[test]
    fn displays_at_its_precision() {
        assert_eq!(PartialDate::year(2009).unwrap().to_string(), "2009");
        assert_eq!(PartialDate::month(2009, 7).unwrap().to_string(), "2009-07");
        assert_eq!(day(2009, 7, 14).with_time(21, 3, 11).to_string(), "2009-07-14T21:03:11");
        assert_eq!(serde_json::to_string(&day(2009, 7, 14)).unwrap(), "\"2009-07-14\"");
    }

    #[test]
    fn parses_exif_dates() {
        assert_eq!(
            PartialDate::from_exif("2009:07:14 21:03:11").unwrap().to_string(),
            "2009-07-14T21:03:11"
        );
        assert_eq!(
            PartialDate::from_exif("2009:07:14 21:03:11\0").unwrap().to_string(),
            "2009-07-14T21:03:11"
        );
        assert_eq!(PartialDate::from_exif("2009:07:14").unwrap().to_string(), "2009-07-14");
        assert_eq!(PartialDate::from_exif("1920:05:01 00:00:00").unwrap().year, 1920);
    }

    #[test]
    fn rejects_unset_camera_clocks() {
        assert!(PartialDate::from_exif("0000:00:00 00:00:00").is_none());
        assert!(PartialDate::from_exif("    :  :     :  :  ").is_none());
        assert!(PartialDate::from_exif("2009:13:01 00:00:00").is_none());
    }

    #[test]
    fn converts_unix_time() {
        assert_eq!(
            PartialDate::from_unix_seconds(0).unwrap().to_string(),
            "1970-01-01T00:00:00"
        );
        assert_eq!(
            PartialDate::from_unix_seconds(1_247_605_391).unwrap().to_string(),
            "2009-07-14T21:03:11"
        );
        assert_eq!(
            PartialDate::from_unix_seconds(951_782_400).unwrap().to_string(),
            "2000-02-29T00:00:00"
        );
        assert!(PartialDate::from_unix_seconds(-2_000_000_000).is_none());
    }

    #[test]
    fn finds_dates_in_common_file_names() {
        let cases = [
            ("IMG_20120304_153012.jpg", "2012-03-04T15:30:12"),
            ("VID_20120304_153012.mp4", "2012-03-04T15:30:12"),
            ("PXL_20210101_120000123.jpg", "2021-01-01T12:00:00"),
            ("IMG-20150601-WA0001.jpg", "2015-06-01"),
            ("2012-03-04 15.30.12.jpg", "2012-03-04T15:30:12"),
            ("Screenshot 2015-06-01 at 10.11.12.png", "2015-06-01T10:11:12"),
            ("Photo 2010_12_25.jpg", "2010-12-25"),
            ("20091231.JPG", "2009-12-31"),
        ];
        for (name, want) in cases {
            assert_eq!(
                date_from_filename(name).map(|d| d.to_string()).as_deref(),
                Some(want),
                "{name}"
            );
        }
    }

    #[test]
    fn ignores_counters_that_look_like_dates() {
        for name in [
            "DSC00001.JPG",
            "IMG_1234.jpg",
            "100_0001.JPG",
            "P1010001.JPG",
            "120304567890.jpg",
            "IMG_20121399.jpg",
        ] {
            assert_eq!(date_from_filename(name), None, "{name}");
        }
    }

    #[test]
    fn folder_dates_and_whether_the_name_is_only_a_date() {
        let cases = [
            ("2009", "2009", true),
            ("2009-07", "2009-07", true),
            ("2009_07_14", "2009-07-14", true),
            ("20090714", "2009-07-14", true),
            ("2009-07 Spain", "2009-07", false),
            ("Spain 2009", "2009", false),
            ("July 2009", "2009-07", true),
            ("Sept. 2011", "2011-09", true),
            ("Christmas December 2011", "2011-12", false),
            ("2011 December", "2011-12", true),
            ("(2009)", "2009", true),
        ];
        for (name, want, only) in cases {
            let got = date_from_folder(name).unwrap_or_else(|| panic!("no date in {name}"));
            assert_eq!(got.date.to_string(), want, "{name}");
            assert_eq!(got.date_only, only, "{name}");
        }
    }

    #[test]
    fn folders_without_dates() {
        for name in [
            "Holidays",
            "Day 2",
            "100CANON",
            "New Folder (2)",
            "Room 101",
            "Marks party",
        ] {
            assert_eq!(date_from_folder(name), None, "{name}");
        }
    }

    #[test]
    fn best_date_prefers_camera_data() {
        let g = |source| DateGuess {
            date: day(2009, 7, 14),
            source,
        };
        let guesses = [
            g(DateSource::Modified),
            g(DateSource::Folder),
            g(DateSource::Exif),
            g(DateSource::Filename),
        ];
        assert_eq!(best_date(&guesses).unwrap().source, DateSource::Exif);
        assert_eq!(best_date(&[]), None);
    }
}
