//! How much work done in the background may take, at any moment.
//!
//! While a backup sends the originals, Keevoro makes the previews beside
//! it, so that they are ready, or nearly, when the originals are done. That
//! is a guest's work: the person's own use of their computer comes first,
//! and so do the originals. This decides how many previews may be made at
//! once, from what the computer says about itself. It is pure: the signals
//! are read elsewhere (`keevoro-engine`), so the rules are tested here with
//! made-up ones, on every platform.
//!
//! The rules, in order:
//!
//! 1. Once the originals have been sent, previews are the backup's own
//!    work and are made as they always were, at the foreground pace.
//! 2. On battery, the background holds off entirely.
//! 3. When other programs keep the processor busy ([`BUSY`] or more of it),
//!    the background holds off too.
//! 4. Otherwise it uses fewer workers than the foreground, and never more
//!    than one per source, so a source is read by the sender and at most
//!    one preview maker. A slow USB stick or network share is not asked to
//!    serve many readers at once.
//! 5. While the person is using the computer (any key or mouse movement in
//!    the last [`IN_USE`] seconds) or other programs are fairly busy
//!    ([`FAIRLY_BUSY`]), one worker at most.
//!
//! A signal the platform cannot give is taken the careful way where that
//! costs little: unknown idleness counts as "in use" (one worker). Unknown
//! processor load and battery are not reasons to wait, since waiting on a
//! signal that never comes would leave every preview until the end.

/// Other programs' share of the processor, as a percentage of the whole
/// computer, at which the background holds off.
pub const BUSY: f32 = 60.0;

/// Other programs' share at which the background slows to one worker.
pub const FAIRLY_BUSY: f32 = 25.0;

/// Seconds since the last key press or mouse movement within which the
/// computer counts as in use.
pub const IN_USE: u64 = 120;

/// What the computer says about itself, as far as it can.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Conditions {
    /// True while the backup is still sending originals.
    pub sending_originals: bool,
    /// `None` when the platform cannot say.
    pub on_battery: Option<bool>,
    /// Seconds since the person last touched the keyboard or mouse.
    pub idle_seconds: Option<u64>,
    /// Other programs' share of the processor, 0 to 100, averaged over the
    /// last few seconds.
    pub others_cpu: Option<f32>,
    /// How many previews the foreground would make at once.
    pub foreground: usize,
    /// How many sources the previews come from.
    pub sources: usize,
}

/// How many previews may be made at once now. Zero means wait.
pub fn preview_workers(c: &Conditions) -> usize {
    let foreground = c.foreground.max(1);
    if !c.sending_originals {
        return foreground;
    }
    if c.on_battery == Some(true) {
        return 0;
    }
    let others = c.others_cpu.unwrap_or(0.0);
    if others >= BUSY {
        return 0;
    }
    let fewer = foreground.saturating_sub(1).max(1).min(c.sources.max(1));
    let in_use = c.idle_seconds.is_none_or(|idle| idle < IN_USE);
    if in_use || others >= FAIRLY_BUSY {
        fewer.min(1)
    } else {
        fewer
    }
}

/// Why the background is waiting, in a few words for the stage list, or
/// `None` when it is not.
pub fn waiting_because(c: &Conditions) -> Option<&'static str> {
    if preview_workers(c) > 0 {
        None
    } else if c.on_battery == Some(true) {
        Some("waiting while this computer is on battery")
    } else {
        Some("waiting while this computer is busy")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quiet() -> Conditions {
        Conditions {
            sending_originals: true,
            on_battery: Some(false),
            idle_seconds: Some(600),
            others_cpu: Some(5.0),
            foreground: 4,
            sources: 3,
        }
    }

    #[test]
    fn a_quiet_computer_gets_fewer_workers_than_the_foreground_and_one_per_source() {
        assert_eq!(preview_workers(&quiet()), 3);
        assert_eq!(preview_workers(&Conditions { sources: 1, ..quiet() }), 1);
        assert_eq!(
            preview_workers(&Conditions {
                foreground: 2,
                ..quiet()
            }),
            1
        );
        assert_eq!(
            preview_workers(&Conditions {
                foreground: 1,
                ..quiet()
            }),
            1
        );
    }

    #[test]
    fn someone_using_the_computer_leaves_one_worker() {
        let using = Conditions {
            idle_seconds: Some(5),
            ..quiet()
        };
        assert_eq!(preview_workers(&using), 1);
        assert_eq!(waiting_because(&using), None);
        // Not knowing counts as in use.
        assert_eq!(
            preview_workers(&Conditions {
                idle_seconds: None,
                ..quiet()
            }),
            1
        );
    }

    #[test]
    fn a_busy_computer_or_a_battery_makes_the_background_wait() {
        let busy = Conditions {
            others_cpu: Some(75.0),
            ..quiet()
        };
        assert_eq!(preview_workers(&busy), 0);
        assert_eq!(waiting_because(&busy), Some("waiting while this computer is busy"));
        assert_eq!(
            preview_workers(&Conditions {
                others_cpu: Some(30.0),
                ..quiet()
            }),
            1
        );
        let battery = Conditions {
            on_battery: Some(true),
            ..quiet()
        };
        assert_eq!(preview_workers(&battery), 0);
        assert_eq!(
            waiting_because(&battery),
            Some("waiting while this computer is on battery")
        );
        // Unknown battery and load are not reasons to wait.
        assert_eq!(
            preview_workers(&Conditions {
                on_battery: None,
                others_cpu: None,
                ..quiet()
            }),
            3
        );
    }

    #[test]
    fn once_the_originals_are_sent_previews_are_made_as_they_always_were() {
        let after = Conditions {
            sending_originals: false,
            on_battery: Some(true),
            others_cpu: Some(90.0),
            ..quiet()
        };
        assert_eq!(preview_workers(&after), 4);
        assert_eq!(preview_workers(&Conditions { foreground: 0, ..after }), 1);
    }
}
