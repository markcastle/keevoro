//! The window opening the way the person left it: its size, its place and
//! whether it was maximised, remembered as it changes and given back when
//! it opens, at the next start and when it comes back from the icon by the
//! clock.
//!
//! Where it opens is decided in `keevoro_api::window_place`, pure, from
//! what is remembered and the screens there are now; this only asks the
//! window and the screens, and does what that says. [`Kept`] follows the
//! window as it moves: a minimised or hidden window is never remembered,
//! and while it is maximised, the place it had before is kept, so
//! un-maximising gives that back. What it holds is written a moment after
//! the window stops moving, and again as it closes, so nothing is lost if
//! Keevoro is ended from its icon.

use crate::Shared;
use keevoro_api::window_place::{self, Rect, Screen, WindowPlace};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;
use tauri::{Manager, PhysicalPosition, PhysicalSize, Runtime, WebviewWindow, Window};

/// How long after the window last moved what it holds is written.
const WRITE_AFTER: Duration = Duration::from_secs(1);

/// What the window is like at a moment, as it says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seen {
    /// Where it is, when it could say (a desktop on Wayland never does).
    pub at: Option<(i32, i32)>,
    /// How big it is, when it could say.
    pub size: Option<(u32, u32)>,
    pub maximised: bool,
    pub minimised: bool,
    pub full_screen: bool,
    pub visible: bool,
}

/// Where the window is, followed while it is open.
#[derive(Default)]
pub struct Kept {
    place: Mutex<Option<WindowPlace>>,
    /// True while a write waits to happen.
    waiting: AtomicBool,
}

impl Kept {
    /// What is held now.
    pub fn place(&self) -> Option<WindowPlace> {
        *self.place.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Starts from where the window opened.
    pub fn opened(&self, place: Option<WindowPlace>) {
        *self.place.lock().unwrap_or_else(PoisonError::into_inner) = place;
    }

    /// Takes in what the window is like now, and answers whether that
    /// changed what is held.
    pub fn noticed(&self, seen: Seen) -> bool {
        // Minimised or hidden is never how it opens, and full screen is the
        // system's to give back.
        if seen.minimised || !seen.visible || seen.full_screen {
            return false;
        }
        let mut held = self.place.lock().unwrap_or_else(PoisonError::into_inner);
        let before = held.map(|p| p.rect);
        let size = seen.size.filter(|(width, height)| *width > 0 && *height > 0);
        let rect = match (seen.maximised, before) {
            // Maximised: the place it had before is what un-maximising gives.
            (true, Some(before)) => before,
            _ => {
                let at = seen.at.or(before.map(|b| (b.x, b.y)));
                let size = size.or(before.map(|b| (b.width, b.height)));
                let (Some((x, y)), Some((width, height))) = (at, size) else {
                    return false;
                };
                Rect { x, y, width, height }
            }
        };
        let now = Some(WindowPlace {
            rect,
            maximised: seen.maximised,
        });
        let changed = *held != now;
        *held = now;
        changed
    }
}

/// The screens this computer has now, as the window sees them.
fn screens<R: Runtime>(window: &WebviewWindow<R>) -> Vec<Screen> {
    let primary = window.primary_monitor().ok().flatten();
    window
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|monitor| {
            let area = monitor.work_area();
            Screen {
                work_area: Rect {
                    x: area.position.x,
                    y: area.position.y,
                    width: area.size.width,
                    height: area.size.height,
                },
                scale: monitor.scale_factor(),
                primary: primary
                    .as_ref()
                    .is_some_and(|p| p.position() == monitor.position() && p.size() == monitor.size()),
            }
        })
        .collect()
}

/// Puts a window just made, not yet shown, where it opens: as it was left,
/// or the first time at `width` by `height`, never bigger than the screen
/// or off it.
pub fn put<R: Runtime>(window: &WebviewWindow<R>, width: f64, height: f64) {
    let shared = window.app_handle().state::<Shared>();
    let remembered = shared.keevoro.window_place();
    let opening = window_place::opening(remembered, width, height, &screens(window));
    if let Some(place) = opening {
        let rect = place.rect;
        let _ = window.set_size(PhysicalSize::new(rect.width, rect.height));
        let _ = window.set_position(PhysicalPosition::new(rect.x, rect.y));
        if place.maximised {
            let _ = window.maximize();
        }
    }
    shared.desk.place.opened(opening.or(remembered));
}

/// What the window says it is like now.
fn seen<R: Runtime>(window: &Window<R>) -> Seen {
    Seen {
        at: window.outer_position().ok().map(|at| (at.x, at.y)),
        size: window.inner_size().ok().map(|size| (size.width, size.height)),
        maximised: window.is_maximized().unwrap_or(false),
        minimised: window.is_minimized().unwrap_or(false),
        full_screen: window.is_fullscreen().unwrap_or(false),
        visible: window.is_visible().unwrap_or(true),
    }
}

/// The window moved or changed size: remembered, written a moment after it
/// stops.
pub fn moved<R: Runtime>(window: &Window<R>) {
    let app = window.app_handle();
    let shared = app.state::<Shared>();
    if !shared.desk.place.noticed(seen(window)) || shared.desk.place.waiting.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(WRITE_AFTER);
        if let Some(shared) = app.try_state::<Shared>() {
            shared.desk.place.waiting.store(false, Ordering::SeqCst);
            write(&shared);
        }
    });
}

/// Writes where the window is now. Failing only means it opens as it would
/// the first time, so nobody is told.
pub fn write(shared: &Shared) {
    if let Some(place) = shared.desk.place.place() {
        let _ = shared.keevoro.keep_window_place(&place);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, width: u32, height: u32) -> Rect {
        Rect { x, y, width, height }
    }

    fn normal(rect: Rect) -> Seen {
        Seen {
            at: Some((rect.x, rect.y)),
            size: Some((rect.width, rect.height)),
            maximised: false,
            minimised: false,
            full_screen: false,
            visible: true,
        }
    }

    #[test]
    fn it_follows_the_window_and_keeps_the_size_from_before_it_was_maximised() {
        let kept = Kept::default();
        let opened = WindowPlace {
            rect: rect(640, 270, 1280, 860),
            maximised: false,
        };
        kept.opened(Some(opened));
        assert!(!kept.noticed(normal(opened.rect)), "nothing changed");
        assert!(kept.noticed(normal(rect(100, 80, 1500, 900))));
        // Maximised: the screen's size is not what is remembered.
        assert!(kept.noticed(Seen {
            maximised: true,
            ..normal(rect(-8, -8, 2576, 1416))
        }));
        assert_eq!(
            kept.place(),
            Some(WindowPlace {
                rect: rect(100, 80, 1500, 900),
                maximised: true
            })
        );
        // Un-maximised, it is back where it was.
        assert!(kept.noticed(normal(rect(100, 80, 1500, 900))));
        assert_eq!(kept.place().map(|p| p.maximised), Some(false));
    }

    #[test]
    fn minimised_hidden_or_full_screen_is_never_remembered() {
        let kept = Kept::default();
        let was = WindowPlace {
            rect: rect(100, 80, 1500, 900),
            maximised: false,
        };
        kept.opened(Some(was));
        for seen in [
            Seen {
                minimised: true,
                ..normal(rect(-32000, -32000, 160, 28))
            },
            Seen {
                visible: false,
                ..normal(rect(0, 0, 800, 600))
            },
            Seen {
                full_screen: true,
                ..normal(rect(0, 0, 2560, 1440))
            },
        ] {
            assert!(!kept.noticed(seen), "{seen:?}");
            assert_eq!(kept.place(), Some(was));
        }
        // A window that cannot say where it is (on Wayland) keeps where it
        // was, with its new size.
        assert!(kept.noticed(Seen {
            at: None,
            ..normal(rect(0, 0, 1200, 800))
        }));
        assert_eq!(kept.place().map(|p| p.rect), Some(rect(100, 80, 1200, 800)));
        // And with nothing known, nothing is made up.
        let fresh = Kept::default();
        assert!(!fresh.noticed(Seen {
            at: None,
            size: None,
            ..normal(rect(0, 0, 0, 0))
        }));
        assert!(!fresh.noticed(Seen {
            at: None,
            ..normal(rect(0, 0, 1200, 800))
        }));
        assert_eq!(fresh.place(), None);
    }
}
