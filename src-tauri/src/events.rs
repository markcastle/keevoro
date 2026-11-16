//! What the app tells the screens while a long job runs.
//!
//! These are sent as Tauri events and received in `ui/src/backend.ts`,
//! which has the same shapes as TypeScript types. Change both together.

use keevoro_engine::GatherStep;
use serde::Serialize;

pub const SCAN: &str = "scan";
pub const GATHER: &str = "gather";
pub const BACKUP: &str = "backup";
pub const CHECK: &str = "check";

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
    pub finished: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

/// Progress backing up or checking, and a last one when it is done.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BackupEvent {
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

impl BackupEvent {
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

/// Progress gathering, and a last one (`step: "finished"`) when it is done.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GatherEvent {
    pub step: &'static str,
    pub done: usize,
    pub total: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

impl From<GatherStep> for GatherEvent {
    fn from(step: GatherStep) -> Self {
        let (step, done, total) = match step {
            GatherStep::Checking { done, total } => ("checking", done, total),
            GatherStep::Copying { done, total } => ("copying", done, total),
            GatherStep::MakingPreviews { done, total } => ("previews", done, total),
            GatherStep::WorkingOnVideos { done, total } => ("videos", done, total),
        };
        Self {
            step,
            done,
            total,
            summary: None,
            problem: None,
        }
    }
}

impl GatherEvent {
    /// The last event of a gather: what happened, or what went wrong.
    pub fn finished(summary: Vec<String>, problem: Option<String>) -> Self {
        Self {
            step: "finished",
            done: 0,
            total: 0,
            summary: Some(summary),
            problem,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_have_the_shape_the_screens_expect() {
        let copying: GatherEvent = GatherStep::Copying { done: 3, total: 10 }.into();
        assert_eq!(
            serde_json::to_value(&copying).unwrap(),
            serde_json::json!({ "step": "copying", "done": 3, "total": 10 })
        );
        let previews: GatherEvent = GatherStep::MakingPreviews { done: 1, total: 2 }.into();
        let videos: GatherEvent = GatherStep::WorkingOnVideos { done: 1, total: 2 }.into();
        assert_eq!((previews.step, videos.step), ("previews", "videos"));

        let done = GatherEvent::finished(vec!["Safely copied 2 files.".into()], None);
        assert_eq!(
            serde_json::to_value(&done).unwrap(),
            serde_json::json!({ "step": "finished", "done": 0, "total": 0, "summary": ["Safely copied 2 files."] })
        );
        let failed = GatherEvent::finished(vec![], Some("There isn't enough room.".into()));
        assert_eq!(
            serde_json::to_value(&failed).unwrap()["problem"],
            "There isn't enough room."
        );

        let going = BackupEvent::progress("3,412 of 9,870 safely backed up");
        assert_eq!(
            serde_json::to_value(&going).unwrap(),
            serde_json::json!({ "line": "3,412 of 9,870 safely backed up", "finished": false })
        );
        let done = BackupEvent::finished(b"Safely backed up 3 files (1.2 MB).\n\nYour private index: x\n", None);
        assert_eq!(
            done.summary,
            ["Safely backed up 3 files (1.2 MB).", "Your private index: x"]
        );
        assert!(done.finished && done.line.is_none());
        let hinted = BackupEvent::finished(b"Couldn't back up 1 file:\n  a.jpg: The connection kept dropping.\nRun `keevoro publish` again to retry.\n", None);
        assert_eq!(
            hinted.summary.len(),
            2,
            "command-line advice is left out: {:?}",
            hinted.summary
        );
        let failed = BackupEvent::finished(b"", Some("Couldn't connect to the storage.".into()));
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
            finished: false,
            problem: None,
        };
        assert_eq!(
            serde_json::to_value(&scan).unwrap(),
            serde_json::json!({ "name": "Dad's old Dell", "photos": 4210, "videos": 312, "files": 98000, "finished": false })
        );
    }
}
