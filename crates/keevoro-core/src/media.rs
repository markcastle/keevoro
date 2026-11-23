//! What counts as a photo, a video or music, and the fourth kind a person
//! can ask for: other files, carried along.

use serde::{Deserialize, Serialize};

/// The kinds of file Keevoro backs up. Photos, videos and music are what
/// a backup can be for; an other file is one a person asked it to take
/// along (a document, a project file).
///
/// Music and other files are found by the ending of their names alone,
/// never opened to see what they are, and never grouped by date: each
/// keeps the folders it came from. They are backed up and proved exactly
/// as a photo is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Photo,
    Video,
    Other,
    /// A song, a recording or a playlist. Listed last, so the order of
    /// the kinds Keevoro had before is unchanged.
    Music,
}

impl MediaKind {
    /// "photo", "video", "track", "other file": one of them, counted.
    pub fn word(self) -> &'static str {
        match self {
            MediaKind::Photo => "photo",
            MediaKind::Video => "video",
            MediaKind::Music => "track",
            MediaKind::Other => "other file",
        }
    }

    /// Photos and videos are opened, shown and grouped by date; music and
    /// other files are not.
    pub fn is_media(self) -> bool {
        matches!(self, MediaKind::Photo | MediaKind::Video)
    }

    /// Found by its ending alone and never opened during a search: music
    /// and other files.
    pub fn by_ending_alone(self) -> bool {
        !self.is_media()
    }
}

/// Classify a detected MIME type. Content detection comes first, so this is
/// the main path.
pub fn kind_from_mime(mime: &str) -> Option<MediaKind> {
    let mime = mime.to_ascii_lowercase();
    if mime.starts_with("image/") {
        // Icons, vector images, documents and drawings are never photos.
        if matches!(
            mime.as_str(),
            "image/x-icon"
                | "image/vnd.microsoft.icon"
                | "image/svg+xml"
                | "image/vnd.adobe.photoshop"
                // Scanned documents and technical drawings, which the
                // detector files under images.
                | "image/vnd.djvu"
                | "image/vnd.dwg"
        ) {
            return None;
        }
        Some(MediaKind::Photo)
    } else if mime.starts_with("video/") {
        Some(MediaKind::Video)
    } else {
        None
    }
}

/// Is this the ending of a camera's raw format?
///
/// A raw file is the camera's own untouched record of a picture, and for
/// the people who shoot them it is the most precious copy there is. Most
/// are built like a TIFF whose first picture is only a small preview, with
/// the real, full-size one tucked further in. So the size in the header is
/// the preview's, not the photo's, and must never be used to judge it.
pub fn is_raw_extension(ext: &str) -> bool {
    crate::kinds::raw_ending(ext)
}

/// Is a file with this extension certainly not a photo or video?
///
/// A drive holds far more programs, documents and music than photos, and
/// opening each one to check is most of the time a search takes. Files
/// with these endings are passed over without being opened. The list only
/// names kinds of file that are never a photo or video; anything
/// unfamiliar, or with no ending at all, is still opened and judged by
/// what is inside.
///
/// Music is on it too, every ending Keevoro knows as music, so a backup
/// that is not for music passes it over as it always did, without opening
/// it. A backup that is for music finds it first on the list of kinds,
/// which is asked before this.
pub fn never_media_extension(ext: &str) -> bool {
    // Asked of nearly every file on a drive, so lowered without allocating.
    // None of these is longer than eight letters.
    let mut buffer = [0u8; 8];
    let Some(ext) = crate::kinds::ascii_lower(ext.trim_start_matches('.'), &mut buffer) else {
        return false;
    };
    matches!(
        ext,
        // Programs and their parts
        "exe" | "dll" | "sys" | "msi" | "cab" | "ocx" | "drv" | "bat" | "cmd" | "ps1" | "sh" | "so" | "dylib"
        | "lib" | "obj" | "o" | "a" | "class" | "jar" | "pyc" | "pdb" | "mui" | "manifest" | "cat" | "inf" | "nls"
        // Documents
        | "txt" | "log" | "md" | "rtf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "pdf" | "odt" | "ods"
        | "csv" | "epub" | "chm" | "hlp"
        // Web pages and code
        | "html" | "htm" | "css" | "js" | "mjs" | "ts" | "tsx" | "jsx" | "json" | "xml" | "yml" | "yaml" | "ini"
        | "cfg" | "conf" | "toml" | "c" | "h" | "cpp" | "hpp" | "cs" | "java" | "py" | "rb" | "go" | "rs" | "php"
        | "sql" | "map" | "lock"
        // Archives and disk images
        | "zip" | "rar" | "7z" | "gz" | "tar" | "bz2" | "xz" | "iso"
        // Music, sound and playlists: every kind Keevoro knows as music
        | "mp3" | "flac" | "m4a" | "aac" | "wav" | "ogg" | "opus" | "wma" | "aiff" | "aif" | "ape" | "wv" | "dsf"
        | "dff" | "mka" | "mpc" | "tta" | "ac3" | "amr" | "mid" | "midi" | "au" | "ra" | "caf" | "m4b" | "oga"
        | "spx" | "tak" | "m3u" | "m3u8" | "pls" | "cue"
        // Fonts and shortcuts
        | "ttf" | "otf" | "woff" | "woff2" | "lnk" | "url"
    )
}

/// What the tests compare with.
#[cfg(test)]
pub(crate) mod tests_support {
    /// The endings counted as camera raw before the list was widened.
    pub const WAS_RAW: &[&str] = &[
        "dng", "cr2", "cr3", "crw", "nef", "nrw", "arw", "srf", "sr2", "orf", "rw2", "raf", "pef", "srw", "x3f", "3fr",
        "erf", "kdc", "mrw", "raw", "iiq", "dcr", "mef", "mos", "rwl",
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_raw_files_are_known_by_their_ending() {
        for ext in ["dng", "DNG", ".cr2", "NEF", "arw", "raf"] {
            assert!(is_raw_extension(ext), "{ext}");
        }
        for ext in ["jpg", "tif", "png", "mp4", "", "abcdefghijklmnop", "ÉCL"] {
            assert!(!is_raw_extension(ext), "{ext}");
        }
        for ext in tests_support::WAS_RAW {
            assert!(is_raw_extension(ext), "{ext}");
        }
    }

    #[test]
    fn programs_documents_and_music_are_passed_over_by_name() {
        for ext in [
            "exe", "DLL", ".txt", "Pdf", "mp3", "zip", "js", "MANIFEST", "woff2", "FLAC", "m3u8", "cue",
        ] {
            assert!(never_media_extension(ext), "{ext}");
        }
        for ext in ["manifests", "ÉXE"] {
            assert!(!never_media_extension(ext), "{ext}");
        }
    }

    #[test]
    fn anything_that_could_be_a_memory_is_still_opened() {
        // Photos, videos, RAW, and the vague endings old software used.
        for ext in ["jpg", "HEIC", "mov", "cr2", "mts", "dat", "bin", "tmp", "bak", ""] {
            assert!(!never_media_extension(ext), "{ext}");
        }
    }

    #[test]
    fn photos_and_videos_by_mime() {
        assert_eq!(kind_from_mime("image/jpeg"), Some(MediaKind::Photo));
        assert_eq!(kind_from_mime("IMAGE/HEIF"), Some(MediaKind::Photo));
        assert_eq!(kind_from_mime("video/quicktime"), Some(MediaKind::Video));
        assert_eq!(kind_from_mime("video/x-msvideo"), Some(MediaKind::Video));
        // A scanned document and a technical drawing are not photos.
        assert_eq!(kind_from_mime("image/vnd.djvu"), None);
        assert_eq!(kind_from_mime("image/vnd.dwg"), None);
    }

    #[test]
    fn icons_documents_and_audio_are_not_media() {
        assert_eq!(kind_from_mime("image/x-icon"), None);
        assert_eq!(kind_from_mime("image/svg+xml"), None);
        assert_eq!(kind_from_mime("application/pdf"), None);
        assert_eq!(kind_from_mime("audio/mpeg"), None);
    }

    #[test]
    fn kind_serialises_lowercase() {
        assert_eq!(serde_json::to_string(&MediaKind::Video).unwrap(), "\"video\"");
        assert_eq!(serde_json::to_string(&MediaKind::Other).unwrap(), "\"other\"");
        assert_eq!(MediaKind::Other.word(), "other file");
        assert_eq!(serde_json::to_string(&MediaKind::Music).unwrap(), "\"music\"");
        assert_eq!(MediaKind::Music.word(), "track");
        assert!(MediaKind::Photo.is_media() && !MediaKind::Other.is_media() && !MediaKind::Music.is_media());
        assert!(MediaKind::Music.by_ending_alone() && !MediaKind::Video.by_ending_alone());
    }
}
