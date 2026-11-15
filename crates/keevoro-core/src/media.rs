//! What counts as a photo or a video.

use serde::{Deserialize, Serialize};

/// The two kinds of file keevoro cares about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Photo,
    Video,
}

/// Classify a detected MIME type. Content detection comes first, so this is
/// the main path.
pub fn kind_from_mime(mime: &str) -> Option<MediaKind> {
    let mime = mime.to_ascii_lowercase();
    if mime.starts_with("image/") {
        // Icons and vector images are never photos.
        if matches!(
            mime.as_str(),
            "image/x-icon" | "image/vnd.microsoft.icon" | "image/svg+xml" | "image/vnd.adobe.photoshop"
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

/// Fallback for formats that magic-byte detection does not recognise
/// (several camera RAW and camcorder formats). Only used when content
/// detection returns nothing.
pub fn kind_from_extension(ext: &str) -> Option<MediaKind> {
    let ext = ext.trim_start_matches('.').to_ascii_lowercase();
    match ext.as_str() {
        // Camera RAW
        "dng" | "cr2" | "cr3" | "crw" | "nef" | "nrw" | "arw" | "srf" | "sr2" | "orf" | "rw2" | "raf" | "pef"
        | "srw" | "x3f" | "3fr" | "erf" | "kdc" | "mrw" | "raw" => Some(MediaKind::Photo),
        // Camcorder and old phone video
        "mts" | "m2ts" | "3gp" | "3g2" | "mod" | "tod" | "vob" | "dv" => Some(MediaKind::Video),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn photos_and_videos_by_mime() {
        assert_eq!(kind_from_mime("image/jpeg"), Some(MediaKind::Photo));
        assert_eq!(kind_from_mime("IMAGE/HEIF"), Some(MediaKind::Photo));
        assert_eq!(kind_from_mime("video/quicktime"), Some(MediaKind::Video));
        assert_eq!(kind_from_mime("video/x-msvideo"), Some(MediaKind::Video));
    }

    #[test]
    fn icons_documents_and_audio_are_not_media() {
        assert_eq!(kind_from_mime("image/x-icon"), None);
        assert_eq!(kind_from_mime("image/svg+xml"), None);
        assert_eq!(kind_from_mime("application/pdf"), None);
        assert_eq!(kind_from_mime("audio/mpeg"), None);
    }

    #[test]
    fn extension_fallback_covers_raw_and_camcorder_formats() {
        assert_eq!(kind_from_extension("CR2"), Some(MediaKind::Photo));
        assert_eq!(kind_from_extension(".nef"), Some(MediaKind::Photo));
        assert_eq!(kind_from_extension("MTS"), Some(MediaKind::Video));
        assert_eq!(kind_from_extension("3gp"), Some(MediaKind::Video));
        assert_eq!(kind_from_extension("txt"), None);
        assert_eq!(kind_from_extension(""), None);
    }

    #[test]
    fn kind_serialises_lowercase() {
        assert_eq!(serde_json::to_string(&MediaKind::Video).unwrap(), "\"video\"");
    }
}
