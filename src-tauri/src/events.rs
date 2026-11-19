//! What the app tells the screens while a long job runs.
//!
//! These are sent as Tauri events and received in `ui/src/backend.ts`,
//! which has the same shapes as TypeScript types. Change both together.
//!
//! The jobs themselves are the API's, and so are the shapes of what they
//! say (`keevoro_api::jobs::said`): the app sends each as it is, under the
//! name it has always had. Only gathering, from the older library model,
//! still has its shape here.

use keevoro_api::Said;
use keevoro_engine::GatherStep;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

pub const GATHER: &str = "gather";

/// The name of the window event a job's words go out as, or `None` for
/// what the screens receive another way: a survey's findings are the
/// answer to `survey_place`.
pub fn window_event(said: &Said) -> Option<&'static str> {
    match said {
        Said::Surveyed(_) => None,
        other => Some(other.name()),
    }
}

/// Hands what a job said to the window, exactly as the screens expect it.
pub fn to_window(app: &AppHandle, said: &Said) {
    if let Some(name) = window_event(said) {
        let _ = app.emit(name, said);
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
    use keevoro_api::jobs::{CheckEvent, HintView, ScanEvent, SendEvent, SurveyEvent, SurveyView};
    use std::collections::BTreeSet;

    const BACKEND: &str = include_str!("../../ui/src/backend.ts");

    /// The fields a type in `backend.ts` names, such as `ScanEvent`.
    fn fields_of(name: &str) -> BTreeSet<String> {
        let start = BACKEND
            .find(&format!("export type {name} = {{"))
            .unwrap_or_else(|| panic!("backend.ts has no type {name}"));
        let body = &BACKEND[start..];
        let open = body.find('{').unwrap() + 1;
        let first_line = &body[..body.find('\n').unwrap_or(body.len())];
        // A type on one line ends on it; a longer one ends with a brace of
        // its own at the start of a line.
        let close = if first_line.trim_end().ends_with('}') {
            first_line.trim_end().len() - 1
        } else {
            open + body[open..].find("\n}").unwrap()
        };
        let body = &body[open..close];
        // Comments say things like "where it is: x", which are not fields.
        let body: String = body
            .lines()
            .map(|line| line.split("/**").next().unwrap_or_default())
            .filter(|line| !line.trim_start().starts_with('*'))
            .collect::<Vec<_>>()
            .join(";");
        body.split(';')
            .filter_map(|part| {
                part.split_once(':')
                    .map(|(field, _)| field.trim().trim_end_matches('?'))
            })
            .filter(|field| !field.is_empty() && field.chars().all(|c| c.is_ascii_alphanumeric()))
            .map(str::to_string)
            .collect()
    }

    fn keys(value: &impl Serialize) -> BTreeSet<String> {
        serde_json::to_value(value)
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    }

    #[test]
    fn what_jobs_say_reaches_the_window_in_the_shapes_and_names_the_screens_expect() {
        let scan = ScanEvent {
            name: "Card".into(),
            photos: 1,
            videos: 2,
            files: 3,
            folder: "DCIM".into(),
            reading: Some(("a.mov".into(), 4)),
            saving: true,
            stopped: true,
            finished: true,
            problem: Some("x".into()),
            unreadable: 1,
            unreadable_names: vec!["b.jpg".into()],
        };
        assert_eq!(keys(&scan), fields_of("ScanEvent"), "every field, filled in");
        let send = SendEvent {
            line: Some("x".into()),
            finished: true,
            summary: vec!["y".into()],
            problem: Some("z".into()),
        };
        assert_eq!(keys(&send), fields_of("SendEvent"));
        let check = CheckEvent {
            done: 1,
            total: 2,
            finished: false,
        };
        assert_eq!(keys(&check), fields_of("CheckEvent"));
        let survey = SurveyEvent {
            folders: 1,
            files: 2,
            photos: 3,
            videos: 4,
        };
        assert_eq!(keys(&survey), fields_of("SurveyProgress"));
        let hint = HintView {
            folder: "x".into(),
            photos: 1,
            videos: 2,
            bytes: 3,
            likely_photos: true,
            already_a_source: false,
        };
        assert_eq!(keys(&hint), fields_of("Hint"));
        let found = SurveyView {
            hints: vec![hint],
            folders: 1,
            files: 2,
            unreadable: 3,
            seconds: 0.5,
            stopped: false,
        };
        assert_eq!(keys(&found), fields_of("Survey"));

        // Each goes out exactly as it is, under the name the screens
        // subscribe to; a survey's findings are an answer, not an event.
        for said in [
            Said::Scan(scan.clone()),
            Said::Send(send),
            Said::Check(check),
            Said::Survey(survey),
        ] {
            let name = window_event(&said).unwrap();
            assert!(
                BACKEND.contains(&format!("subscribe('{name}'")),
                "the screens do not listen for {name}"
            );
        }
        assert_eq!(
            serde_json::to_value(Said::Scan(scan.clone())).unwrap(),
            serde_json::to_value(&scan).unwrap()
        );
        assert_eq!(window_event(&Said::Surveyed(found)), None);
    }

    #[test]
    fn the_progress_of_a_send_has_the_shape_the_screens_expect() {
        let progress = crate::commands::SendProgress::default();
        assert_eq!(keys(&progress), fields_of("SendProgress"));
    }

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
    }
}
