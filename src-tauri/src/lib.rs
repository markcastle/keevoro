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

mod activity;
mod commands;
mod events;
mod log;
mod places;
mod reset;
mod settings;
mod updates;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use tauri::Manager;

/// What the running app holds in memory.
pub struct Shared {
    settings: Mutex<settings::Settings>,
    /// The record of what the app did, for when someone asks.
    log: log::Log,
    /// The app's own settings folder.
    config_dir: PathBuf,
    /// Where previews of not-yet-gathered photos are kept. Safe to delete.
    cache_dir: PathBuf,
    /// True while a drive is being looked through; one at a time.
    scanning: AtomicBool,
    /// True while photos are being gathered.
    gathering: AtomicBool,
    /// True while files a quick search only sampled are being read in the
    /// background.
    checking: AtomicBool,
    /// Set to stop a search that is running.
    stop_scanning: std::sync::Arc<AtomicBool>,
    /// How many bytes the send in hand set out to send, and what the
    /// running total stood at when it began.
    send_total: std::sync::atomic::AtomicU64,
    send_began_at: std::sync::atomic::AtomicU64,
    /// Set to stop a send after the file in hand.
    stop_sending: keevoro_engine::send::Stop,
    /// True while a backup or a check of one is running.
    sending: AtomicBool,
}

pub fn run() {
    let built = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            let cache_dir = app.path().app_cache_dir()?.join("previews");
            activity::watch(&commands::HISTORY);
            let log = log::Log::open(&app.path().app_log_dir()?);
            log.say(&format!(
                "Keevoro {} started on {} ({})",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS,
                std::env::consts::ARCH
            ));
            app.manage(Shared {
                log,
                settings: Mutex::new(settings::Settings::load(&config_dir)),
                config_dir,
                cache_dir,
                scanning: AtomicBool::new(false),
                gathering: AtomicBool::new(false),
                checking: AtomicBool::new(false),
                stop_scanning: std::sync::Arc::new(AtomicBool::new(false)),
                send_total: std::sync::atomic::AtomicU64::new(0),
                send_began_at: std::sync::atomic::AtomicU64::new(0),
                stop_sending: keevoro_engine::send::Stop::default(),
                sending: AtomicBool::new(false),
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
            commands::activity,
            commands::send_progress,
            commands::pause_work,
            commands::reset_everything,
            commands::stop_sending,
            commands::open_place,
            commands::discover,
            commands::scan_place,
            commands::stop_scanning,
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
            commands::gather,
            commands::destination,
            commands::connect_destination,
            commands::disconnect_destination,
            commands::send,
            commands::check_arrived,
        ])
        .run(tauri::generate_context!());
    if let Err(e) = built {
        eprintln!("Keevoro couldn't start: {e}");
        std::process::exit(1);
    }
}
