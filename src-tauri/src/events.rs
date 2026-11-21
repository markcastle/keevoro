//! What the app tells the screens while a long job runs.
//!
//! These are sent as Tauri events and received in `ui/src/backend.ts`,
//! which has the same shapes as TypeScript types. Change both together.
//!
//! The jobs themselves are the API's, and so are the shapes of what they
//! say (`keevoro_api::jobs::said`): the app sends each as it is, under the
//! name it has always had.

use keevoro_api::Said;
use tauri::{AppHandle, Emitter, Runtime};

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
pub fn to_window<R: Runtime>(app: &AppHandle<R>, said: &Said) {
    if let Some(name) = window_event(said) {
        let _ = app.emit(name, said);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use keevoro_api::jobs::{CheckEvent, HintView, Outcome, ScanEvent, SendEvent, SurveyEvent, SurveyView, TopView};
    use serde::Serialize;
    use std::collections::BTreeSet;

    const BACKEND: &str = include_str!("../../ui/src/backend.ts");

    /// The fields a type in `backend.ts` names, such as `ScanEvent`.
    fn fields_of(name: &str) -> BTreeSet<String> {
        shape_of(name).into_keys().collect()
    }

    /// The fields a type in `backend.ts` names, each with whether it may
    /// be left out (`field?: ...`).
    pub(crate) fn shape_of(name: &str) -> std::collections::BTreeMap<String, bool> {
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
            .filter_map(|part| part.split_once(':').map(|(field, _)| field.trim()))
            .map(|field| (field.trim_end_matches('?'), field.ends_with('?')))
            .filter(|(field, _)| !field.is_empty() && field.chars().all(|c| c.is_ascii_alphanumeric()))
            .map(|(field, optional)| (field.to_string(), optional))
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
            others: 5,
            files: 3,
            folder: "DCIM".into(),
            reading: Some(("a.mov".into(), 4)),
            saving: true,
            stopped: true,
            finished: true,
            problem: Some("x".into()),
            unreadable: 1,
            unreadable_names: vec!["b.jpg".into()],
            left_out: vec![keevoro_api::jobs::own::LeftOut {
                folder: "D:\\Keevoro Library".into(),
                what: "oldLibrary".into(),
                said: "Left out D:\\Keevoro Library.".into(),
            }],
            online_only: 412,
            online_only_said: Some("412 files here are in OneDrive only.".into()),
        };
        assert_eq!(keys(&scan), fields_of("ScanEvent"), "every field, filled in");
        let send = SendEvent {
            line: Some("x".into()),
            finished: true,
            summary: vec!["y".into()],
            problem: Some("z".into()),
            outcome: Some(Outcome::Worked),
        };
        assert_eq!(keys(&send), fields_of("SendEvent"));
        let check = CheckEvent {
            done: 1,
            total: 2,
            finished: false,
        };
        assert_eq!(keys(&check), fields_of("CheckEvent"));
        let top = TopView {
            name: "Camera Roll".into(),
            node: "smaller".into(),
            bytes: 9,
            photos: 2,
            videos: 0,
            others: 1,
            photo_bytes: 8,
            video_bytes: 0,
            other_bytes: 1,
            carried: 4,
            carried_bytes: 40,
            folders: 3,
        };
        assert_eq!(keys(&top), fields_of("TopBox"));
        let survey = SurveyEvent {
            folders: 1,
            files: 2,
            photos: 3,
            videos: 4,
            others: 6,
            sizing: true,
            top: vec![top],
        };
        assert_eq!(keys(&survey), fields_of("SurveyProgress"));
        let hint = HintView {
            folder: "x".into(),
            photos: 1,
            videos: 2,
            others: 7,
            bytes: 3,
            likely_photos: true,
            already_a_source: false,
            online_only: 4,
            online_only_said: Some("4 files here are in OneDrive only.".into()),
        };
        assert_eq!(keys(&hint), fields_of("Hint"));
        let found = SurveyView {
            hints: vec![hint],
            folders: 1,
            files: 2,
            unreadable: 3,
            seconds: 0.5,
            stopped: false,
            left_out: vec![keevoro_api::jobs::own::LeftOut {
                folder: "D:\\Backups".into(),
                what: "keevoroBackup".into(),
                said: "Left out a Keevoro backup at D:\\Backups.".into(),
            }],
            online_only: 4,
            online_only_said: Some("4 files here are in OneDrive only.".into()),
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
}
