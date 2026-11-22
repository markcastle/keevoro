//! Turns scan reports into the facts the Discover screen shows: what was
//! found, the collections, duplicates and what was set aside, and the way
//! numbers and sizes are said.

use keevoro_core::{group_into_collections, Collection, CollectionInput, DateSource, JunkReason, MediaKind};
use keevoro_scan::{FoundItem, ScanReport};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Serialize)]
pub struct Summary {
    pub photos: usize,
    pub videos: usize,
    /// Other files a person asked Keevoro to carry along.
    pub others: usize,
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

/// Every file that was found more than once: the copy that will be kept,
/// then the others, which will not be sent because the kept one is the
/// same file. Groups are in the order their first copy was found. The kept
/// copy is chosen exactly as [`unique_items`] chooses it, so this list and
/// what is sent can never disagree.
pub fn duplicate_groups(reports: &[ScanReport]) -> Vec<(&FoundItem, Vec<&FoundItem>)> {
    let mut place: HashMap<&str, usize> = HashMap::new();
    let mut groups: Vec<Vec<&FoundItem>> = Vec::new();
    for item in reports.iter().flat_map(|r| &r.items) {
        let Some(hash) = item.blake3.as_deref() else { continue };
        match place.get(hash) {
            Some(&at) => groups[at].push(item),
            None => {
                place.insert(hash, groups.len());
                groups.push(vec![item]);
            }
        }
    }
    groups
        .into_iter()
        .filter(|g| g.len() > 1)
        .map(|mut group| {
            // The first of the best described, as `unique_items` keeps.
            let mut best = 0;
            for at in 1..group.len() {
                if how_well_described(group[at]) < how_well_described(group[best]) {
                    best = at;
                }
            }
            let kept = group.remove(best);
            (kept, group)
        })
        .collect()
}

/// Ranks copies of the same file; lowest is best. A copy from a source that
/// keeps its folders as they are beats every other. Then a copy filed deeper in
/// named folders beats a shallower or loose one (`Holidays/Cornwall` says
/// more than `Old stuff`), then a stronger date clue beats a weaker one. A
/// copy in a junk folder is the last resort. Ties keep the first found.
fn how_well_described(item: &FoundItem) -> (bool, bool, std::cmp::Reverse<usize>, bool, Option<DateSource>) {
    let source = item.date.map(|d| d.source);
    (
        item.junk.is_some(),
        // A copy from a source that keeps its folders is the one kept, so a
        // person who chose to keep their folders finds it in them.
        !item.folder.kept,
        std::cmp::Reverse(item.folder.album.len()),
        source.is_none(),
        source,
    )
}

/// Said once by the first backup of a backup nobody was asked how to
/// arrange.
pub const ARRANGED_BY_DATE_UNASKED: &str = "Arranged by date. To keep your folders as they are instead, choose so in \
     Settings or run `keevoro arrange folders`.";

/// The words that mark the closing account of a destination's originals
/// among what a backup says ([`account_words`]).
pub const ACCOUNT_MARK: &str = " you are keeping ";

/// The closing account of one destination's originals: how many of the
/// files being kept are safely there now, and how many still need
/// attention and why. Every number comes from one
/// [`keevoro_publish::Account`], which always adds up, so this sentence
/// can never contradict the lines before it.
pub fn account_words(place: &str, account: &keevoro_publish::Account) -> String {
    let n = thousands;
    let given = account.given;
    let attention = account.attention();
    if attention == 0 {
        return if given == 1 {
            format!("The 1 file{ACCOUNT_MARK}is safely at {place}.")
        } else {
            format!("All {} files{ACCOUNT_MARK}are safely at {place}.", n(given))
        };
    }
    let mut why = Vec::new();
    if account.failed > 0 {
        why.push(format!("{} couldn't be sent", n(account.failed)));
    }
    if account.not_tried > 0 {
        why.push(format!(
            "{} {} not tried before the backup was stopped",
            n(account.not_tried),
            if account.not_tried == 1 { "was" } else { "were" }
        ));
    }
    if account.waiting_for_place > 0 {
        why.push(format!(
            "{} {} waiting for it to come back",
            n(account.waiting_for_place),
            if account.waiting_for_place == 1 { "is" } else { "are" }
        ));
    }
    format!(
        "{} of the {} {}{ACCOUNT_MARK}{} safely at {place}. {} still {} attention: {}.",
        n(account.safe()),
        n(given),
        if given == 1 { "file" } else { "files" },
        if account.safe() == 1 { "is" } else { "are" },
        n(attention),
        if attention == 1 { "needs" } else { "need" },
        why.join(" and ")
    )
}

/// Files that could not be sent, said in groups: one sentence for each side
/// and cause, with how many, the first few names and how many more, and
/// "could not be backed up again" for those that failed last time too.
/// Each comes with a short form for the stage's red mark. Largest first.
pub fn failure_groups(
    failed: &[&keevoro_publish::FailedUpload],
    place: &str,
    across_a_network: bool,
    last_time: &std::collections::HashSet<String>,
) -> Vec<(String, String)> {
    use keevoro_core::trouble::{self, Side};
    let mut groups: Vec<((Side, trouble::Cause, bool), Vec<String>)> = Vec::new();
    for f in failed {
        let side = f.side.unwrap_or(Side::Destination);
        let key = (side, f.cause, last_time.contains(&f.key));
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, names)) => names.push(f.key.clone()),
            None => groups.push((key, vec![f.key.clone()])),
        }
    }
    groups.sort_by_key(|g| std::cmp::Reverse(g.1.len()));
    groups
        .into_iter()
        .map(|((side, cause, again), names)| {
            let why = trouble::cause_words(cause, side, place, across_a_network, names.len() == 1);
            let short = format!("Couldn't back up {}: {why}", count(names.len(), "file"));
            (trouble::group_words(&names, &why, again), short)
        })
        .collect()
}

/// Is this one of the lines that say files could not be backed up?
pub fn is_failure_line(line: &str) -> bool {
    line.starts_with("Couldn't back up")
        || (line.starts_with("These ") && line.contains(" could not be backed up again"))
        || line.starts_with("This file could not be backed up again")
}

/// A place that went away part-way through and was not back by the end.
pub fn place_went_away_words(place: &str, files: usize, across_a_network: bool) -> String {
    let went = if across_a_network {
        format!("The connection to {place} was lost part-way through and had not come back by the end")
    } else {
        format!(
            "{} went away part-way through and had not come back by the end",
            crate::confirm::capitalised(place)
        )
    };
    format!(
        "{went}, so {} {} not sent. Everything already sent is safe there. Back up again once it is back.",
        count(files, "file"),
        if files == 1 { "was" } else { "were" }
    )
}

/// The words that mark [`check_advised_words`] among what a backup says.
pub const CHECK_ADVISED_MARK: &str = "\"Check they all arrived\" looks at each one again";

/// What a backup says when a place's connection was lost during it: every
/// copy counted was proved at its place, and a check from a fresh
/// connection is the way to be certain.
pub fn check_advised_words(place: &str, across_a_network: bool) -> String {
    let went = if across_a_network {
        format!("The connection to {place} was lost during this backup")
    } else {
        format!("{} went away during this backup", crate::confirm::capitalised(place))
    };
    format!(
        "{went}. Every copy Keevoro counted was read back at its place before it was counted; to be certain, \
         \"Check they all arrived\" looks at each one again from a fresh connection."
    )
}

/// The words that mark [`source_unplugged`] among what a backup says.
pub const UNPLUGGED_PART_WAY: &str = " was unplugged part-way through, so ";

/// A backup that could not read some files because their source went
/// part-way: the drive unplugged.
pub fn source_unplugged(name: &str, files: usize) -> String {
    format!(
        "{name}{UNPLUGGED_PART_WAY}{} from it {} not backed up. Everything already sent is safe; plug it in and \
         back up again to carry on.",
        count(files, "file"),
        if files == 1 { "was" } else { "were" }
    )
}

/// How long ago a moment was, both given in seconds since 1970, as a
/// person says it: "just now", "5 minutes ago", "3 hours ago", "2 days
/// ago", and the date beyond a month.
pub fn ago(then: i64, now: i64) -> String {
    let gone = now.saturating_sub(then).max(0);
    let say = |n: i64, unit: &str| format!("{} ago", count(usize::try_from(n).unwrap_or(0), unit));
    match gone {
        0..60 => "just now".to_string(),
        60..3_600 => say(gone / 60, "minute"),
        3_600..86_400 => say(gone / 3_600, "hour"),
        86_400..2_592_000 => say(gone / 86_400, "day"),
        _ => match keevoro_core::PartialDate::from_unix_seconds(then) {
            Some(d) => format!("on {:04}-{:02}-{:02}", d.year, d.month.unwrap_or(1), d.day.unwrap_or(1)),
            None => "a long time ago".to_string(),
        },
    }
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
            kept: i.folder.kept,
        })
        .collect();
    let summary = Summary {
        photos: unique.iter().filter(|i| i.kind == MediaKind::Photo).count(),
        videos: unique.iter().filter(|i| i.kind == MediaKind::Video).count(),
        others: unique.iter().filter(|i| i.kind == MediaKind::Other).count(),
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

/// "1 photo", "9,870 photos": [`keevoro_core::words::count`], the one way
/// counts are said.
pub use keevoro_core::words::{count, files, thousands};

/// What is said about files in `place` that could be photos or videos but
/// are only in online storage, not on this computer, so were not opened:
/// how many, how big together when that is known, which service holds them
/// (from the folder's own name, else the one this computer's system has),
/// and how to include them. The same on screen, at the command line and to
/// an AI app. Nothing when there are none.
pub fn online_only(n: usize, bytes: Option<u64>, place: &std::path::Path) -> Option<String> {
    // In a bucket source, these are objects in archive storage.
    if let Some(provider) = keevoro_core::bucket::provider_of(place) {
        return archived(n, bytes.unwrap_or_default(), provider);
    }
    let service = keevoro_core::synced::service_of(place).map_or(keevoro_scan::online::SERVICE, |s| s.name());
    online_only_in(n, bytes, service)
}

fn online_only_in(n: usize, bytes: Option<u64>, service: &str) -> Option<String> {
    let (are, them) = if n == 1 { ("is", "it") } else { ("are", "them") };
    let how = format!("To include {them}, make {them} available offline in {service} first, then look again.");
    (n > 0).then(|| match bytes {
        Some(bytes) => format!(
            "{}, {}, {are} in {service} only, not on this computer, so Keevoro did not open {them}. {how}",
            count(n, "file"),
            self::bytes(bytes)
        ),
        None => format!(
            "{} here {are} in {service} only, not on this computer, so Keevoro did not open {them}. {how}",
            count(n, "file")
        ),
    })
}

/// What is said of files in a bucket source that are in archive storage
/// (Amazon's Glacier Flexible Retrieval or Deep Archive), which cannot be
/// read until they are restored, so are not backed up.
fn archived(n: usize, bytes: u64, provider: &str) -> Option<String> {
    let (is, it) = if n == 1 { ("is", "it") } else { ("are", "them") };
    (n > 0).then(|| {
        format!(
            "{}, {}, {is} in archive storage, which can't be read until {} restored. Keevoro can't do that yet; \
             to include {it}, restore {it} in {provider} first, then look again.",
            count(n, "file"),
            self::bytes(bytes),
            if n == 1 { "it is" } else { "they are" }
        )
    })
}

/// What is said of files in a bucket source kept in one of Google's colder
/// classes (Nearline, Coldline or Archive), which can be read at once but
/// cost more to read. They are backed up all the same.
pub fn colder(n: usize, bytes: u64) -> Option<String> {
    let (are, them) = if n == 1 { ("is", "it") } else { ("are", "them") };
    (n > 0).then(|| {
        format!(
            "{}, {}, {are} in Google’s Nearline, Coldline or Archive storage, which costs more to read out. \
             Keevoro backs {them} up all the same.",
            count(n, "file"),
            self::bytes(bytes)
        )
    })
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
                kept: false,
            },
            width: None,
            height: None,
            junk: None,
        }
    }

    #[test]
    fn files_only_online_are_said_plainly_with_how_much_and_how_to_include_them() {
        assert_eq!(
            online_only_in(412, Some(19_756_849_561), "OneDrive").unwrap(),
            "412 files, 18.4 GB, are in OneDrive only, not on this computer, so Keevoro did not open them. To \
             include them, make them available offline in OneDrive first, then look again."
        );
        assert_eq!(
            online_only_in(1, Some(2_202_009), "Dropbox").unwrap(),
            "1 file, 2.1 MB, is in Dropbox only, not on this computer, so Keevoro did not open it. To include it, \
             make it available offline in Dropbox first, then look again."
        );
        // Looked through by a version that only counted them: said as it was.
        assert_eq!(
            online_only_in(412, None, "OneDrive").unwrap(),
            "412 files here are in OneDrive only, not on this computer, so Keevoro did not open them. To include \
             them, make them available offline in OneDrive first, then look again."
        );
        assert_eq!(online_only(0, None, std::path::Path::new("/x")), None);
        // The service is the folder's own when its name says, else this
        // computer's.
        let dropbox = online_only(2, None, std::path::Path::new("/Users/mark/Dropbox/Camera Uploads")).unwrap();
        assert!(dropbox.contains("in Dropbox only"), "{dropbox}");
        let plain = online_only(2, None, std::path::Path::new("/media/card")).unwrap();
        assert!(plain.contains(keevoro_scan::online::SERVICE), "{plain}");
    }

    fn aug(d: u8) -> Option<(DateSource, PartialDate)> {
        Some((DateSource::Exif, PartialDate::day(2008, 8, d).unwrap()))
    }

    #[test]
    fn each_duplicate_is_listed_with_the_copy_kept_and_the_copies_left_out() {
        let a = ScanReport {
            items: vec![
                found("loose.jpg", Some("aa"), &[], aug(1), MediaKind::Photo),
                found("2.jpg", Some("bb"), &["Cornwall"], aug(2), MediaKind::Photo),
            ],
            ..Default::default()
        };
        let b = ScanReport {
            items: vec![
                found(
                    "filed.jpg",
                    Some("aa"),
                    &["Holidays", "Cornwall"],
                    aug(1),
                    MediaKind::Photo,
                ),
                found("again.jpg", Some("aa"), &[], aug(1), MediaKind::Photo),
            ],
            ..Default::default()
        };
        let reports = [a, b];
        let groups = duplicate_groups(&reports);
        assert_eq!(groups.len(), 1, "a file found once is not listed");
        let name = |i: &FoundItem| i.path.file_name().unwrap().to_string_lossy().to_string();
        let (kept, others) = &groups[0];
        // The copy filed in named folders is kept, from whichever source.
        assert_eq!(name(kept), "filed.jpg");
        assert_eq!(
            others.iter().map(|i| name(i)).collect::<Vec<_>>(),
            ["loose.jpg", "again.jpg"]
        );
        // It is the same copy that would be sent.
        let (unique, _) = unique_items(&reports);
        assert!(unique.iter().any(|u| std::ptr::eq(*u, *kept)));
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
        assert_eq!(s.duplicates, None, "not checked, so not said to be none");
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
        assert_eq!(s.junk.get(&JunkReason::AppIcons), Some(&1), "and said, not hidden");
    }

    #[test]
    fn a_drive_of_nothing_but_junk_still_counts_what_was_left_out() {
        let mut icon = found("save.png", Some("cc"), &[], None, MediaKind::Photo);
        icon.junk = Some(JunkReason::AppIcons);
        let r = [ScanReport {
            items: vec![icon],
            ..Default::default()
        }];
        let (s, kept) = summarise(&r);
        assert!(kept.is_empty() && s.collections.is_empty());
        assert_eq!(s.junk.get(&JunkReason::AppIcons), Some(&1));
    }

    #[test]
    fn what_could_not_be_read_is_counted() {
        let r = [ScanReport {
            unreadable: vec![keevoro_scan::Unreadable {
                path: "x".into(),
                error: "e".into(),
            }],
            ..Default::default()
        }];
        let (s, _) = summarise(&r);
        assert_eq!((s.photos, s.videos, s.unreadable), (0, 0, 1));
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

    #[test]
    fn the_closing_account_says_every_number_once_and_they_add_up() {
        use keevoro_publish::Account;
        let place = "the folder Y:\\personal";
        let all = Account {
            given: 72_024,
            already_there: 71_848,
            sent: 176,
            ..Account::default()
        };
        assert!(all.adds_up());
        assert_eq!(
            account_words(place, &all),
            "All 72,024 files you are keeping are safely at the folder Y:\\personal."
        );
        let first_run = Account {
            given: 72_024,
            already_there: 722,
            sent: 71_126,
            failed: 176,
            ..Account::default()
        };
        assert!(first_run.adds_up());
        assert_eq!(
            account_words(place, &first_run),
            "71,848 of the 72,024 files you are keeping are safely at the folder Y:\\personal. 176 still need \
             attention: 176 couldn't be sent."
        );
        let stopped = Account {
            given: 10,
            sent: 3,
            failed: 1,
            not_tried: 6,
            ..Account::default()
        };
        assert_eq!(
            account_words(place, &stopped),
            "3 of the 10 files you are keeping are safely at the folder Y:\\personal. 7 still need attention: 1 \
             couldn't be sent and 6 were not tried before the backup was stopped."
        );
        let one = Account {
            given: 1,
            sent: 1,
            ..Account::default()
        };
        assert_eq!(
            account_words(place, &one),
            "The 1 file you are keeping is safely at the folder Y:\\personal."
        );
    }
}
