//! How many of each kind were found, and what a backup holds, said the one
//! way everywhere.
//!
//! "238 photos, 11 videos and 40 other files." Every screen says counts of
//! what is found or kept through [`Tally::said`] (or [`files`], without
//! music), and the command line and an agent's summaries through
//! [`Tally::said_in_full`] (or [`photos_and_videos`]), which always names
//! photos and videos, as they always have, unless there is only music; so
//! nothing can drift apart. With no music or other files each says
//! exactly what Keevoro always said. The app's screens say the same through
//! `files` in `ui/src/format.ts`, and both are tested against one table of
//! cases, `ui/src/countWords.json`.
//!
//! What a backup holds is named from what it is for ([`named`], and
//! [`heading`] for a step's title): "photos and videos", "music", "photos,
//! videos and music"; "Find your music". Music is counted in tracks:
//! "1,204 tracks".

use crate::wants::Wants;

/// "1 photo", "1,204 photos". The word is made plural with an s, which
/// suits every word said this way.
pub fn count(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{} {word}s", thousands(n))
    }
}

/// "1,204"
pub fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "238 photos, 11 videos and 40 other files", leaving out whichever
/// there are none of, and saying "0 photos" when there is nothing at all.
pub fn files(photos: usize, videos: usize, others: usize) -> String {
    let mut parts = Vec::new();
    if photos > 0 || (videos == 0 && others == 0) {
        parts.push(count(photos, "photo"));
    }
    if videos > 0 {
        parts.push(count(videos, "video"));
    }
    if others > 0 {
        parts.push(count(others, "other file"));
    }
    match parts.as_slice() {
        [rest @ .., last] if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => parts.concat(),
    }
}

/// How many there are of each kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    pub photos: usize,
    pub videos: usize,
    pub music: usize,
    pub others: usize,
}

impl Tally {
    fn parts(self) -> [(usize, &'static str); 4] {
        [
            (self.photos, "photo"),
            (self.videos, "video"),
            (self.music, "track"),
            (self.others, "other file"),
        ]
    }

    /// "238 photos and 1,204 tracks", leaving out whichever there are
    /// none of, and "0 photos" when there is nothing at all.
    pub fn said(self) -> String {
        self.said_for(Wants::default())
    }

    /// As [`Tally::said`], with nothing at all said in the first kind the
    /// backup is for: "0 tracks" for a backup of music.
    pub fn said_for(self, wants: Wants) -> String {
        let named: Vec<String> = self
            .parts()
            .iter()
            .filter(|(n, _)| *n > 0)
            .map(|(n, word)| count(*n, word))
            .collect();
        if named.is_empty() {
            return count(0, first_word(wants));
        }
        join(&named)
    }

    /// Every kind the backup is for said, none or not, then any other kind
    /// there are some of: "30 photos and 0 videos", "1,204 tracks". Other
    /// files are said only when there are some.
    pub fn counted(self, wants: Wants) -> String {
        let takes = [wants.photos, wants.videos, wants.music, false];
        let named: Vec<String> = self
            .parts()
            .iter()
            .zip(takes)
            .filter(|((n, _), wanted)| *wanted || *n > 0)
            .map(|((n, word), _)| count(*n, word))
            .collect();
        if named.is_empty() {
            return count(0, first_word(wants));
        }
        join(&named)
    }

    /// As the command line and an agent have always said a count: photos
    /// and videos both, then music and other files when there are some;
    /// but music alone when there is nothing else, as in a backup of music.
    pub fn said_in_full(self) -> String {
        if self.music > 0 && self.photos == 0 && self.videos == 0 {
            return self.counted(Wants::only(crate::MediaKind::Music));
        }
        self.counted(Wants::default())
    }
}

fn first_word(wants: Wants) -> &'static str {
    if !wants.photos && !wants.videos && wants.music {
        "track"
    } else if !wants.photos && wants.videos {
        "video"
    } else {
        "photo"
    }
}

fn join(parts: &[String]) -> String {
    match parts {
        [rest @ .., last] if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => parts.concat(),
    }
}

/// What a backup for `wants` holds, in a few words: "photos", "photos and
/// videos", "music", "photos and music", "photos, videos and music". Other
/// files are not named: they are a person's own addition, said apart.
pub fn named(wants: Wants) -> String {
    let named: Vec<String> = [
        (wants.photos, "photos"),
        (wants.videos, "videos"),
        (wants.music, "music"),
    ]
    .into_iter()
    .filter(|(on, _)| *on)
    .map(|(_, word)| word.to_string())
    .collect();
    if named.is_empty() {
        return "files".into();
    }
    join(&named)
}

/// The things a step's title names, as short as reads naturally: "Find
/// your photos" for photos and videos, as Keevoro always said, "Find your
/// music", "Find your photos and music".
pub fn heading(wants: Wants) -> String {
    let named: Vec<&str> = [
        (
            wants.photos || wants.videos,
            if wants.photos { "photos" } else { "videos" },
        ),
        (wants.music, "music"),
    ]
    .into_iter()
    .filter_map(|(on, word)| on.then_some(word))
    .collect();
    match named.as_slice() {
        [] => "files".into(),
        [one] => (*one).to_string(),
        _ => named.join(" and "),
    }
}

/// As [`Tally::said_in_full`]: "30 photos and 2 videos", "1,204 tracks",
/// "238 photos, 0 videos and 1,204 tracks", with other files after when
/// there are any. What the command line and an agent's summaries say.
pub fn in_full(photos: usize, videos: usize, music: usize, others: usize) -> String {
    Tally {
        photos,
        videos,
        music,
        others,
    }
    .said_in_full()
}

/// "30 photos and 2 videos", both always said, as the command line and an
/// agent's summaries say a count, with other files after when there are
/// any: "30 photos, 2 videos and 40 other files".
pub fn photos_and_videos(photos: usize, videos: usize, others: usize) -> String {
    let (photos, videos) = (count(photos, "photo"), count(videos, "video"));
    match others {
        0 => format!("{photos} and {videos}"),
        n => format!("{photos}, {videos} and {}", count(n, "other file")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MediaKind;

    fn wants(photos: bool, videos: bool, music: bool) -> Wants {
        Wants {
            photos,
            videos,
            music,
            others: true,
        }
    }

    #[test]
    fn what_a_backup_holds_is_named_from_what_it_is_for() {
        for (p, v, m, said, title) in [
            (true, false, false, "photos", "photos"),
            (true, true, false, "photos and videos", "photos"),
            (false, true, false, "videos", "videos"),
            (false, false, true, "music", "music"),
            (true, false, true, "photos and music", "photos and music"),
            (false, true, true, "videos and music", "videos and music"),
            (true, true, true, "photos, videos and music", "photos and music"),
        ] {
            assert_eq!(named(wants(p, v, m)), said);
            assert_eq!(heading(wants(p, v, m)), title, "{said}");
        }
        assert_eq!(named(Wants::only(MediaKind::Other)), "files");
    }

    #[test]
    fn music_is_counted_in_tracks_and_counts_follow_the_backup() {
        let music = Tally {
            music: 1204,
            ..Tally::default()
        };
        assert_eq!(music.said(), "1,204 tracks");
        assert_eq!(music.said_in_full(), "1,204 tracks");
        let mixed = Tally {
            photos: 238,
            music: 1204,
            ..Tally::default()
        };
        assert_eq!(mixed.said(), "238 photos and 1,204 tracks");
        assert_eq!(mixed.said_in_full(), "238 photos, 0 videos and 1,204 tracks");
        assert_eq!(Tally::default().said_for(Wants::only(MediaKind::Music)), "0 tracks");
        assert_eq!(Tally::default().said_for(Wants::default()), "0 photos");
        assert_eq!(
            Tally {
                photos: 238,
                videos: 11,
                ..Tally::default()
            }
            .counted(wants(true, true, true)),
            "238 photos, 11 videos and 0 tracks"
        );
        assert_eq!(Tally::default().counted(Wants::only(MediaKind::Music)), "0 tracks");
        assert_eq!(
            Tally {
                photos: 30,
                ..Tally::default()
            }
            .said_in_full(),
            photos_and_videos(30, 0, 0)
        );
    }

    #[test]
    fn counts_are_said_as_people_say_them() {
        assert_eq!(count(1, "photo"), "1 photo");
        assert_eq!(count(1204, "other file"), "1,204 other files");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn both_kinds_are_always_said_and_other_files_only_when_there_are_some() {
        assert_eq!(photos_and_videos(30, 0, 0), "30 photos and 0 videos");
        assert_eq!(photos_and_videos(1, 2, 40), "1 photo, 2 videos and 40 other files");
    }

    /// The one table the app's screens are tested against too.
    #[test]
    fn files_are_said_as_the_shared_table_says() {
        let cases: Vec<(usize, usize, usize, usize, String)> =
            serde_json::from_str(include_str!("../../../ui/src/countWords.json")).unwrap();
        assert!(cases.len() >= 8);
        for (photos, videos, others, music, said) in cases {
            let tally = Tally {
                photos,
                videos,
                music,
                others,
            };
            assert_eq!(tally.said(), said, "{photos} {videos} {others} {music}");
            if music == 0 {
                assert_eq!(files(photos, videos, others), said, "{photos} {videos} {others}");
            }
        }
    }
}
