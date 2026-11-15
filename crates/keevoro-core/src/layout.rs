//! Where a kept file goes inside the library.
//!
//! The library mirrors the user's own folders, because their folders are
//! their organisation:
//!
//! ```text
//! originals/Holidays/Spain 2009/Day 2/IMG_1234.jpg   came from a named folder
//! originals/By date/2009/2009-07/IMG_0042.jpg        no useful folder, but dated
//! originals/Undated/scan0001.jpg                     no believable date anywhere
//! ```
//!
//! Names are made safe for every disk the library might live on. A library
//! built on a Mac must still open on a Windows PC, so the stricter Windows
//! rules are applied everywhere.

use crate::date::{DateGuess, DateSource};

pub const ORIGINALS: &str = "originals";
pub const BY_DATE: &str = "By date";
pub const UNDATED: &str = "Undated";

/// The folders, from the library root down, that a file belongs in.
pub fn library_folder(album: &[String], date: Option<&DateGuess>) -> Vec<String> {
    let mut path = vec![ORIGINALS.to_string()];
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
            library_folder(&album(&["Holidays", "Spain 2009", "Day 2"]), Some(&g)),
            ["originals", "Holidays", "Spain 2009", "Day 2"]
        );
    }

    #[test]
    fn unnamed_files_go_by_date() {
        let day = guess("2009-07-14T21:03:11", DateSource::Exif);
        assert_eq!(
            library_folder(&[], Some(&day)),
            ["originals", "By date", "2009", "2009-07"]
        );
        let year = guess("2009", DateSource::Folder);
        assert_eq!(library_folder(&[], Some(&year)), ["originals", "By date", "2009"]);
    }

    #[test]
    fn a_copy_date_is_not_a_capture_date() {
        let copied = guess("2015-01-01", DateSource::Modified);
        assert_eq!(library_folder(&[], Some(&copied)), ["originals", "Undated"]);
        assert_eq!(library_folder(&[], None), ["originals", "Undated"]);
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
        assert_eq!(library_folder(&album(&["What?"]), None), ["originals", "What_"]);
    }

    #[test]
    fn clashes_are_numbered_like_windows() {
        assert_eq!(numbered_name("IMG_1234.jpg", 2), "IMG_1234 (2).jpg");
        assert_eq!(numbered_name("archive.tar.gz", 3), "archive.tar (3).gz");
        assert_eq!(numbered_name("README", 2), "README (2)");
        assert_eq!(numbered_name(".hidden", 2), ".hidden (2)");
    }
}
