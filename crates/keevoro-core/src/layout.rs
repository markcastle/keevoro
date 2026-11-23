//! Where a kept file goes inside the library.
//!
//! The library mirrors the user's own folders, because their folders are
//! their organisation:
//!
//! ```text
//! originals/Holidays/Spain 2009/Day 2/IMG_1234.jpg   came from a named folder
//! originals/By date/2009/2009-07/IMG_0042.jpg        no useful folder, but dated
//! originals/Undated/scan0001.jpg                     no believable date anywhere
//! originals/Other files/Mum/Documents/Tax/return.pdf an other file, in its folder
//! originals/Music/The Made-Up Band/Pretend Songs/01.flac music, in its folders
//! ```
//!
//! Music and other files keep the folders they came from, starting with
//! the source's own folder's name, each under one clear folder of its own,
//! so they never mix with the photos, however the photos are arranged.
//! Music from a source that is itself a folder called Music (as most
//! computers have) is not put in `Music/Music`. Two sources with the same
//! folders share them; a name already taken by a different file is
//! numbered, as for photos.
//!
//! Names are made safe for every disk the library might live on. A library
//! built on a Mac must still open on a Windows PC, so the stricter Windows
//! rules are applied everywhere.

use crate::date::{DateGuess, DateSource};
use crate::media::MediaKind;

pub const ORIGINALS: &str = "originals";
pub const BY_DATE: &str = "By date";
pub const UNDATED: &str = "Undated";
/// Where other files go, inside `originals`.
pub const OTHER_FILES: &str = "Other files";
/// Where music goes, inside `originals`.
pub const MUSIC: &str = "Music";

/// The folders, from the library root down, that a file belongs in. For an
/// other file, `album` is the folder it came from.
pub fn library_folder(kind: MediaKind, album: &[String], date: Option<&DateGuess>) -> Vec<String> {
    let mut path = vec![ORIGINALS.to_string()];
    if kind == MediaKind::Other {
        path.push(OTHER_FILES.to_string());
        path.extend(album.iter().map(|a| safe_name(a)));
        return path;
    }
    if kind == MediaKind::Music {
        path.push(MUSIC.to_string());
        let inner = match album.split_first() {
            Some((first, rest)) if first.eq_ignore_ascii_case(MUSIC) => rest,
            _ => album,
        };
        path.extend(inner.iter().map(|a| safe_name(a)));
        return path;
    }
    if !album.is_empty() {
        path.extend(album.iter().map(|a| safe_name(a)));
        return path;
    }
    // A file's modified time is usually the day it was copied, so it is not
    // trusted enough to file a photo under a month.
    match date.filter(|d| d.source != DateSource::Modified) {
        Some(d) => {
            path.push(BY_DATE.to_string());
            path.push(format!("{:04}", d.date.year));
            if let Some(m) = d.date.month {
                path.push(format!("{:04}-{m:02}", d.date.year));
            }
        }
        None => path.push(UNDATED.to_string()),
    }
    path
}

/// Makes one file or folder name safe on Windows, macOS and Linux, changing
/// as little as possible.
pub fn safe_name(name: &str) -> String {
    const RESERVED: &[&str] = &[
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1",
        "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    // Windows silently drops trailing dots and spaces, which would make two
    // different names collide.
    let cleaned = cleaned.trim().trim_end_matches(['.', ' ']);
    if cleaned.is_empty() {
        return "_".to_string();
    }
    let stem = cleaned.split('.').next().unwrap_or(cleaned).to_lowercase();
    if RESERVED.contains(&stem.as_str()) {
        return format!("_{cleaned}");
    }
    cleaned.to_string()
}

/// The most bytes of UTF-8 one folder or file name may hold: the tightest
/// of NTFS and exFAT (255 UTF-16 units) and APFS (255 bytes).
pub const LONGEST_NAME: usize = 255;

/// The most bytes a whole place may hold, `originals/` included. Cloud
/// storage allows 1,024; the rest is room for a clash number, `(2)`.
pub const LONGEST_PLACE: usize = 1000;

/// A name longer than this, in characters, is cut when a place is too long.
const CUT_ABOVE: usize = 64;

/// What a cut name keeps of its start, in characters.
const CUT_TO: usize = 60;

/// An ending longer than this is not kept apart when a file name is cut.
const LONGEST_ENDING: usize = 16;

/// Keeps a place within what every destination can hold, changing nothing
/// when it already fits.
///
/// When it does not, each folder name longer than 64 characters is cut to
/// its first 60, then `~` and three characters of a fingerprint of the
/// whole name, so two long names that start alike stay apart. If that is
/// not enough, the deepest folders are joined into one, cut the same way.
/// A file name is cut last, and keeps its ending.
///
/// Only new places go through this, so a place already agreed is never
/// changed by it.
pub fn fit_place(mut folders: Vec<String>, mut name: String) -> (Vec<String>, String) {
    if fits(&folders, &name) {
        return (folders, name);
    }
    for folder in &mut folders {
        if too_long(folder) {
            *folder = cut(folder);
        }
    }
    // `originals` and the folder below it are never joined.
    while !fits(&folders, &name) && folders.len() > 2 {
        let (Some(inner), Some(outer)) = (folders.pop(), folders.pop()) else {
            break;
        };
        let joined = format!("{outer} - {inner}");
        folders.push(if too_long(&joined) { cut(&joined) } else { joined });
    }
    if !fits(&folders, &name) || name.len() > LONGEST_NAME {
        name = cut_file_name(&name);
    }
    (folders, name)
}

fn fits(folders: &[String], name: &str) -> bool {
    let total = folders.iter().map(|f| f.len() + 1).sum::<usize>() + name.len();
    total <= LONGEST_PLACE && name.len() <= LONGEST_NAME && folders.iter().all(|f| f.len() <= LONGEST_NAME)
}

fn too_long(name: &str) -> bool {
    name.chars().count() > CUT_ABOVE || name.len() > LONGEST_NAME
}

/// The first 60 characters, then `~` and three of a fingerprint of the
/// whole name.
fn cut(name: &str) -> String {
    let start: String = name.chars().take(CUT_TO).collect();
    // A cut must not leave a name ending in a space or a dot, which
    // Windows drops.
    let start = start.trim_end_matches(['.', ' ']);
    let print = blake3::hash(name.as_bytes()).to_hex();
    format!("{start}~{}", &print.as_str()[..3])
}

fn cut_file_name(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, ending)) if !stem.is_empty() && ending.chars().count() <= LONGEST_ENDING => {
            format!("{}.{ending}", cut(stem))
        }
        _ => cut(name),
    }
}

/// The name to try when `name` is already taken by a different file:
/// `IMG_1234 (2).jpg`, the way Windows does it. `n` starts at 2.
pub fn numbered_name(name: &str, n: u32) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => format!("{stem} ({n}).{ext}"),
        _ => format!("{name} ({n})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::date::PartialDate;

    fn guess(text: &str, source: DateSource) -> DateGuess {
        DateGuess {
            date: PartialDate::parse(text).unwrap(),
            source,
        }
    }

    fn album(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn named_folders_are_kept_as_they_were() {
        let g = guess("2009-07-14", DateSource::Exif);
        assert_eq!(
            library_folder(MediaKind::Photo, &album(&["Holidays", "Spain 2009", "Day 2"]), Some(&g)),
            ["originals", "Holidays", "Spain 2009", "Day 2"]
        );
    }

    #[test]
    fn unnamed_files_go_by_date() {
        let day = guess("2009-07-14T21:03:11", DateSource::Exif);
        assert_eq!(
            library_folder(MediaKind::Photo, &[], Some(&day)),
            ["originals", "By date", "2009", "2009-07"]
        );
        let year = guess("2009", DateSource::Folder);
        assert_eq!(
            library_folder(MediaKind::Photo, &[], Some(&year)),
            ["originals", "By date", "2009"]
        );
    }

    #[test]
    fn a_copy_date_is_not_a_capture_date() {
        let copied = guess("2015-01-01", DateSource::Modified);
        assert_eq!(
            library_folder(MediaKind::Photo, &[], Some(&copied)),
            ["originals", "Undated"]
        );
        assert_eq!(library_folder(MediaKind::Photo, &[], None), ["originals", "Undated"]);
    }

    #[test]
    fn other_files_keep_their_folders_under_their_own_and_are_never_put_by_date() {
        let day = guess("2009-07-14", DateSource::Exif);
        assert_eq!(
            library_folder(MediaKind::Other, &album(&["Mum", "Documents", "Tax?"]), Some(&day)),
            ["originals", "Other files", "Mum", "Documents", "Tax_"]
        );
        assert_eq!(
            library_folder(MediaKind::Other, &[], None),
            ["originals", "Other files"]
        );
    }

    #[test]
    fn music_keeps_its_folders_under_its_own_and_is_never_put_by_date() {
        let day = guess("2009-07-14", DateSource::Modified);
        assert_eq!(
            library_folder(
                MediaKind::Music,
                &album(&["Music", "The Made-Up Band", "Pretend Songs"]),
                Some(&day)
            ),
            ["originals", "Music", "The Made-Up Band", "Pretend Songs"]
        );
        assert_eq!(
            library_folder(MediaKind::Music, &album(&["Old laptop", "Music", "Albums"]), None),
            ["originals", "Music", "Old laptop", "Music", "Albums"]
        );
        assert_eq!(library_folder(MediaKind::Music, &[], None), ["originals", "Music"]);
    }

    #[test]
    fn names_are_made_safe_for_windows() {
        assert_eq!(safe_name("Spain 2009"), "Spain 2009");
        assert_eq!(safe_name("Mum & Dad: 40th?"), "Mum & Dad_ 40th_");
        assert_eq!(safe_name("a/b\\c"), "a_b_c");
        assert_eq!(safe_name("Trip... "), "Trip");
        assert_eq!(safe_name("tab\there"), "tab_here");
        assert_eq!(safe_name("CON"), "_CON");
        assert_eq!(safe_name("nul.jpg"), "_nul.jpg");
        assert_eq!(safe_name("Console"), "Console");
        assert_eq!(safe_name(" . "), "_");
        assert_eq!(
            library_folder(MediaKind::Photo, &album(&["What?"]), None),
            ["originals", "What_"]
        );
    }

    fn total(folders: &[String], name: &str) -> usize {
        folders.iter().map(|f| f.len() + 1).sum::<usize>() + name.len()
    }

    #[test]
    fn a_place_that_fits_is_left_exactly_as_it_is() {
        let folders = album(&["originals", &"a".repeat(200), "Smith"]);
        let (f, n) = fit_place(folders.clone(), "IMG_0001.jpg".into());
        assert_eq!((f, n.as_str()), (folders, "IMG_0001.jpg"));
    }

    #[test]
    fn a_name_over_255_bytes_is_cut_keeping_its_ending() {
        let long = format!("{}.jpg", "\u{e9}".repeat(150));
        assert!(long.len() > LONGEST_NAME);
        let (_, n) = fit_place(album(&["originals", "Scans"]), long);
        assert!(n.len() <= LONGEST_NAME, "{n}");
        assert!(n.ends_with(".jpg"), "{n}");
        assert!(n.contains('~'));
    }

    #[test]
    fn a_folder_of_300_characters_is_cut_and_two_that_start_alike_stay_apart() {
        let a = format!("{}A", "x".repeat(299));
        let b = format!("{}B", "x".repeat(299));
        let (fa, _) = fit_place(album(&["originals", &a]), "1.jpg".into());
        let (fb, _) = fit_place(album(&["originals", &b]), "1.jpg".into());
        assert_eq!(fa[1].chars().count(), CUT_TO + 4);
        assert_ne!(fa[1], fb[1], "a fingerprint of the whole name keeps them apart");
        assert_eq!(
            fa,
            fit_place(album(&["originals", &a]), "1.jpg".into()).0,
            "the same every time"
        );
    }

    #[test]
    fn a_place_of_2000_characters_is_brought_within_1000_bytes() {
        let deep: Vec<String> = std::iter::once("originals".to_string())
            .chain((0..20).map(|i| format!("Folder number {i} {}", "y".repeat(80))))
            .collect();
        let name = format!("{}.tiff", "z".repeat(300));
        assert!(total(&deep, &name) > 2000);
        let (f, n) = fit_place(deep, name);
        assert!(total(&f, &n) <= LONGEST_PLACE, "{}", total(&f, &n));
        assert_eq!(f[0], "originals");
        assert!(f.iter().all(|x| x.len() <= LONGEST_NAME));
        assert!(n.ends_with(".tiff"));
        // Room is left for a clash number under cloud storage's 1,024.
        assert!(total(&f, &numbered_name(&n, 99)) < 1024);
    }

    #[test]
    fn a_cut_never_leaves_a_name_ending_in_a_dot_or_a_space() {
        let name = format!("{} .{}", "a".repeat(58), "b".repeat(300));
        let (f, _) = fit_place(album(&["originals", &name]), "x.jpg".into());
        assert!(!f[1].contains(" ~") && !f[1].contains(".~"), "{}", f[1]);
    }

    #[test]
    fn clashes_are_numbered_like_windows() {
        assert_eq!(numbered_name("IMG_1234.jpg", 2), "IMG_1234 (2).jpg");
        assert_eq!(numbered_name("archive.tar.gz", 3), "archive.tar (3).gz");
        assert_eq!(numbered_name("README", 2), "README (2)");
        assert_eq!(numbered_name(".hidden", 2), ".hidden (2)");
    }
}
