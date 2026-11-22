//! What closing the window, minimising it and quitting Keevoro do,
//! decided here, pure, so every rule is tested without a window. The
//! glue in `window.rs` asks these and does what they say.
//!
//! - Closing the window keeps Keevoro running by the clock when the person
//!   chose that, or did not choose and this computer shows icons by the
//!   clock (Windows, a Mac). The first time, it says so first. Otherwise
//!   closing quits, as it always did, asking first if work is in hand.
//! - Minimising is an ordinary minimise, unless the person asked for it to
//!   hide the window to the icon by the clock too.
//! - Quitting with nothing in hand just quits; with a backup, a check or a
//!   search in hand, it asks first, in the window.

use keevoro_api::JobKind;

/// Whether this computer shows Keevoro's icon by the clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tray {
    /// It does: the notification area on Windows, the menu bar on a Mac.
    Shown,
    /// The icon was made, but the desktop may not show it: on Linux, GNOME
    /// shows none without an extension.
    MayNotShow,
    /// There is none: it could not be made.
    None,
}

impl Tray {
    /// Where the icon was made: on which system. Pure.
    pub fn made_on(os: &str) -> Self {
        match os {
            "windows" | "macos" => Self::Shown,
            _ => Self::MayNotShow,
        }
    }

    /// Whether closing keeps Keevoro running when the person has not said.
    pub fn keeps_running_unless_told(self) -> bool {
        self == Self::Shown
    }

    /// Whether the window may be hidden at all: never with nothing to bring
    /// it back from.
    pub fn may_hide(self) -> bool {
        self != Self::None
    }
}

/// Whether closing the window keeps Keevoro running by the clock: the
/// person's choice, or where the icon is surely shown. Never where there
/// is no icon.
pub fn keeps_running(choice: Option<bool>, tray: Tray) -> bool {
    tray.may_hide() && choice.unwrap_or(tray.keeps_running_unless_told())
}

/// What is in hand in this program that quitting would cut short.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InHand {
    BackUp,
    Check,
    Look,
    Survey,
}

impl InHand {
    /// The most important of the jobs this program runs, if any but the
    /// background check for duplicates, which carries on next time.
    pub fn of(kinds: impl IntoIterator<Item = JobKind>) -> Option<Self> {
        kinds
            .into_iter()
            .filter_map(|kind| match kind {
                JobKind::BackUp => Some(Self::BackUp),
                JobKind::Check => Some(Self::Check),
                JobKind::Look | JobKind::LookAgain => Some(Self::Look),
                JobKind::Survey => Some(Self::Survey),
                JobKind::Duplicates => None,
            })
            .min_by_key(|in_hand| *in_hand as u8)
    }

    /// The question quitting asks.
    pub fn question(self) -> &'static str {
        match self {
            Self::BackUp => "A backup is running. Stop it after the file in hand and quit? Nothing is lost.",
            Self::Check => "Keevoro is checking a backup. Stop it after the file in hand and quit? Nothing is lost.",
            Self::Look => {
                "Keevoro is looking through a source. Stop it and quit? Nothing is lost, and it can look again later."
            }
            Self::Survey => "Keevoro is finding where your photos are. Stop it and quit? Nothing is lost.",
        }
    }
}

/// What closing the window does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Close {
    /// Hide it: Keevoro keeps running by the clock.
    Hide,
    /// Keep the window, and say first, once, that Keevoro keeps running.
    SayFirst,
    /// Quit, as Keevoro always did.
    Quit,
    /// Keep the window, and ask before cutting this short.
    AskFirst(&'static str),
}

/// Closing the window, given whether it keeps running, whether that has
/// been said, whether it is being said now (closing again while it is
/// hides it), and what is in hand. Pure.
pub fn on_close(keeps_running: bool, said: bool, saying_now: bool, in_hand: Option<InHand>) -> Close {
    match (keeps_running, in_hand) {
        (true, _) if said || saying_now => Close::Hide,
        (true, _) => Close::SayFirst,
        (false, Some(in_hand)) => Close::AskFirst(in_hand.question()),
        (false, None) => Close::Quit,
    }
}

/// Minimising the window: true to hide it to the icon by the clock, false
/// for an ordinary minimise. Pure.
pub fn hides_on_minimise(minimise_to_tray: bool, tray: Tray) -> bool {
    minimise_to_tray && tray.may_hide()
}

/// What quitting does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quit {
    Now,
    /// Ask first, in the window.
    AskFirst(&'static str),
    /// Stop what is in hand after the file in hand, then quit.
    StopFirst,
}

/// Quitting, given what is in hand and whether the person already said to
/// stop it. Pure.
pub fn on_quit(in_hand: Option<InHand>, stop: bool) -> Quit {
    match (in_hand, stop) {
        (None, _) => Quit::Now,
        (Some(_), true) => Quit::StopFirst,
        (Some(in_hand), false) => Quit::AskFirst(in_hand.question()),
    }
}

/// What the app is started with when it starts with the computer, so it
/// comes up quietly, by the clock, with no window.
pub const IN_BACKGROUND: &str = "--in-background";

/// Whether to open the window as Keevoro starts. Pure.
pub fn starts_with_its_window(args: &[String]) -> bool {
    !args.iter().any(|a| a == IN_BACKGROUND)
}

/// Where the icon is, said the first time the window is hidden, and beside
/// the setting.
pub fn where_the_icon_is(os: &str) -> &'static str {
    match os {
        "windows" => {
            "Its icon is by the clock, at the bottom right of the screen (it may be under the ^ arrow there). Open or quit Keevoro from there."
        }
        "macos" => "Its icon is in the menu bar, at the top right of the screen. Open or quit Keevoro from there.",
        _ => "Its icon is in the system tray, where your desktop has one. Open or quit Keevoro from there.",
    }
}

/// Said beside the setting where the icon may not show.
pub const MAY_NOT_SHOW: &str = "This desktop may not show icons by the clock (GNOME needs an extension for them), so closing Keevoro quits it unless you turn this on. Turn it on only if you can see Keevoro's icon.";

/// Said beside the setting where there is no icon at all.
pub const NO_TRAY: &str =
    "This computer has no place for Keevoro's icon by the clock, so closing Keevoro quits it, as it always did.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_keeps_keevoro_running_where_the_icon_shows_and_quits_elsewhere_unless_chosen() {
        assert!(keeps_running(None, Tray::made_on("windows")));
        assert!(keeps_running(None, Tray::made_on("macos")));
        assert!(!keeps_running(None, Tray::made_on("linux")), "GNOME may show no icon");
        assert!(keeps_running(Some(true), Tray::MayNotShow), "the person chose it");
        assert!(!keeps_running(Some(false), Tray::Shown), "the person chose not to");
        assert!(
            !keeps_running(Some(true), Tray::None),
            "never hidden with nothing to bring it back from"
        );
    }

    #[test]
    fn closing_says_once_that_keevoro_keeps_running_then_hides() {
        assert_eq!(on_close(true, false, false, None), Close::SayFirst);
        assert_eq!(on_close(true, false, false, Some(InHand::BackUp)), Close::SayFirst);
        assert_eq!(on_close(true, true, false, None), Close::Hide);
        assert_eq!(
            on_close(true, true, false, Some(InHand::BackUp)),
            Close::Hide,
            "the backup carries on, hidden"
        );
        assert_eq!(
            on_close(true, false, true, None),
            Close::Hide,
            "closed again while it is being said"
        );
    }

    #[test]
    fn closing_without_keeping_running_quits_and_asks_first_when_work_is_in_hand() {
        assert_eq!(on_close(false, false, false, None), Close::Quit);
        assert_eq!(on_close(false, true, false, None), Close::Quit);
        assert_eq!(
            on_close(false, false, false, Some(InHand::BackUp)),
            Close::AskFirst("A backup is running. Stop it after the file in hand and quit? Nothing is lost.")
        );
    }

    #[test]
    fn quitting_with_nothing_in_hand_just_quits_and_otherwise_asks_then_stops_first() {
        assert_eq!(on_quit(None, false), Quit::Now);
        assert_eq!(on_quit(None, true), Quit::Now);
        assert_eq!(
            on_quit(Some(InHand::Check), false),
            Quit::AskFirst(InHand::Check.question())
        );
        assert_eq!(on_quit(Some(InHand::Look), true), Quit::StopFirst);
    }

    #[test]
    fn what_is_in_hand_is_the_most_important_job_and_never_the_check_for_duplicates() {
        assert_eq!(InHand::of([]), None);
        assert_eq!(InHand::of([JobKind::Duplicates]), None);
        assert_eq!(
            InHand::of([JobKind::Duplicates, JobKind::LookAgain, JobKind::BackUp]),
            Some(InHand::BackUp)
        );
        assert_eq!(InHand::of([JobKind::Survey, JobKind::Check]), Some(InHand::Check));
        assert_eq!(InHand::of([JobKind::Survey]), Some(InHand::Survey));
        for in_hand in [InHand::BackUp, InHand::Check, InHand::Look, InHand::Survey] {
            let question = in_hand.question();
            assert!(question.ends_with("Nothing is lost.") || question.contains("Nothing is lost,"));
            assert!(!question.contains('—'), "no em dashes");
        }
    }

    #[test]
    fn minimising_is_ordinary_unless_asked_and_there_is_an_icon() {
        assert!(!hides_on_minimise(false, Tray::Shown));
        assert!(hides_on_minimise(true, Tray::Shown));
        assert!(hides_on_minimise(true, Tray::MayNotShow));
        assert!(!hides_on_minimise(true, Tray::None));
    }

    #[test]
    fn started_with_the_computer_it_comes_up_with_no_window() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(starts_with_its_window(&args(&["keevoro-app"])));
        assert!(!starts_with_its_window(&args(&["keevoro-app", IN_BACKGROUND])));
    }

    #[test]
    fn where_the_icon_is_is_said_for_each_system() {
        assert!(where_the_icon_is("windows").contains("^ arrow"));
        assert!(where_the_icon_is("macos").contains("menu bar"));
        assert!(where_the_icon_is("linux").contains("system tray"));
    }
}
