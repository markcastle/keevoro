//! What Keevoro is busy with, right now.
//!
//! A person looking at a busy computer wants one plain answer: what is it
//! doing? Each long job says so by holding a [`Doing`] for as long as it
//! runs, and anything that wants to tell the person (the app's "What
//! Keevoro is doing" page) asks [`now`].
//!
//! It is one small shared tally, like [`crate::pause`], and for the same
//! reason: "what is Keevoro doing?" is one question about the whole
//! program. The words shown to people are not here. This only names the
//! kinds of work; the app chooses how to say them.

use std::sync::atomic::{AtomicUsize, Ordering};

/// A kind of long-running work. Listed with the heaviest first, which is
/// the order [`now`] reports them in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Work {
    /// Copying the files being kept to where they are backed up.
    BackingUp,
    /// Reading files from start to finish to tell true copies apart.
    CheckingForDuplicates,
    /// Making the stills and playable versions of videos.
    MakingVideoPreviews,
    /// Making the small and screen-sized versions of photos for a backup.
    MakingPreviews,
    /// Walking a source to find the photos and videos on it.
    LookingThroughASource,
    /// Reading folder listings to find where the photos are, opening nothing.
    Surveying,
    /// Making a picture to show on screen, because one was asked for.
    MakingThumbnails,
}

/// Every kind, in the order they are reported.
pub const ALL: [Work; 7] = [
    Work::BackingUp,
    Work::CheckingForDuplicates,
    Work::MakingVideoPreviews,
    Work::MakingPreviews,
    Work::LookingThroughASource,
    Work::Surveying,
    Work::MakingThumbnails,
];

static IN_HAND: [AtomicUsize; ALL.len()] = [const { AtomicUsize::new(0) }; ALL.len()];

fn slot(work: Work) -> &'static AtomicUsize {
    &IN_HAND[ALL.iter().position(|w| *w == work).unwrap_or(0)]
}

/// Held while a job runs. Dropping it, however the job ends, says the job
/// is over, so a job that fails or is stopped never leaves a stale claim.
#[must_use = "the work only counts as under way while this is held"]
pub struct Doing(Work);

impl Drop for Doing {
    fn drop(&mut self) {
        slot(self.0).fetch_sub(1, Ordering::SeqCst);
    }
}

/// Says that `work` has begun. Keep what it returns until the work ends.
pub fn doing(work: Work) -> Doing {
    slot(work).fetch_add(1, Ordering::SeqCst);
    Doing(work)
}

/// Every kind of work under way at this moment, heaviest first.
pub fn now() -> Vec<Work> {
    ALL.into_iter()
        .filter(|w| slot(*w).load(Ordering::SeqCst) > 0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // One test, because the tally is shared by the whole program and
    // tests run side by side. It uses a kind nothing else in this crate
    // touches.
    #[test]
    fn work_is_under_way_for_exactly_as_long_as_it_is_held() {
        let has = || now().contains(&Work::MakingThumbnails);
        assert!(!has());
        let first = doing(Work::MakingThumbnails);
        let second = doing(Work::MakingThumbnails);
        assert!(has());
        drop(first);
        assert!(has(), "two were making thumbnails and one still is");
        drop(second);
        assert!(!has());
        // A job that fails part-way still lets go.
        fn fails() -> Result<(), ()> {
            let _busy = doing(Work::MakingThumbnails);
            Err(())
        }
        assert!(fails().is_err() && !has());
    }

    #[test]
    fn the_heaviest_work_is_named_first() {
        assert_eq!(ALL[0], Work::BackingUp);
        assert_eq!(ALL[ALL.len() - 1], Work::MakingThumbnails);
    }
}
