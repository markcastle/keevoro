//! Telling the person, with the system's own notification, when a backup
//! by itself finishes with something worth saying: files sent, something
//! waiting for their say, or a backup that could not be done. Nothing is
//! said when there was nothing new, when the person stopped it, or when it
//! waited for a better moment, and nothing while the window is in front of
//! them, where the backup screen says it already. At most one notice for a
//! run. "Tell me when a backup by itself finishes" (Settings, Backing up
//! by itself) turns them off (`notify` in `window.json`).
//!
//! What is said is decided by [`notice`], pure, from the run's last word,
//! in the run's own sentences. A click on the notice brings the window
//! back on that backup's screen, where the system says so (the official
//! notification plugin reports a click on Windows while the notice is on
//! screen, and on a Mac).

use keevoro_api::jobs::{Outcome, SendEvent};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Mutex, PoisonError};
use tauri::{AppHandle, Manager, Runtime};

/// One notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub title: String,
    pub body: String,
}

/// What to tell the person when a backup by itself called `name` ends with
/// `last`, if anything. Pure.
pub fn notice(name: &str, last: &SendEvent) -> Option<Notice> {
    if !last.finished || last.not_now.is_some() {
        return None;
    }
    let sent = last.summary.iter().find(|l| l.starts_with("Safely backed up"));
    let body = match (last.outcome, last.held_back.first(), sent) {
        (Some(Outcome::Stopped), _, _) => return None,
        (_, Some(held), Some(sent)) => format!("{sent} Waiting for your say: {held}"),
        (_, Some(held), None) => format!("Waiting for your say: {held}"),
        (Some(Outcome::SomeFiles | Outcome::Unreachable | Outcome::Refused), _, _) => last
            .problem
            .clone()
            .or_else(|| last.summary.first().cloned())
            .unwrap_or_else(|| "It could not back everything up. Open Keevoro to see why.".into()),
        (_, None, Some(sent)) => sent.clone(),
        // Nothing new: nothing to say.
        (_, None, None) => return None,
    };
    let title = match last.outcome {
        Some(Outcome::Unreachable | Outcome::Refused) => format!("{name} couldn't be backed up"),
        _ => name.to_string(),
    };
    Some(Notice { title, body })
}

/// What to tell the person when a source they asked to be backed up
/// whenever it is plugged in has been, and its backup has started. Pure.
pub fn plugged_in(source: &str, backup: &str) -> Notice {
    Notice {
        title: source.to_string(),
        body: format!("Plugged in, so Keevoro is backing it up to \"{backup}\" now. It will say when it is done."),
    }
}

/// The backups notices were about, by the notice's number, so a click
/// opens the right one.
#[derive(Default)]
pub struct Sent {
    next: AtomicI32,
    about: Mutex<HashMap<i32, String>>,
}

/// Tells the person, if they want to be told and are not looking.
pub fn tell<R: Runtime>(app: &AppHandle<R>, backup: &str, notice: &Notice) {
    use tauri_plugin_notification::NotificationExt;
    let shared = app.state::<crate::Shared>();
    if !shared.keevoro.window_settings().notifies() {
        return;
    }
    let looking = app
        .get_webview_window(crate::window::WINDOW)
        .is_some_and(|w| w.is_focused().unwrap_or(false) && w.is_visible().unwrap_or(false));
    if looking {
        return;
    }
    let Some(sent) = app.try_state::<Sent>() else { return };
    let id = sent.next.fetch_add(1, Ordering::SeqCst) + 1;
    sent.about
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(id, backup.to_string());
    let shown = app
        .notification()
        .builder()
        .id(id)
        .title(&notice.title)
        .body(&notice.body)
        .show();
    if let Err(e) = shown {
        shared.keevoro.log().say(&format!("Couldn't show a notice: {e}"));
    }
}

/// Listens for clicks on notices: each brings the window back on the
/// backup it was about.
pub fn listen<R: Runtime>(app: &AppHandle<R>) {
    use tauri_plugin_notification::NotificationExt;
    app.manage(Sent::default());
    let handle = app.clone();
    let _ = app.notification().on_action(move |performed| {
        let backup = performed.notification().and_then(|n| {
            handle
                .state::<Sent>()
                .about
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(&n.id())
                .cloned()
        });
        let shared = handle.state::<crate::Shared>();
        // The window is made afresh on that backup, never switched under
        // a window already showing another, nor while work is in hand.
        if let Some(id) = backup {
            let window_open = handle.get_webview_window(crate::window::WINDOW).is_some();
            if !window_open && !shared.keevoro.busy_here() {
                let _ = shared.keevoro.switch_to(&id);
            }
        }
        crate::window::show(&handle, Some("send"));
    });
}

#[cfg(test)]
mod plugged_in_tests {
    #[test]
    fn a_source_plugged_in_is_said_by_its_name_with_the_backup_it_goes_to() {
        let said = super::plugged_in("Mum's card", "Family photos");
        assert_eq!(said.title, "Mum's card");
        assert_eq!(
            said.body,
            "Plugged in, so Keevoro is backing it up to \"Family photos\" now. It will say when it is done."
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ended(outcome: Outcome, summary: &[&str]) -> SendEvent {
        SendEvent::finished(summary.join("\n").as_bytes(), None, outcome)
    }

    #[test]
    fn files_sent_are_said_in_the_runs_own_words() {
        let last = ended(
            Outcome::Worked,
            &["Safely backed up 23 files (41.0 MB).", "Took 2 minutes."],
        );
        assert_eq!(
            notice("Family photos", &last),
            Some(Notice {
                title: "Family photos".into(),
                body: "Safely backed up 23 files (41.0 MB).".into()
            })
        );
    }

    #[test]
    fn nothing_new_a_stop_and_a_wait_for_a_better_moment_are_not_said() {
        assert_eq!(
            notice("Family photos", &ended(Outcome::Worked, &["Nothing new to back up."])),
            None
        );
        assert_eq!(
            notice(
                "Family photos",
                &ended(Outcome::Stopped, &["Safely backed up 2 files (1 MB)."])
            ),
            None
        );
        let waiting = SendEvent {
            not_now: Some("Waiting while this computer is on battery".into()),
            ..ended(Outcome::Worked, &[])
        };
        assert_eq!(notice("Family photos", &waiting), None);
        assert_eq!(notice("Family photos", &SendEvent::progress("Sending")), None);
    }

    #[test]
    fn what_waits_for_the_person_and_what_went_wrong_are_said() {
        let held = SendEvent {
            held_back: vec!["4 new pictures were set aside as probably not photos.".into()],
            ..ended(Outcome::Worked, &["Safely backed up 3 files (9 MB)."])
        };
        assert_eq!(
            notice("Family photos", &held).unwrap().body,
            "Safely backed up 3 files (9 MB). Waiting for your say: 4 new pictures were set aside as probably not photos."
        );
        let unplugged = SendEvent {
            problem: Some("Keevoro can't find the folder E:\\Backup. Is it plugged in?".into()),
            ..ended(Outcome::Unreachable, &[])
        };
        assert_eq!(
            notice("Family photos", &unplugged),
            Some(Notice {
                title: "Family photos couldn't be backed up".into(),
                body: "Keevoro can't find the folder E:\\Backup. Is it plugged in?".into()
            })
        );
        let some = ended(
            Outcome::SomeFiles,
            &["Couldn't back up 2 files, because they could not be read."],
        );
        assert_eq!(
            notice("Family photos", &some).unwrap().body,
            "Couldn't back up 2 files, because they could not be read."
        );
    }
}
