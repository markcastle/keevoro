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

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

static PAUSED: AtomicBool = AtomicBool::new(false);

/// Pauses or resumes all of Keevoro's heavy work.
pub fn set_paused(paused: bool) {
    PAUSED.store(paused, Ordering::SeqCst);
}

pub fn is_paused() -> bool {
    PAUSED.load(Ordering::SeqCst)
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

    #[test]
    fn work_waits_while_paused_and_carries_on_when_resumed() {
        let done = Arc::new(AtomicUsize::new(0));
        set_paused(true);
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
        set_paused(false);
        worker.join().unwrap();
        assert_eq!(done.load(Ordering::SeqCst), 5, "and all of it once resumed");
        assert!(!is_paused());
    }
}
