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

/// What decides whether a backup that runs by itself may start now.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct StartFacts {
    /// `None` when the platform cannot say.
    pub on_battery: Option<bool>,
    /// Other programs' share of the processor, 0 to 100, averaged over a
    /// little while.
    pub others_cpu: Option<f32>,
    /// Whether the connection is metered (a phone's hotspot), when known.
    pub metered: Option<bool>,
    /// True when the run reads or sends over the internet: a cloud
    /// destination or a bucket source. A run to a USB drive does not.
    pub needs_internet: bool,
    /// Seconds since the person last used Keevoro's own window, when it
    /// is open and has been used.
    pub window_used: Option<u64>,
    /// The person allowed running on battery, or on a metered connection.
    pub allow_battery: bool,
    pub allow_metered: bool,
    /// The person asked for this run (Run now), so their being in
    /// Keevoro's window is no reason to wait.
    pub person_asked: bool,
    /// How full the battery is, 0 to 100, when the computer says.
    pub battery_percent: Option<u8>,
    /// The person asked for backups by themselves only between certain
    /// hours, and it is not one of them now.
    pub outside_hours: bool,
}

/// On battery, a backup by itself waits while the battery is below this
/// share full, or when the computer does not say how full it is.
pub const BATTERY_LOW: u8 = 50;

/// The hours of the day a person allows backups by themselves in, in the
/// computer's own time: from `from` until `to`, across midnight when `to`
/// comes first ("22:00 until 07:00").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hours {
    pub from: jiff::civil::Time,
    pub to: jiff::civil::Time,
}

impl Hours {
    /// True when `at` is within them. The same time twice means the whole
    /// day.
    pub fn contain(&self, at: jiff::civil::Time) -> bool {
        match self.from.cmp(&self.to) {
            std::cmp::Ordering::Equal => true,
            std::cmp::Ordering::Less => at >= self.from && at < self.to,
            std::cmp::Ordering::Greater => at >= self.from || at < self.to,
        }
    }

    /// "between 22:00 and 07:00".
    pub fn words(&self) -> String {
        format!(
            "between {} and {}",
            crate::schedule::time_words(self.from),
            crate::schedule::time_words(self.to)
        )
    }
}

/// Seconds since the window was last used within which a run by itself
/// waits, so it never starts sending while the person is choosing.
pub const WINDOW_IN_USE: u64 = 10 * 60;

/// Why a backup that runs by itself waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wait {
    OnBattery,
    Metered,
    Busy,
    WindowInUse,
    OutsideHours,
}

impl Wait {
    /// In words, as the backup screen, the tray and the command line say it.
    pub fn words(self) -> &'static str {
        match self {
            Wait::OnBattery => "Waiting while this computer is on battery",
            Wait::Metered => "Waiting for a connection that isn't metered",
            Wait::Busy => "Waiting for a quieter moment",
            Wait::WindowInUse => "Waiting until you have finished in Keevoro",
            Wait::OutsideHours => "Waiting for the hours you chose",
        }
    }
}

/// Whether a backup that runs by itself may start now. A signal the
/// computer cannot give is never a reason to wait.
pub fn may_start(f: &StartFacts) -> Result<(), Wait> {
    let low = f.battery_percent.is_none_or(|p| p < BATTERY_LOW);
    if f.on_battery == Some(true) && low && !f.allow_battery {
        return Err(Wait::OnBattery);
    }
    if f.outside_hours {
        return Err(Wait::OutsideHours);
    }
    if f.needs_internet && f.metered == Some(true) && !f.allow_metered {
        return Err(Wait::Metered);
    }
    if f.others_cpu.is_some_and(|cpu| cpu >= FAIRLY_BUSY) {
        return Err(Wait::Busy);
    }
    if !f.person_asked && f.window_used.is_some_and(|secs| secs < WINDOW_IN_USE) {
        return Err(Wait::WindowInUse);
    }
    Ok(())
}

#[cfg(test)]
mod may_start_tests {
    use super::*;

    #[test]
    fn a_quiet_computer_on_the_mains_starts_at_once() {
        assert_eq!(may_start(&StartFacts::default()), Ok(()));
        let quiet = StartFacts {
            on_battery: Some(false),
            others_cpu: Some(3.0),
            metered: Some(false),
            needs_internet: true,
            window_used: Some(3600),
            ..StartFacts::default()
        };
        assert_eq!(may_start(&quiet), Ok(()));
    }

    #[test]
    fn it_waits_on_battery_unless_allowed() {
        let on = StartFacts {
            on_battery: Some(true),
            ..StartFacts::default()
        };
        assert_eq!(may_start(&on), Err(Wait::OnBattery));
        assert_eq!(Wait::OnBattery.words(), "Waiting while this computer is on battery");
        assert_eq!(
            may_start(&StartFacts {
                allow_battery: true,
                ..on
            }),
            Ok(())
        );
    }

    #[test]
    fn on_battery_it_runs_once_the_battery_is_at_least_half_full() {
        let on = StartFacts {
            on_battery: Some(true),
            battery_percent: Some(BATTERY_LOW),
            ..StartFacts::default()
        };
        assert_eq!(may_start(&on), Ok(()));
        let low = StartFacts {
            battery_percent: Some(BATTERY_LOW - 1),
            ..on
        };
        assert_eq!(may_start(&low), Err(Wait::OnBattery));
        let mains = StartFacts {
            on_battery: Some(false),
            ..low
        };
        assert_eq!(may_start(&mains), Ok(()), "how full matters only on battery");
    }

    #[test]
    fn outside_the_hours_chosen_it_waits() {
        let outside = StartFacts {
            outside_hours: true,
            ..StartFacts::default()
        };
        assert_eq!(may_start(&outside), Err(Wait::OutsideHours));
        assert_eq!(Wait::OutsideHours.words(), "Waiting for the hours you chose");
    }

    #[test]
    fn hours_can_cross_midnight_and_the_same_time_twice_is_all_day() {
        let t = |text: &str| crate::schedule::time_named(text).unwrap();
        let night = Hours {
            from: t("22:00"),
            to: t("07:00"),
        };
        assert!(night.contain(t("23:30")) && night.contain(t("02:30")) && night.contain(t("22:00")));
        assert!(!night.contain(t("07:00")) && !night.contain(t("12:00")));
        let day = Hours {
            from: t("09:00"),
            to: t("17:00"),
        };
        assert!(day.contain(t("09:00")) && day.contain(t("16:59")));
        assert!(!day.contain(t("17:00")) && !day.contain(t("08:59")));
        assert!(Hours {
            from: t("03:00"),
            to: t("03:00")
        }
        .contain(t("12:00")));
        assert_eq!(night.words(), "between 22:00 and 07:00");
    }

    #[test]
    fn a_metered_connection_matters_only_to_a_run_over_the_internet() {
        let metered = StartFacts {
            metered: Some(true),
            ..StartFacts::default()
        };
        assert_eq!(may_start(&metered), Ok(()), "a USB drive needs no connection");
        let cloud = StartFacts {
            needs_internet: true,
            ..metered
        };
        assert_eq!(may_start(&cloud), Err(Wait::Metered));
        assert_eq!(
            may_start(&StartFacts {
                allow_metered: true,
                ..cloud
            }),
            Ok(())
        );
    }

    #[test]
    fn it_waits_for_a_quieter_moment_and_for_the_person_to_finish_in_keevoro() {
        let busy = StartFacts {
            others_cpu: Some(FAIRLY_BUSY),
            ..StartFacts::default()
        };
        assert_eq!(may_start(&busy), Err(Wait::Busy));
        let choosing = StartFacts {
            window_used: Some(30),
            ..StartFacts::default()
        };
        assert_eq!(may_start(&choosing), Err(Wait::WindowInUse));
        assert_eq!(
            may_start(&StartFacts {
                person_asked: true,
                ..choosing
            }),
            Ok(())
        );
        assert_eq!(
            may_start(&StartFacts {
                window_used: Some(WINDOW_IN_USE),
                ..StartFacts::default()
            }),
            Ok(())
        );
    }

    #[test]
    fn what_the_computer_cannot_say_is_no_reason_to_wait() {
        let unknown = StartFacts {
            on_battery: None,
            others_cpu: None,
            metered: None,
            needs_internet: true,
            window_used: None,
            ..StartFacts::default()
        };
        assert_eq!(may_start(&unknown), Ok(()));
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
