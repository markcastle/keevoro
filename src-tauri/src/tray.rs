//! Keevoro's icon by the clock and its menu: the notification area on
//! Windows, the menu bar on a Mac, the system tray on Linux where there is
//! one.
//!
//! What the menu holds is decided by one pure function, [`menu`], from
//! Keevoro at a glance (`keevoro_api::glance`), so it is tested without a
//! window; the words are the API's, the same the window and the command
//! line use. The rest is glue: making the icon, putting a new menu,
//! tooltip and icon on it when the glance changes, and doing what a click
//! asks.
//!
//! It keeps itself up to date without looking for itself: whatever changes
//! the glance nudges it ([`nudge`]), a job saying anything, a pause, the
//! window opening, and the scheduler's look once a minute. It then looks
//! at most once every two seconds, and while something is going, once
//! every five seconds besides, so it sees a job end. With nothing going it
//! sleeps until nudged.
//!
//! How a click works follows each system: on Windows a click on the icon
//! brings the window back and a right click opens the menu; on a Mac any
//! click opens the menu, as menu-bar items do, and the menu has Open
//! Keevoro; on Linux the tray only ever opens the menu.

use crate::Shared;
use keevoro_api::glance::{Glance, Mood, PauseFor};
use keevoro_api::{JobId, Which};
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::Duration;
use tauri::image::Image;
use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime};

/// What a click on the menu does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    Open,
    Pause(PauseFor),
    CarryOn,
    BackUp(String),
    Stop(JobId),
    Settings,
    Quit,
}

impl Act {
    /// The menu item's id, which a click gives back.
    pub fn id(&self) -> String {
        match self {
            Self::Open => "open".into(),
            Self::Pause(PauseFor::AnHour) => "pause:hour".into(),
            Self::Pause(PauseFor::UntilTomorrow) => "pause:tomorrow".into(),
            Self::Pause(PauseFor::UntilCarryOn) => "pause:on".into(),
            Self::CarryOn => "carry-on".into(),
            Self::BackUp(backup) => format!("back-up:{backup}"),
            Self::Stop(job) => format!("stop:{}", job.0),
            Self::Settings => "settings".into(),
            Self::Quit => "quit".into(),
        }
    }

    /// The act a menu item's id stands for.
    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "open" => Self::Open,
            "pause:hour" => Self::Pause(PauseFor::AnHour),
            "pause:tomorrow" => Self::Pause(PauseFor::UntilTomorrow),
            "pause:on" => Self::Pause(PauseFor::UntilCarryOn),
            "carry-on" => Self::CarryOn,
            "settings" => Self::Settings,
            "quit" => Self::Quit,
            _ => {
                if let Some(backup) = id.strip_prefix("back-up:") {
                    Self::BackUp(backup.to_string())
                } else {
                    Self::Stop(JobId(id.strip_prefix("stop:")?.parse().ok()?))
                }
            }
        })
    }
}

/// One line of the menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// Words that cannot be clicked: what is happening.
    Said(String),
    Line,
    /// Words that do something, and whether they can be clicked now.
    Do(Act, String, bool),
    /// Words that open more.
    More(String, Vec<Item>),
}

/// The words of the menu, shared with the window where it says the same.
pub const OPEN: &str = "Open Keevoro";
pub const PAUSE: &str = "Pause";
pub const CARRY_ON: &str = "Carry on";
pub const BACK_UP_NOW: &str = "Back up now";
pub const STOP: &str = "Stop this backup";
pub const SETTINGS: &str = "Settings…";
pub const QUIT: &str = "Quit Keevoro";

/// The menu, from Keevoro at a glance. Pure.
pub fn menu(glance: &Glance) -> Vec<Item> {
    let mut items = vec![
        Item::Said(glance.line.clone()),
        Item::Line,
        Item::Do(Act::Open, OPEN.into(), true),
    ];
    if glance.paused {
        items.push(Item::Do(Act::CarryOn, CARRY_ON.into(), true));
    } else if glance.pausable {
        let pauses = [PauseFor::AnHour, PauseFor::UntilTomorrow, PauseFor::UntilCarryOn]
            .into_iter()
            .map(|p| Item::Do(Act::Pause(p), p.words().into(), true))
            .collect();
        items.push(Item::More(PAUSE.into(), pauses));
    }
    match glance.backups.as_slice() {
        [] => {}
        [only] => items.push(Item::Do(Act::BackUp(only.id.clone()), BACK_UP_NOW.into(), !only.busy)),
        several => items.push(Item::More(
            BACK_UP_NOW.into(),
            several
                .iter()
                .map(|b| Item::Do(Act::BackUp(b.id.clone()), b.name.clone(), !b.busy))
                .collect(),
        )),
    }
    if let Some((job, _)) = &glance.stoppable {
        items.push(Item::Do(Act::Stop(*job), STOP.into(), true));
    }
    items.extend([
        Item::Line,
        Item::Do(Act::Settings, SETTINGS.into(), true),
        Item::Do(Act::Quit, QUIT.into(), true),
    ]);
    items
}

/// What hovering over the icon says: the same first line.
pub fn tooltip(glance: &Glance) -> String {
    format!("Keevoro\n{}", glance.line)
}

/// The icon for each mood, in colour, or for a Mac's menu bar in black and
/// clear (made by `cargo xtask tray-icons`).
fn icon_bytes(mood: Mood) -> &'static [u8] {
    let mac = cfg!(target_os = "macos");
    match (mood, mac) {
        (Mood::Calm, false) => include_bytes!("../icons/tray/calm.png"),
        (Mood::Working, false) => include_bytes!("../icons/tray/working.png"),
        (Mood::Paused, false) => include_bytes!("../icons/tray/paused.png"),
        (Mood::NeedsYou, false) => include_bytes!("../icons/tray/needs-you.png"),
        (Mood::Calm, true) => include_bytes!("../icons/tray/calm-mac.png"),
        (Mood::Working, true) => include_bytes!("../icons/tray/working-mac.png"),
        (Mood::Paused, true) => include_bytes!("../icons/tray/paused-mac.png"),
        (Mood::NeedsYou, true) => include_bytes!("../icons/tray/needs-you-mac.png"),
    }
}

/// The icon by the clock, once made, and what it last showed.
pub struct Icon<R: Runtime> {
    tray: TrayIcon<R>,
    shown: Mutex<Option<(Mood, String, Vec<Item>)>>,
}

/// Makes the icon by the clock, or `None` where it cannot be made (Linux
/// without its tray library, which the library says by panicking, so that
/// is caught here: Keevoro then behaves as it always did).
pub fn make<R: Runtime>(app: &AppHandle<R>) -> Option<()> {
    let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        TrayIconBuilder::with_id("keevoro")
            .icon(Image::from_bytes(icon_bytes(Mood::Calm)).ok()?)
            .icon_as_template(cfg!(target_os = "macos"))
            .tooltip("Keevoro")
            .show_menu_on_left_click(cfg!(target_os = "macos"))
            .on_menu_event(|app, event| {
                if let Some(act) = Act::from_id(event.id().as_ref()) {
                    act_on(app, act);
                }
            })
            .on_tray_icon_event(|tray, event| {
                // Windows: a click brings the window back. A double click
                // (the habit of older programs) does the same.
                let open = match event {
                    TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    }
                    | TrayIconEvent::DoubleClick {
                        button: MouseButton::Left,
                        ..
                    } => !cfg!(target_os = "macos"),
                    _ => false,
                };
                if open {
                    crate::window::show(tray.app_handle(), None);
                }
            })
            .build(app)
            .ok()
    }));
    let tray = built.ok().flatten()?;
    app.manage(Icon {
        tray,
        shown: Mutex::new(None),
    });
    Some(())
}

/// Puts what Keevoro is like now on the icon, if it has changed.
fn show_glance<R: Runtime>(app: &AppHandle<R>, glance: &Glance) {
    let Some(icon) = app.try_state::<Icon<R>>() else { return };
    let items = menu(glance);
    let now = (glance.mood, glance.line.clone(), items.clone());
    let mut shown = icon.shown.lock().unwrap_or_else(PoisonError::into_inner);
    if shown.as_ref() == Some(&now) {
        return;
    }
    let mood_changed = shown.as_ref().is_none_or(|(mood, _, _)| *mood != glance.mood);
    if let Ok(menu) = build_menu(app, &items) {
        let _ = icon.tray.set_menu(Some(menu));
    }
    let _ = icon.tray.set_tooltip(Some(tooltip(glance)));
    if mood_changed {
        if let Ok(image) = Image::from_bytes(icon_bytes(glance.mood)) {
            let _ = icon
                .tray
                .set_icon_with_as_template(Some(image), cfg!(target_os = "macos"));
        }
    }
    *shown = Some(now);
}

fn build_menu<R: Runtime>(app: &AppHandle<R>, items: &[Item]) -> tauri::Result<Menu<R>> {
    let menu = Menu::new(app)?;
    for item in items {
        menu.append(&*made(app, item)?)?;
    }
    Ok(menu)
}

fn made<R: Runtime>(app: &AppHandle<R>, item: &Item) -> tauri::Result<Box<dyn IsMenuItem<R>>> {
    Ok(match item {
        Item::Said(words) => Box::new(MenuItem::with_id(app, "said", words, false, None::<&str>)?),
        Item::Line => Box::new(PredefinedMenuItem::separator(app)?),
        Item::Do(act, words, enabled) => Box::new(MenuItem::with_id(app, act.id(), words, *enabled, None::<&str>)?),
        Item::More(words, inside) => {
            let more = Submenu::new(app, words, true)?;
            for item in inside {
                more.append(&*made(app, item)?)?;
            }
            Box::new(more)
        }
    })
}

/// Does what a click on the menu asks.
fn act_on<R: Runtime>(app: &AppHandle<R>, act: Act) {
    let shared = app.state::<Shared>();
    let keevoro = &shared.keevoro;
    match act {
        Act::Open => crate::window::show(app, None),
        Act::Pause(how_long) => keevoro.pause_for(how_long),
        Act::CarryOn => keevoro.carry_on(),
        Act::BackUp(id) => {
            // An ordinary backup, as Back up now in the window starts:
            // what it says reaches the window, and the icon follows it.
            let started = keevoro
                .backups()
                .all()
                .into_iter()
                .find(|b| b.id == id)
                .ok_or_else(|| "That backup is no longer there.".to_string())
                .and_then(|named| keevoro.run_backup(&named, None).map_err(|e| e.message));
            if let Err(said) = started {
                crate::window::ask(app, crate::window::Ask::note(said));
            }
        }
        Act::Stop(job) => {
            keevoro.stop(Which::Job(job));
        }
        Act::Settings => crate::window::show(app, Some("settings")),
        Act::Quit => crate::window::quit_from_the_icon(app),
    }
    nudge(app);
}

/// Whether the glance may have changed, and the updates thread's wake-up.
#[derive(Default)]
pub struct Updates {
    nudged: Mutex<bool>,
    woken: Condvar,
}

/// Says the glance may have changed; the icon looks within two seconds.
pub fn nudge<R: Runtime>(app: &AppHandle<R>) {
    if let Some(updates) = app.try_state::<Updates>() {
        *updates.nudged.lock().unwrap_or_else(PoisonError::into_inner) = true;
        updates.woken.notify_all();
    }
}

/// Starts keeping the icon up to date, on a thread of its own.
pub fn keep_up_to_date<R: Runtime>(app: &AppHandle<R>) -> std::io::Result<()> {
    app.manage(Updates::default());
    let app = app.clone();
    std::thread::Builder::new().name("keevoro icon".into()).spawn(move || {
        let mut working = false;
        loop {
            {
                let updates = app.state::<Updates>();
                let mut nudged = updates.nudged.lock().unwrap_or_else(PoisonError::into_inner);
                while !*nudged {
                    if working {
                        // While something is going, look now and then too,
                        // so its end is seen even if nothing says so.
                        let (again, waited) = updates
                            .woken
                            .wait_timeout(nudged, Duration::from_secs(5))
                            .unwrap_or_else(PoisonError::into_inner);
                        nudged = again;
                        if waited.timed_out() {
                            break;
                        }
                    } else {
                        nudged = updates.woken.wait(nudged).unwrap_or_else(PoisonError::into_inner);
                    }
                }
                *nudged = false;
            }
            let glance = {
                let shared = app.state::<Shared>();
                shared.keevoro.at_a_glance(shared.desk.seen_since())
            };
            working = glance.mood == Mood::Working;
            show_glance(&app, &glance);
            std::thread::sleep(Duration::from_secs(2));
        }
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use keevoro_api::glance::GlanceBackup;

    fn glance() -> Glance {
        Glance {
            mood: Mood::Calm,
            line: "All backed up. Next: tonight at 02:30".into(),
            paused: false,
            pausable: true,
            backups: vec![GlanceBackup {
                id: "backup-1".into(),
                name: "Family photos".into(),
                busy: false,
            }],
            stoppable: None,
        }
    }

    fn words(items: &[Item]) -> Vec<String> {
        items
            .iter()
            .map(|item| match item {
                Item::Said(w) => format!("[{w}]"),
                Item::Line => "---".into(),
                Item::Do(_, w, true) => w.clone(),
                Item::Do(_, w, false) => format!("({w})"),
                Item::More(w, inside) => format!("{w} > {}", words(inside).join(" / ")),
            })
            .collect()
    }

    #[test]
    fn calm_with_one_backup_and_a_schedule() {
        assert_eq!(
            words(&menu(&glance())),
            [
                "[All backed up. Next: tonight at 02:30]",
                "---",
                "Open Keevoro",
                "Pause > For an hour / Until tomorrow morning / Until I carry on",
                "Back up now",
                "---",
                "Settings…",
                "Quit Keevoro",
            ]
        );
        assert_eq!(tooltip(&glance()), "Keevoro\nAll backed up. Next: tonight at 02:30");
    }

    #[test]
    fn a_backup_going_can_be_stopped_and_is_not_offered_again() {
        let going = Glance {
            mood: Mood::Working,
            line: "Backing up Family photos, about 12 minutes left".into(),
            backups: vec![GlanceBackup {
                busy: true,
                ..glance().backups[0].clone()
            }],
            stoppable: Some((JobId(7), "Family photos".into())),
            ..glance()
        };
        let menu = menu(&going);
        assert_eq!(
            words(&menu),
            [
                "[Backing up Family photos, about 12 minutes left]",
                "---",
                "Open Keevoro",
                "Pause > For an hour / Until tomorrow morning / Until I carry on",
                "(Back up now)",
                "Stop this backup",
                "---",
                "Settings…",
                "Quit Keevoro",
            ]
        );
        assert!(menu.contains(&Item::Do(Act::Stop(JobId(7)), STOP.into(), true)));
    }

    #[test]
    fn paused_offers_carry_on_and_with_several_backups_lists_them() {
        let paused = Glance {
            mood: Mood::Paused,
            line: "Paused until 13:00".into(),
            paused: true,
            backups: vec![
                glance().backups[0].clone(),
                GlanceBackup {
                    id: "backup-2".into(),
                    name: "Drone footage".into(),
                    busy: true,
                },
            ],
            ..glance()
        };
        assert_eq!(
            words(&menu(&paused)),
            [
                "[Paused until 13:00]",
                "---",
                "Open Keevoro",
                "Carry on",
                "Back up now > Family photos / (Drone footage)",
                "---",
                "Settings…",
                "Quit Keevoro",
            ]
        );
    }

    #[test]
    fn with_nothing_to_pause_and_no_backup_the_menu_is_short() {
        let none = Glance {
            line: "Nothing running now".into(),
            pausable: false,
            backups: Vec::new(),
            ..glance()
        };
        assert_eq!(
            words(&menu(&none)),
            [
                "[Nothing running now]",
                "---",
                "Open Keevoro",
                "---",
                "Settings…",
                "Quit Keevoro"
            ]
        );
        let needs = Glance {
            mood: Mood::NeedsYou,
            line: keevoro_api::glance::NEEDS_YOU.into(),
            ..none
        };
        assert_eq!(words(&menu(&needs))[0], "[Something needs your say]");
    }

    #[test]
    fn every_act_comes_back_from_its_id() {
        for act in [
            Act::Open,
            Act::Pause(PauseFor::AnHour),
            Act::Pause(PauseFor::UntilTomorrow),
            Act::Pause(PauseFor::UntilCarryOn),
            Act::CarryOn,
            Act::BackUp("backup-3".into()),
            Act::BackUp("stop:9".into()),
            Act::Stop(JobId(123_000_001)),
            Act::Settings,
            Act::Quit,
        ] {
            assert_eq!(Act::from_id(&act.id()), Some(act.clone()), "{act:?}");
        }
        assert_eq!(Act::from_id("said"), None, "the first line does nothing");
        assert_eq!(Act::from_id("stop:many"), None);
    }

    #[test]
    fn every_mood_has_its_icon_and_they_differ() {
        let all = [Mood::Calm, Mood::Working, Mood::Paused, Mood::NeedsYou].map(icon_bytes);
        for (i, a) in all.iter().enumerate() {
            assert!(Image::from_bytes(a).is_ok());
            for b in &all[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }
}
