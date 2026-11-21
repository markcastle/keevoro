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

    fn serde_json_name(a: Arrange) -> String {
        serde_json::to_string(&a).unwrap()
    }
}
