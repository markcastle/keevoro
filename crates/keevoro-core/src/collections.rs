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
//!
//! Music and other files (documents and the like, which a person asked
//! Keevoro to carry along) are never grouped by date. They are listed by
//! **the folder they came from**, as the person arranged them (an album of
//! music is its folder): one collection per folder, holding the files
//! directly in it, after every collection of photos and videos, in the
//! order of their folders.
//!
//! Photos and videos from a source that keeps its folders as they are
//! (`crate::arrange`) are grouped the same way: one collection per folder
//! that holds them, in folder order, after the dated ones, never by date,
//! because each is exactly one folder in the backup.

use crate::date::PartialDate;
use crate::media::MediaKind;
use serde::Serialize;
use std::collections::BTreeMap;

/// Days without photos that end one moment and start the next.
pub const MOMENT_GAP_DAYS: i64 = 3;

/// The facts about one file that grouping needs.
#[derive(Debug, Clone)]
pub struct CollectionInput {
    /// For a photo or video, its album; for an other file, the folder it
    /// is in, from the source's own folder down.
    pub album: Vec<String>,
    pub date: Option<PartialDate>,
    pub kind: MediaKind,
    pub bytes: u64,
    /// True when `album` is the file's folders kept as they are.
    pub kept: bool,
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
    pub others: usize,
    /// Tracks of music, playlists among them.
    pub music: usize,
    /// True for a folder of music or other files: listed by the folder,
    /// never by date.
    pub by_folder: bool,
    /// True for a folder of photos and videos from a source that keeps its
    /// folders: shown as pictures, listed in folder order, never by date.
    pub kept_folders: bool,
    /// Index into the input of the file to show large.
    pub cover: usize,
    /// Indices into the input, in date order.
    pub members: Vec<usize>,
}

impl Collection {
    /// "August 2008 · 83 photos · 6 videos", the line under the title.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if let Some(range) = date_range_label(self.first, self.last).filter(|_| !self.by_folder && !self.kept_folders) {
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
        if self.music > 0 {
            parts.push(plural(self.music, "track"));
        }
        if self.others > 0 {
            parts.push(plural(self.others, "other file"));
        }
        parts.join(" \u{b7} ")
    }
}

/// What a folder of other files at the very top of a drive is called.
pub const NOT_IN_A_FOLDER: &str = "Not in a folder";

/// Groups files into collections, newest first, then the folders of other
/// files in folder order.
pub fn group_into_collections(items: &[CollectionInput]) -> Vec<Collection> {
    let mut albums: BTreeMap<&[String], Vec<usize>> = BTreeMap::new();
    let mut folders: BTreeMap<&[String], Vec<usize>> = BTreeMap::new();
    let mut kept: BTreeMap<&[String], Vec<usize>> = BTreeMap::new();
    let mut loose_dated = Vec::new();
    let mut undated = Vec::new();
    for (i, it) in items.iter().enumerate() {
        if it.kind.by_ending_alone() {
            folders.entry(it.album.as_slice()).or_default().push(i);
        } else if it.kept && !it.album.is_empty() {
            kept.entry(it.album.as_slice()).or_default().push(i);
        } else if !it.album.is_empty() {
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
    name_apart(&mut out);
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
    out.extend(kept.into_iter().map(|(album, members)| {
        let (parents, title) = album.split_at(album.len() - 1);
        let mut c = build(items, title[0].clone(), parents.to_vec(), members);
        // Kept as the person arranged them, by name, not by date.
        c.members.sort_by_key(|&i| i);
        c.kept_folders = true;
        c
    }));
    out.extend(folders.into_iter().map(|(folder, members)| {
        let (parents, title) = match folder.split_last() {
            Some((title, parents)) => (parents.to_vec(), title.clone()),
            None => (Vec::new(), NOT_IN_A_FOLDER.to_string()),
        };
        let mut c = build(items, title, parents, members);
        // A folder is listed as the person arranged it, by name.
        c.members.sort_by_key(|&i| i);
        c.by_folder = true;
        c
    }));
    out
}

/// Two moments in the same month would both be called "October 2004",
/// and nobody could tell which was which. Where titles clash, each is
/// named to the day instead: "2 to 3 October 2004", "20 October 2004".
/// Named albums are left alone; their names are the person's own.
fn name_apart(collections: &mut [Collection]) {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for c in collections.iter().filter(|c| c.parents.is_empty()) {
        *seen.entry(c.title.clone()).or_default() += 1;
    }
    for c in collections.iter_mut().filter(|c| c.parents.is_empty()) {
        if seen.get(&c.title).is_some_and(|&n| n > 1) {
            if let Some(to_the_day) = day_range_label(c.first, c.last) {
                c.title = to_the_day;
            }
        }
    }
}

/// "20 October 2004" or "2 to 3 October 2004", when both ends are known to
/// the day and fall in one month. `None` otherwise.
fn day_range_label(first: Option<PartialDate>, last: Option<PartialDate>) -> Option<String> {
    let (a, b) = (first?, last?);
    let (month, from, to) = (a.month?, a.day?, b.day?);
    if a.year != b.year || b.month != Some(month) {
        return None;
    }
    Some(if from == to {
        format!("{from} {} {}", month_name(month), a.year)
    } else {
        format!("{from} to {to} {} {}", month_name(month), a.year)
    })
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
    let of = |kind: MediaKind| members.iter().filter(|&&i| items[i].kind == kind).count();
    let (photos, videos, others) = (of(MediaKind::Photo), of(MediaKind::Video), of(MediaKind::Other));
    let music = of(MediaKind::Music);
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
        others,
        music,
        by_folder: false,
        kept_folders: false,
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

/// "January" for 1, up to "December" for 12.
pub fn month_name(m: u8) -> &'static str {
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
            kept: false,
        }
    }

    #[test]
    fn folders_kept_as_they_are_are_one_collection_each_in_folder_order_after_the_dated_ones() {
        let d = PartialDate::parse("2021-01-05");
        let kept = |album: &[&str]| CollectionInput {
            kept: true,
            ..photo(album, d, 1)
        };
        let items = vec![
            kept(&["Family Scans", "Smith"]),
            photo(&[], d, 1),
            kept(&["Family Scans"]),
            kept(&["Family Scans", "Jones"]),
            kept(&["Family Scans", "Smith"]),
        ];
        let c = group_into_collections(&items);
        let titles: Vec<&str> = c.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, ["January 2021", "Family Scans", "Jones", "Smith"]);
        assert!(c[1..].iter().all(|c| c.kept_folders && !c.by_folder));
        assert_eq!(c[3].members, [0, 4]);
        assert_eq!(c[3].summary(), "2 photos", "no date: a scan's date says nothing");
        assert_eq!(c[3].parents, ["Family Scans"]);
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
    fn two_moments_in_one_month_are_named_to_the_day_so_they_can_be_told_apart() {
        let items = vec![
            photo(&[], day(2004, 10, 2), 1),
            photo(&[], day(2004, 10, 3), 1),
            photo(&[], day(2004, 10, 20), 1),
            photo(&[], day(2005, 6, 1), 1),
        ];
        let titles: Vec<String> = group_into_collections(&items).into_iter().map(|c| c.title).collect();
        // A month with one moment keeps its plain name.
        assert_eq!(titles, ["June 2005", "20 October 2004", "2 to 3 October 2004"]);
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

    fn other(folder: &[&str], date: Option<PartialDate>) -> CollectionInput {
        CollectionInput {
            kind: MediaKind::Other,
            ..photo(folder, date, 2_000)
        }
    }

    #[test]
    fn other_files_are_listed_by_their_folder_after_every_collection_and_never_by_date() {
        let items = vec![
            other(&["Mum", "Documents", "Tax 2019"], day(2019, 4, 1)),
            photo(&["Holidays"], day(2008, 8, 10), 1),
            other(&["Mum", "Music"], day(2001, 1, 1)),
            other(&["Mum", "Documents", "Tax 2019"], day(2010, 1, 1)),
            other(&[], None),
            photo(&[], None, 1),
        ];
        let c = group_into_collections(&items);
        let titles: Vec<(&str, bool)> = c.iter().map(|c| (c.title.as_str(), c.by_folder)).collect();
        assert_eq!(
            titles,
            [
                ("Holidays", false),
                ("Undated", false),
                (NOT_IN_A_FOLDER, true),
                ("Tax 2019", true),
                ("Music", true),
            ]
        );
        let tax = &c[3];
        assert_eq!(tax.parents, ["Mum", "Documents"]);
        assert_eq!(tax.members, [0, 3], "in the order found, not by date");
        assert_eq!((tax.photos, tax.videos, tax.others), (0, 0, 2));
        assert_eq!(tax.summary(), "2 other files", "no dates for a folder");
        assert_eq!(c[0].others, 0);
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

    #[test]
    fn music_is_listed_by_its_album_folder_and_never_by_date() {
        let track = |folder: &[&str], date| CollectionInput {
            kind: MediaKind::Music,
            ..other(folder, date)
        };
        let items = vec![
            track(&["Music", "The Made-Up Band", "Pretend Songs"], day(2019, 1, 2)),
            photo(&[], day(2019, 1, 2), 10),
            track(&["Music", "The Made-Up Band", "Pretend Songs"], day(2011, 5, 6)),
            track(&["Music", "Willow Street Trio", "Kitchen Sessions"], None),
        ];
        let c = group_into_collections(&items);
        assert_eq!(c.len(), 3);
        assert!(!c[0].by_folder && c[0].photos == 1);
        let boats = &c[1];
        assert_eq!(
            (boats.title.as_str(), boats.music, boats.by_folder),
            ("Pretend Songs", 2, true)
        );
        assert_eq!(boats.parents, ["Music", "The Made-Up Band"]);
        assert_eq!(boats.summary(), "2 tracks");
        assert_eq!(c[2].title, "Kitchen Sessions");
    }
}
