//! The Keevoro desktop app.
//!
//! This crate is deliberately thin. It opens a window, and it answers the
//! handful of requests the screens make (the `commands` below) by calling
//! `keevoro-api`. It contains no rules about photos: if you find yourself
//! writing one here, it belongs in the API or the engine, where the command
//! line gets it too and where it can be tested without a window. Some
//! commands still call the engine directly or hold logic of their own;
//! `parity.rs` lists them, and that list only gets shorter.
//!
//! How a request travels:
//!
//! ```text
//! ui/src/App.tsx  ->  ui/src/backend.ts  ->  a #[tauri::command] here  ->  keevoro-api  ->  keevoro-engine
//! ```
//!
//! Long jobs (looking through a drive, surveying, backing up, checking) are
//! run by the API on their own threads. What they say is handed to the
//! window as events, so the window never freezes. The event shapes are in
//! `keevoro-api` (`jobs::said`) and mirrored in `ui/src/backend.ts`;
//! `events.rs` sends them. Gathering, from the older library model, still
//! runs here.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod commands;
mod events;
#[cfg(test)]
mod parity;
mod updates;
#[cfg(test)]
mod wiring;

use std::sync::atomic::AtomicBool;
use tauri::{Manager, Runtime};

/// What the running app holds in memory.
pub struct Shared {
    /// Keevoro itself: its backups, settings, log, keychain and jobs,
    /// through the API the command line uses too.
    keevoro: keevoro_api::Keevoro,
    /// True while photos are being gathered.
    gathering: AtomicBool,
}

pub fn run() {
    let built = app(tauri::Builder::default(), |app| {
        Ok(keevoro_api::Keevoro::open(&keevoro_api::Folders {
            config: app.path().app_config_dir()?,
            data: app.path().app_data_dir()?,
            // Previews of photos not yet backed up. Safe to delete.
            cache: app.path().app_cache_dir()?.join("previews"),
            logs: app.path().app_log_dir()?,
        }))
    })
    .run(context());
    if let Err(e) = built {
        eprintln!("Keevoro couldn't start: {e}");
        std::process::exit(1);
    }
}

/// The app's own settings, icons and screens, as `tauri.conf.json` and
/// `capabilities/` describe them, for whichever runtime it runs on.
fn context<R: Runtime>() -> tauri::Context<R> {
    tauri::generate_context!()
}

/// The whole app, ready to run: its plugins, its commands, and the
/// listener that hands what jobs say to the window. `open` makes Keevoro
/// once the app is starting: in the computer's usual folders for an app in
/// `run`, in a temporary folder in a test. Built on any Tauri runtime, so
/// the tests drive exactly what a person's window drives, through Tauri's
/// stand-in for a window (`tauri::test`, in `wiring.rs`).
fn app<R: Runtime>(
    builder: tauri::Builder<R>,
    open: impl FnOnce(&tauri::App<R>) -> Result<keevoro_api::Keevoro, Box<dyn std::error::Error>> + Send + 'static,
) -> tauri::Builder<R> {
    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            let keevoro = open(app)?;
            keevoro_api::activity::watch();
            keevoro.log().say(&format!(
                "Keevoro {} started on {} ({})",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS,
                std::env::consts::ARCH
            ));
            // Everything a job says goes to the window, as the event the
            // screens listen for.
            let window = app.handle().clone();
            keevoro.listen(move |event| events::to_window(&window, &event.said));
            app.manage(Shared {
                keevoro,
                gathering: AtomicBool::new(false),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_state,
            commands::use_library,
            commands::start_backup,
            commands::switch_backup,
            commands::open_destination,
            commands::room,
            commands::previews_wanted,
            commands::set_previews_wanted,
            commands::activity,
            commands::duplicates,
            commands::acceleration,
            commands::set_acceleration,
            commands::send_progress,
            commands::send_history,
            commands::last_backup_said,
            commands::pause_work,
            commands::reset_everything,
            commands::stop_sending,
            commands::open_place,
            commands::discover,
            commands::scan_place,
            commands::stop_scanning,
            commands::survey_place,
            commands::stop_survey,
            commands::places,
            commands::open_library,
            commands::check_update,
            commands::keep_checking,
            commands::log_tail,
            commands::open_log,
            commands::open_release_page,
            commands::install_update,
            commands::keep_collection,
            commands::collection_photos,
            commands::set_aside,
            commands::forget_source,
            commands::file_types,
            commands::include_file_type,
            commands::keep_photo,
            commands::preview,
            commands::closer_look,
            commands::gather,
            commands::destination,
            commands::connect_destination,
            commands::disconnect_destination,
            commands::send,
            commands::check_arrived,
        ])
}
