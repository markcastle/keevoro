//! How a backup arranges photos and videos where it sends them: by date,
//! or keeping the person's own folders exactly as they are.
//!
//! By date (what Keevoro always did) keeps folders with real names and
//! puts everything else under `By date/<year>/<year-month>`. That suits a
//! camera card. It does not suit a folder of scans, whose dates are the
//! day of scanning, or folders a person has already sorted: our first
//! outside tester's `Pictures / Scans / Family Scans` went under
//! `By date / 2021 / 2021-01`.
//!
//! Keeping the folders gives each photo or video the folders it was in on
//! its source, starting with the source's own folder, exactly as named
//! (only made safe for every disk): `originals/Family Scans/Smith/a.jpg`.
//! Nothing is trimmed as noise, and nothing is put by date.
//!
//! The choice is the backup's, and each source can have its own. It only
//! changes where files go from then on: a file's place is agreed once.

use serde::{Deserialize, Serialize};
use std::path::{Component, Path, Prefix};

/// How photos and videos are arranged in a backup.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Arrange {
    /// A folder for each year and month, keeping folders with real names.
    #[default]
    ByDate,
    /// The same folders and names as on the source.
    Folders,
}

impl Arrange {
    /// Its name for a program: `byDate` or `folders`.
    pub fn name(self) -> &'static str {
        match self {
            Arrange::ByDate => "byDate",
            Arrange::Folders => "folders",
        }
    }

    /// From a word as typed: `by-date`, `bydate`, `date`, `folders`,
    /// `keep`, in any case.
    pub fn from_word(word: &str) -> Option<Self> {
        match word.trim().to_lowercase().replace(['-', '_', ' '], "").as_str() {
            "bydate" | "date" | "dates" => Some(Arrange::ByDate),
            "folders" | "folder" | "keep" | "keepfolders" | "keepmyfolders" => Some(Arrange::Folders),
            _ => None,
        }
    }

    /// In a few words: "by date", "keeping your folders as they are".
    pub fn words(self) -> &'static str {
        match self {
            Arrange::ByDate => "by date",
            Arrange::Folders => "keeping your folders as they are",
        }
    }
}

/// What the top folder is called for a source at `root`: the folder's own
/// name, or for a whole drive or share (which has none) its friendly name
/// with any `(E:)` taken off, else `Drive E` or `share on server`.
pub fn top_folder(root: &Path, source_name: &str) -> String {
    if let Some(name) = root.file_name() {
        return crate::safe_name(&name.to_string_lossy());
    }
    let label = without_drive_letter(source_name);
    if !label.is_empty() && !looks_like_a_path(label) {
        return crate::safe_name(label);
    }
    crate::safe_name(&unnamed_root(root))
}

/// The folders a photo or video kept in its folders goes in: the source's
/// own folder name, then each folder below it down to the file, exactly as
/// named. Each is made safe when the place is made
/// (`crate::layout::library_folder`).
pub fn kept_folders(root: &Path, source_name: &str, file: &Path) -> Vec<String> {
    let mut folders = vec![top_folder(root, source_name)];
    folders.extend(
        file.strip_prefix(root)
            .ok()
            .and_then(Path::parent)
            .into_iter()
            .flat_map(Path::components)
            .filter_map(|c| match c {
                Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
                _ => None,
            }),
    );
    folders
}

/// The whole arrangement of a backup in one line of text, so a backup can
/// tell whether it changed since its places were agreed: the backup's
/// choice, then each source with its own, by place. A backup that never
/// chose, with no source choosing, says `byDate`, as before the choice.
pub fn arrangement_said(choices: &crate::Choices) -> String {
    let mut own: Vec<String> = choices
        .sources
        .iter()
        .filter_map(|s| s.arrange.map(|a| format!("{}\t{}", a.name(), s.root.display())))
        .collect();
    own.sort();
    std::iter::once(choices.arrange.name().to_string())
        .chain(own)
        .collect::<Vec<_>>()
        .join("\n")
}

/// One source, for working out which share a folder in the backup.
#[derive(Debug, Clone)]
pub struct SourceArranged<'a> {
    pub root: &'a Path,
    pub name: &'a str,
    /// How its photos and videos are arranged, its own choice or else the
    /// backup's.
    pub arrange: Arrange,
}

/// For each source that keeps its folders, the others that keep theirs
/// under the same top folder, so their files merge into one folder in the
/// backup (`D:\Photos` and `E:\Photos` both into `originals/Photos`), by
/// their place in `sources`. Compared without case, as Windows and a Mac
/// compare names. Empty for a source arranged by date.
pub fn sharing_a_folder(sources: &[SourceArranged<'_>]) -> Vec<Vec<usize>> {
    let tops: Vec<Option<String>> = sources
        .iter()
        .map(|s| (s.arrange == Arrange::Folders).then(|| top_folder(s.root, s.name).to_lowercase()))
        .collect();
    tops.iter()
        .enumerate()
        .map(|(i, mine)| match mine {
            Some(mine) => tops
                .iter()
                .enumerate()
                .filter(|(j, theirs)| *j != i && theirs.as_ref() == Some(mine))
                .map(|(j, _)| j)
                .collect(),
            None => Vec::new(),
        })
        .collect()
}

/// "In the backup, its folder Photos is shared with D:\Photos." The places
/// are as a person would write them.
pub fn shared_words(top: &str, with: &[String]) -> Option<String> {
    let names = match with {
        [] => return None,
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    };
    Some(format!("In the backup, its folder {top} is shared with {names}."))
}

/// Fewer photos and videos than this on a source: no suggestion.
pub const MIN_FILES: usize = 20;
/// The commonest day holds at least this share of the files dated to the
/// day: they were probably copied or scanned that day.
pub const ONE_DAY_SHARE: f64 = 0.80;
/// At least this share would go into "Undated".
pub const UNDATED_SHARE: f64 = 0.80;
/// At least this share sits in folders with real names...
pub const NAMED_SHARE: f64 = 0.70;
/// ...across at least this many of them.
pub const NAMED_FOLDERS: usize = 3;
/// A camera card's dates spread over at least this many days.
pub const CAMERA_DAYS: usize = 7;

/// What a suggestion needs to know of one photo or video on a source.
#[derive(Debug, Clone)]
pub struct FileSeen<'a> {
    /// The folders between the source's own folder and the file.
    pub folders: Vec<&'a str>,
    /// What by date would keep of them (`crate::folder_hints`).
    pub album: &'a [String],
    pub date: Option<crate::DateGuess>,
    /// The file's own name.
    pub name: &'a str,
    pub kind: crate::MediaKind,
}

/// What is known of one source for suggesting how to arrange it. Built
/// from what was found, with nothing read from the disk.
#[derive(Debug, Clone)]
pub struct SourceFacts<'a> {
    /// The source's own folder name.
    pub root_name: &'a str,
    pub files: Vec<FileSeen<'a>>,
}

/// Why an arrangement is suggested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// The source or a folder in it is called "Scans", or most names
    /// start with "scan".
    Scans { kinds: &'static str },
    /// Most would go into "Undated".
    Undated { kinds: &'static str },
    /// Most share one day.
    OneDate {
        same: usize,
        of: usize,
        day: String,
        kinds: &'static str,
    },
    /// Most are already in named folders, such as these two.
    Named { such_as: Vec<String>, kinds: &'static str },
    /// It looks like a camera card.
    Camera { kinds: &'static str },
}

/// An arrangement worth suggesting for one source, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub arrange: Arrange,
    pub reason: Reason,
}

impl Suggestion {
    /// Why, for the source's own card: "412 of its 420 photos have the
    /// same date, 14 March 2021, which is probably when they were copied
    /// or scanned, not taken."
    pub fn why_for_the_source(&self) -> String {
        match &self.reason {
            Reason::Scans { .. } => "These look like scans.".to_string(),
            Reason::Undated { .. } => "Most have no date, so by date they would all go into Undated.".to_string(),
            Reason::OneDate { same, of, day, kinds } => format!(
                "{} of its {} {kinds} have the same date, {day}, which is probably when they were copied or \
                 scanned, not taken.",
                crate::words::thousands(*same),
                crate::words::thousands(*of)
            ),
            Reason::Named { such_as, kinds } => format!(
                "Its {kinds} are already sorted into named folders, such as {}.",
                such_as.join(" and ")
            ),
            Reason::Camera { .. } => "These look like they came straight from a camera.".to_string(),
        }
    }

    /// Why, when the whole backup is asked about, naming the source:
    /// "412 of the 420 photos in Family Scans have the same date, ...".
    /// Starts in lower case, to follow "Suggested because".
    pub fn why_for_the_backup(&self, source: &str) -> String {
        match &self.reason {
            Reason::Scans { kinds } => format!("the {kinds} in {source} look like scans."),
            Reason::Undated { kinds } => {
                format!("most of the {kinds} in {source} have no date, so by date they would all go into Undated.")
            }
            Reason::OneDate { same, of, day, kinds } => format!(
                "{} of the {} {kinds} in {source} have the same date, {day}, which is probably when they were \
                 copied or scanned, not taken.",
                crate::words::thousands(*same),
                crate::words::thousands(*of)
            ),
            Reason::Named { such_as, kinds } => format!(
                "the {kinds} in {source} are already sorted into named folders, such as {}.",
                such_as.join(" and ")
            ),
            Reason::Camera { kinds } => format!("the {kinds} in {source} look like they came straight from a camera."),
        }
    }
}

/// How to arrange one source, if its files say clearly: the first of these
/// that holds wins.
///
/// 1. It looks like scans (a folder called scan, scans, scanned or
///    scanner, or at least half the names start with "scan"): keep the
///    folders.
/// 2. At least [`UNDATED_SHARE`] have no believable date: keep the folders.
/// 3. The commonest day holds at least [`ONE_DAY_SHARE`] of those dated to
///    the day: keep the folders.
/// 4. At least [`NAMED_SHARE`] sit in at least [`NAMED_FOLDERS`] folders
///    with real names, and the source's own name is not noise: keep them.
/// 5. At least half sit in camera folders by date would drop, dated over at
///    least [`CAMERA_DAYS`] days: by date.
///
/// Otherwise nothing is suggested. Fewer than [`MIN_FILES`]: nothing.
/// One pass over the files, nothing read from the disk.
pub fn suggest(facts: &SourceFacts<'_>) -> Option<Suggestion> {
    let files = &facts.files;
    let total = files.len();
    if total < MIN_FILES {
        return None;
    }
    let kinds = kinds_of(files.iter().map(|f| f.kind));
    let share = |n: usize| n as f64 / total as f64;
    let folders_say_scans = std::iter::once(facts.root_name)
        .chain(files.iter().flat_map(|f| f.folders.iter().copied()))
        .any(says_scan);
    let named_scan = files
        .iter()
        .filter(|f| f.name.to_lowercase().starts_with("scan"))
        .count();
    if folders_say_scans || share(named_scan) >= 0.5 {
        return Some(keep(Reason::Scans { kinds }));
    }
    let believable = |f: &FileSeen<'_>| f.date.filter(|d| d.source != crate::DateSource::Modified);
    let undated = files.iter().filter(|f| believable(f).is_none()).count();
    if share(undated) >= UNDATED_SHARE {
        return Some(keep(Reason::Undated { kinds }));
    }
    let mut days: std::collections::HashMap<(u16, u8, u8), usize> = std::collections::HashMap::new();
    let mut dated_to_the_day = Vec::new();
    for f in files {
        if let Some(d) = believable(f) {
            if let (Some(m), Some(day)) = (d.date.month, d.date.day) {
                dated_to_the_day.push(f.kind);
                *days.entry((d.date.year, m, day)).or_default() += 1;
            }
        }
    }
    let of = dated_to_the_day.len();
    // The commonest day; of two as common, the earlier.
    if let Some((&(y, m, d), &same)) = days.iter().max_by_key(|(day, n)| (**n, std::cmp::Reverse(**day))) {
        if of > 0 && same as f64 / of as f64 >= ONE_DAY_SHARE {
            return Some(keep(Reason::OneDate {
                same,
                of,
                day: format!("{d} {} {y}", crate::collections::month_name(m)),
                kinds: kinds_of(dated_to_the_day.iter().copied()),
            }));
        }
    }
    let mut named: std::collections::HashMap<&[String], usize> = std::collections::HashMap::new();
    for f in files.iter().filter(|f| !f.album.is_empty()) {
        *named.entry(f.album).or_default() += 1;
    }
    let in_named: usize = named.values().sum();
    let root_is_noise = crate::folder_hints(&[facts.root_name]).album.is_empty();
    if named.len() >= NAMED_FOLDERS && share(in_named) >= NAMED_SHARE && !root_is_noise {
        let mut biggest: Vec<(&[String], usize)> = named.into_iter().collect();
        biggest.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        let such_as = biggest
            .iter()
            .filter_map(|(album, _)| album.last().cloned())
            .take(2)
            .collect();
        return Some(keep(Reason::Named { such_as, kinds }));
    }
    let in_camera_folders = files
        .iter()
        .filter(|f| !f.folders.is_empty() && f.album.is_empty())
        .count();
    if share(in_camera_folders) >= 0.5 && days.len() >= CAMERA_DAYS {
        return Some(Suggestion {
            arrange: Arrange::ByDate,
            reason: Reason::Camera { kinds },
        });
    }
    None
}

fn keep(reason: Reason) -> Suggestion {
    Suggestion {
        arrange: Arrange::Folders,
        reason,
    }
}

/// "photos", "videos", or "photos and videos".
fn kinds_of(kinds: impl Iterator<Item = crate::MediaKind>) -> &'static str {
    let (mut photos, mut videos) = (false, false);
    for kind in kinds {
        match kind {
            crate::MediaKind::Video => videos = true,
            _ => photos = true,
        }
    }
    match (photos, videos) {
        (false, true) => "videos",
        (true, true) => "photos and videos",
        _ => "photos",
    }
}

/// Is any word of this name scan, scans, scanned or scanner?
fn says_scan(name: &str) -> bool {
    name.split(|c: char| !c.is_alphanumeric())
        .any(|word| matches!(word.to_lowercase().as_str(), "scan" | "scans" | "scanned" | "scanner"))
}

/// "KINGSTON (E:)" becomes "KINGSTON"; "Drive E:" stays.
fn without_drive_letter(name: &str) -> &str {
    let name = name.trim();
    match name.rsplit_once(" (") {
        Some((label, rest)) if rest.len() == 3 && rest.ends_with(":)") => label.trim(),
        _ => name,
    }
}

fn looks_like_a_path(name: &str) -> bool {
    name.contains(['\\', '/']) || (name.len() == 2 && name.ends_with(':'))
}

/// A drive or share with no friendly name: `Drive E`, `share on server`,
/// or `Whole disk` for `/`.
fn unnamed_root(root: &Path) -> String {
    let Some(Component::Prefix(prefix)) = root.components().next() else {
        return "Whole disk".to_string();
    };
    match prefix.kind() {
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
            format!("Drive {}", char::from(letter).to_ascii_uppercase())
        }
        Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
            format!("{} on {}", share.to_string_lossy(), server.to_string_lossy())
        }
        _ => "Whole disk".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn kept(root: &str, name: &str, file: &str) -> Vec<String> {
        kept_folders(&PathBuf::from(root), name, &PathBuf::from(file))
    }

    #[test]
    fn the_testers_scans_keep_their_folder_and_every_folder_below_it() {
        assert_eq!(
            kept(
                "/home/t/Pictures/Scans/Family Scans",
                "Family Scans",
                "/home/t/Pictures/Scans/Family Scans/a.jpg"
            ),
            ["Family Scans"]
        );
        assert_eq!(
            kept(
                "/home/t/Pictures/Scans/Family Scans",
                "Family Scans",
                "/home/t/Pictures/Scans/Family Scans/Smith/1950s/b.jpg"
            ),
            ["Family Scans", "Smith", "1950s"]
        );
    }

    #[test]
    fn names_a_date_by_folder_would_drop_as_noise_are_kept() {
        assert_eq!(
            kept("/m/Photos", "Photos", "/m/Photos/DCIM/100CANON/2009_07_14/a.jpg"),
            ["Photos", "DCIM", "100CANON", "2009_07_14"]
        );
    }

    #[test]
    fn the_top_folder_is_made_safe() {
        assert_eq!(top_folder(Path::new("/m/What?"), "What?"), "What_");
        assert_eq!(top_folder(Path::new("/m/CON"), "CON"), "_CON");
    }

    #[test]
    fn a_whole_drive_is_named_by_its_label_or_its_letter() {
        assert_eq!(top_folder(Path::new("/"), "KINGSTON (E:)"), "KINGSTON");
        assert_eq!(top_folder(Path::new("/"), "/"), "Whole disk");
        assert_eq!(top_folder(Path::new("/"), ""), "Whole disk");
    }

    #[cfg(windows)]
    #[test]
    fn drive_and_share_roots_on_windows() {
        assert_eq!(top_folder(Path::new(r"E:\"), "KINGSTON (E:)"), "KINGSTON");
        assert_eq!(top_folder(Path::new(r"\\?\E:\"), r"E:\"), "Drive E");
        assert_eq!(top_folder(Path::new(r"E:\"), "E:"), "Drive E");
        assert_eq!(
            top_folder(Path::new(r"\\server\photos"), r"\\server\photos"),
            "photos on server"
        );
        assert_eq!(
            top_folder(Path::new(r"\\?\UNC\server\photos"), r"\\?\UNC\server\photos"),
            "photos on server"
        );
        assert_eq!(
            kept(r"\\?\E:\", "KINGSTON (E:)", r"\\?\E:\Smith\a.jpg"),
            ["KINGSTON", "Smith"]
        );
    }

    #[test]
    fn a_mac_volume_is_named_by_its_folder() {
        assert_eq!(top_folder(Path::new("/Volumes/KINGSTON"), "KINGSTON"), "KINGSTON");
        assert_eq!(
            kept("/Volumes/KINGSTON", "KINGSTON", "/Volumes/KINGSTON/Smith/a.jpg"),
            ["KINGSTON", "Smith"]
        );
    }

    #[test]
    fn words_as_typed_are_understood() {
        for word in ["by-date", "ByDate", "date", "by date"] {
            assert_eq!(Arrange::from_word(word), Some(Arrange::ByDate), "{word}");
        }
        for word in ["folders", "keep", "keep-my-folders"] {
            assert_eq!(Arrange::from_word(word), Some(Arrange::Folders), "{word}");
        }
        assert_eq!(Arrange::from_word("sideways"), None);
        assert_eq!(serde_json_name(Arrange::ByDate), "\"byDate\"");
        assert_eq!(serde_json_name(Arrange::Folders), "\"folders\"");
    }

    #[test]
    fn two_sources_called_photos_that_keep_their_folders_share_one_and_each_says_so() {
        let d = PathBuf::from("/mnt/d/Photos");
        let e = PathBuf::from("/mnt/e/PHOTOS");
        let card = PathBuf::from("/media/card");
        let sources = [
            SourceArranged {
                root: &d,
                name: "Photos",
                arrange: Arrange::Folders,
            },
            SourceArranged {
                root: &e,
                name: "PHOTOS",
                arrange: Arrange::Folders,
            },
            SourceArranged {
                root: &card,
                name: "card",
                arrange: Arrange::Folders,
            },
        ];
        assert_eq!(sharing_a_folder(&sources), [vec![1], vec![0], Vec::<usize>::new()]);
        let by_date = [
            SourceArranged {
                arrange: Arrange::ByDate,
                ..sources[0].clone()
            },
            sources[1].clone(),
        ];
        assert_eq!(
            sharing_a_folder(&by_date),
            [Vec::<usize>::new(), Vec::new()],
            "a source by date shares nothing"
        );
        assert_eq!(
            shared_words("Photos", &[r"E:\Photos".to_string()]).as_deref(),
            Some(r"In the backup, its folder Photos is shared with E:\Photos.")
        );
        assert_eq!(
            shared_words("Photos", &["E:\\Photos".into(), "F:\\Photos".into()]).as_deref(),
            Some("In the backup, its folder Photos is shared with E:\\Photos and F:\\Photos.")
        );
        assert_eq!(shared_words("Photos", &[]), None);
    }

    #[test]
    fn the_arrangement_is_said_in_one_line_that_changes_with_any_choice() {
        let mut choices = crate::Choices::default();
        assert_eq!(arrangement_said(&choices), "byDate");
        choices.sources.push(crate::SourceChoices {
            root: PathBuf::from("/m/Scans"),
            arrange: Some(Arrange::Folders),
            ..crate::SourceChoices::default()
        });
        assert_eq!(arrangement_said(&choices), "byDate\nfolders\t/m/Scans");
        assert_eq!(choices.arrangement_in(Path::new("/m/Scans")), Arrange::Folders);
        assert_eq!(choices.arrangement_in(Path::new("/m/Card")), Arrange::ByDate);
    }

    use crate::{DateGuess, DateSource, MediaKind, PartialDate};

    /// One made-up file: its folders, its album by date, its date and name.
    type Made = (Vec<&'static str>, Vec<String>, Option<DateGuess>, String);

    /// A source of `n` photos, made by `each` from the file's number.
    fn source(
        n: usize,
        each: impl Fn(usize) -> (Vec<&'static str>, Vec<String>, Option<&'static str>, String),
    ) -> Vec<Made> {
        (0..n)
            .map(|i| {
                let (folders, album, date, name) = each(i);
                let date = date.map(|d| DateGuess {
                    date: PartialDate::parse(d).unwrap(),
                    source: DateSource::Exif,
                });
                (folders, album, date, name)
            })
            .collect()
    }

    fn suggested(root: &str, files: &[Made]) -> Option<Suggestion> {
        suggest(&SourceFacts {
            root_name: root,
            files: files
                .iter()
                .map(|(folders, album, date, name)| FileSeen {
                    folders: folders.clone(),
                    album,
                    date: *date,
                    name,
                    kind: MediaKind::Photo,
                })
                .collect(),
        })
    }

    const DAYS: [&str; 10] = [
        "2019-01-01",
        "2019-02-03",
        "2019-03-05",
        "2019-04-07",
        "2019-05-09",
        "2019-06-11",
        "2019-07-13",
        "2019-08-15",
        "2019-09-17",
        "2019-10-19",
    ];

    #[test]
    fn fewer_than_twenty_files_suggest_nothing() {
        let few = source(MIN_FILES - 1, |i| (vec![], vec![], None, format!("scan{i}.jpg")));
        assert_eq!(suggested("Family Scans", &few), None);
        let enough = source(MIN_FILES, |i| (vec![], vec![], None, format!("scan{i}.jpg")));
        assert!(suggested("Family Scans", &enough).is_some());
    }

    #[test]
    fn scans_are_known_by_a_folder_or_by_most_of_their_names() {
        let dated = |i: usize| Some(DAYS[i % DAYS.len()]);
        let in_folder = source(30, |i| (vec!["Scanned"], vec![], dated(i), format!("{i}.jpg")));
        let s = suggested("Pictures", &in_folder).unwrap();
        assert_eq!(s.arrange, Arrange::Folders);
        assert_eq!(s.why_for_the_source(), "These look like scans.");
        assert_eq!(
            s.why_for_the_backup("Pictures"),
            "the photos in Pictures look like scans."
        );
        // "Scansion" is not a scan.
        let words = source(30, |i| {
            (
                vec!["Scansion"],
                vec!["Scansion".to_string()],
                dated(i),
                format!("{i}.jpg"),
            )
        });
        assert_eq!(suggested("Pictures", &words).map(|s| s.reason), None);
        let half = source(30, |i| {
            (
                vec![],
                vec![],
                dated(i),
                if i < 15 {
                    format!("SCAN_{i}.jpg")
                } else {
                    format!("{i}.jpg")
                },
            )
        });
        assert!(matches!(
            suggested("Pictures", &half).unwrap().reason,
            Reason::Scans { .. }
        ));
        let under = source(30, |i| {
            (
                vec![],
                vec![],
                dated(i),
                if i < 14 {
                    format!("scan{i}.jpg")
                } else {
                    format!("{i}.jpg")
                },
            )
        });
        assert_eq!(suggested("Pictures", &under), None);
    }

    #[test]
    fn mostly_undated_keeps_the_folders_at_80_percent_and_not_below() {
        let at = source(20, |i| {
            (vec![], vec![], (i >= 16).then_some(DAYS[i % 10]), format!("{i}.jpg"))
        });
        let s = suggested("Old stuff", &at).unwrap();
        assert_eq!(
            s.why_for_the_source(),
            "Most have no date, so by date they would all go into Undated."
        );
        let below = source(20, |i| {
            (vec![], vec![], (i >= 15).then_some(DAYS[i % 10]), format!("{i}.jpg"))
        });
        assert!(!matches!(
            suggested("Old stuff", &below).map(|s| s.reason),
            Some(Reason::Undated { .. })
        ));
    }

    #[test]
    fn one_date_for_most_says_it_is_probably_when_they_were_copied() {
        let files = source(420, |i| {
            (
                vec![],
                vec![],
                Some(if i < 412 { "2021-03-14" } else { DAYS[i % 10] }),
                format!("{i}.jpg"),
            )
        });
        let s = suggested("Family", &files).unwrap();
        assert_eq!(s.arrange, Arrange::Folders);
        assert_eq!(
            s.why_for_the_source(),
            "412 of its 420 photos have the same date, 14 March 2021, which is probably when they were copied or \
             scanned, not taken."
        );
        assert_eq!(
            s.why_for_the_backup("Family"),
            "412 of the 420 photos in Family have the same date, 14 March 2021, which is probably when they were \
             copied or scanned, not taken."
        );
        // Just under 80%: 79 of 100.
        let under = source(100, |i| {
            (
                vec![],
                vec![],
                Some(if i < 79 { "2021-03-14" } else { DAYS[i % 10] }),
                format!("{i}.jpg"),
            )
        });
        assert_eq!(suggested("Family", &under), None);
        let at = source(100, |i| {
            (
                vec![],
                vec![],
                Some(if i < 80 { "2021-03-14" } else { DAYS[i % 10] }),
                format!("{i}.jpg"),
            )
        });
        assert!(matches!(
            suggested("Family", &at).unwrap().reason,
            Reason::OneDate { same: 80, of: 100, .. }
        ));
    }

    #[test]
    fn named_folders_need_three_of_them_holding_most_and_a_source_with_a_real_name() {
        let album = |name: &str| vec![name.to_string()];
        let names = ["Smith", "Jones", "Brown"];
        let named = source(30, |i| {
            (
                vec![names[i % 3]],
                album(names[i % 3]),
                Some(DAYS[i % 10]),
                format!("{i}.jpg"),
            )
        });
        let s = suggested("Family", &named).unwrap();
        assert_eq!(s.arrange, Arrange::Folders);
        assert!(s
            .why_for_the_source()
            .starts_with("Its photos are already sorted into named folders, such as "));
        assert_eq!(
            suggested("Pictures", &named),
            None,
            "a source called Pictures says nothing of its own"
        );
        let two = source(30, |i| {
            (
                vec![names[i % 2]],
                album(names[i % 2]),
                Some(DAYS[i % 10]),
                format!("{i}.jpg"),
            )
        });
        assert_eq!(suggested("Family", &two), None, "two folders are not enough");
        // 20 of 30 is under 70%.
        let fewer = source(30, |i| {
            if i < 20 {
                (
                    vec![names[i % 3]],
                    album(names[i % 3]),
                    Some(DAYS[i % 10]),
                    format!("{i}.jpg"),
                )
            } else {
                (vec![], vec![], Some(DAYS[i % 10]), format!("{i}.jpg"))
            }
        });
        assert_eq!(suggested("Family", &fewer), None);
    }

    #[test]
    fn a_camera_card_spread_over_days_is_suggested_by_date() {
        let card = source(40, |i| {
            (
                vec!["DCIM", "100CANON"],
                vec![],
                Some(DAYS[i % 10]),
                format!("IMG_{i}.jpg"),
            )
        });
        let s = suggested("CANON", &card).unwrap();
        assert_eq!(s.arrange, Arrange::ByDate);
        assert_eq!(
            s.why_for_the_source(),
            "These look like they came straight from a camera."
        );
        let few_days = source(40, |i| {
            (
                vec!["DCIM", "100CANON"],
                vec![],
                Some(DAYS[i % 6]),
                format!("IMG_{i}.jpg"),
            )
        });
        assert_eq!(suggested("CANON", &few_days), None, "six days is not enough");
    }

    #[test]
    fn the_rules_are_asked_in_order() {
        // Scans and one date both hold: scans comes first.
        let both = source(30, |i| (vec!["Scans"], vec![], Some("2021-03-14"), format!("{i}.jpg")));
        assert!(matches!(
            suggested("Pictures", &both).unwrap().reason,
            Reason::Scans { .. }
        ));
    }

    fn serde_json_name(a: Arrange) -> String {
        serde_json::to_string(&a).unwrap()
    }
}
