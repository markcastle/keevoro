//! How many of each kind were found, said the one way everywhere.
//!
//! "238 photos, 11 videos and 40 other files." Every screen says counts of
//! what is found or kept through [`files`], and the command line and an
//! agent's summaries through [`photos_and_videos`], which always names
//! both, as they always have; so nothing can drift apart. With no other
//! files each says exactly what Keevoro always said. The app's screens say the same through `files` in
//! `ui/src/format.ts`, and both are tested against one table of cases,
//! `ui/src/countWords.json`.

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
        let cases: Vec<(usize, usize, usize, String)> =
            serde_json::from_str(include_str!("../../../ui/src/countWords.json")).unwrap();
        assert!(cases.len() >= 8);
        for (photos, videos, others, said) in cases {
            assert_eq!(files(photos, videos, others), said, "{photos} {videos} {others}");
        }
    }
}
