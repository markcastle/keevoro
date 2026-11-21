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
    /// What its files suggest, when that is not what it does now.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<SuggestionView>,
}

/// What a source's photos and videos, as found, suggest. Pure arithmetic
/// over what the catalog holds: nothing is read from the source.
pub fn suggestion_for(
    root: &std::path::Path,
    name: &str,
    items: &[keevoro_scan::FoundItem],
) -> Option<keevoro_core::arrange::Suggestion> {
    use keevoro_core::arrange::{FileSeen, SourceFacts};
    let root_name = keevoro_core::arrange::top_folder(root, name);
    let files = items
        .iter()
        .filter(|i| i.kind != keevoro_core::MediaKind::Other)
        .map(|i| FileSeen {
            folders: i
                .path
                .strip_prefix(root)
                .ok()
                .and_then(std::path::Path::parent)
                .map(|p| p.iter().filter_map(std::ffi::OsStr::to_str).collect())
                .unwrap_or_default(),
            album: &i.folder.album,
            date: i.date,
            name: i.path.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or_default(),
            kind: i.kind,
        })
        .collect();
    keevoro_core::arrange::suggest(&SourceFacts {
        root_name: &root_name,
        files,
    })
}

/// "Suggestion: keep its folders as they are. These look like scans."
fn for_a_source(suggestion: &keevoro_core::arrange::Suggestion) -> SuggestionView {
    let what = match suggestion.arrange {
        Arrange::Folders => "Suggestion: keep its folders as they are.",
        Arrange::ByDate => "Suggestion: arrange it by date.",
    };
    SuggestionView {
        arrange: suggestion.arrange,
        said: format!("{what} {}", suggestion.why_for_the_source()),
    }
}

/// What is said of one source's arrangement.
pub fn source_said(name: &str, arrange: Arrange, own: bool) -> String {
    match (own, arrange) {
        (true, Arrange::Folders) => format!("Files from {name} keep their folders as they are."),
        (true, Arrange::ByDate) => format!("Files from {name} are arranged by date."),
        (false, arrange) => format!("Files from {name} are arranged as the backup is: {}.", arrange.words()),
    }
}

/// An arrangement worth suggesting, and why, in words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestionView {
    /// `byDate` or `folders`.
    pub arrange: Arrange,
    /// "Suggested because 412 of the 420 photos in Family Scans have the
    /// same date, 14 March 2021, which is probably when they were copied
    /// or scanned, not taken."
    pub said: String,
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
    /// What the files found suggest, for the whole backup: the suggestion
    /// that covers most of its photos and videos, by size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<SuggestionView>,
    /// True when its sources suggest different arrangements, so each can
    /// be chosen on its own later.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub sources_differ: bool,
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
        let (suggestion, sources_differ) = self.suggestion()?;
        Ok(ArrangeView {
            arrange,
            asked: chosen.is_some() || self.catalog().placed_and_sent()? > 0,
            said: said_of(arrange),
            suggestion,
            sources_differ,
        })
    }

    /// What the files found suggest for the whole backup: of each source's
    /// suggestion (or none), the one covering most bytes of photos and
    /// videos, saying why from the biggest source it came from; and whether
    /// the sources differ.
    fn suggestion(&self) -> Result<(Option<SuggestionView>, bool)> {
        // Each outcome (or none), the bytes it covers, and why, from the
        // biggest source that suggested it, with that source's bytes.
        type Why = Option<(u64, String)>;
        let mut by_outcome: Vec<(Option<Arrange>, u64, Why)> = Vec::new();
        for source in self.catalog().sources()? {
            let items = self.catalog().found_on(source.id)?;
            let bytes: u64 = items
                .iter()
                .filter(|i| i.kind != keevoro_core::MediaKind::Other)
                .map(|i| i.bytes)
                .sum();
            let suggested = suggestion_for(&source.root, &source.name, &items);
            let outcome = suggested.as_ref().map(|s| s.arrange);
            let why = suggested.map(|s| (bytes, s.why_for_the_backup(&source.name)));
            match by_outcome.iter_mut().find(|(o, _, _)| *o == outcome) {
                Some((_, total, best)) => {
                    *total += bytes;
                    if let Some(why) = why {
                        if best.as_ref().is_none_or(|(b, _)| why.0 > *b) {
                            *best = Some(why);
                        }
                    }
                }
                None => by_outcome.push((outcome, bytes, why)),
            }
        }
        let differ = by_outcome.iter().filter(|(o, _, _)| o.is_some()).count() > 1;
        let biggest = by_outcome.into_iter().max_by_key(|(_, bytes, _)| *bytes);
        Ok((
            biggest.and_then(|(outcome, _, why)| {
                Some(SuggestionView {
                    arrange: outcome?,
                    said: format!("Suggested because {}", why?.1),
                })
            }),
            differ,
        ))
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
    /// `sources.arrange`, read. Asks only the record, never the whole
    /// Discover view, so each source's card can ask it quickly.
    pub fn source_arrangement(&self, source: i64) -> Result<SourceArrangeView> {
        let sources = self.catalog().sources()?;
        let at = sources
            .iter()
            .position(|s| s.id == source)
            .ok_or_else(|| Error::not_found("That source isn't part of this backup."))?;
        let choices = self.catalog().choices()?;
        let (kept_as, shares_folder_with, shared) = keevoro_engine::discover::kept_as(&sources, &choices)
            .into_iter()
            .nth(at)
            .unwrap_or_default();
        let found = &sources[at];
        let own = self.catalog().source_arrange(source)?;
        let arrange = choices.arrangement_in(&found.root);
        let mut said = source_said(&found.name, arrange, own.is_some());
        if let Some(shared) = shared {
            said = format!("{said} {shared}");
        }
        let suggestion = suggestion_for(&found.root, &found.name, &self.catalog().found_on(source)?)
            .filter(|s| s.arrange != arrange)
            .map(|s| for_a_source(&s));
        Ok(SourceArrangeView {
            arrange,
            own: own.is_some(),
            kept_as,
            shares_folder_with,
            said,
            suggestion,
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

    #[test]
    fn a_folder_of_scans_all_dated_the_same_day_is_suggested_to_keep_its_folders() {
        use keevoro_scan::FoundItem;
        let root = std::path::PathBuf::from("/m/Family");
        let one = |i: usize| FoundItem {
            path: root.join(format!("{i:04}.jpg")),
            kind: keevoro_core::MediaKind::Photo,
            mime: "image/jpeg".into(),
            bytes: 1,
            blake3: None,
            hashed: true,
            date: Some(keevoro_core::DateGuess {
                date: keevoro_core::PartialDate::parse(if i < 412 { "2021-03-14" } else { "2020-01-02" }).unwrap(),
                source: keevoro_core::DateSource::Exif,
            }),
            folder: keevoro_core::FolderHints::default(),
            width: None,
            height: None,
            junk: None,
        };
        let items: Vec<FoundItem> = (0..420).map(one).collect();
        let s = suggestion_for(&root, "Family", &items).unwrap();
        assert_eq!(s.arrange, Arrange::Folders);
        assert_eq!(
            for_a_source(&s).said,
            "Suggestion: keep its folders as they are. 412 of its 420 photos have the same date, 14 March 2021, \
             which is probably when they were copied or scanned, not taken."
        );
        // The tester's own folder says what it is by its name.
        let scans = std::path::PathBuf::from("/m/Family Scans");
        let moved: Vec<FoundItem> = items
            .into_iter()
            .map(|mut i| {
                i.path = scans.join(i.path.file_name().unwrap());
                i
            })
            .collect();
        assert_eq!(
            for_a_source(&suggestion_for(&scans, "Family Scans", &moved).unwrap()).said,
            "Suggestion: keep its folders as they are. These look like scans."
        );
    }
}
