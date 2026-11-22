//! The window's commands for keeping Keevoro running by the clock: the
//! choices in Settings, the first-close explanation's answer, quitting,
//! and what waits for the window when it comes forward. All of them are
//! about being a window (`WINDOW_ONLY` in `parity.rs`): the command line
//! and an agent have no window to close.

use crate::closing::{self, Tray};
use crate::window::{self, Ask};
use crate::Shared;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime, State};

type Said<T> = Result<T, String>;

/// The choices about closing, minimising and starting with the computer,
/// as Settings shows them.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeepRunningView {
    /// Closing the window keeps Keevoro running by the clock.
    pub keep_running: bool,
    /// Minimising hides the window to the icon by the clock too.
    pub minimise_to_tray: bool,
    /// Keevoro starts, quietly, when the computer starts.
    pub start_with_computer: bool,
    /// A backup by itself tells the person when it finishes with something
    /// worth saying.
    pub notify: bool,
    /// Whether the icon by the clock is shown: `shown`, `mayNotShow`
    /// (Linux, where GNOME shows none) or `none`.
    pub tray: &'static str,
    /// Where the icon is, in a sentence.
    pub where_the_icon_is: &'static str,
    /// What to say beside the choices where the icon may not show.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub said: Option<&'static str>,
}

/// A change to those choices; each part given replaces what was there.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeepRunningChange {
    #[serde(default)]
    pub keep_running: Option<bool>,
    #[serde(default)]
    pub minimise_to_tray: Option<bool>,
    #[serde(default)]
    pub start_with_computer: Option<bool>,
    #[serde(default)]
    pub notify: Option<bool>,
}

fn view(shared: &Shared) -> KeepRunningView {
    let tray = shared.desk.tray();
    let settings = shared.keevoro.window_settings();
    KeepRunningView {
        keep_running: closing::keeps_running(settings.keep_running, tray),
        minimise_to_tray: settings.minimise_to_tray && tray.may_hide(),
        start_with_computer: shared.desk.startup.is_on().unwrap_or(false),
        notify: settings.notifies(),
        tray: match tray {
            Tray::Shown => "shown",
            Tray::MayNotShow => "mayNotShow",
            Tray::None => "none",
        },
        where_the_icon_is: closing::where_the_icon_is(std::env::consts::OS),
        said: match tray {
            Tray::Shown => None,
            Tray::MayNotShow => Some(closing::MAY_NOT_SHOW),
            Tray::None => Some(closing::NO_TRAY),
        },
    }
}

/// Puts Keevoro on the computer's list of programs it starts with, or
/// takes it off, and says so in the log.
fn start_with_computer(shared: &Shared, on: bool) -> Said<()> {
    shared
        .desk
        .startup
        .set(on)
        .map_err(|why| crate::startup::could_not(on, &why))?;
    shared.keevoro.log().say(if on {
        "Keevoro will start, by the clock, when this computer starts"
    } else {
        "Keevoro will no longer start when this computer starts"
    });
    Ok(())
}

/// How closing, minimising and starting with the computer are set.
#[tauri::command]
pub fn keep_running(shared: State<'_, Shared>) -> KeepRunningView {
    view(&shared)
}

/// Changes how closing, minimising and starting with the computer are set.
#[tauri::command]
pub fn set_keep_running(shared: State<'_, Shared>, change: KeepRunningChange) -> Said<KeepRunningView> {
    if change.keep_running.is_some() || change.minimise_to_tray.is_some() || change.notify.is_some() {
        shared
            .keevoro
            .change_window_settings(|w| {
                if let Some(on) = change.keep_running {
                    w.keep_running = Some(on);
                }
                if let Some(on) = change.minimise_to_tray {
                    w.minimise_to_tray = on;
                }
                if let Some(on) = change.notify {
                    w.notify = Some(on);
                }
            })
            .map_err(|e| e.message)?;
    }
    if let Some(on) = change.start_with_computer {
        start_with_computer(&shared, on)?;
    }
    Ok(view(&shared))
}

/// What waits for the window as it opens or comes forward, if anything.
#[tauri::command]
pub fn window_ask(shared: State<'_, Shared>) -> Option<Ask> {
    shared.desk.take_ask()
}

/// The first-close explanation was read: it is not said again, Keevoro
/// starts with the computer if that was ticked, and the window is hidden.
#[tauri::command]
pub fn hide_to_tray<R: Runtime>(window: tauri::Window<R>, start_with_computer: bool) -> Said<()> {
    let app = window.app_handle();
    let shared = app.state::<Shared>();
    shared
        .keevoro
        .change_window_settings(|w| w.explained_close = true)
        .map_err(|e| e.message)?;
    if start_with_computer {
        self::start_with_computer(&shared, true)?;
    }
    // Closing again now hides it, as it will from now on.
    window.close().map_err(|e| e.to_string())
}

/// What quitting answered: nothing when it quits, or the question to ask
/// first when work is in hand.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuitAnswer {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asked: Option<&'static str>,
}

/// Quits Keevoro. With work in hand it answers the question to ask first,
/// unless `stop` says to stop it after the file in hand (nothing is lost).
#[tauri::command]
pub async fn quit_keevoro<R: Runtime>(app: AppHandle<R>, stop: bool) -> Said<QuitAnswer> {
    tauri::async_runtime::spawn_blocking(move || window::quit(&app, stop))
        .await
        .map_err(|e| e.to_string())?
        .map(|asked| QuitAnswer { asked })
}
