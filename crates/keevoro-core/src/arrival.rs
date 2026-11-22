//! When a source the person asked about is plugged in: "Back up when this
//! is plugged in". Pure: whether a source's place is there, and the time,
//! are given; nothing here looks at a drive or a clock.
//!
//! The rules:
//!
//! - **Plugged in** means its place was not there and now is. A card
//!   already in when Keevoro starts was not just plugged in, and is left
//!   alone until it is taken out and plugged in again.
//! - **Settle:** it must stay there [`SETTLE`] seconds first, so a card
//!   pushed in and pulled straight out, or a drive still waking, does
//!   nothing.
//! - **Not too often:** at most one run for a source every
//!   [`AT_MOST_EVERY`] seconds, so a loose lead that comes and goes does
//!   not start a run each time.
//! - **Paused:** while Keevoro is paused nothing starts; a card still
//!   there when it carries on is backed up then.
//! - **Busy:** if it cannot start because another backup is running, it is
//!   tried again after [`BUSY_RETRY`] seconds, while still plugged in.

/// The catalog setting holding one source's choice: `go` when it is
/// backed up whenever it is plugged in; missing or anything else is off.
pub fn key(root: &std::path::Path) -> String {
    format!("arrival:{}", root.display())
}

/// Seconds a source must stay plugged in before it is backed up.
pub const SETTLE: i64 = 10;

/// At most one run for a source this often, in seconds.
pub const AT_MOST_EVERY: i64 = 10 * 60;

/// Seconds before trying again when another backup was running.
pub const BUSY_RETRY: i64 = 60;

/// What is known of one source's place between looks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Watch {
    /// Looked at at least once.
    seen: bool,
    /// When it was last plugged in, while it is there.
    there_since: Option<i64>,
    /// Plugged in, and not yet backed up for it.
    owed: bool,
    /// When a run last started for it.
    last_run: Option<i64>,
    /// Not tried before this, after finding another backup running.
    not_before: i64,
}

impl Watch {
    /// One look at the source's place: `there` now, at `now` (seconds),
    /// with Keevoro `paused` or not. True means start a backup of it now.
    pub fn look(&mut self, there: bool, now: i64, paused: bool) -> bool {
        if !self.seen {
            // There already when Keevoro started: not just plugged in.
            self.seen = true;
            self.there_since = there.then_some(now);
            return false;
        }
        if !there {
            self.there_since = None;
            self.owed = false;
            return false;
        }
        let since = *self.there_since.get_or_insert_with(|| {
            self.owed = true;
            now
        });
        if !self.owed || paused || now < self.not_before || now - since < SETTLE {
            return false;
        }
        self.owed = false;
        // A clock set back is no reason to refuse.
        if self.last_run.is_some_and(|at| at <= now && now - at < AT_MOST_EVERY) {
            return false;
        }
        self.last_run = Some(now);
        true
    }

    /// The run could not start, as another backup was running: try again
    /// in a while, if it is still plugged in.
    pub fn busy(&mut self, now: i64) {
        self.owed = true;
        self.last_run = None;
        self.not_before = now + BUSY_RETRY;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Looks at each second from `from` to `to`, saying when it started.
    fn looks(w: &mut Watch, from: i64, to: i64, there: bool) -> Vec<i64> {
        (from..=to).filter(|t| w.look(there, *t, false)).collect()
    }

    #[test]
    fn a_card_plugged_in_is_backed_up_once_it_has_settled() {
        let mut w = Watch::default();
        assert!(!w.look(false, 0, false));
        assert_eq!(looks(&mut w, 100, 200, true), [100 + SETTLE], "once, after settling");
    }

    #[test]
    fn a_card_already_in_when_keevoro_starts_is_left_alone_until_plugged_in_again() {
        let mut w = Watch::default();
        assert_eq!(looks(&mut w, 0, 100, true), Vec::<i64>::new());
        looks(&mut w, 101, 101, false);
        assert_eq!(looks(&mut w, 2_000, 2_100, true), [2_000 + SETTLE]);
    }

    #[test]
    fn a_card_pulled_out_before_it_settles_does_nothing() {
        let mut w = Watch::default();
        w.look(false, 0, false);
        assert!(looks(&mut w, 100, 100 + SETTLE - 2, true).is_empty());
        assert!(looks(&mut w, 100 + SETTLE - 1, 200, false).is_empty());
    }

    #[test]
    fn a_loose_lead_coming_and_going_is_one_run_in_ten_minutes() {
        let mut w = Watch::default();
        w.look(false, 0, false);
        assert_eq!(looks(&mut w, 100, 130, true), [100 + SETTLE]);
        looks(&mut w, 131, 131, false);
        assert!(looks(&mut w, 200, 300, true).is_empty(), "too soon after the last");
        looks(&mut w, 301, 301, false);
        let later = 100 + SETTLE + AT_MOST_EVERY + 5;
        assert_eq!(looks(&mut w, later, later + 30, true), [later + SETTLE]);
    }

    #[test]
    fn paused_nothing_starts_and_a_card_still_there_is_backed_up_on_carrying_on() {
        let mut w = Watch::default();
        w.look(false, 0, false);
        assert!((100..200).all(|t| !w.look(true, t, true)));
        assert!(w.look(true, 200, false));
    }

    #[test]
    fn another_backup_running_means_trying_again_in_a_while_if_still_there() {
        let mut w = Watch::default();
        w.look(false, 0, false);
        assert_eq!(looks(&mut w, 100, 110, true), [110]);
        w.busy(110);
        assert!(looks(&mut w, 111, 110 + BUSY_RETRY - 1, true).is_empty());
        assert!(w.look(true, 110 + BUSY_RETRY, false));
    }
}
