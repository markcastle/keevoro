//! What a running job says, in the app's own words.
//!
//! These are the shapes the app's screens have always received, kept here
//! so the command line and an agent hear exactly what the app hears. The
//! app sends each as a window event named by [`Said::name`], and
//! `ui/src/backend.ts` has the same shapes as TypeScript types. Change both
//! together; the app's tests compare them.

use serde::Serialize;

/// How far a survey of a drive has got. Counts are by file name alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SurveyEvent {
    pub folders: usize,
    pub files: usize,
    pub photos: usize,
    pub videos: usize,
}

/// Progress reading, in the background, the files a quick search only
/// sampled, and a last one when the pass ends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CheckEvent {
    pub done: usize,
    pub total: usize,
    pub finished: bool,
}

/// Progress looking through a drive, and a last one when it is done.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScanEvent {
    /// The drive's friendly name.
    pub name: String,
    pub photos: u64,
    pub videos: u64,
    /// Every file looked at so far, photos or not.
    pub files: u64,
    /// The folder being looked in right now, for the curious.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub folder: String,
    /// A large file being read right now: where it is and its size in
    /// bytes. Reading one can take minutes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reading: Option<(String, u64)>,
    /// True once the looking is over and what was found is being saved.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub saving: bool,
    /// On the last event: true when the search was stopped part-way, so
    /// nothing from it was kept.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub stopped: bool,
    pub finished: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
    /// On the last event: how many files could not be read, and the first
    /// few by name. All of them are in the log.
    #[serde(skip_serializing_if = "is_zero")]
    pub unreadable: usize,
    #[serde(rename = "unreadableNames", skip_serializing_if = "Vec::is_empty")]
    pub unreadable_names: Vec<String>,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// Progress backing up or checking, and a last one when it is done.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SendEvent {
    /// A short line saying how far it has got, while it runs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    pub finished: bool,
    /// On the last event: what happened, as sentences to show.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub summary: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

impl SendEvent {
    pub fn progress(line: &str) -> Self {
        Self {
            line: Some(line.to_string()),
            finished: false,
            summary: Vec::new(),
            problem: None,
        }
    }

    /// The last event: the sentences the engine wrote, and any problem.
    pub fn finished(said: &[u8], problem: Option<String>) -> Self {
        Self {
            line: None,
            finished: true,
            summary: String::from_utf8_lossy(said)
                .lines()
                .map(str::trim_end)
                // Advice about commands to type belongs to the command
                // line; the app offers a button instead.
                .filter(|l| !l.is_empty() && !l.contains("`keevoro "))
                .map(str::to_string)
                .collect(),
            problem,
        }
    }
}

/// A folder a survey suggests looking through.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HintView {
    /// The folder, as it is shown and as it is handed back to be searched.
    pub folder: String,
    pub photos: usize,
    pub videos: usize,
    pub bytes: u64,
    /// False when the pictures are so small they are probably not photos.
    pub likely_photos: bool,
    /// True when this folder is already one of the backup's sources.
    pub already_a_source: bool,
}

/// What a survey of a drive found: its last word.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SurveyView {
    pub hints: Vec<HintView>,
    pub folders: usize,
    pub files: usize,
    pub unreadable: usize,
    pub seconds: f32,
    pub stopped: bool,
}

/// One thing a job said. It is written out as the shape inside it, with
/// nothing around it, which is exactly what the app's screens receive.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Said {
    /// Looking through a source, and its end.
    Scan(ScanEvent),
    /// How far a survey has got.
    Survey(SurveyEvent),
    /// What a survey found, when it ends. The app has it as the answer to
    /// `survey_place`, not as an event.
    Surveyed(SurveyView),
    /// Backing up or checking, and its end.
    Send(SendEvent),
    /// Checking for duplicates in the background, and its end.
    Check(CheckEvent),
}

impl Said {
    /// Its name, which is also the name of the window event the app sends
    /// it as: `scan`, `survey`, `send` or `check`. A survey's result is
    /// `surveyed`.
    pub fn name(&self) -> &'static str {
        match self {
            Said::Scan(_) => "scan",
            Said::Survey(_) => "survey",
            Said::Surveyed(_) => "surveyed",
            Said::Send(_) => "send",
            Said::Check(_) => "check",
        }
    }

    /// True for the last thing a job says.
    pub fn is_last(&self) -> bool {
        match self {
            Said::Scan(e) => e.finished,
            Said::Send(e) => e.finished,
            Said::Check(e) => e.finished,
            Said::Surveyed(_) => true,
            Said::Survey(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_have_the_shape_the_screens_expect() {
        let going = SendEvent::progress("3,412 of 9,870 safely backed up");
        assert_eq!(
            serde_json::to_value(&going).unwrap(),
            serde_json::json!({ "line": "3,412 of 9,870 safely backed up", "finished": false })
        );
        let done = SendEvent::finished(b"Safely backed up 3 files (1.2 MB).\n\nYour private index: x\n", None);
        assert_eq!(
            done.summary,
            ["Safely backed up 3 files (1.2 MB).", "Your private index: x"]
        );
        assert!(done.finished && done.line.is_none());
        let hinted = SendEvent::finished(b"Couldn't back up 1 file:\n  a.jpg: The connection kept dropping.\nRun `keevoro publish` again to retry.\n", None);
        assert_eq!(
            hinted.summary.len(),
            2,
            "command-line advice is left out: {:?}",
            hinted.summary
        );
        let failed = SendEvent::finished(b"", Some("Couldn't connect to the storage.".into()));
        assert_eq!(
            serde_json::to_value(&failed).unwrap(),
            serde_json::json!({ "finished": true, "problem": "Couldn't connect to the storage." })
        );

        let scan = ScanEvent {
            name: "Dad's old Dell".into(),
            photos: 4210,
            videos: 312,
            files: 98_000,
            folder: String::new(),
            reading: None,
            saving: false,
            stopped: false,
            finished: false,
            problem: None,
            unreadable: 0,
            unreadable_names: Vec::new(),
        };
        assert_eq!(
            serde_json::to_value(&scan).unwrap(),
            serde_json::json!({ "name": "Dad's old Dell", "photos": 4210, "videos": 312, "files": 98000, "finished": false })
        );
    }

    #[test]
    fn what_is_said_is_written_as_the_shape_inside_it_and_named_as_the_app_names_it() {
        let check = CheckEvent {
            done: 2,
            total: 9,
            finished: false,
        };
        assert_eq!(
            serde_json::to_value(Said::Check(check.clone())).unwrap(),
            serde_json::to_value(&check).unwrap()
        );
        let survey = SurveyEvent {
            folders: 3,
            files: 40,
            photos: 30,
            videos: 1,
        };
        assert_eq!(
            serde_json::to_value(Said::Survey(survey)).unwrap(),
            serde_json::json!({ "folders": 3, "files": 40, "photos": 30, "videos": 1 })
        );
        let names: Vec<&str> = [
            Said::Scan(ScanEvent {
                name: String::new(),
                photos: 0,
                videos: 0,
                files: 0,
                folder: String::new(),
                reading: Some(("a".into(), 1)),
                saving: true,
                stopped: true,
                finished: true,
                problem: Some("x".into()),
                unreadable: 1,
                unreadable_names: vec!["b".into()],
            }),
            Said::Survey(survey),
            Said::Send(SendEvent::progress("x")),
            Said::Check(check),
        ]
        .iter()
        .map(Said::name)
        .collect();
        assert_eq!(names, ["scan", "survey", "send", "check"]);
        assert!(!Said::Send(SendEvent::progress("x")).is_last());
        assert!(Said::Send(SendEvent::finished(b"", None)).is_last());
    }
}
