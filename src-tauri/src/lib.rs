//! The Keevoro desktop app.
//!
//! This crate is deliberately thin. It opens a window, and it answers the
//! handful of requests the screens make (the `commands` below) by calling
//! `keevoro-api`. It contains no rules about photos: if you find yourself
//! writing one here, it belongs in the API or the engine, where the command
//! line gets it too and where it can be tested without a window.
//! `parity.rs` checks that every command is backed by the API or is only
//! about being a window.
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
//! `events.rs` sends them.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod closing;
mod commands;
mod events;
mod keep_running;
mod notices;
#[cfg(test)]
mod parity;
mod placing;
mod relaunch;
mod startup;
mod tray;
mod updates;
mod window;
#[cfg(test)]
mod wiring;

use std::sync::Arc;
use tauri::{Emitter, Manager, Runtime};

/// The window event that says a drive arrived or left, where the system
/// says so; it carries nothing, and the first screen lists the drives again.
pub const DRIVES: &str = "drives";

/// How often, in seconds, Keevoro looks whether a source asked to be
/// backed up when it is plugged in has been.
const ARRIVALS_LOOK_EVERY: u64 = 3;

/// What the running app holds in memory.
pub struct Shared {
    /// Keevoro itself: its backups, settings, log, keychain and jobs,
    /// through the API the command line uses too.
    keevoro: keevoro_api::Keevoro,
    /// Its window coming and going, and its icon by the clock.
    desk: window::Desk,
}

/// Where the app runs: on a person's desktop, or on Tauri's stand-in for a
/// window in a test, which has no icon by the clock and no list of
/// programs the computer starts with, so the test is given its own.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum Desktop {
    Real {
        /// False when started with the computer: it comes up with no window.
        with_window: bool,
    },
    StandIn {
        tray: closing::Tray,
    },
}

pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    let builder = tauri::Builder::default()
        // First, so that Keevoro started again (from the Start menu, say,
        // while it waits by the clock) brings this one's window forward and
        // goes. The command line and the agent server are other programs
        // and never ask.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            window::show(app, None)
        }))
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name("Keevoro")
                .arg(closing::IN_BACKGROUND)
                .build(),
        )
        .plugin(tauri_plugin_notification::init());
    let desktop = Desktop::Real {
        with_window: closing::starts_with_its_window(&args),
    };
    let built = app(builder, desktop, |_| {
        // The same folders the command line uses, so both see the same
        // backups.
        let folders =
            keevoro_api::Folders::for_this_computer().ok_or("this computer does not say where an app's folders go")?;
        Ok(keevoro_api::Keevoro::open(&folders))
    })
    .build(context());
    match built {
        Ok(app) => app.run(window::on_run_event),
        Err(e) => {
            eprintln!("Keevoro couldn't start: {e}");
            std::process::exit(1);
        }
    }
}

/// Tells the person how a backup by itself went, if it was one and has
/// something worth saying (`notices.rs`).
fn told_after<R: Runtime>(app: &tauri::AppHandle<R>, job: keevoro_api::JobId, last: &keevoro_api::jobs::SendEvent) {
    let Some(shared) = app.try_state::<Shared>() else {
        return;
    };
    let handle = shared
        .keevoro
        .jobs(None)
        .into_iter()
        .find(|j| j.handle.id == job)
        .or_else(|| shared.keevoro.ended_job(job))
        .map(|j| j.handle);
    let Some(handle) = handle.filter(|h| h.by_itself.is_some()) else {
        return;
    };
    let Some(backup) = handle.backup else { return };
    if let Some(notice) = notices::notice(&backup.name, last) {
        notices::tell(app, &backup.id, &notice);
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
    desktop: Desktop,
    open: impl FnOnce(&tauri::App<R>) -> Result<keevoro_api::Keevoro, Box<dyn std::error::Error>> + Send + 'static,
) -> tauri::Builder<R> {
    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .on_window_event(window::on_window_event)
        .setup(move |app| {
            let keevoro = open(app)?;
            keevoro.log().say(&format!(
                "Keevoro {} started on {} ({}){}",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS,
                std::env::consts::ARCH,
                match desktop {
                    Desktop::Real { with_window: false } => ", by the clock, with the computer",
                    _ => "",
                }
            ));
            // Everything a job says goes to the window, as the event the
            // screens listen for, and may change what the icon by the
            // clock says.
            let window = app.handle().clone();
            keevoro.listen(move |event| {
                events::to_window(&window, &event.said);
                tray::nudge(&window);
                // A backup by itself that ended may have something to say.
                if let keevoro_api::Said::Send(last) = &event.said {
                    if last.finished {
                        let (app, job, last) = (window.clone(), event.job, last.clone());
                        std::thread::spawn(move || told_after(&app, job, &last));
                    }
                }
            });
            // The icon by the clock, where the computer has a place for one.
            let handle = app.handle();
            let (startup, tray, with_window): (Arc<dyn startup::StartWithComputer>, _, _) = match desktop {
                Desktop::Real { with_window } => (
                    {
                        notices::listen(handle);
                        Arc::new(startup::ThroughThePlugin(handle.clone()))
                    },
                    tray::make(handle).map_or(closing::Tray::None, |()| closing::Tray::made_on(std::env::consts::OS)),
                    with_window,
                ),
                Desktop::StandIn { tray } => (Arc::new(startup::InMemory::default()), tray, true),
            };
            if tray == closing::Tray::None {
                keevoro
                    .log()
                    .say("This computer has no place for an icon by the clock, so closing Keevoro quits it");
            }
            let seen_at = keevoro.window_settings().seen_at;
            app.manage(Shared {
                keevoro,
                desk: window::Desk::new(startup, tray, seen_at),
            });
            tray::keep_up_to_date(handle)?;
            // Measuring Keevoro's own work, for "What Keevoro is doing",
            // rests while no window can show it.
            keevoro_api::activity::watch();
            if with_window {
                window::open(handle)?;
                window::show(handle, None);
            } else {
                keevoro_api::activity::rest(true);
                #[cfg(target_os = "macos")]
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }
            // Asked for afresh: on a Mac the line above changes the app,
            // which the handle taken earlier may not be held across.
            tray::nudge(app.handle());
            // Where the system says when a drive arrives or leaves
            // (Windows), the first screen hears it at once, as `drives`;
            // elsewhere it looks every couple of seconds while it shows.
            let told = app.handle().clone();
            keevoro_api::places::watch_for_drives(move || {
                let _ = told.emit(DRIVES, ());
            });
            // Backups that back up by themselves run while Keevoro is open:
            // a look every minute at what is in memory, never in the first
            // five minutes after opening or waking (see `schedule::Ticker`).
            let scheduler = app.handle().clone();
            std::thread::Builder::new()
                .name("keevoro schedule".into())
                .spawn(move || {
                    let keevoro = &scheduler.state::<Shared>().keevoro;
                    let mut ticker = keevoro_api::schedule::Ticker::started_at(keevoro.now());
                    loop {
                        std::thread::sleep(std::time::Duration::from_secs(60));
                        // A pause for a while from the icon ends by itself.
                        keevoro.carry_on_if_the_pause_is_over();
                        if ticker.may_run(keevoro.now()) {
                            keevoro.run_schedules_due();
                        }
                        // "Tonight" becomes "tomorrow" as the clock moves.
                        tray::nudge(&scheduler);
                        // On a Mac, a new version dragged over this one.
                        #[cfg(target_os = "macos")]
                        relaunch::if_updated(&scheduler);
                    }
                })?;
            // Sources the person asked to be backed up whenever they are
            // plugged in: a look every few seconds at whether each one's
            // place is there, which costs next to nothing, and a notice for
            // each backup it starts (see `keevoro_api::arrival`).
            let watcher = app.handle().clone();
            std::thread::Builder::new()
                .name("keevoro arrivals".into())
                .spawn(move || {
                    let keevoro = &watcher.state::<Shared>().keevoro;
                    loop {
                        std::thread::sleep(std::time::Duration::from_secs(ARRIVALS_LOOK_EVERY));
                        for arrived in keevoro.notice_arrivals() {
                            notices::tell(
                                &watcher,
                                &arrived.backup.id,
                                &notices::plugged_in(&arrived.source, &arrived.backup.name),
                            );
                            tray::nudge(&watcher);
                        }
                    }
                })?;
            // Backups removed over a week ago are cleared away for good,
            // quietly, off the thread that opens the window.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                handle.state::<Shared>().keevoro.clear_away_old_removed();
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_state,
            commands::start_backup,
            commands::switch_backup,
            commands::backup_details,
            commands::rename_backup,
            commands::find_backup_at,
            commands::open_backup_from,
            commands::connect_source_again,
            commands::remove_backup,
            commands::removed_backups,
            commands::put_back_backup,
            commands::clear_away_backup,
            commands::open_destination,
            commands::room,
            commands::previews_wanted,
            commands::set_previews_wanted,
            commands::activity,
            commands::duplicates,
            commands::acceleration,
            commands::set_acceleration,
            commands::agent_access,
            commands::set_agent_access,
            commands::backup_wants,
            commands::set_backup_wants,
            commands::backup_arrange,
            commands::set_backup_arrange,
            commands::source_arrange,
            commands::set_source_arrange,
            commands::source_wants,
            commands::set_source_wants,
            commands::file_kinds,
            commands::add_file_kind,
            commands::change_file_kind,
            commands::remove_file_kind,
            commands::reset_file_kinds,
            commands::send_progress,
            commands::send_history,
            commands::last_backup_said,
            commands::pause_work,
            commands::reset_everything,
            commands::stop_sending,
            commands::open_place,
            commands::forget_maps,
            commands::tidy_folder,
            commands::discover,
            commands::scan_place,
            commands::look_again,
            commands::schedule,
            commands::set_schedule,
            commands::skip_next_run,
            commands::run_scheduled_now,
            commands::window_used,
            commands::away,
            commands::away_seen,
            commands::source_on_arrival,
            commands::background_settings,
            commands::stop_scanning,
            commands::survey_place,
            commands::stop_survey,
            commands::places,
            commands::places_if_changed,
            commands::check_update,
            commands::keep_checking,
            commands::pause_checking,
            commands::stop_checking,
            commands::check_duplicates_now,
            commands::log_tail,
            commands::open_log,
            commands::prepare_report,
            commands::send_report,
            commands::reports,
            commands::refresh_reports,
            commands::note_report,
            commands::reports_asking,
            commands::reports_heard,
            commands::reporting_code,
            commands::open_release_page,
            commands::open_web_page,
            commands::compare_storage,
            commands::install_update,
            commands::keep_collection,
            commands::collection_photos,
            commands::set_aside,
            commands::forget_source,
            commands::file_types,
            commands::include_file_type,
            commands::leave_out_folder,
            commands::keep_photo,
            commands::preview,
            commands::closer_look,
            commands::folders_inside,
            commands::folder_files,
            commands::file_picture,
            commands::space_map,
            commands::destination,
            commands::connect_destination,
            commands::add_bucket_source,
            commands::disconnect_destination,
            commands::send,
            commands::check_arrived,
            commands::put_right,
            commands::put_right_plan,
            keep_running::keep_running,
            keep_running::set_keep_running,
            keep_running::window_ask,
            keep_running::hide_to_tray,
            keep_running::quit_keevoro,
        ])
}
