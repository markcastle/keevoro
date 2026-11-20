//! Pausing Keevoro's heavy work.
//!
//! Looking through a disk, reading files in full and sending them all lean
//! on the computer. The person whose computer it is may want it back for a
//! while: a video call, a game, a laptop on battery. Pausing makes every
//! long job wait at its next safe point (between one file and the next) and
//! carry on from exactly there when resumed. Nothing is lost or redone.
//!
//! There is one switch for the whole program, because "is Keevoro paused?"
//! is one question. Each long loop calls [`wait_while_paused`] once per
//! file. It costs nothing while not paused.
//!
//! The switch remembers who asked, because the two last differently. A
//! pause asked for in this program (the app's Pause button, or `keevoro
//! pause` run here) stays until someone asks to carry on. A pause another
//! Keevoro program asked for is about the work that program could see,
//! so when that work ends here, and this program no longer holds anything
//! another could see or reach, it is lifted
//! ([`lift_pause_asked_for_elsewhere`]): otherwise work left running here,
//! such as the background check for duplicates, would sit paused where
//! nobody else could see it or tell it to carry on.

use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

/// Who asked for a pause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PausedBy {
    /// The person, in this program.
    ThisProgram,
    /// Another Keevoro program, which asked this one to pause.
    AnotherProgram,
}

const NOT_PAUSED: u8 = 0;
const BY_ANOTHER_PROGRAM: u8 = 1;
const BY_THIS_PROGRAM: u8 = 2;

/// The one switch: not paused, or paused and by whom.
static PAUSED: AtomicU8 = AtomicU8::new(NOT_PAUSED);

/// Pauses all of Keevoro's heavy work. A pause asked for in this program
/// outranks one asked for by another, so another program's asking never
/// turns this program's own pause into one that is lifted without a word.
pub fn pause(by: PausedBy) {
    match by {
        PausedBy::ThisProgram => PAUSED.store(BY_THIS_PROGRAM, Ordering::SeqCst),
        PausedBy::AnotherProgram => {
            // Only from not paused: already paused here stays paused here.
            let _ = PAUSED.compare_exchange(NOT_PAUSED, BY_ANOTHER_PROGRAM, Ordering::SeqCst, Ordering::SeqCst);
        }
    }
}

/// Lets all paused work carry on, whoever paused it.
pub fn carry_on() {
    PAUSED.store(NOT_PAUSED, Ordering::SeqCst);
}

pub fn is_paused() -> bool {
    PAUSED.load(Ordering::SeqCst) != NOT_PAUSED
}

/// Who paused Keevoro, or `None` when it is not paused.
pub fn paused_by() -> Option<PausedBy> {
    match PAUSED.load(Ordering::SeqCst) {
        BY_ANOTHER_PROGRAM => Some(PausedBy::AnotherProgram),
        BY_THIS_PROGRAM => Some(PausedBy::ThisProgram),
        _ => None,
    }
}

/// Lifts the pause if, and only if, another program asked for it. True if
/// it was lifted. Called when this program lets go of the last thing
/// another program could see; a pause asked for here is left alone.
pub fn lift_pause_asked_for_elsewhere() -> bool {
    PAUSED
        .compare_exchange(BY_ANOTHER_PROGRAM, NOT_PAUSED, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
}

/// Returns at once unless paused; while paused, waits here. Call it between
/// one file and the next, never part-way through one.
pub fn wait_while_paused() {
    while is_paused() {
        std::thread::sleep(Duration::from_millis(150));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    // One test, because the switch is one for the whole program and tests
    // run side by side.
    #[test]
    fn work_waits_while_paused_and_each_kind_of_pause_lasts_as_it_should() {
        let done = Arc::new(AtomicUsize::new(0));
        pause(PausedBy::ThisProgram);
        let worker = {
            let done = done.clone();
            std::thread::spawn(move || {
                for _ in 0..5 {
                    wait_while_paused();
                    done.fetch_add(1, Ordering::SeqCst);
                }
            })
        };
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(done.load(Ordering::SeqCst), 0, "nothing is done while paused");
        carry_on();
        worker.join().unwrap();
        assert_eq!(done.load(Ordering::SeqCst), 5, "and all of it once resumed");
        assert!(!is_paused());
        assert_eq!(paused_by(), None);

        // Another program's pause is lifted when its work here ends.
        pause(PausedBy::AnotherProgram);
        assert!(is_paused());
        assert_eq!(paused_by(), Some(PausedBy::AnotherProgram));
        assert!(lift_pause_asked_for_elsewhere());
        assert!(!is_paused());
        assert!(!lift_pause_asked_for_elsewhere(), "nothing to lift");

        // The person's own pause is not, and another program asking too
        // does not make it liftable.
        pause(PausedBy::ThisProgram);
        pause(PausedBy::AnotherProgram);
        assert_eq!(paused_by(), Some(PausedBy::ThisProgram));
        assert!(!lift_pause_asked_for_elsewhere());
        assert!(is_paused(), "only carrying on lifts the person's own pause");

        // The person pausing over another program's pause makes it theirs.
        carry_on();
        pause(PausedBy::AnotherProgram);
        pause(PausedBy::ThisProgram);
        assert!(!lift_pause_asked_for_elsewhere());
        assert_eq!(paused_by(), Some(PausedBy::ThisProgram));

        // Carrying on lifts either kind.
        carry_on();
        assert!(!is_paused());
    }
}
