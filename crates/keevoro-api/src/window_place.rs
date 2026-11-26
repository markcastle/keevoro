//! Where Keevoro's window was, so it opens the way the person left it:
//! the same size, in the same place, and maximised if it was.
//!
//! Kept in a file of its own, `window place.json` in Keevoro's config
//! folder, beside `window.json` and never in `settings.json`. It is only a
//! convenience: a missing or damaged file means nothing is remembered and
//! the window opens as it would the first time, never an error anyone sees.
//!
//! Where the window opens is decided here too, purely ([`opening`]), from
//! what is remembered and the screens this computer has now, so a window
//! is never opened bigger than the screen or somewhere nobody can see it.
//! Everything is in the screen's own pixels, as the system reports them.

use crate::keevoro::Keevoro;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// The file, in Keevoro's config folder.
pub const FILE: &str = "window place.json";

const KNOWN: &[&str] = &["v", "x", "y", "width", "height", "maximised"];

/// A rectangle on the desktop, in the screen's own pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    fn area(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    fn right(&self) -> i64 {
        i64::from(self.x) + i64::from(self.width)
    }

    fn bottom(&self) -> i64 {
        i64::from(self.y) + i64::from(self.height)
    }

    /// How much of this lies on `other`.
    fn overlap(&self, other: &Rect) -> u64 {
        let width = self.right().min(other.right()) - i64::from(self.x.max(other.x));
        let height = self.bottom().min(other.bottom()) - i64::from(self.y.max(other.y));
        if width <= 0 || height <= 0 {
            return 0;
        }
        u64::try_from(width * height).unwrap_or(0)
    }

    /// This size, made no bigger than `within`.
    fn fitted_to(&self, within: &Rect) -> (u32, u32) {
        (self.width.min(within.width), self.height.min(within.height))
    }
}

/// Where the window was and how: its place when not maximised (so
/// un-maximising gives that back), and whether it was maximised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowPlace {
    pub rect: Rect,
    pub maximised: bool,
}

/// One screen as the system describes it now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Screen {
    /// The part a window may use, without the taskbar or the menu bar.
    pub work_area: Rect,
    /// How many of the screen's pixels make one of the window's.
    pub scale: f64,
    /// The screen the system calls its main one.
    pub primary: bool,
}

/// Where and how big the window opens, given what is `remembered` and the
/// `screens` there are now. `width` and `height` are the size it opens at
/// the first time, as `tauri.conf.json` gives it (before the screen's
/// scale). `None` when no screen is known: the window then opens as it
/// always did.
pub fn opening(remembered: Option<WindowPlace>, width: f64, height: f64, screens: &[Screen]) -> Option<WindowPlace> {
    let primary = screens.iter().find(|s| s.primary).or(screens.first())?;
    let Some(place) = remembered.filter(|p| p.rect.area() > 0) else {
        // The first time: the size it was made for, if the screen has room.
        let wanted = Rect {
            x: 0,
            y: 0,
            width: pixels(width * primary.scale),
            height: pixels(height * primary.scale),
        };
        let fits = wanted.fitted_to(&primary.work_area) == (wanted.width, wanted.height);
        return Some(WindowPlace {
            rect: centred(wanted.fitted_to(&primary.work_area), &primary.work_area),
            maximised: !fits,
        });
    };
    let seen = screens
        .iter()
        .map(|s| place.rect.overlap(&s.work_area))
        .sum::<u64>()
        .min(place.rect.area());
    let rect = if seen == place.rect.area() {
        // All of it can be seen, perhaps across two screens: as it was.
        place.rect
    } else if seen * 2 >= place.rect.area() {
        // Mostly on a screen: moved, and made smaller if need be, to lie
        // wholly on the one it is most on.
        let on = screens
            .iter()
            .max_by_key(|s| place.rect.overlap(&s.work_area))
            .unwrap_or(primary);
        inside(place.rect, &on.work_area)
    } else {
        // On a screen that is no longer there, or mostly off every one.
        centred(place.rect.fitted_to(&primary.work_area), &primary.work_area)
    };
    Some(WindowPlace {
        rect,
        maximised: place.maximised,
    })
}

/// Whole pixels, at least one.
fn pixels(size: f64) -> u32 {
    if size.is_finite() && size >= 1.0 {
        size.round().min(f64::from(u32::MAX)) as u32
    } else {
        1
    }
}

fn centred((width, height): (u32, u32), within: &Rect) -> Rect {
    let x = i64::from(within.x) + (i64::from(within.width) - i64::from(width)) / 2;
    let y = i64::from(within.y) + (i64::from(within.height) - i64::from(height)) / 2;
    Rect {
        x: i32::try_from(x).unwrap_or(within.x),
        y: i32::try_from(y).unwrap_or(within.y),
        width,
        height,
    }
}

/// `rect` made to fit `within` and moved as little as it takes to lie in it.
fn inside(rect: Rect, within: &Rect) -> Rect {
    let (width, height) = rect.fitted_to(within);
    let x = i64::from(rect.x).clamp(i64::from(within.x), within.right() - i64::from(width));
    let y = i64::from(rect.y).clamp(i64::from(within.y), within.bottom() - i64::from(height));
    Rect {
        x: i32::try_from(x).unwrap_or(within.x),
        y: i32::try_from(y).unwrap_or(within.y),
        width,
        height,
    }
}

impl WindowPlace {
    /// What `config` remembers, or `None` when there is no file or it
    /// cannot be understood.
    pub fn read(config: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(config.join(FILE)).ok()?;
        let all: serde_json::Value = serde_json::from_str(&text).ok()?;
        let number = |name: &str| all.get(name).and_then(serde_json::Value::as_i64);
        let rect = Rect {
            x: i32::try_from(number("x")?).ok()?,
            y: i32::try_from(number("y")?).ok()?,
            width: u32::try_from(number("width")?).ok().filter(|w| *w > 0)?,
            height: u32::try_from(number("height")?).ok().filter(|h| *h > 0)?,
        };
        let maximised = all
            .get("maximised")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        Some(Self { rect, maximised })
    }

    /// Writes it to `config`, beside the real file and then moved over it,
    /// so a crash never leaves half a file. Parts a later version wrote
    /// are kept.
    pub fn save(&self, config: &Path) -> std::io::Result<()> {
        static WRITES: AtomicU64 = AtomicU64::new(0);
        let mut all = std::fs::read_to_string(config.join(FILE))
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        all.retain(|key, _| !KNOWN.contains(&key.as_str()));
        all.insert("v".into(), 1.into());
        all.insert("x".into(), self.rect.x.into());
        all.insert("y".into(), self.rect.y.into());
        all.insert("width".into(), self.rect.width.into());
        all.insert("height".into(), self.rect.height.into());
        all.insert("maximised".into(), self.maximised.into());
        std::fs::create_dir_all(config)?;
        let text = serde_json::to_string_pretty(&serde_json::Value::Object(all)).map_err(std::io::Error::other)?;
        let write = WRITES.fetch_add(1, Ordering::Relaxed);
        let temp = config.join(format!(".{FILE}.{}.{write}.part", std::process::id()));
        std::fs::write(&temp, text)?;
        std::fs::rename(&temp, config.join(FILE)).inspect_err(|_| {
            let _ = std::fs::remove_file(&temp);
        })
    }
}

impl Keevoro {
    /// Where the window was when it was last seen, if that is remembered.
    pub fn window_place(&self) -> Option<WindowPlace> {
        WindowPlace::read(self.backups().config_folder())
    }

    /// Remembers where the window is. Failing only means it opens as it
    /// would the first time, so the caller need not tell anyone.
    pub fn keep_window_place(&self, place: &WindowPlace) -> std::io::Result<()> {
        place.save(self.backups().config_folder())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A laptop screen and a big one, in their own pixels, less the taskbar.
    const BIG: Rect = Rect {
        x: 0,
        y: 0,
        width: 2560,
        height: 1400,
    };
    const SMALL: Rect = Rect {
        x: 0,
        y: 0,
        width: 1366,
        height: 728,
    };

    fn screen(work_area: Rect, primary: bool) -> Screen {
        Screen {
            work_area,
            scale: 1.0,
            primary,
        }
    }

    fn rect(x: i32, y: i32, width: u32, height: u32) -> Rect {
        Rect { x, y, width, height }
    }

    fn place(rect: Rect, maximised: bool) -> Option<WindowPlace> {
        Some(WindowPlace { rect, maximised })
    }

    #[test]
    fn the_first_time_on_a_big_screen_it_opens_at_its_own_size_in_the_middle() {
        let opened = opening(None, 1280.0, 860.0, &[screen(BIG, true)]).unwrap();
        assert_eq!(opened.rect, rect(640, 270, 1280, 860));
        assert!(!opened.maximised);
        // On a screen that doubles its pixels, it is the same size to the eye.
        let sharp = Screen {
            scale: 2.0,
            ..screen(rect(0, 0, 5120, 2800), true)
        };
        assert_eq!(
            opening(None, 1280.0, 860.0, &[sharp]).unwrap().rect,
            rect(1280, 540, 2560, 1720)
        );
    }

    #[test]
    fn the_first_time_on_a_small_screen_it_fits_and_is_maximised() {
        let opened = opening(None, 1280.0, 860.0, &[screen(SMALL, true)]).unwrap();
        assert!(opened.maximised, "it does not fit, so it fills the screen");
        assert_eq!(opened.rect, rect(43, 0, 1280, 728), "and un-maximised, it still fits");
        // It opens on the main screen, wherever that is.
        let left = screen(rect(-1366, 0, 1366, 728), false);
        let opened = opening(None, 1280.0, 860.0, &[left, screen(BIG, true)]).unwrap();
        assert_eq!(opened.rect, rect(640, 270, 1280, 860));
    }

    #[test]
    fn remembered_and_still_on_a_screen_it_opens_just_as_it_was() {
        let was = rect(100, 80, 1500, 900);
        assert_eq!(
            opening(place(was, false), 1280.0, 860.0, &[screen(BIG, true)]),
            place(was, false)
        );
        // Across two screens side by side, still all there to see.
        let across = rect(2000, 100, 1200, 800);
        let right = screen(rect(2560, 0, 1920, 1040), false);
        assert_eq!(
            opening(place(across, false), 1280.0, 860.0, &[screen(BIG, true), right]),
            place(across, false)
        );
    }

    #[test]
    fn mostly_on_a_screen_it_is_moved_onto_it_and_made_to_fit() {
        // Hanging off the bottom right, and taller than the screen now is.
        let was = rect(200, 100, 1300, 800);
        let opened = opening(place(was, false), 1280.0, 860.0, &[screen(SMALL, true)]).unwrap();
        assert_eq!(opened.rect, rect(66, 0, 1300, 728));
    }

    #[test]
    fn remembered_on_a_screen_that_is_gone_it_opens_in_the_middle_of_the_main_one() {
        // It was on a second screen to the right, now unplugged.
        let was = rect(2800, 100, 1400, 900);
        let opened = opening(place(was, false), 1280.0, 860.0, &[screen(BIG, true)]).unwrap();
        assert_eq!(opened.rect, rect(580, 250, 1400, 900), "at the size it was");
        // Mostly off every screen counts the same, and too big is made to fit.
        let was = rect(-1500, -300, 1600, 1000);
        let opened = opening(place(was, false), 1280.0, 860.0, &[screen(SMALL, true)]).unwrap();
        assert_eq!(opened.rect, rect(0, 0, 1366, 728));
        // Where Windows puts a minimised window is never somewhere to open.
        let minimised = rect(-32000, -32000, 160, 28);
        let opened = opening(place(minimised, false), 1280.0, 860.0, &[screen(BIG, true)]).unwrap();
        assert_eq!(opened.rect, rect(1200, 686, 160, 28));
    }

    #[test]
    fn remembered_maximised_it_opens_maximised_with_its_earlier_size_kept_for_later() {
        let before = rect(200, 150, 1300, 880);
        assert_eq!(
            opening(place(before, true), 1280.0, 860.0, &[screen(BIG, true)]),
            place(before, true)
        );
        // And maximised still, on the main screen, when its screen is gone.
        let elsewhere = rect(3000, 150, 1300, 880);
        let opened = opening(place(elsewhere, true), 1280.0, 860.0, &[screen(BIG, true)]).unwrap();
        assert!(opened.maximised);
        assert_eq!(opened.rect, rect(630, 260, 1300, 880));
    }

    #[test]
    fn with_no_screen_known_it_opens_as_it_always_did() {
        assert_eq!(opening(None, 1280.0, 860.0, &[]), None);
        assert_eq!(opening(place(rect(0, 0, 800, 600), false), 1280.0, 860.0, &[]), None);
        // Without one called the main screen, the first will do.
        let opened = opening(None, 1280.0, 860.0, &[screen(BIG, false)]).unwrap();
        assert_eq!(opened.rect, rect(640, 270, 1280, 860));
    }

    #[test]
    fn it_is_saved_and_read_back_and_a_missing_or_damaged_file_means_nothing_remembered() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(WindowPlace::read(dir.path()), None, "nothing there yet");
        let kept = WindowPlace {
            rect: rect(-1200, 40, 1100, 760),
            maximised: true,
        };
        kept.save(dir.path()).unwrap();
        assert_eq!(WindowPlace::read(dir.path()), Some(kept));
        let text = std::fs::read_to_string(dir.path().join(FILE)).unwrap();
        assert!(
            text.contains("\"maximised\": true") && text.contains("\"v\": 1"),
            "{text}"
        );
        let leftover: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(leftover.len(), 1, "no part-written file is left behind");

        for damaged in [
            "{ half a fi",
            "[1, 2]",
            r#"{ "v": 1, "x": 10, "y": 10, "width": "wide", "height": 600 }"#,
            r#"{ "v": 1, "x": 10, "y": 10, "width": 0, "height": 600 }"#,
            r#"{ "v": 1, "x": 99999999999, "y": 10, "width": 800, "height": 600 }"#,
        ] {
            std::fs::write(dir.path().join(FILE), damaged).unwrap();
            assert_eq!(WindowPlace::read(dir.path()), None, "{damaged}");
        }
        // And opening with nothing remembered is the first time again.
        let opened = opening(WindowPlace::read(dir.path()), 1280.0, 860.0, &[screen(BIG, true)]).unwrap();
        assert_eq!(opened.rect, rect(640, 270, 1280, 860));
        // Written again over a damaged file, it is whole once more.
        kept.save(dir.path()).unwrap();
        assert_eq!(WindowPlace::read(dir.path()), Some(kept));
    }

    #[test]
    fn parts_a_later_version_wrote_are_kept_when_it_is_written_again() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(FILE),
            r#"{ "v": 2, "x": 1, "y": 2, "width": 800, "height": 600, "screen": "the left one" }"#,
        )
        .unwrap();
        let read = WindowPlace::read(dir.path()).unwrap();
        assert!(!read.maximised, "not said counts as not maximised");
        WindowPlace {
            maximised: true,
            ..read
        }
        .save(dir.path())
        .unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.path().join(FILE)).unwrap()).unwrap();
        assert_eq!(value["screen"], "the left one");
        assert_eq!(value["maximised"], true);
    }

    #[test]
    fn keevoro_keeps_it_in_its_config_folder_leaves_the_settings_file_alone_and_a_reset_forgets_it() {
        let dir = tempfile::tempdir().unwrap();
        let keevoro = crate::keevoro::tests::keevoro(dir.path());
        keevoro.start_backup("Family photos").unwrap();
        let settings_file = keevoro.backups().config_folder().join("settings.json");
        let before = std::fs::read_to_string(&settings_file).unwrap();
        assert_eq!(keevoro.window_place(), None);
        let kept = WindowPlace {
            rect: rect(100, 80, 1500, 900),
            maximised: false,
        };
        keevoro.keep_window_place(&kept).unwrap();
        assert_eq!(keevoro.window_place(), Some(kept));
        assert!(keevoro.backups().config_folder().join(FILE).is_file());
        assert_eq!(std::fs::read_to_string(&settings_file).unwrap(), before);
        keevoro.reset().unwrap();
        assert!(!keevoro.backups().config_folder().join(FILE).exists());
        assert_eq!(keevoro.window_place(), None);
    }
}
