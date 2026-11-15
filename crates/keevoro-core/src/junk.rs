//! Spotting pictures that are almost certainly not memories.
//!
//! Old drives are full of app icons, game artwork and web page leftovers.
//! They are real image files, so the scanner finds them, but nobody wants
//! them in a family library. Keevoro never hides them silently: each one is
//! left unticked with a plain reason, and the user can tick it back.
//!
//! The rules lean towards keeping. A wrongly kept icon is a small annoyance;
//! a wrongly dropped photo is a lost memory. Videos are never called junk.

use crate::media::MediaKind;
use serde::{Deserialize, Serialize};

/// Photos smaller than this on their longest side are too small to be worth
/// keeping. Even a 2001 camera phone took 352 x 288.
pub const TINY_EDGE_PX: u32 = 300;

/// The largest size app icons are normally shipped at.
const MAX_ICON_EDGE_PX: u32 = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JunkReason {
    BrowserCache,
    GameTextures,
    AppIcons,
    TinyImage,
}

impl JunkReason {
    /// Wording for the UI, shown next to the unticked box.
    pub fn describe(self) -> &'static str {
        match self {
            Self::BrowserCache => "Browser cache",
            Self::GameTextures => "Game textures",
            Self::AppIcons => "Looks like app icons",
            Self::TinyImage => "Tiny images (under 300 px)",
        }
    }

    /// The stable name stored in the catalog.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BrowserCache => "browser_cache",
            Self::GameTextures => "game_textures",
            Self::AppIcons => "app_icons",
            Self::TinyImage => "tiny_image",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::BrowserCache, Self::GameTextures, Self::AppIcons, Self::TinyImage]
            .into_iter()
            .find(|r| r.as_str() == s)
    }
}

/// The facts about one file that the junk rules need.
#[derive(Debug, Clone, Copy)]
pub struct JunkInput<'a, S: AsRef<str>> {
    pub kind: MediaKind,
    /// Pixel size, when the scanner could read it.
    pub size: Option<(u32, u32)>,
    /// Folder names between the source root and the file.
    pub folders: &'a [S],
    pub file_name: &'a str,
}

const CACHE_FOLDERS: &[&str] = &[
    "cache",
    "cache2",
    "caches",
    "code cache",
    "gpucache",
    "temporary internet files",
    "inetcache",
    "webcache",
    "content.ie5",
];

const TEXTURE_FOLDERS: &[&str] = &["textures", "texture", "sprites", "spritesheets", "tilesets", "skins"];

const ICON_FOLDERS: &[&str] = &["icons", "icon", "favicons", "emoji", "emoticons", "smileys", "toolbar"];

/// Does this file look like junk, and why? `None` means keep it ticked.
pub fn junk_hint<S: AsRef<str>>(input: &JunkInput<'_, S>) -> Option<JunkReason> {
    if input.kind != MediaKind::Photo {
        return None;
    }
    let in_folder = |names: &[&str]| {
        input
            .folders
            .iter()
            .any(|f| names.contains(&f.as_ref().trim().to_lowercase().as_str()))
    };
    if in_folder(CACHE_FOLDERS) {
        return Some(JunkReason::BrowserCache);
    }
    if in_folder(TEXTURE_FOLDERS) {
        return Some(JunkReason::GameTextures);
    }

    let name = input.file_name.to_lowercase();
    let named_like_icon = name.starts_with("favicon") || name.starts_with("icon") || name.contains("_icon");
    let long_edge = input.size.map(|(w, h)| w.max(h));
    // A folder called "Icons" full of big pictures is someone's album, so an
    // icon needs to be icon-sized as well. Unknown size counts as small here
    // only when the name itself says icon.
    let icon_sized = match input.size {
        Some((w, h)) => w == h && w <= MAX_ICON_EDGE_PX,
        None => false,
    };
    if (in_folder(ICON_FOLDERS) || named_like_icon) && (icon_sized || long_edge.is_some_and(|e| e < TINY_EDGE_PX)) {
        return Some(JunkReason::AppIcons);
    }
    if long_edge.is_some_and(|e| e < TINY_EDGE_PX) {
        return Some(JunkReason::TinyImage);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hint(folders: &[&str], name: &str, size: Option<(u32, u32)>) -> Option<JunkReason> {
        junk_hint(&JunkInput {
            kind: MediaKind::Photo,
            size,
            folders,
            file_name: name,
        })
    }

    #[test]
    fn ordinary_photos_are_kept() {
        assert_eq!(
            hint(&["Holidays", "Spain 2009"], "IMG_1234.jpg", Some((3264, 2448))),
            None
        );
        assert_eq!(hint(&[], "old phone.jpg", Some((352, 288))), None);
    }

    #[test]
    fn unknown_size_is_never_called_tiny() {
        assert_eq!(hint(&["Family"], "scan.tif", None), None);
    }

    #[test]
    fn tiny_images_are_junk() {
        assert_eq!(
            hint(&["Family"], "thumb.jpg", Some((160, 120))),
            Some(JunkReason::TinyImage)
        );
        assert_eq!(
            hint(&["Family"], "edge.jpg", Some((299, 200))),
            Some(JunkReason::TinyImage)
        );
        assert_eq!(hint(&["Family"], "edge.jpg", Some((300, 200))), None);
    }

    #[test]
    fn cache_and_texture_folders_are_junk_at_any_size() {
        assert_eq!(
            hint(&["Firefox", "Cache2", "entries"], "A1B2", Some((1920, 1080))),
            Some(JunkReason::BrowserCache)
        );
        assert_eq!(
            hint(&["Games", "Quake", "Textures"], "wall.png", Some((1024, 1024))),
            Some(JunkReason::GameTextures)
        );
    }

    #[test]
    fn icons_need_to_be_icon_sized() {
        assert_eq!(
            hint(&["app", "icons"], "save.png", Some((48, 48))),
            Some(JunkReason::AppIcons)
        );
        assert_eq!(
            hint(&["site"], "favicon.png", Some((32, 32))),
            Some(JunkReason::AppIcons)
        );
        assert_eq!(
            hint(&["site"], "app_icon.png", Some((256, 256))),
            Some(JunkReason::AppIcons)
        );
        // A big photo in a folder the user happened to call "Icons" is kept.
        assert_eq!(hint(&["Icons"], "IMG_0001.jpg", Some((4000, 3000))), None);
        // So is a file called "icon" when its size is unknown.
        assert_eq!(hint(&[], "iconic sunset.jpg", None), None);
    }

    #[test]
    fn videos_are_never_junk() {
        let v = junk_hint(&JunkInput {
            kind: MediaKind::Video,
            size: Some((160, 120)),
            folders: &["cache"],
            file_name: "clip.3gp",
        });
        assert_eq!(v, None);
    }

    #[test]
    fn reasons_round_trip_and_read_plainly() {
        for r in [
            JunkReason::BrowserCache,
            JunkReason::GameTextures,
            JunkReason::AppIcons,
            JunkReason::TinyImage,
        ] {
            assert_eq!(JunkReason::parse(r.as_str()), Some(r));
            assert!(!r.describe().is_empty());
        }
        assert_eq!(JunkReason::parse("nonsense"), None);
    }
}
