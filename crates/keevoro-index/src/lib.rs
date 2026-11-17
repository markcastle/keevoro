//! The index: one JSON file that describes a published library.
//!
//! A bucket full of photos is just a pile of files. The index is what turns
//! it into something a gallery can show: which albums exist, what is in
//! them, how big each picture is, when it was taken and where its thumbnail
//! lives. A gallery site reads this one file and can lay out a whole page
//! before a single image has loaded.
//!
//! Two formats are written:
//!
//! - **v2** ([`Index`]) is keevoro's own. Every file is listed once under a
//!   short content id and albums refer to those ids, so a photo can sit in
//!   two albums without being described twice. Paths are relative to one
//!   `baseUrl`. Its JSON Schema is in `schema/` and is checked by a test, so
//!   the schema and the code cannot drift apart.
//! - **v1** ([`GalleryV1`]) is the older `gallery.json` that the existing
//!   gallery site reads. It is produced from a v2 index so both always agree.
//!
//! This crate only builds data. It never reads a disk or a network; the
//! caller decides what goes in and where the result is written.
//!
//! Privacy note for anyone extending this: the index never carries GPS
//! positions or camera serial numbers. Old phone photos often give away a
//! home address, so location stays in the originals and nowhere else.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use keevoro_core::{DateGuess, DateSource, MediaKind};
use schemars::JsonSchema;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

pub const SCHEMA_NAME: &str = "keevoro.index/2";

/// The smaller copies made of each file, and where they live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Rendition {
    /// 400 px on the long edge, for grids.
    Thumb,
    /// 2048 px on the long edge, for viewing.
    Display,
    /// A still frame from a video.
    Poster,
    /// A video converted to MP4.
    Video,
}

/// How to find one kind of rendition: replace `{id}` in `path`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenditionRule {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_edge: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codec: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ItemType {
    Photo,
    Video,
}

/// Where an item's date came from, so a gallery can show a weak guess as
/// "Roughly 2009".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum IndexDateSource {
    Exif,
    Video,
    Filename,
    Folder,
    Mtime,
}

impl From<DateSource> for IndexDateSource {
    fn from(s: DateSource) -> Self {
        match s {
            DateSource::Exif => Self::Exif,
            DateSource::Video => Self::Video,
            DateSource::Filename => Self::Filename,
            DateSource::Folder => Self::Folder,
            DateSource::Modified => Self::Mtime,
        }
    }
}

/// One photo or video.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    #[serde(rename = "type")]
    pub kind: ItemType,
    /// Path of the original, relative to `baseUrl`, not URL-encoded.
    pub path: String,
    /// Local wall-clock time with no timezone, as precise as is known:
    /// `2009`, `2009-07`, `2009-07-14` or `2009-07-14T21:03:11`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub taken: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_source: Option<IndexDateSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub w: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub h: Option<u32>,
    /// Seconds, videos only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    pub bytes: u64,
    pub mime: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blurhash: Option<String>,
    /// The renditions that exist for this item. Always present, so a
    /// gallery never has to guess; empty when no preview could be made.
    pub renditions: Vec<Rendition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Album {
    /// A readable slug, stable while the album's folder path is.
    pub id: String,
    /// The id of the album this one sits inside.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub title: String,
    pub description: String,
    /// First and last day, `YYYY-MM-DD`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_range: Option<(String, String)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    pub items: Vec<String>,
}

/// The v2 index.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Index {
    pub schema: String,
    /// UTC, `2026-11-15T13:30:00Z`.
    pub generated_at: String,
    /// Where the files are, always ending with `/`. Either a web address
    /// (`https://photos.example.com/`), or one relative to this index
    /// (`../`, when it sits in a folder with the files) so the folder can
    /// be moved without breaking it.
    pub base_url: String,
    pub renditions: BTreeMap<Rendition, RenditionRule>,
    pub albums: Vec<Album>,
    /// Keyed by item id: the first 16 hex characters of the file's BLAKE3.
    pub items: BTreeMap<String, Item>,
}

/// What the caller knows about one published file.
#[derive(Debug, Clone)]
pub struct ItemInput {
    /// The 16-character content id.
    pub id: String,
    pub kind: MediaKind,
    /// Library-relative path with `/` between folders.
    pub path: String,
    pub date: Option<DateGuess>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub bytes: u64,
    pub mime: String,
    pub blurhash: Option<String>,
    /// Seconds, for videos that could be read.
    pub duration: Option<f64>,
    pub renditions: Vec<Rendition>,
}

/// An album as a path of titles from the outermost in, plus its item ids in
/// the order they should be shown.
#[derive(Debug, Clone)]
pub struct AlbumInput {
    pub path: Vec<String>,
    pub items: Vec<String>,
}

/// Where each rendition is stored. The library and the bucket use the same
/// layout, so these are also the paths on disk.
pub fn rendition_rules() -> BTreeMap<Rendition, RenditionRule> {
    let rule = |path: &str, max_edge, codec: Option<&str>| RenditionRule {
        path: path.to_string(),
        max_edge,
        codec: codec.map(str::to_string),
    };
    BTreeMap::from([
        (Rendition::Thumb, rule("renditions/thumb/{id}.webp", Some(400), None)),
        (
            Rendition::Display,
            rule("renditions/display/{id}.webp", Some(2048), None),
        ),
        (Rendition::Poster, rule("renditions/poster/{id}.webp", Some(2048), None)),
        (
            Rendition::Video,
            rule("renditions/video/{id}.mp4", None, Some("h264+aac")),
        ),
    ])
}

/// The library-relative path of one rendition of one item.
pub fn rendition_path(kind: Rendition, id: &str) -> String {
    rendition_rules()
        .get(&kind)
        .map(|r| r.path.replace("{id}", id))
        .unwrap_or_default()
}

/// Builds the v2 index. Albums that only exist as parents of other albums
/// are added automatically, and album ids are made unique.
pub fn build_index(base_url: &str, generated_at_unix: i64, albums: &[AlbumInput], items: Vec<ItemInput>) -> Index {
    let items: BTreeMap<String, Item> = items
        .into_iter()
        .map(|i| {
            let item = Item {
                kind: match i.kind {
                    MediaKind::Photo => ItemType::Photo,
                    MediaKind::Video => ItemType::Video,
                },
                path: i.path,
                taken: i.date.map(|d| d.date.to_string()),
                date_source: i.date.map(|d| d.source.into()),
                w: i.width,
                h: i.height,
                // Milliseconds are plenty, and keep the file tidy.
                duration: i.duration.map(|d| (d * 1000.0).round() / 1000.0),
                bytes: i.bytes,
                mime: i.mime,
                blurhash: i.blurhash,
                renditions: i.renditions,
            };
            (i.id, item)
        })
        .collect();

    // Every album path, plus every parent path, each exactly once.
    let mut by_path: BTreeMap<Vec<String>, Vec<String>> = BTreeMap::new();
    for a in albums.iter().filter(|a| !a.path.is_empty()) {
        for depth in 1..a.path.len() {
            by_path.entry(a.path[..depth].to_vec()).or_default();
        }
        let known = a.items.iter().filter(|id| items.contains_key(*id)).cloned();
        by_path.entry(a.path.clone()).or_default().extend(known);
    }

    let mut ids: BTreeMap<Vec<String>, String> = BTreeMap::new();
    let mut taken_ids = BTreeSet::new();
    for path in by_path.keys() {
        let base = slug(&path.join(" "));
        let mut id = base.clone();
        let mut n = 2;
        while !taken_ids.insert(id.clone()) {
            id = format!("{base}-{n}");
            n += 1;
        }
        ids.insert(path.clone(), id);
    }

    let albums = by_path
        .iter()
        .map(|(path, members)| {
            let days: Vec<String> = members
                .iter()
                .filter_map(|id| items.get(id))
                // A file's modified time is not a capture date.
                .filter(|i| i.date_source != Some(IndexDateSource::Mtime))
                .filter_map(|i| i.taken.as_deref())
                .filter(|t| t.len() >= 10)
                .map(|t| t[..10].to_string())
                .collect();
            Album {
                id: ids.get(path).cloned().unwrap_or_default(),
                parent: path.split_last().and_then(|(_, parent)| ids.get(parent)).cloned(),
                title: path.last().cloned().unwrap_or_default(),
                description: String::new(),
                date_range: days.iter().min().cloned().zip(days.iter().max().cloned()),
                // Prefer something a gallery can actually show.
                cover: members
                    .iter()
                    .find(|id| items.get(*id).is_some_and(|i| i.renditions.contains(&Rendition::Thumb)))
                    .or(members.first())
                    .cloned(),
                items: members.clone(),
            }
        })
        .collect();

    Index {
        schema: SCHEMA_NAME.to_string(),
        generated_at: utc_timestamp(generated_at_unix),
        base_url: if base_url.ends_with('/') {
            base_url.to_string()
        } else {
            format!("{base_url}/")
        },
        renditions: rendition_rules(),
        albums,
        items,
    }
}

/// The JSON Schema for the v2 index, pretty-printed.
pub fn schema_json() -> String {
    let schema = schemars::schema_for!(Index);
    let mut text = serde_json::to_string_pretty(&schema).unwrap_or_default();
    text.push('\n');
    text
}

/// One photo in the v1 format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PhotoV1 {
    pub filename: String,
    pub url: String,
    pub thumbnail: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumV1 {
    pub id: String,
    pub name: String,
    pub path: String,
    pub title: String,
    pub description: String,
    pub is_private: bool,
    pub photos: Vec<PhotoV1>,
}

/// The older `gallery.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GalleryV1 {
    pub albums: Vec<AlbumV1>,
}

/// Converts a v2 index to the v1 format the current gallery site reads.
/// v1 knows only photos and flat albums, so videos and empty parent albums
/// are left out, and a nested album's title spells out where it sits.
pub fn gallery_v1(index: &Index) -> GalleryV1 {
    let title_of = |album: &Album| {
        let mut parts = vec![album.title.clone()];
        let mut parent = album.parent.clone();
        while let Some(p) = parent.and_then(|id| index.albums.iter().find(|a| a.id == id)) {
            parts.push(p.title.clone());
            parent = p.parent.clone();
        }
        parts.reverse();
        parts.join(" / ")
    };
    let albums = index
        .albums
        .iter()
        .filter_map(|album| {
            let photos: Vec<PhotoV1> = album
                .items
                .iter()
                .filter_map(|id| Some((id, index.items.get(id)?)))
                .filter(|(_, item)| item.kind == ItemType::Photo)
                .map(|(id, item)| {
                    // Without a thumbnail the full picture is the only choice.
                    let thumb = if item.renditions.contains(&Rendition::Thumb) {
                        rendition_path(Rendition::Thumb, id)
                    } else {
                        item.path.clone()
                    };
                    PhotoV1 {
                        filename: item.path.rsplit('/').next().unwrap_or_default().to_string(),
                        url: format!("{}{}", index.base_url, url_encode_path(&item.path)),
                        thumbnail: format!("{}{}", index.base_url, url_encode_path(&thumb)),
                        path: item.path.clone(),
                    }
                })
                .collect();
            (!photos.is_empty()).then(|| AlbumV1 {
                id: album.id.clone(),
                name: album.id.clone(),
                path: album.id.clone(),
                title: title_of(album),
                description: album.description.clone(),
                is_private: false,
                photos,
            })
        })
        .collect();
    GalleryV1 { albums }
}

/// The storage key for an index, from 16 random bytes. The name is what
/// keeps casual visitors out, so the bytes must come from the operating
/// system's random generator, never from a clock or a counter.
pub fn index_key(random: [u8; 16]) -> String {
    const ALPHABET: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut n = u128::from_be_bytes(random);
    let mut name = String::with_capacity(22);
    // 22 base-62 characters hold all 128 bits.
    for _ in 0..22 {
        name.push(char::from(ALPHABET[usize::try_from(n % 62).unwrap_or(0)]));
        n /= 62;
    }
    format!("index/{name}.kvx")
}

/// Percent-encodes a path for use in a URL, leaving the `/` separators.
pub fn url_encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Lowercase words joined by hyphens: `Holidays Spain 2009` becomes
/// `holidays-spain-2009`.
fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-');
    if out.is_empty() {
        "album".to_string()
    } else {
        out.to_string()
    }
}

fn utc_timestamp(unix: i64) -> String {
    keevoro_core::PartialDate::from_unix_seconds(unix)
        .map_or_else(|| "1970-01-01T00:00:00Z".to_string(), |d| format!("{d}Z"))
}

#[cfg(test)]
mod tests;
