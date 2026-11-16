//! Turns scan reports into the plain-English summary the CLI prints.
//! The desktop app will show the same facts as its Discover screen.

use keevoro_core::{group_into_collections, Collection, CollectionInput, DateSource, JunkReason, MediaKind};
use keevoro_scan::{FoundItem, ScanReport};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

#[derive(Debug, Serialize)]
pub struct Summary {
    pub photos: usize,
    pub videos: usize,
    /// Bytes of unique files.
    pub bytes: u64,
    /// `None` when hashing was off, so duplicates are unknown.
    pub duplicates: Option<Duplicates>,
    /// Pictures left out because they do not look like memories, by reason.
    /// They are not counted in `photos` or shown in any collection.
    pub junk: BTreeMap<JunkReason, usize>,
    pub dates: BTreeMap<DateSource, usize>,
    pub undated: usize,
    pub skipped_folders: usize,
    pub unreadable: usize,
    pub collections: Vec<Collection>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Duplicates {
    /// Extra copies beyond the first of each file.
    pub copies: usize,
    pub bytes: u64,
}

/// Keeps one copy of each file across every source: the one that says the
/// most about itself. A photo sitting in `Holidays/Cornwall` on one drive
/// and loose in a backup folder on another belongs in Cornwall, whichever
/// drive was scanned first.
pub fn unique_items(reports: &[ScanReport]) -> (Vec<&FoundItem>, Option<Duplicates>) {
    let all: Vec<&FoundItem> = reports.iter().flat_map(|r| &r.items).collect();
    if all.iter().any(|i| i.blake3.is_none()) {
        return (all, None);
    }
    let mut place: HashMap<Option<&str>, usize> = HashMap::new();
    let mut unique: Vec<&FoundItem> = Vec::new();
    let mut dup = Duplicates { copies: 0, bytes: 0 };
    for item in all {
        match place.get(&item.blake3.as_deref()) {
            Some(&at) => {
                dup.copies += 1;
                dup.bytes += item.bytes;
                if how_well_described(item) < how_well_described(unique[at]) {
                    unique[at] = item;
                }
            }
            None => {
                place.insert(item.blake3.as_deref(), unique.len());
                unique.push(item);
            }
        }
    }
    (unique, Some(dup))
}

/// Ranks copies of the same file; lowest is best. A copy filed deeper in
/// named folders beats a shallower or loose one (`Holidays/Cornwall` says
/// more than `Old stuff`), then a stronger date clue beats a weaker one. A
/// copy in a junk folder is the last resort. Ties keep the first found.
fn how_well_described(item: &FoundItem) -> (bool, std::cmp::Reverse<usize>, bool, Option<DateSource>) {
    let source = item.date.map(|d| d.source);
    (
        item.junk.is_some(),
        std::cmp::Reverse(item.folder.album.len()),
        source.is_none(),
        source,
    )
}

/// Sums up what was found. The items returned are the ones worth keeping
/// (unique and not junk); collection `members` and `cover` index into them.
#[cfg(test)]
pub fn summarise(reports: &[ScanReport]) -> (Summary, Vec<&FoundItem>) {
    summarise_with(reports, &HashSet::new())
}

/// As [`summarise`], but files whose content hash is in `rescued` are
/// treated as ordinary photos even if they look like junk. That is how a
/// picture the user has chosen to keep comes back into its collection.
pub fn summarise_with<'a>(reports: &'a [ScanReport], rescued: &HashSet<String>) -> (Summary, Vec<&'a FoundItem>) {
    let (all_unique, duplicates) = unique_items(reports);
    let mut junk = BTreeMap::new();
    let mut unique = Vec::new();
    for i in all_unique {
        let was_rescued = i.blake3.as_ref().is_some_and(|h| rescued.contains(h));
        match i.junk {
            Some(reason) if !was_rescued => *junk.entry(reason).or_insert(0) += 1,
            _ => unique.push(i),
        }
    }
    let mut dates = BTreeMap::new();
    let mut undated = 0;
    for i in &unique {
        match i.date {
            Some(d) => *dates.entry(d.source).or_insert(0) += 1,
            None => undated += 1,
        }
    }
    let inputs: Vec<CollectionInput> = unique
        .iter()
        .map(|i| CollectionInput {
            album: i.folder.album.clone(),
            // A file's modified time is too weak to place it in a moment.
            date: i.date.filter(|d| d.source != DateSource::Modified).map(|d| d.date),
            kind: i.kind,
            bytes: i.bytes,
        })
        .collect();
    let summary = Summary {
        photos: unique.iter().filter(|i| i.kind == MediaKind::Photo).count(),
        videos: unique.iter().filter(|i| i.kind == MediaKind::Video).count(),
        bytes: unique.iter().map(|i| i.bytes).sum(),
        duplicates,
        junk,
        dates,
        undated,
        skipped_folders: reports.iter().map(|r| r.skipped.len()).sum(),
        unreadable: reports.iter().map(|r| r.unreadable.len()).sum(),
        collections: group_into_collections(&inputs),
    };
    (summary, unique)
}

pub fn render(s: &Summary, max_collections: usize) -> String {
    let mut out = String::new();
    if s.photos + s.videos == 0 {
        out.push_str("No photos or videos found.\n");
        render_junk(&mut out, s);
        render_footer(&mut out, s);
        return out;
    }
    let _ = writeln!(
        out,
        "Found {} and {} ({}).",
        count(s.photos, "photo"),
        count(s.videos, "video"),
        bytes(s.bytes)
    );
    match &s.duplicates {
        Some(d) if d.copies > 0 => {
            let _ = writeln!(
                out,
                "{} ({}) will be kept once.",
                count(d.copies, "duplicate"),
                bytes(d.bytes)
            );
        }
        Some(_) => out.push_str("No duplicates.\n"),
        None => out.push_str("Duplicates not checked (hashing was off).\n"),
    }
    render_junk(&mut out, s);

    out.push_str("\nCollections\n");
    let width = s
        .collections
        .iter()
        .take(max_collections)
        .map(|c| c.title.chars().count())
        .max()
        .unwrap_or(0);
    for c in s.collections.iter().take(max_collections) {
        let _ = write!(out, "  {:<width$}  {}", c.title, c.summary());
        if !c.parents.is_empty() {
            let _ = write!(out, "  ({})", c.parents.join(" / "));
        }
        out.push('\n');
    }
    if s.collections.len() > max_collections {
        let _ = writeln!(out, "  and {} more", s.collections.len() - max_collections);
    }

    out.push('\n');
    let mut clues = Vec::new();
    for (source, words) in [
        (DateSource::Exif, "from camera data"),
        (DateSource::Video, "from video data"),
        (DateSource::Filename, "from file names"),
        (DateSource::Folder, "from folder names"),
        (DateSource::Modified, "guessed from file times"),
    ] {
        if let Some(n) = s.dates.get(&source) {
            clues.push(format!("{} {words}", thousands(*n)));
        }
    }
    if s.undated > 0 {
        clues.push(format!("{} undated", thousands(s.undated)));
    }
    let _ = writeln!(out, "Dates: {}.", clues.join(", "));
    render_footer(&mut out, s);
    out
}

/// Junk is never hidden: the user is told what was left out and why.
fn render_junk(out: &mut String, s: &Summary) {
    let total: usize = s.junk.values().sum();
    if total == 0 {
        return;
    }
    let _ = writeln!(
        out,
        "Left out {} that {} look like memories:",
        count(total, "picture"),
        if total == 1 { "doesn't" } else { "don't" }
    );
    for (reason, n) in &s.junk {
        let _ = writeln!(out, "  {}  {}", thousands(*n), reason.describe());
    }
}

fn render_footer(out: &mut String, s: &Summary) {
    if s.skipped_folders > 0 {
        let _ = writeln!(
            out,
            "Skipped {} (system, app and hidden).",
            count(s.skipped_folders, "folder")
        );
    }
    if s.unreadable > 0 {
        let what = if s.unreadable == 1 {
            "1 file or folder".to_string()
        } else {
            format!("{} files or folders", thousands(s.unreadable))
        };
        let _ = writeln!(out, "Couldn't read {what}. Run again with --json to see which.");
    }
}

/// "1 photo", "9,870 photos".
pub fn count(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{} {word}s", thousands(n))
    }
}

pub fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Sizes the way Windows Explorer and Finder users read them.
pub fn bytes(b: u64) -> String {
    const UNITS: [&str; 5] = ["bytes", "KB", "MB", "GB", "TB"];
    #[allow(clippy::cast_precision_loss)]
    let mut v = b as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{b} bytes")
    } else {
        format!("{v:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use keevoro_core::{DateGuess, FolderHints, PartialDate};
    use std::path::PathBuf;

    fn found(
        name: &str,
        hash: Option<&str>,
        album: &[&str],
        date: Option<(DateSource, PartialDate)>,
        kind: MediaKind,
    ) -> FoundItem {
        FoundItem {
            path: PathBuf::from(name),
            kind,
            mime: "image/jpeg".into(),
            bytes: 1024 * 1024,
            blake3: hash.map(str::to_string),
            hashed: true,
            date: date.map(|(source, date)| DateGuess { date, source }),
            folder: FolderHints {
                album: album.iter().map(|s| (*s).to_string()).collect(),
                date: None,
            },
            width: None,
            height: None,
            junk: None,
        }
    }

    fn aug(d: u8) -> Option<(DateSource, PartialDate)> {
        Some((DateSource::Exif, PartialDate::day(2008, 8, d).unwrap()))
    }

    #[test]
    fn duplicates_across_sources_are_counted_once() {
        let a = ScanReport {
            items: vec![
                found("1.jpg", Some("aa"), &["Cornwall"], aug(1), MediaKind::Photo),
                found("2.jpg", Some("bb"), &["Cornwall"], aug(2), MediaKind::Photo),
            ],
            ..Default::default()
        };
        let b = ScanReport {
            items: vec![found("1 copy.jpg", Some("aa"), &["Cornwall"], aug(1), MediaKind::Photo)],
            ..Default::default()
        };
        let reports = [a, b];
        let (s, unique) = summarise(&reports);
        assert_eq!(unique.len(), 2);
        assert_eq!(s.photos, 2);
        assert_eq!(
            s.duplicates,
            Some(Duplicates {
                copies: 1,
                bytes: 1024 * 1024
            })
        );
        assert_eq!(s.collections.len(), 1);
        assert_eq!(s.collections[0].photos, 2);
    }

    #[test]
    fn the_best_described_copy_of_a_duplicate_is_the_one_kept() {
        let loose = found("copy.jpg", Some("aa"), &[], None, MediaKind::Photo);
        let filename_dated = found(
            "IMG_20080801.jpg",
            Some("aa"),
            &[],
            Some((DateSource::Filename, PartialDate::day(2008, 8, 1).unwrap())),
            MediaKind::Photo,
        );
        let shallow = found("0.jpg", Some("aa"), &["Old stuff"], aug(1), MediaKind::Photo);
        let in_album = found("1.jpg", Some("aa"), &["Holidays", "Cornwall"], None, MediaKind::Photo);
        let mut cached = found("f_00a1", Some("aa"), &["Cornwall"], aug(1), MediaKind::Photo);
        cached.junk = Some(JunkReason::BrowserCache);
        let other = found("2.jpg", Some("bb"), &[], None, MediaKind::Photo);

        let reports = [
            ScanReport {
                items: vec![loose.clone(), other, filename_dated.clone()],
                ..Default::default()
            },
            ScanReport {
                items: vec![cached, shallow, in_album],
                ..Default::default()
            },
        ];
        let (unique, dup) = unique_items(&reports);
        let names: Vec<_> = unique.iter().map(|i| i.path.to_string_lossy().to_string()).collect();
        assert_eq!(
            names,
            ["1.jpg", "2.jpg"],
            "the album copy wins and keeps its place in the list"
        );
        assert_eq!(dup.unwrap().copies, 4);

        let dated_wins = [ScanReport {
            items: vec![loose, filename_dated],
            ..Default::default()
        }];
        assert_eq!(
            unique_items(&dated_wins).0[0].path.to_string_lossy(),
            "IMG_20080801.jpg"
        );
    }

    #[test]
    fn duplicates_unknown_without_hashes() {
        let r = [ScanReport {
            items: vec![found("1.jpg", None, &[], None, MediaKind::Photo)],
            ..Default::default()
        }];
        let (s, _) = summarise(&r);
        assert_eq!(s.duplicates, None);
        assert!(render(&s, 10).contains("Duplicates not checked"));
    }

    #[test]
    fn modified_times_do_not_create_fake_moments() {
        let r = [ScanReport {
            items: vec![found(
                "1.jpg",
                Some("aa"),
                &[],
                Some((DateSource::Modified, PartialDate::day(2015, 1, 1).unwrap())),
                MediaKind::Photo,
            )],
            ..Default::default()
        }];
        let (s, _) = summarise(&r);
        assert_eq!(s.collections[0].title, "Undated");
        assert_eq!(s.dates.get(&DateSource::Modified), Some(&1));
    }

    #[test]
    fn renders_like_the_discover_screen() {
        let r = [ScanReport {
            items: vec![
                found("1.jpg", Some("aa"), &["Holidays", "Cornwall"], aug(1), MediaKind::Photo),
                found("2.mov", Some("bb"), &["Holidays", "Cornwall"], aug(2), MediaKind::Video),
                found(
                    "3.jpg",
                    Some("cc"),
                    &[],
                    Some((DateSource::Filename, PartialDate::day(2011, 12, 25).unwrap())),
                    MediaKind::Photo,
                ),
            ],
            ..Default::default()
        }];
        let (s, _) = summarise(&r);
        let text = render(&s, 10);
        assert!(
            text.starts_with("Found 2 photos and 1 video (3.0 MB).\nNo duplicates.\n"),
            "{text}"
        );
        assert!(
            text.contains("Cornwall       August 2008 \u{b7} 1 photo \u{b7} 1 video  (Holidays)"),
            "{text}"
        );
        assert!(text.contains("December 2011  1 photo"), "{text}");
        assert!(text.contains("Dates: 2 from camera data, 1 from file names."), "{text}");
        assert!(!text.contains("Skipped"));
    }

    #[test]
    fn junk_is_left_out_but_never_hidden() {
        let mut icon = found("save.png", Some("cc"), &["App"], None, MediaKind::Photo);
        icon.junk = Some(JunkReason::AppIcons);
        let mut tiny = found("t.jpg", Some("dd"), &["Cornwall"], aug(3), MediaKind::Photo);
        tiny.junk = Some(JunkReason::TinyImage);
        let mut tiny_copy = tiny.clone();
        tiny_copy.path = "t copy.jpg".into();
        let r = [ScanReport {
            items: vec![
                found("1.jpg", Some("aa"), &["Cornwall"], aug(1), MediaKind::Photo),
                icon,
                tiny,
                tiny_copy,
            ],
            ..Default::default()
        }];
        let (s, kept) = summarise(&r);
        assert_eq!((s.photos, kept.len()), (1, 1));
        assert_eq!(s.collections.len(), 1, "no collection is made of junk alone");
        assert_eq!(s.collections[0].photos, 1);
        assert_eq!(
            s.junk.get(&JunkReason::TinyImage),
            Some(&1),
            "a duplicate of junk is counted once"
        );
        let text = render(&s, 10);
        assert!(
            text.contains(
                "Left out 2 pictures that don't look like memories:\n  1  Looks like app icons\n  1  Tiny images (under 300 px)\n"
            ),
            "{text}"
        );
    }

    #[test]
    fn a_drive_of_nothing_but_junk_still_explains_itself() {
        let mut icon = found("save.png", Some("cc"), &[], None, MediaKind::Photo);
        icon.junk = Some(JunkReason::AppIcons);
        let r = [ScanReport {
            items: vec![icon],
            ..Default::default()
        }];
        let text = render(&summarise(&r).0, 10);
        assert!(
            text.starts_with("No photos or videos found.\nLeft out 1 picture that doesn't look like memories:"),
            "{text}"
        );
    }

    #[test]
    fn caps_the_collection_list() {
        let items = (1..=5)
            .map(|i| {
                found(
                    &format!("{i}.jpg"),
                    Some(&i.to_string()),
                    &[&format!("Album {i}")],
                    None,
                    MediaKind::Photo,
                )
            })
            .collect();
        let r = [ScanReport {
            items,
            ..Default::default()
        }];
        let (s, _) = summarise(&r);
        assert!(render(&s, 2).contains("  and 3 more\n"));
    }

    #[test]
    fn empty_scan_says_so() {
        let r = [ScanReport {
            unreadable: vec![keevoro_scan::Unreadable {
                path: "x".into(),
                error: "e".into(),
            }],
            ..Default::default()
        }];
        let (s, _) = summarise(&r);
        let text = render(&s, 10);
        assert!(text.starts_with("No photos or videos found.\n"));
        assert!(text.contains("Couldn't read 1 file or folder."));
    }

    #[test]
    fn numbers_read_naturally() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1,000");
        assert_eq!(thousands(9_870_123), "9,870,123");
        assert_eq!(count(1, "photo"), "1 photo");
        assert_eq!(count(2104, "duplicate"), "2,104 duplicates");
        assert_eq!(bytes(512), "512 bytes");
        assert_eq!(bytes(1536), "1.5 KB");
        assert_eq!(bytes(44_023_414_784), "41.0 GB");
    }
}
