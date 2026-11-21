//! `backup.arrange` and `sources.arrange`: how a backup arranges photos
//! and videos where it sends them, by date or keeping the folders as they
//! are (see `keevoro_core::arrange`), and how one source does.
//!
//! It only changes where files go from now on. A file's place is agreed
//! once, so a file already backed up stays exactly where it is, is never
//! sent twice, and is never counted as missing; a file given a place but
//! never sent anywhere follows the new choice. A backup made before the
//! choice existed is by date, and is not asked.

use crate::backup::Backup;
use crate::error::{Error, Result};
use keevoro_core::words::count;
pub use keevoro_core::Arrange;
use serde::{Deserialize, Serialize};

/// How to arrange, as a request says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ArrangeWord {
    /// A folder for each year and month, keeping folders with real names.
    ByDate,
    /// The same folders and names as on the source, starting with the
    /// source's own folder.
    Folders,
}

impl ArrangeWord {
    pub fn arrange(self) -> Arrange {
        match self {
            ArrangeWord::ByDate => Arrange::ByDate,
            ArrangeWord::Folders => Arrange::Folders,
        }
    }
}

/// `backup.arrange`: without `arrange` it only reads.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackupArrangeRequest {
    /// `byDate` or `folders`. Left out, nothing changes.
    #[serde(default)]
    pub arrange: Option<ArrangeWord>,
}

/// A source's choice, as a request says it: `backup` to follow its
/// backup, or its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SourceArrangeWord {
    /// The same as the backup.
    Backup,
    /// By date, whatever the backup does.
    ByDate,
    /// Keeping its folders as they are, whatever the backup does.
    Folders,
}

impl SourceArrangeWord {
    /// Its own choice, or `None` to follow the backup.
    pub fn own(self) -> Option<Arrange> {
        match self {
            SourceArrangeWord::Backup => None,
            SourceArrangeWord::ByDate => Some(Arrange::ByDate),
            SourceArrangeWord::Folders => Some(Arrange::Folders),
        }
    }
}

/// `sources.arrange`: without `arrange` it only reads.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceArrangeRequest {
    /// The source's id, as `sources.list` gives it.
    pub source: i64,
    /// `backup` (the same as the backup), `byDate` or `folders`. Left out,
    /// nothing changes.
    #[serde(default)]
    pub arrange: Option<SourceArrangeWord>,
}

/// How one source's photos and videos are arranged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceArrangeView {
    /// What is in effect: `byDate` or `folders`.
    pub arrange: Arrange,
    /// True when it has its own choice, false when it follows the backup.
    pub own: bool,
    /// The folder its files go in at the top of the backup, when it keeps
    /// its folders.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kept_as: Option<String>,
    /// Other sources whose files go into that same folder.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub shares_folder_with: Vec<String>,
    /// In words: "Files from Family Scans keep their folders as they are."
    /// with what is shared, and after a change, what stays where it is.
    pub said: String,
}

/// What is said of one source's arrangement.
pub fn source_said(name: &str, arrange: Arrange, own: bool) -> String {
    match (own, arrange) {
        (true, Arrange::Folders) => format!("Files from {name} keep their folders as they are."),
        (true, Arrange::ByDate) => format!("Files from {name} are arranged by date."),
        (false, arrange) => format!("Files from {name} are arranged as the backup is: {}.", arrange.words()),
    }
}

/// How a backup is arranged, and what that means.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArrangeView {
    /// What is in effect: `byDate` or `folders`.
    pub arrange: Arrange,
    /// True once the person has chosen, or the backup has sent anything:
    /// the app asks the first time only while this is false.
    pub asked: bool,
    /// In words: "This backup is arranged by date."
    pub said: String,
}

/// What is said of a backup's arrangement.
pub fn said_of(arrange: Arrange) -> String {
    match arrange {
        Arrange::ByDate => "This backup is arranged by date.".to_string(),
        Arrange::Folders => "This backup keeps your folders as they are.".to_string(),
    }
}

/// What is said after a change, so nobody fears for what is already safe:
/// "From now on, new files are kept in your folders. The 1,204 files
/// already backed up stay where they are."
pub fn changed_words(arrange: Arrange, already: usize) -> String {
    let from_now_on = match arrange {
        Arrange::ByDate => "From now on, new files are arranged by date.",
        Arrange::Folders => "From now on, new files are kept in your folders.",
    };
    match already {
        0 => from_now_on.to_string(),
        1 => format!("{from_now_on} The 1 file already backed up stays where it is."),
        n => format!(
            "{from_now_on} The {} already backed up stay where they are.",
            count(n, "file")
        ),
    }
}

impl Backup {
    /// `backup.arrange`, read.
    pub fn arrangement(&self) -> Result<ArrangeView> {
        let chosen = self.catalog().arrange()?;
        let arrange = chosen.unwrap_or_default();
        Ok(ArrangeView {
            arrange,
            asked: chosen.is_some() || self.catalog().placed_and_sent()? > 0,
            said: said_of(arrange),
        })
    }

    /// `backup.arrange`, set. Only what is sent from now on follows it.
    pub fn set_arrangement(&self, arrange: Arrange) -> Result<ArrangeView> {
        let before = self.catalog().arrange()?.unwrap_or_default();
        let already = self.catalog().placed_and_sent()?;
        self.catalog().set_arrange(arrange)?;
        if before != arrange && already > 0 {
            self.catalog().set_arrange_changed_at(keevoro_engine::confirm::now())?;
        }
        let mut view = self.arrangement()?;
        if before != arrange {
            view.said = changed_words(arrange, already);
        }
        Ok(view)
    }
}

impl Backup {
    /// `sources.arrange`, read.
    pub fn source_arrangement(&self, source: i64) -> Result<SourceArrangeView> {
        let view = self
            .sources()?
            .into_iter()
            .find(|s| s.id == source)
            .ok_or_else(|| Error::not_found("That source isn't part of this backup."))?;
        let own = self.catalog().source_arrange(source)?;
        let arrange = own.unwrap_or(self.catalog().arrange()?.unwrap_or_default());
        let mut said = source_said(&view.name, arrange, own.is_some());
        if let Some(shared) = &view.shared_said {
            said = format!("{said} {shared}");
        }
        Ok(SourceArrangeView {
            arrange,
            own: own.is_some(),
            kept_as: view.kept_as,
            shares_folder_with: view.shares_folder_with,
            said,
        })
    }

    /// `sources.arrange`, set: its own choice, or with `None` the backup's.
    pub fn set_source_arrangement(&self, source: i64, own: Option<Arrange>) -> Result<SourceArrangeView> {
        let before = self.source_arrangement(source)?.arrange;
        if !self.catalog().set_source_arrange(source, own)? {
            return Err(Error::not_found("That source isn't part of this backup."));
        }
        let mut view = self.source_arrangement(source)?;
        if view.arrange != before {
            let already = self.catalog().placed_and_sent()?;
            if already > 0 {
                self.catalog().set_arrange_changed_at(keevoro_engine::confirm::now())?;
            }
            view.said = format!("{} {}", view.said, from_now_on(view.arrange));
        }
        Ok(view)
    }
}

/// After a source's choice changes: what it means for what is safe.
fn from_now_on(arrange: Arrange) -> &'static str {
    match arrange {
        Arrange::Folders => {
            "From now on its new files are kept in its folders; files already backed up stay where they are."
        }
        Arrange::ByDate => {
            "From now on its new files are arranged by date; files already backed up stay where they are."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::tests::one_backup;

    #[test]
    fn a_new_backup_has_not_been_asked_and_choosing_says_what_happens_to_what_is_safe() {
        let (_dir, _card, backup) = one_backup();
        let read = backup.arrangement().unwrap();
        assert_eq!(
            (read.arrange, read.asked, read.said.as_str()),
            (Arrange::ByDate, false, "This backup is arranged by date.")
        );
        let set = backup.set_arrangement(Arrange::Folders).unwrap();
        assert_eq!(set.arrange, Arrange::Folders);
        assert!(set.asked);
        assert_eq!(set.said, "From now on, new files are kept in your folders.");
        assert_eq!(
            backup.arrangement().unwrap().said,
            "This backup keeps your folders as they are."
        );
        assert_eq!(
            backup.catalog().arrange_changed_at().unwrap(),
            None,
            "nothing was sent, so nothing was arranged another way"
        );
        // The same again changes nothing, and says what is.
        assert_eq!(
            backup.set_arrangement(Arrange::Folders).unwrap().said,
            "This backup keeps your folders as they are."
        );
    }

    #[test]
    fn the_words_after_a_change_count_what_stays_where_it_is() {
        assert_eq!(
            changed_words(Arrange::Folders, 1204),
            "From now on, new files are kept in your folders. The 1,204 files already backed up stay where they are."
        );
        assert_eq!(
            changed_words(Arrange::ByDate, 1),
            "From now on, new files are arranged by date. The 1 file already backed up stays where it is."
        );
    }

    #[test]
    fn an_agent_may_read_it_and_change_it_only_once_changes_are_allowed_and_a_backup_can_start_with_it() {
        use crate::{call, call_for_agent, Op};
        use serde_json::json;
        let dir = tempfile::tempdir().unwrap();
        let k = crate::keevoro::tests::keevoro(dir.path());
        call(
            &k,
            Op::BackupsStart,
            None,
            json!({ "name": "Scans", "arrange": "folders" }),
        )
        .unwrap();
        let read = call_for_agent(&k, Op::BackupArrange, Some("Scans"), json!({})).unwrap();
        assert_eq!(
            (read["arrange"].as_str(), read["asked"].as_bool()),
            (Some("folders"), Some(true))
        );
        let e = call_for_agent(&k, Op::BackupArrange, Some("Scans"), json!({ "arrange": "byDate" })).unwrap_err();
        assert_eq!(e.message, crate::for_agents::LOOK_ONLY);
        k.set_agent_access(crate::AgentsAccessRequest {
            look_in: None,
            back_up_to: None,
            may_change: Some(true),
        })
        .unwrap();
        let set = call_for_agent(&k, Op::BackupArrange, Some("Scans"), json!({ "arrange": "byDate" })).unwrap();
        assert_eq!(set["arrange"], "byDate");
        let e = call_for_agent(&k, Op::BackupArrange, Some("Scans"), json!({ "arrange": "sideways" })).unwrap_err();
        assert_eq!(e.kind, crate::Kind::NotUnderstood);
        assert!(k
            .log_tail(10)
            .contains("This backup now arranges new files by date. Nothing already backed up was moved"));
    }

    #[test]
    fn a_source_can_keep_its_folders_while_the_backup_goes_by_date_and_back_again() {
        let (_dir, _card, backup) = one_backup();
        let source = backup.sources().unwrap()[0].id;
        let name = backup.sources().unwrap()[0].name.clone();
        let read = backup.source_arrangement(source).unwrap();
        assert_eq!((read.arrange, read.own), (Arrange::ByDate, false));
        assert_eq!(
            read.said,
            format!("Files from {name} are arranged as the backup is: by date.")
        );
        let set = backup.set_source_arrangement(source, Some(Arrange::Folders)).unwrap();
        assert_eq!((set.arrange, set.own), (Arrange::Folders, true));
        assert!(set.kept_as.is_some());
        assert!(
            set.said.starts_with(&format!(
                "Files from {name} keep their folders as they are. From now on"
            )),
            "{}",
            set.said
        );
        let back = backup.set_source_arrangement(source, None).unwrap();
        assert!(!back.own);
        assert_eq!(backup.source_arrangement(9999).unwrap_err().kind, crate::Kind::NotFound);
        assert_eq!(
            backup.set_source_arrangement(9999, None).unwrap_err().kind,
            crate::Kind::NotFound
        );
    }
}
