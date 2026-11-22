//! Is the drive at a source's place the one that was looked through there
//! before?
//!
//! A source is remembered by its place, such as `E:\`. Another card pushed
//! into the same reader is given the same place, so a place alone cannot
//! say that it holds the same source. What the computer calls the drive
//! (its volume: on Windows the `\\?\Volume{...}\` name, which stays the same
//! whatever letter it is given; on a Mac or Linux the device number) is
//! remembered each time a source is looked through, as a [`Mark`].
//!
//! The volume is not enough on its own either way. A device number can
//! change when the very same card is plugged in again, and a volume can be
//! unknown (a network share). So when it differs, Keevoro looks for a few
//! of the files found there before: if any is there at the same size, it is
//! the same source with a new number. [`recognise`] decides; it is pure, and
//! the looking is done by whoever calls it.

use serde::{Deserialize, Serialize};

/// How many of the files found on a source before are looked for, when its
/// drive has a different volume now. Few enough to cost nothing on a card;
/// spread through the list so a folder deleted since does not decide it.
pub const FILES_LOOKED_FOR: usize = 24;

/// What a source's drive was last seen as.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mark {
    /// The volume, as `keevoro_engine::reach::volume_of` gives it, when the
    /// computer said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<String>,
    /// What the drive was called, such as "KINGSTON (E:)", when the source
    /// is a whole drive, to name it once it is no longer plugged in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drive: Option<String>,
    /// For a source set aside because another drive took its place: the
    /// place it was looked through at, where it is looked for again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub was_at: Option<String>,
}

/// Whether the drive at a source's place now is the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recognised {
    /// The same drive, or nothing remembered to tell otherwise: looked
    /// through as ever.
    Same,
    /// The same source, on a drive the computer numbers differently now
    /// (some of its files are there). Looked through, and the new volume
    /// remembered.
    Renumbered,
    /// Another drive. Not looked through as this source without the
    /// person's say, so what was found on it is not replaced.
    Different,
}

/// Decides whether the drive at a source's place is the source.
///
/// - `remembered`: the volume remembered at the last look, if any. A source
///   looked through before volumes were remembered has none, and is looked
///   through as before.
/// - `now`: the volume there now, if the computer says.
/// - `looked_for` and `found`: how many of the files found there before were
///   looked for, and how many were there at the same size. Only asked when
///   the volumes differ.
pub fn recognise(
    remembered: Option<&str>,
    now: Option<&str>,
    files_there: impl FnOnce() -> (usize, usize),
) -> Recognised {
    let (Some(remembered), Some(now)) = (remembered, now) else {
        return Recognised::Same;
    };
    if remembered == now {
        return Recognised::Same;
    }
    match files_there() {
        // Nothing was found there before, so nothing found would be lost.
        (0, _) => Recognised::Renumbered,
        (_, found) if found > 0 => Recognised::Renumbered,
        _ => Recognised::Different,
    }
}

/// Which of `of` files to look for, at most [`FILES_LOOKED_FOR`], spread
/// evenly through them, by their place in the list.
pub fn files_to_look_for(of: usize) -> Vec<usize> {
    let n = of.min(FILES_LOOKED_FOR);
    (0..n).map(|i| i * of / n.max(1)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_with_no_volume_remembered_is_looked_through_as_before() {
        let never = || -> (usize, usize) { panic!("nothing is looked for") };
        assert_eq!(recognise(None, Some("device 5"), never), Recognised::Same);
        assert_eq!(recognise(Some("device 5"), None, never), Recognised::Same);
        assert_eq!(recognise(Some("device 5"), Some("device 5"), never), Recognised::Same);
    }

    #[test]
    fn a_card_given_a_new_number_is_known_by_its_files() {
        assert_eq!(recognise(Some("a"), Some("b"), || (24, 1)), Recognised::Renumbered);
        assert_eq!(recognise(Some("a"), Some("b"), || (3, 3)), Recognised::Renumbered);
        // Nothing found there before: nothing to lose by looking.
        assert_eq!(recognise(Some("a"), Some("b"), || (0, 0)), Recognised::Renumbered);
    }

    #[test]
    fn another_card_in_the_same_place_is_not_taken_for_the_source() {
        assert_eq!(recognise(Some("a"), Some("b"), || (24, 0)), Recognised::Different);
        assert_eq!(recognise(Some("a"), Some("b"), || (1, 0)), Recognised::Different);
    }

    #[test]
    fn the_files_looked_for_are_few_and_spread_through_the_list() {
        assert!(files_to_look_for(0).is_empty());
        assert_eq!(files_to_look_for(3), vec![0, 1, 2]);
        let many = files_to_look_for(10_000);
        assert_eq!(many.len(), FILES_LOOKED_FOR);
        assert_eq!(many[0], 0);
        assert!(many[FILES_LOOKED_FOR - 1] > 9_000);
        assert!(many.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn a_mark_with_nothing_in_it_is_written_as_nothing() {
        assert_eq!(serde_json::to_string(&Mark::default()).unwrap(), "{}");
        let read: Mark = serde_json::from_str(r#"{"volume":"device 9","later":"kept out"}"#).unwrap();
        assert_eq!(read.volume.as_deref(), Some("device 9"));
    }
}
