//! Grouping found files into collections, the unit the Discover screen shows.
//!
//! People think in moments ("Cornwall, August 2008"), not folders and files.
//! A collection is either:
//!
//! - **A named album** from the user's own folders (`Holidays / Cornwall`),
//!   titled with its innermost name, or
//! - **A moment** of unnamed files grouped by time, titled with its date
//!   range ("August 2008"). A new moment starts after a gap of more than
//!   [`MOMENT_GAP_DAYS`] days.
//!
//! Files with no believable date and no album go into one "Undated"
//! collection.

use crate::date::PartialDate;
use crate::media::MediaKind;
use serde::Serialize;
use std::collections::BTreeMap;

/// Days without photos that end one moment and start the next.
pub const MOMENT_GAP_DAYS: i64 = 3;

/// The facts about one file that grouping needs.
#[derive(Debug, Clone)]
pub struct CollectionInput {
    pub album: Vec<String>,
    pub date: Option<PartialDate>,
    pub kind: MediaKind,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Collection {
    /// "Cornwall", "August 2008" or "Undated".
    pub title: String,
    /// Outer album names, for a quiet subtitle like "Holidays".
    pub parents: Vec<String>,
    pub first: Option<PartialDate>,
    pub last: Option<PartialDate>,
    pub photos: usize,
    pub videos: usize,
    /// Index into the input of the file to show large.
    pub cover: usize,
    /// Indices into the input, in date order.
    pub members: Vec<usize>,
}

impl Collection {
    /// "August 2008 · 83 photos · 6 videos", the line under the title.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if let Some(range) = date_range_label(self.first, self.last) {
            if range != self.title {
                parts.push(range);
            }
        }
        if self.photos > 0 {
            parts.push(plural(self.photos, "photo"));
        }
        if self.videos > 0 {
            parts.push(plural(self.videos, "video"));
        }
        parts.join(" \u{b7} ")
    }
}

/// Groups files into collections, newest first.
pub fn group_into_collections(items: &[CollectionInput]) -> Vec<Collection> {
    let mut albums: BTreeMap<&[String], Vec<usize>> = BTreeMap::new();
    let mut loose_dated = Vec::new();
    let mut undated = Vec::new();
    for (i, it) in items.iter().enumerate() {
        if !it.album.is_empty() {
            albums.entry(it.album.as_slice()).or_default().push(i);
        } else if it.date.is_some() {
            loose_dated.push(i);
        } else {
            undated.push(i);
        }
    }

    let mut out: Vec<Collection> = albums
        .into_iter()
        .map(|(album, members)| {
            let (parents, title) = album.split_at(album.len() - 1);
            build(items, title[0].clone(), parents.to_vec(), members)
        })
        .collect();

    loose_dated.sort_by_key(|&i| items[i].date);
    let mut moment: Vec<usize> = Vec::new();
    for i in loose_dated {
        if let (Some(&prev), Some(d)) = (moment.last(), items[i].date) {
            if let Some(p) = items[prev].date {
                if day_number(d) - day_number(p) > MOMENT_GAP_DAYS {
                    out.push(moment_collection(items, std::mem::take(&mut moment)));
                }
            }
        }
        moment.push(i);
    }
    if !moment.is_empty() {
        out.push(moment_collection(items, moment));
    }
    if !undated.is_empty() {
        out.push(build(items, "Undated".into(), Vec::new(), undated));
    }

    // Newest first; undated last.
    out.sort_by(|a, b| {
        b.last
            .is_some()
            .cmp(&a.last.is_some())
            .then(b.last.cmp(&a.last))
            .then(a.title.cmp(&b.title))
    });
    out
}

fn moment_collection(items: &[CollectionInput], members: Vec<usize>) -> Collection {
    let mut c = build(items, String::new(), Vec::new(), members);
    c.title = date_range_label(c.first, c.last).unwrap_or_else(|| "Undated".into());
    c
}

fn build(items: &[CollectionInput], title: String, parents: Vec<String>, mut members: Vec<usize>) -> Collection {
    members.sort_by_key(|&i| (items[i].date, i));
    let dates = members.iter().filter_map(|&i| items[i].date);
    let first = dates.clone().min();
    let last = dates.max();
    let photos = members.iter().filter(|&&i| items[i].kind == MediaKind::Photo).count();
    let videos = members.len() - photos;
    // The largest photo is a decent first guess at the best one: originals
    // beat thumbnails and resized copies. Fall back to a video.
    let cover = members
        .iter()
        .copied()
        .max_by_key(|&i| (items[i].kind == MediaKind::Photo, items[i].bytes))
        .unwrap_or(0);
    Collection {
        title,
        parents,
        first,
        last,
        photos,
        videos,
        cover,
        members,
    }
}

/// "August 2008", "August to September 2008", "December 2008 to January 2009",
/// "2008 to 2010".
pub fn date_range_label(first: Option<PartialDate>, last: Option<PartialDate>) -> Option<String> {
    let (a, b) = match (first, last) {
        (Some(a), Some(b)) => (a, b),
        (Some(a), None) | (None, Some(a)) => (a, a),
        (None, None) => return None,
    };
    let label = match (a.month, b.month) {
        (Some(ma), Some(mb)) if a.year == b.year && ma == mb => {
            format!("{} {}", month_name(ma), a.year)
        }
        (Some(ma), Some(mb)) if a.year == b.year => {
            format!("{} to {} {}", month_name(ma), month_name(mb), a.year)
        }
        (Some(ma), Some(mb)) if b.year - a.year <= 1 => {
            format!("{} {} to {} {}", month_name(ma), a.year, month_name(mb), b.year)
        }
        _ if a.year == b.year => a.year.to_string(),
        _ => format!("{} to {}", a.year, b.year),
    };
    Some(label)
}

fn month_name(m: u8) -> &'static str {
    const NAMES: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    NAMES.get(usize::from(m.saturating_sub(1))).copied().unwrap_or("")
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// Days since a fixed epoch, using the first day of an unknown month or year.
fn day_number(d: PartialDate) -> i64 {
    let y = i64::from(d.year);
    let m = i64::from(d.month.unwrap_or(1));
    let day = i64::from(d.day.unwrap_or(1));
    // Days from civil, Howard Hinnant.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: u16, m: u8, d: u8) -> Option<PartialDate> {
        PartialDate::day(y, m, d)
    }

    fn photo(album: &[&str], date: Option<PartialDate>, bytes: u64) -> CollectionInput {
        CollectionInput {
            album: album.iter().map(|s| (*s).to_string()).collect(),
            date,
            kind: MediaKind::Photo,
            bytes,
        }
    }

    fn video(album: &[&str], date: Option<PartialDate>) -> CollectionInput {
        CollectionInput {
            kind: MediaKind::Video,
            ..photo(album, date, 50_000_000)
        }
    }

    #[test]
    fn named_albums_become_collections_titled_by_their_innermost_name() {
        let items = vec![
            photo(&["Holidays", "Cornwall"], day(2008, 8, 10), 3_000_000),
            photo(&["Holidays", "Cornwall"], day(2008, 8, 12), 4_000_000),
            video(&["Holidays", "Cornwall"], day(2008, 8, 11)),
        ];
        let c = group_into_collections(&items);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].title, "Cornwall");
        assert_eq!(c[0].parents, vec!["Holidays"]);
        assert_eq!(c[0].summary(), "August 2008 \u{b7} 2 photos \u{b7} 1 video");
        assert_eq!(c[0].cover, 1, "largest photo is the cover, not the bigger video");
        assert_eq!(c[0].members, vec![0, 2, 1], "members are in date order");
    }

    #[test]
    fn loose_files_split_into_moments_by_gaps() {
        let items = vec![
            photo(&[], day(2011, 12, 25), 1),
            photo(&[], day(2011, 12, 24), 1),
            photo(&[], day(2012, 3, 2), 1),
            photo(&[], day(2012, 3, 3), 1),
        ];
        let c = group_into_collections(&items);
        let titles: Vec<_> = c.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, vec!["March 2012", "December 2011"], "newest first");
        assert_eq!(c[1].members, vec![1, 0]);
        assert_eq!(c[1].summary(), "2 photos", "range is not repeated when it is the title");
    }

    #[test]
    fn undated_unnamed_files_go_last() {
        let items = vec![photo(&[], None, 1), photo(&["Garden"], day(2010, 5, 1), 1)];
        let c = group_into_collections(&items);
        assert_eq!(c.last().unwrap().title, "Undated");
        assert_eq!(c[0].title, "Garden");
    }

    #[test]
    fn album_without_dates_still_works() {
        let items = vec![photo(&["Scans"], None, 1)];
        let c = group_into_collections(&items);
        assert_eq!(c[0].title, "Scans");
        assert_eq!(c[0].summary(), "1 photo");
    }

    #[test]
    fn date_range_labels_read_naturally() {
        let m = |y, mo| PartialDate::month(y, mo);
        assert_eq!(date_range_label(m(2008, 8), m(2008, 8)).unwrap(), "August 2008");
        assert_eq!(
            date_range_label(m(2008, 8), m(2008, 9)).unwrap(),
            "August to September 2008"
        );
        assert_eq!(
            date_range_label(m(2008, 12), m(2009, 1)).unwrap(),
            "December 2008 to January 2009"
        );
        assert_eq!(date_range_label(m(2008, 1), m(2010, 6)).unwrap(), "2008 to 2010");
        assert_eq!(date_range_label(PartialDate::year(2009), None).unwrap(), "2009");
        assert_eq!(date_range_label(None, None), None);
    }

    #[test]
    fn day_numbers_count_days() {
        let a = day_number(PartialDate::day(2008, 2, 28).unwrap());
        let b = day_number(PartialDate::day(2008, 3, 1).unwrap());
        assert_eq!(b - a, 2, "2008 is a leap year");
        let c = day_number(PartialDate::day(2009, 1, 1).unwrap());
        let d = day_number(PartialDate::day(2008, 12, 31).unwrap());
        assert_eq!(c - d, 1);
    }
}
