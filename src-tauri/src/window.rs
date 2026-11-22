//! The window coming and going while Keevoro keeps running by the clock.
//!
//! Hiding the window destroys it, webview and all, rather than only
//! hiding it: on Linux that gave back about two thirds of what Keevoro held
//! (see "Running with the window closed" in `docs/developing.md`), and the
//! page's own timers stop with it. Bringing it back makes it again from
//! `tauri.conf.json`, which takes about as long as opening Keevoro does.
//! While no window is open, Tauri would end the program; [`on_run_event`]
//! keeps it running when the window was hidden on purpose.
//!
//! What each event does is decided in `closing.rs`, pure; this only does
//! it. What the window must be told when it opens or comes forward (a
//! page to show, a question before quitting, the first-close explanation,
//! something the icon's menu could not do) waits in [`Ask`] until the page
//! takes it with the `window_ask` command, nudged by the `window_ask`
//! event: a window just made is not listening yet, so it asks as it starts.

use crate::closing::{self, Close, InHand, Quit, Tray};
use crate::startup::StartWithComputer;
use crate::Shared;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use tauri::{AppHandle, Emitter, Manager, RunEvent, Runtime, WebviewWindow, Window, WindowEvent};

/// The window's label in `tauri.conf.json` and the capabilities.
pub const WINDOW: &str = "main";

/// The event that tells the page something waits for it.
pub const ASKED: &str = "window_ask";

/// What the window must be told when it comes forward.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ask {
    /// The page to open: `settings`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
    /// Say first, once, that closing keeps Keevoro running, and where its
    /// icon is: these words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closing: Option<String>,
    /// Ask before quitting cuts work short: this question.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quitting: Option<String>,
    /// Something the icon's menu could not do, said.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Ask {
    pub fn note(said: impl Into<String>) -> Self {
        Self {
            note: Some(said.into()),
            ..Self::default()
        }
    }

    /// Adds what another asks, its parts winning.
    fn and(&mut self, other: Ask) {
        let Ask {
            page,
            closing,
            quitting,
            note,
        } = other;
        self.page = page.or(self.page.take());
        self.closing = closing.or(self.closing.take());
        self.quitting = quitting.or(self.quitting.take());
        self.note = note.or(self.note.take());
    }
}

/// What the app keeps about its window and its icon by the clock.
pub struct Desk {
    /// Whether this computer shows the icon (set once it is made).
    tray: Mutex<Tray>,
    /// True from deciding to hide the window until it is shown again.
    hiding: AtomicBool,
    /// True while the window says, the first time, that closing keeps
    /// Keevoro running.
    saying: AtomicBool,
    /// What waits for the window.
    asks: Mutex<Ask>,
    /// The computer's list of programs started when someone signs in.
    pub startup: Arc<dyn StartWithComputer>,
    /// When the person last had the window open, in seconds since 1970.
    seen_since: AtomicI64,
}

impl Desk {
    pub fn new(startup: Arc<dyn StartWithComputer>, tray: Tray, seen_since: Option<i64>) -> Self {
        Self {
            tray: Mutex::new(tray),
            hiding: AtomicBool::new(false),
            saying: AtomicBool::new(false),
            asks: Mutex::default(),
            startup,
            seen_since: AtomicI64::new(seen_since.unwrap_or(0)),
        }
    }

    pub fn tray(&self) -> Tray {
        *self.tray.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn seen_since(&self) -> i64 {
        self.seen_since.load(Ordering::SeqCst)
    }

    /// Takes what waits for the window, if anything does.
    pub fn take_ask(&self) -> Option<Ask> {
        let ask = std::mem::take(&mut *self.asks.lock().unwrap_or_else(PoisonError::into_inner));
        (ask != Ask::default()).then_some(ask)
    }

    fn add_ask(&self, ask: Ask) {
        self.asks.lock().unwrap_or_else(PoisonError::into_inner).and(ask);
    }

    pub fn is_hiding(&self) -> bool {
        self.hiding.load(Ordering::SeqCst)
    }
}

/// Whether closing the window keeps Keevoro running, as things are now.
pub fn keeps_running(shared: &Shared) -> bool {
    closing::keeps_running(shared.keevoro.window_settings().keep_running, shared.desk.tray())
}

/// What this program has in hand that quitting would cut short.
pub fn in_hand(shared: &Shared) -> Option<InHand> {
    InHand::of(
        shared
            .keevoro
            .jobs(None)
            .into_iter()
            .filter(|job| !job.elsewhere && !job.ended)
            .map(|job| job.handle.job),
    )
}

/// Notes that the person has the window now: what a backup by itself held
/// back before this has been seen.
fn seen<R: Runtime>(app: &AppHandle<R>) {
    let shared = app.state::<Shared>();
    let now = shared.keevoro.now().as_second();
    shared.desk.seen_since.store(now, Ordering::SeqCst);
    let _ = shared.keevoro.change_window_settings(|w| w.seen_at = Some(now));
}

/// Opens the window, as `tauri.conf.json` describes it.
pub fn open<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<WebviewWindow<R>> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == WINDOW)
        .cloned()
        .ok_or_else(|| tauri::Error::WindowNotFound)?;
    let window = tauri::WebviewWindowBuilder::from_config(app, &config)?.build()?;
    keevoro_api::activity::rest(false);
    Ok(window)
}

/// Brings the window back, or opens it again: restored if minimised, in
/// front, and on `page` if one is named.
pub fn show<R: Runtime>(app: &AppHandle<R>, page: Option<&str>) {
    let shared = app.state::<Shared>();
    shared.desk.hiding.store(false, Ordering::SeqCst);
    if let Some(page) = page {
        shared.desk.add_ask(Ask {
            page: Some(page.to_string()),
            ..Ask::default()
        });
    }
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    match app.get_webview_window(WINDOW) {
        Some(window) => {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
            let _ = app.emit_to(WINDOW, ASKED, ());
        }
        None => {
            // Made off the thread that called, which may be the one that
            // draws windows: making one there can stall on Windows.
            let app = app.clone();
            std::thread::spawn(move || match open(&app) {
                Ok(window) => {
                    let _ = window.set_focus();
                }
                Err(e) => app
                    .state::<Shared>()
                    .keevoro
                    .log()
                    .say(&format!("Couldn't open Keevoro's window again: {e}")),
            });
        }
    }
    seen(app);
    crate::tray::nudge(app);
}

/// Brings the window forward to tell it something.
pub fn ask<R: Runtime>(app: &AppHandle<R>, ask: Ask) {
    app.state::<Shared>().desk.add_ask(ask);
    show(app, None);
}

/// Hides the window to the icon by the clock: it is closed, webview and
/// all, and Keevoro keeps running.
pub fn hide<R: Runtime>(window: &Window<R>) {
    window
        .app_handle()
        .state::<Shared>()
        .desk
        .hiding
        .store(true, Ordering::SeqCst);
    let _ = window.destroy();
}

/// What a window event does.
pub fn on_window_event<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    if window.label() != WINDOW {
        return;
    }
    let app = window.app_handle();
    let shared = app.state::<Shared>();
    match event {
        WindowEvent::CloseRequested { api, .. } => {
            let said = shared.keevoro.window_settings().explained_close;
            let saying = shared.desk.saying.load(Ordering::SeqCst);
            match closing::on_close(keeps_running(&shared), said, saying, in_hand(&shared)) {
                // The close goes ahead; Keevoro stays (see `on_run_event`).
                Close::Hide => {
                    if saying {
                        let _ = shared.keevoro.change_window_settings(|w| w.explained_close = true);
                    }
                    shared.desk.hiding.store(true, Ordering::SeqCst);
                }
                Close::Quit => {}
                Close::SayFirst => {
                    api.prevent_close();
                    shared.desk.saying.store(true, Ordering::SeqCst);
                    let words = closing::where_the_icon_is(std::env::consts::OS).to_string();
                    ask(
                        app,
                        Ask {
                            closing: Some(words),
                            ..Ask::default()
                        },
                    );
                }
                Close::AskFirst(question) => {
                    api.prevent_close();
                    ask(
                        app,
                        Ask {
                            quitting: Some(question.to_string()),
                            ..Ask::default()
                        },
                    );
                }
            }
        }
        // Minimised, and the person asked for that to hide it too.
        WindowEvent::Resized(_) => {
            let to_tray = shared.keevoro.window_settings().minimise_to_tray;
            if closing::hides_on_minimise(to_tray, shared.desk.tray()) && window.is_minimized().unwrap_or(false) {
                hide(window);
            }
        }
        WindowEvent::Destroyed if shared.desk.is_hiding() => {
            shared.desk.saying.store(false, Ordering::SeqCst);
            // Nobody can see what Keevoro measures of itself now.
            keevoro_api::activity::rest(true);
            seen(app);
            #[cfg(target_os = "macos")]
            let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            crate::tray::nudge(app);
        }
        _ => {}
    }
}

/// What the app does as it runs: stays when the window was hidden on
/// purpose, and on a Mac, opens the window when the Dock icon is clicked.
pub fn on_run_event<R: Runtime>(app: &AppHandle<R>, event: RunEvent) {
    match event {
        RunEvent::ExitRequested { code: None, api, .. } if app.state::<Shared>().desk.is_hiding() => {
            api.prevent_exit();
        }
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => show(app, None),
        _ => {}
    }
}

/// Quits Keevoro, or, with work in hand that `stop` does not say to stop,
/// answers the question to ask first. With `stop`, the work is stopped
/// after the file in hand first (nothing is lost), waiting up to a minute.
pub fn quit<R: Runtime>(app: &AppHandle<R>, stop: bool) -> Result<Option<&'static str>, String> {
    let shared = app.state::<Shared>();
    match closing::on_quit(in_hand(&shared), stop) {
        Quit::AskFirst(question) => return Ok(Some(question)),
        Quit::StopFirst => {
            if !shared.keevoro.stop_here_and_wait(std::time::Duration::from_secs(60)) {
                return Err(
                    "Keevoro couldn't stop the work in hand in time, so it hasn't quit. Try again in a moment.".into(),
                );
            }
        }
        Quit::Now => {}
    }
    shared.desk.hiding.store(false, Ordering::SeqCst);
    shared.keevoro.log().say("Quitting, as asked");
    app.exit(0);
    Ok(None)
}

/// Quit Keevoro, from the icon's menu: at once with nothing in hand, and
/// otherwise asked in the window, brought forward.
pub fn quit_from_the_icon<R: Runtime>(app: &AppHandle<R>) {
    match quit(app, false) {
        Ok(None) => {}
        Ok(Some(question)) => ask(
            app,
            Ask {
                quitting: Some(question.to_string()),
                ..Ask::default()
            },
        ),
        Err(said) => ask(app, Ask::note(said)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_waits_for_the_window_adds_up_and_is_taken_once() {
        let desk = Desk::new(Arc::new(crate::startup::InMemory::default()), Tray::Shown, None);
        assert_eq!(desk.take_ask(), None);
        desk.add_ask(Ask {
            page: Some("settings".into()),
            ..Ask::default()
        });
        desk.add_ask(Ask::note("That backup is no longer there."));
        let taken = desk.take_ask().unwrap();
        assert_eq!(taken.page.as_deref(), Some("settings"));
        assert_eq!(taken.note.as_deref(), Some("That backup is no longer there."));
        assert_eq!(desk.take_ask(), None, "taken once");
        let shape = serde_json::to_value(Ask::note("x")).unwrap();
        assert_eq!(shape, serde_json::json!({ "note": "x" }));
    }
}
