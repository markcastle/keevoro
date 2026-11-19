//! Numbers said the way Keevoro says them everywhere: "1,204 photos",
//! "4.2 GB", "about 3 minutes". The app, the command line and an agent use
//! these so that the same thing reads the same in each.

pub use keevoro_engine::history::duration;
pub use keevoro_engine::summary::{bytes, count, thousands};

/// A moment, given in seconds since 1970, as `2026-11-16 14:02:11` in UTC,
/// the same way the log writes it.
pub fn date_and_time(seconds: i64) -> String {
    crate::log::stamp(u64::try_from(seconds).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_read_as_a_person_would_say_them() {
        assert_eq!(count(1, "photo"), "1 photo");
        assert_eq!(count(1204, "photo"), "1,204 photos");
        assert_eq!(thousands(9870), "9,870");
        assert!(bytes(4_200_000_000).ends_with("GB"), "{}", bytes(4_200_000_000));
        assert!(!duration(200).is_empty());
        assert_eq!(date_and_time(0), "1970-01-01 00:00:00");
        assert_eq!(date_and_time(-5), "1970-01-01 00:00:00");
    }
}
