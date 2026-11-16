//! The Keevoro desktop app.
//!
//! This crate is deliberately thin. It opens a window, and it answers the
//! handful of requests the screens make (the `commands` below) by calling
//! `keevoro-engine`. It contains no rules about photos: if you find
//! yourself writing one here, it belongs in the engine, where the command
//! line gets it too and where it can be tested without a window.
//!
//! How a request travels:
//!
//! ```text
//! ui/src/App.tsx  ->  ui/src/backend.ts  ->  a #[tauri::command] here  ->  keevoro-engine
//! ```
//!
//! Long jobs (looking through a drive, gathering) run on their own thread
//! and report back with events, so the window never freezes. The event
//! shapes are in `events.rs` and mirrored in `ui/src/backend.ts`.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod commands;
mod events;
mod settings;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use tauri::Manager;

/// What the running app holds in memory.
pub struct Shared {
    settings: Mutex<settings::Settings>,
    /// The app's own settings folder.
    config_dir: PathBuf,
    /// Where previews of not-yet-gathered photos are kept. Safe to delete.
    cache_dir: PathBuf,
    /// True while a drive is being looked through; one at a time.
    scanning: AtomicBool,
    /// True while photos are being gathered.
    gathering: AtomicBool,
    /// True while a backup or a check of one is running.
    backing_up: AtomicBool,
}

pub fn run() {
    let built = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            let cache_dir = app.path().app_cache_dir()?.join("previews");
            app.manage(Shared {
                settings: Mutex::new(settings::Settings::load(&config_dir)),
                config_dir,
                cache_dir,
                scanning: AtomicBool::new(false),
                gathering: AtomicBool::new(false),
                backing_up: AtomicBool::new(false),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_state,
            commands::use_library,
            commands::discover,
            commands::scan_place,
            commands::keep_collection,
            commands::collection_photos,
            commands::set_aside,
            commands::keep_photo,
            commands::preview,
            commands::gather,
            commands::backup_state,
            commands::connect_backup,
            commands::disconnect_backup,
            commands::back_up,
            commands::check_backup,
        ])
        .run(tauri::generate_context!());
    if let Err(e) = built {
        eprintln!("Keevoro couldn't start: {e}");
        std::process::exit(1);
    }
}
