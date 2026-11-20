//! `sources.leave_out`: leaves a folder out of a backup, or puts it back.
//!
//! A folder left out is not looked through by this backup's searches or
//! surveys (they pass it by without even listing it), and what was found
//! beneath it before is no longer offered, at once, with no new search.
//! It may be a folder in no source yet, so a person can leave out the
//! games folder before looking through the drive it is on. Nothing in the
//! folder is touched, and nothing already backed up is touched: what was
//! sent stays where it was sent. Putting it back offers again what was
//! found there.
//!
//! How it composes with the other choices (the kinds of file Keevoro looks
//! for, photos or videos, each ending on a source, and the folders Keevoro
//! made) is `keevoro_core::wants`. Folders are kept and compared as real
//! paths: the one asked about is followed to where it really is first.

use crate::backup::Backup;
use crate::error::{Error, Result};
use keevoro_engine::friendly_path;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// `sources.leave_out`
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LeaveOutRequest {
    /// The folder, written whole, such as `D:\Games`.
    pub folder: String,
    /// True (the default) to leave it out, false to put it back.
    #[serde(default = "yes")]
    pub left_out: bool,
}

fn yes() -> bool {
    true
}

/// What `sources.leave_out` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftOutFolder {
    /// The folder, as a person would write it.
    pub folder: String,
    /// Whether it is left out now.
    pub left_out: bool,
    /// How many files found before are beneath it: no longer offered, or
    /// offered again.
    pub files: usize,
    /// A folder it is in that is still left out, after putting it back.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub still_inside: Option<String>,
    /// What happened, in a sentence or two, saying that nothing already
    /// backed up is touched.
    pub said: String,
}

impl Backup {
    /// `sources.leave_out`.
    pub fn leave_out(&self, folder: &Path, left_out: bool) -> Result<LeftOutFolder> {
        let real = self.real_left_out_folder(folder, left_out)?;
        self.catalog().set_folder_left_out(&real, left_out)?;
        let files = self.catalog().found_beneath(&real)?;
        let still_inside = self.catalog().choices()?.left_out_folder(&real).map(friendly_path);
        let at = friendly_path(&real);
        let found = keevoro_engine::summary::count(files, "file");
        let said = match (left_out, files, &still_inside) {
            (true, 0, _) => format!(
                "Left out {at}. This backup won't look through it. Nothing in it is changed, and nothing already \
                 backed up is touched."
            ),
            (true, _, _) => format!(
                "Left out {at}. This backup won't look through it, and the {found} found there before {} no longer \
                 offered. Nothing in it is changed, and nothing already backed up is touched.",
                if files == 1 { "is" } else { "are" }
            ),
            (false, _, Some(parent)) => format!(
                "Put back {at}, but it is inside {parent}, which is still left out. Put that back to include it."
            ),
            (false, 0, None) => format!("Put back {at}. It is looked through again the next time its source is."),
            (false, _, None) => format!(
                "Put back {at}. The {found} found there before {} offered again.",
                if files == 1 { "is" } else { "are" }
            ),
        };
        Ok(LeftOutFolder {
            folder: at,
            left_out,
            files,
            still_inside,
            said,
        })
    }

    /// The folder as the computer gives its full path. A folder that is
    /// not there any more can still be put back, by the very words Keevoro
    /// gave for it, which are matched against the folders left out as
    /// Keevoro writes them, never against a path typed some other way.
    fn real_left_out_folder(&self, folder: &Path, left_out: bool) -> Result<PathBuf> {
        match std::fs::canonicalize(folder) {
            Ok(real) if real.is_dir() => Ok(real),
            Ok(_) => Err(Error::refused(format!(
                "{} is a file. Only a folder can be left out.",
                friendly_path(folder)
            ))),
            Err(_) if !left_out => {
                let said = folder.display().to_string();
                self.catalog()
                    .left_out_folders()?
                    .into_iter()
                    .find(|f| friendly_path(f) == said)
                    .ok_or_else(|| Error::not_found(format!("{said} isn't left out of this backup.")))
            }
            Err(_) => Err(Error::not_found(format!(
                "Keevoro can't find the folder {}. Is it plugged in?",
                friendly_path(folder)
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::tests::one_backup;

    fn offered(backup: &Backup) -> usize {
        backup
            .catalog()
            .load_reports()
            .unwrap()
            .iter()
            .map(|r| r.items.len())
            .sum()
    }

    /// Every file under `folder`, with its bytes.
    fn everything(folder: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        let mut all = Vec::new();
        for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
            if entry.path().is_dir() {
                all.extend(everything(&entry.path()));
            } else {
                all.push((entry.path(), std::fs::read(entry.path()).unwrap()));
            }
        }
        all.sort();
        all
    }

    #[test]
    fn a_folder_left_out_is_no_longer_offered_and_put_back_it_is_with_the_source_untouched() {
        let (_dir, card, backup) = one_backup();
        let before = everything(&card);
        let all = offered(&backup);
        let cornwall = card.join("Holidays").join("Cornwall");
        let beneath = std::fs::read_dir(&cornwall).unwrap().count();

        let out = backup.leave_out(&cornwall, true).unwrap();
        assert!(out.left_out);
        assert_eq!(out.files, beneath);
        assert!(
            out.said.contains("nothing already backed up is touched"),
            "{}",
            out.said
        );
        assert_eq!(offered(&backup), all - beneath);

        let back = backup.leave_out(&cornwall, false).unwrap();
        assert!(!back.left_out && back.still_inside.is_none());
        assert!(back.said.contains("offered again"), "{}", back.said);
        assert_eq!(offered(&backup), all);
        assert_eq!(everything(&card), before, "the source is byte for byte as it was");
    }

    #[test]
    fn a_folder_inside_one_still_left_out_says_so_when_put_back() {
        let (_dir, card, backup) = one_backup();
        let holidays = card.join("Holidays");
        backup.leave_out(&holidays, true).unwrap();
        backup.leave_out(&holidays.join("Cornwall"), true).unwrap();
        let back = backup.leave_out(&holidays.join("Cornwall"), false).unwrap();
        assert!(back.still_inside.is_some());
        assert!(back.said.contains("still left out"), "{}", back.said);
    }

    #[test]
    fn a_folder_gone_can_be_put_back_by_the_words_keevoro_gave_but_not_left_out() {
        let (dir, _card, backup) = one_backup();
        let games = dir.path().join("Games");
        std::fs::create_dir_all(&games).unwrap();
        let said = backup.leave_out(&games, true).unwrap().folder;
        std::fs::remove_dir(&games).unwrap();
        assert_eq!(backup.leave_out(&games, true).unwrap_err().kind, crate::Kind::NotFound);
        assert!(!backup.leave_out(Path::new(&said), false).unwrap().left_out);
        assert!(backup.catalog().left_out_folders().unwrap().is_empty());
        assert_eq!(
            backup.leave_out(Path::new(&said), false).unwrap_err().kind,
            crate::Kind::NotFound,
            "nothing left to put back"
        );
    }
}
