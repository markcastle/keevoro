//! What the app tells the screens while a long job runs.
//!
//! These are sent as Tauri events and received in `ui/src/backend.ts`,
//! which has the same shapes as TypeScript types. Change both together.

use keevoro_engine::GatherStep;
use serde::Serialize;

pub const SCAN: &str = "scan";
pub const GATHER: &str = "gather";

/// Progress looking through a drive, and a last one when it is done.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScanEvent {
    /// The drive's friendly name.
    pub name: String,
    pub photos: u64,
    pub videos: u64,
    pub finished: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
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

        let scan = ScanEvent {
            name: "Dad's old Dell".into(),
            photos: 4210,
            videos: 312,
            finished: false,
            problem: None,
        };
        assert_eq!(
            serde_json::to_value(&scan).unwrap(),
            serde_json::json!({ "name": "Dad's old Dell", "photos": 4210, "videos": 312, "finished": false })
        );
    }
}
