//! The Discover view: everything found so far, shaped for showing.
//!
//! The Discover screen is the heart of the app: big photos grouped into
//! collections a person recognises ("Cornwall, August 2008"), each with one
//! button to keep it. This module turns the catalog into exactly the data
//! that screen draws, so the screen itself contains no decisions.
//!
//! A collection is identified by its [`CollectionView::key`], made from its
//! album path. That is what the "Keep collection" button sends back.
//!
//! Other files a person asked Keevoro to carry along come last, one
//! collection per folder they came from ([`CollectionView::by_folder`]),
//! so they are kept or left out a folder at a time with the same buttons,
//! and counted, sent and proved by exactly the same rules as photos.

use crate::{summary, wanted};
use anyhow::{bail, Result};
use keevoro_catalog::Catalog;
use keevoro_core::MediaKind;
use keevoro_index::{rendition_path, Rendition};
use keevoro_scan::FoundItem;
use serde::Serialize;
use std::path::Path;

/// How many pictures to offer for a collection's collage.
const PREVIEWS_PER_COLLECTION: usize = 5;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceView {
    pub id: i64,
    /// The friendly name, like "Dad's old Dell".
    pub name: String,
    pub root: String,
    pub items: u64,
    /// The size of everything found on it, in bytes.
    pub bytes: u64,
    /// Files there that could be photos or videos but were only in online
    /// storage when it was last looked through, so were not opened.
    #[serde(skip_serializing_if = "is_zero")]
    pub online_only: u64,
    /// The same in words, with how to include them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub online_only_said: Option<String>,
    /// How big those files are together, as their listings said; left out
    /// when not known (looked through by an older version).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub online_only_bytes: Option<u64>,
    /// Where it is, in words, when it is cloud storage: "the Amazon S3
    /// bucket family-photos". Left out for a folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bucket: Option<String>,
    /// Folders left out of this backup that are in it, or that it is in,
    /// as a person would write them, each with Put back beside it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub left_out: Vec<String>,
    /// Its own way of arranging its photos and videos, when it has one;
    /// left out when it follows the backup.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arrange: Option<keevoro_core::Arrange>,
    /// The folder its files are kept in at the top of the backup, when it
    /// keeps its folders as they are.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kept_as: Option<String>,
    /// Other sources whose folders go into that same folder, as a person
    /// would write them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub shares_folder_with: Vec<String>,
    /// The same in words: "In the backup, its folder Photos is shared with
    /// E:\Photos."
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_said: Option<String>,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_none_of_them(n: &usize) -> bool {
    *n == 0
}

/// A path as a person would write it. Windows gives long paths back as
/// `\\?\C:\Users\...`, which means nothing to most people.
pub fn friendly_path(path: &Path) -> String {
    let shown = path.display().to_string();
    if let Some(share) = shown.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{share}")
    } else if let Some(plain) = shown.strip_prefix(r"\\?\") {
        plain.to_string()
    } else {
        shown
    }
}

/// Stops Keevoro remembering a source: what was found there leaves the list.
///
/// Nothing on the source is touched, and photos already gathered stay in
/// the library. Returns false when there was no such source.
pub fn forget_source(catalog: &mut Catalog, id: i64) -> Result<bool> {
    Ok(catalog.forget_source(id)?)
}

/// One photo or video, with enough for the app to show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhotoView {
    /// The short content id.
    pub id: String,
    pub is_video: bool,
    /// An other file: shown as its name and kind, never as a picture.
    #[serde(skip_serializing_if = "is_false")]
    pub is_other: bool,
    pub name: String,
    /// Where it is on its source. The app asks for a preview by `id`,
    /// never by this path; it is here for showing, not for reading.
    pub source_path: String,
    /// The thumbnail a backup made of it, relative to the backup's own
    /// folder. `None` before then.
    pub thumbnail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionView {
    pub key: String,
    /// "Cornwall", "August 2008" or "Undated".
    pub title: String,
    /// "August 2008 · 83 photos · 6 videos".
    pub summary: String,
    /// Outer album names, for a quiet line like "Holidays".
    pub parents: Vec<String>,
    pub photos: usize,
    pub videos: usize,
    /// Other files in it. Only a folder of other files has any.
    #[serde(skip_serializing_if = "is_none_of_them")]
    pub others: usize,
    /// A folder of other files, listed under "Other files" by the folder
    /// it came from rather than shown as photos.
    #[serde(skip_serializing_if = "is_false")]
    pub by_folder: bool,
    /// A folder of photos and videos from a source that keeps its folders
    /// as they are: shown as it will be in the backup, in folder order.
    #[serde(skip_serializing_if = "is_false")]
    pub kept_folders: bool,
    /// The room its files take, for a folder of other files.
    #[serde(skip_serializing_if = "is_zero")]
    pub bytes: u64,
    /// False when the user has skipped every file in it.
    pub kept: bool,
    /// How many of its files are safely backed up already.
    pub gathered: usize,
    pub cover: PhotoView,
    /// The first few files, cover first, for a collage.
    pub previews: Vec<PhotoView>,
}

/// One place a backup goes to, and how much of what is being kept has
/// reached it. A backup can go to several; each is counted on its own, so
/// a second copy that is only half made is never mistaken for a whole one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DestinationView {
    /// In words: "the folder F:\\Family Backup".
    pub name: String,
    /// `folder`, `r2`, `s3` or `endpoint`.
    pub kind: String,
    /// The folder, when it is one, for opening.
    pub folder: Option<String>,
    /// Kept files that are there.
    pub there: usize,
    /// Kept files still to go, and the room they need.
    pub waiting: usize,
    pub waiting_bytes: u64,
    /// What Keevoro found when it last looked there, and how sure it can
    /// be now (see [`crate::confirm`]). `None` before it has looked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub looked: Option<crate::confirm::LookedView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Discover {
    pub photos: usize,
    pub videos: usize,
    /// Other files a person asked Keevoro to carry along. Never said when
    /// there are none.
    #[serde(skip_serializing_if = "is_none_of_them")]
    pub others: usize,
    pub bytes: u64,
    /// Extra copies that will be kept once.
    pub duplicates: usize,
    /// How much room those extra copies would have taken, in bytes: what
    /// leaving them out saves at the destination.
    pub duplicate_bytes: u64,
    /// Pictures set aside as probably not memories.
    pub junk: usize,
    /// Files being kept that are not yet safely at every destination.
    pub waiting_to_gather: usize,
    /// How much room those waiting files will need where they are sent,
    /// in bytes.
    pub waiting_bytes: u64,
    pub sources: Vec<SourceView>,
    /// Each place this backup goes to, with what has reached it.
    pub destinations: Vec<DestinationView>,
    /// Newest first; the first is the one to show large.
    pub collections: Vec<CollectionView>,
}

/// The key each collection is known by: its album path, joined. Two
/// collections can share a path (two moments in a month known only to the
/// month), and a key must name exactly one, so a repeat is numbered:
/// "October 2004", then "October 2004 (2)". The order collections come in
/// is fixed, so the same key always means the same collection.
fn keys_of(collections: &[keevoro_core::collections::Collection]) -> Vec<String> {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    collections
        .iter()
        .map(|c| {
            // A folder of other files, or one kept as it is, is never taken
            // for an album of the same name.
            let first = if c.by_folder {
                Some(keevoro_core::OTHER_FILES)
            } else {
                c.kept_folders.then_some(KEPT_AS_THEY_ARE)
            };
            let plain = first
                .into_iter()
                .chain(c.parents.iter().map(String::as_str))
                .chain([c.title.as_str()])
                .collect::<Vec<_>>()
                .join(" / ");
            let times = seen.entry(plain.clone()).or_insert(0);
            *times += 1;
            if *times == 1 {
                plain
            } else {
                format!("{plain} ({times})")
            }
        })
        .collect()
}

/// What the key of a folder kept as it is starts with.
pub const KEPT_AS_THEY_ARE: &str = "Kept as they are";

/// The collection with this key, if it is still there.
fn with_key<'a>(
    collections: &'a [keevoro_core::collections::Collection],
    key: &str,
) -> Option<&'a keevoro_core::collections::Collection> {
    keys_of(collections)
        .iter()
        .position(|k| k == key)
        .and_then(|at| collections.get(at))
}

fn photo_view(library: &Path, catalog: &Catalog, item: &FoundItem) -> Result<PhotoView> {
    let hash = item.blake3.as_deref().unwrap_or_default();
    let id = hash[..hash.len().min(16)].to_string();
    let is_video = item.kind == MediaKind::Video;
    let is_other = item.kind == MediaKind::Other;
    // A thumbnail exists once a backup has made the file's previews.
    let kind = if is_video { Rendition::Poster } else { Rendition::Thumb };
    let relative = rendition_path(kind, &id);
    let on_disk = relative.split('/').fold(library.to_path_buf(), |p, s| p.join(s));
    let thumbnail = (!is_other && !id.is_empty() && catalog.collected_path(hash)?.is_some() && on_disk.is_file())
        .then_some(relative);
    Ok(PhotoView {
        id,
        is_video,
        is_other,
        name: item
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        source_path: friendly_path(&item.path),
        thumbnail,
    })
}

/// Where this backup goes, and whether a file has reached each place: the
/// one rule for what is safely backed up, used by the Discover view, what
/// is waiting for each destination, and the map's "backed up" figures.
struct Reach<'a> {
    catalog: &'a Catalog,
    places: Vec<crate::connect::Saved>,
}

impl<'a> Reach<'a> {
    fn of(catalog: &'a Catalog) -> Result<Self> {
        Ok(Reach {
            catalog,
            places: crate::connect::destinations(catalog)?,
        })
    }

    /// Whether `item` is safely backed up, and whether it has reached each
    /// destination, in order. Safe means a copy is everywhere it is meant to
    /// be: at every destination of the backup. At some but not all is not
    /// safe yet; that is the whole point of having more than one. With no
    /// destinations yet, a copy gathered anywhere counts.
    fn safe(&self, item: &FoundItem) -> Result<(bool, Vec<bool>)> {
        let catalog = self.catalog;
        let place = match &item.blake3 {
            Some(hash) => catalog.collected_path(hash)?,
            None => None,
        };
        // A file has reached a destination when the catalog has a record of
        // it under that destination's name. One saved before names were
        // remembered is believed if the file is recorded anywhere, which is
        // what was true when there could only be one.
        let mut there = Vec::with_capacity(self.places.len());
        for to in &self.places {
            there.push(match (&place, &to.target) {
                (Some(place), Some(target)) => catalog.is_uploaded(target, place)?,
                (Some(place), None) => catalog.stored_anywhere(place)?,
                (None, _) => false,
            });
        }
        let everywhere = !there.is_empty() && there.iter().all(|t| *t);
        let is_in = match &place {
            Some(place) => everywhere || (self.places.is_empty() && catalog.stored_anywhere(place)?),
            None => false,
        };
        Ok((is_in, there))
    }
}

/// How many of the files this backup keeps are in each folder, and how many
/// of those are safely backed up, by the same rule the Discover view and
/// what is waiting use: `(safe, kept)` by the folder each file is directly
/// in, as its full path. Worked out when asked, never stored.
pub fn safe_by_folder(catalog: &Catalog) -> Result<std::collections::HashMap<std::path::PathBuf, (usize, usize)>> {
    let reports = catalog.load_reports()?;
    let rescued = catalog.kept_hashes()?;
    let (summary, items) = summary::summarise_with(&reports, &rescued);
    let reach = Reach::of(catalog)?;
    let mut by_folder: std::collections::HashMap<std::path::PathBuf, (usize, usize)> = std::collections::HashMap::new();
    for c in &summary.collections {
        for item in c.members.iter().filter_map(|&m| items.get(m).copied()) {
            if !wanted(catalog, item)? {
                continue;
            }
            let Some(folder) = item.path.parent() else { continue };
            let (is_in, _) = reach.safe(item)?;
            let counts = by_folder.entry(folder.to_path_buf()).or_default();
            counts.0 += usize::from(is_in);
            counts.1 += 1;
        }
    }
    Ok(by_folder)
}

/// Where each source's files go at the top of the backup when it keeps its
/// folders as they are, which other sources share that folder (as a person
/// would write them), and that in words. Nothing is read but the record.
pub fn kept_as(
    sources: &[keevoro_catalog::Source],
    choices: &keevoro_core::Choices,
) -> Vec<(Option<String>, Vec<String>, Option<String>)> {
    let arranged: Vec<keevoro_core::arrange::SourceArranged<'_>> = sources
        .iter()
        .map(|s| keevoro_core::arrange::SourceArranged {
            root: &s.root,
            name: &s.name,
            arrange: choices.arrangement_in(&s.root),
        })
        .collect();
    let sharing = keevoro_core::arrange::sharing_a_folder(&arranged);
    arranged
        .iter()
        .zip(&sharing)
        .map(|(s, with)| {
            let top = (s.arrange == keevoro_core::Arrange::Folders)
                .then(|| keevoro_core::arrange::top_folder(s.root, s.name));
            let with: Vec<String> = with.iter().map(|&j| friendly_path(arranged[j].root)).collect();
            let said = top
                .as_deref()
                .and_then(|top| keevoro_core::arrange::shared_words(top, &with));
            (top, with, said)
        })
        .collect()
}

/// Builds the Discover view from everything the backup remembers.
pub fn discover(library: &Path, catalog: &Catalog) -> Result<Discover> {
    let reports = catalog.load_reports()?;
    let rescued = catalog.kept_hashes()?;
    let (summary, items) = summary::summarise_with(&reports, &rescued);

    let reach = Reach::of(catalog)?;
    let places = &reach.places;
    let mut tallies = vec![(0usize, 0usize, 0u64); places.len()];

    let mut waiting_to_gather = 0;
    let mut waiting_bytes = 0;
    let mut collections = Vec::new();
    let keys = keys_of(&summary.collections);
    for (at, c) in summary.collections.iter().enumerate() {
        let members: Vec<&FoundItem> = c.members.iter().filter_map(|&m| items.get(m).copied()).collect();
        let mut kept = false;
        let mut gathered = 0;
        for item in &members {
            let is_wanted = wanted(catalog, item)?;
            kept |= is_wanted;
            let (is_in, there) = reach.safe(item)?;
            if is_wanted {
                for (there, tally) in there.into_iter().zip(tallies.iter_mut()) {
                    if there {
                        tally.0 += 1;
                    } else {
                        tally.1 += 1;
                        tally.2 += item.bytes;
                    }
                }
            }
            gathered += usize::from(is_in);
            waiting_to_gather += usize::from(is_wanted && !is_in);
            if is_wanted && !is_in {
                waiting_bytes += item.bytes;
            }
        }
        let Some(cover) = items.get(c.cover).or(members.first()) else {
            continue;
        };
        let cover = photo_view(library, catalog, cover)?;
        let mut previews = vec![cover.clone()];
        for item in &members {
            if previews.len() >= PREVIEWS_PER_COLLECTION {
                break;
            }
            let view = photo_view(library, catalog, item)?;
            if view.id != cover.id {
                previews.push(view);
            }
        }
        collections.push(CollectionView {
            key: keys[at].clone(),
            title: c.title.clone(),
            summary: c.summary(),
            parents: c.parents.clone(),
            photos: c.photos,
            videos: c.videos,
            others: c.others,
            by_folder: c.by_folder,
            kept_folders: c.kept_folders,
            bytes: if c.by_folder {
                members.iter().map(|i| i.bytes).sum()
            } else {
                0
            },
            kept,
            gathered,
            cover,
            previews,
        });
    }

    let left_out = catalog.left_out_folders()?;
    let found = catalog.sources()?;
    let choices = catalog.choices()?;
    let kept_as = kept_as(&found, &choices);
    let sources = found
        .iter()
        .cloned()
        .zip(kept_as)
        .map(|(s, (kept_as, shares_folder_with, shared_said))| SourceView {
            arrange: choices
                .sources
                .iter()
                .find(|c| keevoro_core::places::same_place(&c.root, &s.root))
                .and_then(|c| c.arrange),
            kept_as,
            shares_folder_with,
            shared_said,
            left_out: left_out
                .iter()
                .filter(|f| keevoro_core::places::within(f, &s.root) || keevoro_core::places::within(&s.root, f))
                .map(|f| friendly_path(f))
                .collect(),
            id: s.id,
            name: s.name,
            root: friendly_path(&s.root),
            items: s.items,
            bytes: s.bytes,
            online_only: s.online_only,
            online_only_said: crate::summary::online_only(
                usize::try_from(s.online_only).unwrap_or(usize::MAX),
                s.online_only_bytes,
                &s.root,
            ),
            online_only_bytes: s.online_only_bytes,
            bucket: crate::bucket::place_of(s.bucket.as_deref()).map(|p| p.describe()),
        })
        .collect();
    Ok(Discover {
        photos: summary.photos,
        videos: summary.videos,
        others: summary.others,
        bytes: summary.bytes,
        duplicates: summary.duplicates.as_ref().map_or(0, |d| d.copies),
        duplicate_bytes: summary.duplicates.as_ref().map_or(0, |d| d.bytes),
        junk: summary.junk.values().sum(),
        waiting_to_gather,
        waiting_bytes,
        sources,
        destinations: {
            let now = crate::confirm::now();
            let mut views = Vec::with_capacity(places.len());
            for (to, (there, waiting, bytes)) in places.iter().zip(tallies) {
                views.push(DestinationView {
                    name: to.describe(),
                    kind: to.kind.clone(),
                    folder: (to.kind == "folder").then(|| to.place.clone()),
                    there,
                    waiting,
                    waiting_bytes: bytes,
                    looked: crate::confirm::as_it_stands(catalog, to, now)?,
                });
            }
            views
        },
        collections,
    })
}

/// Keeps or skips every file in the collection with this key. Returns how
/// many files the choice was recorded for.
pub fn set_collection_kept(catalog: &Catalog, key: &str, keep: bool) -> Result<usize> {
    let reports = catalog.load_reports()?;
    let rescued = catalog.kept_hashes()?;
    let (summary, items) = summary::summarise_with(&reports, &rescued);
    let Some(collection) = with_key(&summary.collections, key) else {
        bail!("That collection isn't there any more. It may have changed since the screen was drawn.");
    };
    let mut changed = 0;
    for hash in collection
        .members
        .iter()
        .filter_map(|&m| items.get(m)?.blake3.as_deref())
    {
        catalog.set_choice(hash, keep)?;
        changed += 1;
    }
    Ok(changed)
}

/// One file inside a collection, for the screen where single photos are
/// ticked and unticked.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhotoChoice {
    #[serde(flatten)]
    pub photo: PhotoView,
    /// When it was taken, as precisely as is known, or `None`. Never for
    /// an other file, which has no such date.
    pub taken: Option<String>,
    /// For an other file: the day it last changed, as a file manager says
    /// it, and its size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    pub kept: bool,
    /// Why it was set aside, for pictures in the junk list.
    pub junk_reason: Option<String>,
}

fn choice(library: &Path, catalog: &Catalog, item: &FoundItem) -> Result<PhotoChoice> {
    let other = item.kind == MediaKind::Other;
    Ok(PhotoChoice {
        photo: photo_view(library, catalog, item)?,
        taken: item.date.filter(|_| !other).map(|d| d.date.to_string()),
        changed: item.date.filter(|_| other).map(|d| d.date.to_string()),
        bytes: other.then_some(item.bytes),
        kept: wanted(catalog, item)?,
        junk_reason: item.junk.map(|r| r.describe().to_string()),
    })
}

/// Every file in the collection with this key, in date order.
pub fn collection_photos(library: &Path, catalog: &Catalog, key: &str) -> Result<Vec<PhotoChoice>> {
    let reports = catalog.load_reports()?;
    let rescued = catalog.kept_hashes()?;
    let (summary, items) = summary::summarise_with(&reports, &rescued);
    let Some(collection) = with_key(&summary.collections, key) else {
        bail!("That collection isn't there any more. It may have changed since the screen was drawn.");
    };
    collection
        .members
        .iter()
        .filter_map(|&m| items.get(m))
        .map(|item| choice(library, catalog, item))
        .collect()
}

/// The pictures set aside as probably not memories, each with its reason.
/// One the user has since chosen to keep is no longer listed.
pub fn set_aside(library: &Path, catalog: &Catalog) -> Result<Vec<PhotoChoice>> {
    let reports = catalog.load_reports()?;
    let rescued = catalog.kept_hashes()?;
    let (everything, _) = summary::unique_items(&reports);
    everything
        .into_iter()
        .filter(|i| i.junk.is_some() && !i.blake3.as_ref().is_some_and(|h| rescued.contains(h)))
        .map(|item| choice(library, catalog, item))
        .collect()
}

/// One copy of a file that will not be sent, because another copy of the
/// very same file is being kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyView {
    pub name: String,
    /// The folder it is in, for showing and for opening.
    pub folder: String,
}

/// A file found more than once: the copy being kept, and the others.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroup {
    /// The copy that is kept and sent.
    #[serde(flatten)]
    pub photo: PhotoView,
    /// The folder the kept copy is in.
    pub folder: String,
    /// The size of each copy. They are all the same size.
    pub bytes: u64,
    /// True once every copy has been read from start to finish and found
    /// identical. False while that is still being checked, when the copies
    /// only look the same so far; nothing is left out on that alone.
    pub confirmed: bool,
    pub copies: Vec<CopyView>,
}

fn folder_of(item: &FoundItem) -> String {
    item.path.parent().map(friendly_path).unwrap_or_default()
}

/// Every file found more than once, so a person can see for themselves
/// which copy is kept and which are left out before anything is sent.
pub fn duplicates(library: &Path, catalog: &Catalog) -> Result<Vec<DuplicateGroup>> {
    let reports = catalog.load_reports()?;
    summary::duplicate_groups(&reports)
        .into_iter()
        .map(|(kept, others)| {
            Ok(DuplicateGroup {
                photo: photo_view(library, catalog, kept)?,
                folder: folder_of(kept),
                bytes: kept.bytes,
                confirmed: kept.hashed && others.iter().all(|o| o.hashed),
                copies: others
                    .iter()
                    .map(|o| CopyView {
                        name: o
                            .path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default(),
                        folder: folder_of(o),
                    })
                    .collect(),
            })
        })
        .collect()
}

/// Keeps or skips the one file whose content id starts with `id`.
pub fn set_photo_kept(catalog: &Catalog, id: &str, keep: bool) -> Result<()> {
    let Some((_, _, hash)) = catalog.find_item(id)? else {
        bail!("That photo isn't known to this backup.");
    };
    catalog.set_choice(&hash, keep)?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::scan_into;
    use std::fs;

    pub(crate) fn real_jpeg(shade: u8) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(640, 480, image::Rgb([shade, 120, 60]));
        let mut bytes = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
            .unwrap();
        bytes
    }

    /// A drive with one album, one loose dated photo and a duplicate.
    fn world() -> (tempfile::TempDir, std::path::PathBuf, Catalog) {
        let dir = tempfile::tempdir().unwrap();
        let drive = dir.path().join("Dads old Dell");
        for (rel, shade) in [
            ("Holidays/Cornwall/IMG_20080812_101500.jpg", 10),
            ("Holidays/Cornwall/IMG_20080813_090000.jpg", 90),
            ("DCIM/100CANON/IMG_20120304_153012.jpg", 170),
            ("Backup/IMG_20080812_101500.jpg", 10),
        ] {
            let p = drive.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, real_jpeg(shade)).unwrap();
        }
        let library = dir.path().join("Keevoro Library");
        let mut catalog = Catalog::open_in_library(&library).unwrap();
        let mut ticks = 0;
        scan_into(&mut catalog, &drive, Some("Dad's old Dell"), &mut |_| ticks += 1).unwrap();
        assert_eq!(ticks, 4);
        (dir, library, catalog)
    }

    #[test]
    fn forgetting_a_source_never_touches_its_files() {
        let (dir, library, mut catalog) = world();
        let drive = dir.path().join("Dads old Dell");
        let id = discover(&library, &catalog).unwrap().sources[0].id;

        assert!(forget_source(&mut catalog, id).unwrap());
        let after = discover(&library, &catalog).unwrap();
        assert!(after.sources.is_empty());
        assert_eq!((after.photos, after.collections.len()), (0, 0));
        for rel in [
            "Holidays/Cornwall/IMG_20080812_101500.jpg",
            "Holidays/Cornwall/IMG_20080813_090000.jpg",
            "DCIM/100CANON/IMG_20120304_153012.jpg",
            "Backup/IMG_20080812_101500.jpg",
        ] {
            assert!(drive.join(rel).is_file(), "{rel} must still be on the source");
        }
        // Asking twice is harmless.
        assert!(!forget_source(&mut catalog, id).unwrap());
    }

    /// Says a file is only online when its name says so, as Windows'
    /// attributes or a Mac's flags would.
    #[derive(Debug)]
    struct NamedOnline;

    impl keevoro_scan::OnlineCheck for NamedOnline {
        fn online_only(&self, file: &Path, _: &std::fs::Metadata) -> bool {
            file.to_string_lossy().contains("(online)")
        }
    }

    #[test]
    fn a_source_in_onedrive_says_how_many_files_and_how_much_are_only_online_and_how_to_include_them() {
        let dir = tempfile::tempdir().unwrap();
        let onedrive = dir.path().join("OneDrive");
        fs::create_dir_all(onedrive.join("Pictures")).unwrap();
        fs::write(onedrive.join("Pictures/here.jpg"), real_jpeg(5)).unwrap();
        fs::write(onedrive.join("Pictures/away (online).jpg"), vec![0u8; 3_000]).unwrap();
        fs::write(onedrive.join("Pictures/clip (online).mp4"), vec![0u8; 7_000]).unwrap();
        let library = dir.path().join("Keevoro Library");
        let mut catalog = Catalog::open_in_library(&library).unwrap();
        let options = keevoro_scan::ScanOptions {
            online: std::sync::Arc::new(NamedOnline),
            ..Default::default()
        };
        crate::scan_into_with(&mut catalog, &onedrive, None, &options, &mut |_| {}, &mut || {}).unwrap();
        let source = &discover(&library, &catalog).unwrap().sources[0];
        assert_eq!(
            (source.items, source.online_only, source.online_only_bytes),
            (1, 2, Some(10_000))
        );
        assert_eq!(
            source.online_only_said.as_deref(),
            Some(
                "2 files, 9.8 KB, are in OneDrive only, not on this computer, so Keevoro did not open them. To \
                 include them, make them available offline in OneDrive first, then look again."
            )
        );
        // Kept for a later step that fetches them: each, with its size.
        let files = catalog.online_only_files(source.id).unwrap();
        assert_eq!(files.iter().map(|(_, b)| b).sum::<u64>(), 10_000);
    }

    #[test]
    fn two_collections_with_one_name_still_have_a_key_each() {
        let named = |title: &str, cover: usize| keevoro_core::collections::Collection {
            title: title.to_string(),
            parents: Vec::new(),
            first: None,
            last: None,
            photos: 1,
            videos: 0,
            others: 0,
            by_folder: false,
            kept_folders: false,
            cover,
            members: vec![cover],
        };
        let all = [named("October 2004", 0), named("Cornwall", 1), named("October 2004", 2)];
        assert_eq!(keys_of(&all), ["October 2004", "Cornwall", "October 2004 (2)"]);
        // Each key opens its own collection, never its namesake.
        assert_eq!(with_key(&all, "October 2004").unwrap().cover, 0);
        assert_eq!(with_key(&all, "October 2004 (2)").unwrap().cover, 2);
        assert!(with_key(&all, "Gone").is_none());
    }

    #[test]
    fn windows_long_paths_are_shown_the_ordinary_way() {
        let shown = |p: &str| friendly_path(Path::new(p));
        assert_eq!(shown(r"\\?\C:\Users\Mark\Pictures"), r"C:\Users\Mark\Pictures");
        assert_eq!(shown(r"\\?\UNC\nas\photos"), r"\\nas\photos");
        assert_eq!(shown("/Volumes/KINGSTON"), "/Volumes/KINGSTON");
    }

    #[test]
    fn discover_describes_what_was_found_in_screen_ready_form() {
        let (_dir, library, catalog) = world();
        let d = discover(&library, &catalog).unwrap();
        assert_eq!((d.photos, d.videos, d.duplicates, d.junk), (3, 0, 1, 0));
        assert!(
            d.duplicate_bytes > 0,
            "and the room the duplicate would have taken is known"
        );
        // The duplicate can be looked at: which copy is kept, which is not.
        let groups = duplicates(&library, &catalog).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].copies.len(), 1);
        assert!(groups[0].confirmed, "a full search has read every byte");
        assert_ne!(
            (&groups[0].photo.name, &groups[0].folder),
            (&groups[0].copies[0].name, &groups[0].copies[0].folder),
            "the two copies are different files on the source"
        );
        assert_eq!(d.waiting_to_gather, 3);
        // The room they need is the size of the three files kept, the
        // duplicate counted once.
        let kept: u64 = [10, 90, 170].iter().map(|s| real_jpeg(*s).len() as u64).sum();
        assert_eq!(d.waiting_bytes, kept);
        assert_eq!(d.sources.len(), 1);
        assert_eq!((d.sources[0].name.as_str(), d.sources[0].items), ("Dad's old Dell", 4));
        // Its size is every file found on it, the duplicate included.
        let all: u64 = [10, 90, 170, 10].iter().map(|s| real_jpeg(*s).len() as u64).sum();
        assert_eq!(d.sources[0].bytes, all);

        let titles: Vec<&str> = d.collections.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, ["March 2012", "Cornwall"]);
        let cornwall = &d.collections[1];
        assert_eq!(cornwall.key, "Holidays / Cornwall");
        assert_eq!(cornwall.summary, "August 2008 \u{b7} 2 photos");
        assert_eq!((cornwall.photos, cornwall.kept, cornwall.gathered), (2, true, 0));
        assert_eq!(cornwall.previews.len(), 2);
        assert_eq!(cornwall.previews[0], cornwall.cover, "the cover leads the collage");
        assert_eq!(cornwall.cover.id.len(), 16);
        assert_eq!(cornwall.cover.thumbnail, None, "nothing is gathered yet");
        assert!(
            cornwall.cover.source_path.contains("Cornwall"),
            "the album copy, not the backup one"
        );

        // It is what the app receives: camelCase JSON.
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["waitingToGather"], 3);
        assert_eq!(json["collections"][1]["cover"]["isVideo"], false);
    }

    #[test]
    fn keeping_and_skipping_a_collection_is_one_call() {
        let (_dir, library, catalog) = world();
        assert_eq!(set_collection_kept(&catalog, "Holidays / Cornwall", false).unwrap(), 2);
        let d = discover(&library, &catalog).unwrap();
        assert!(!d.collections[1].kept);
        assert!(d.collections[0].kept, "other collections are untouched");
        assert_eq!(d.waiting_to_gather, 1);

        set_collection_kept(&catalog, "Holidays / Cornwall", true).unwrap();
        assert_eq!(discover(&library, &catalog).unwrap().waiting_to_gather, 3);
        let err = set_collection_kept(&catalog, "Narnia", true).unwrap_err();
        assert!(err.to_string().starts_with("That collection isn't there any more."));
    }

    #[test]
    fn previews_made_for_a_backup_fill_in_thumbnails() {
        let (_dir, library, catalog) = world();
        set_collection_kept(&catalog, "March 2012", false).unwrap();
        let mut steps = Vec::new();
        crate::check_and_place(&library, &catalog, &|| true, &mut |s| steps.push(s)).unwrap();
        crate::make_previews(&library, &catalog, None, &|| true, &mut |s| steps.push(s)).unwrap();
        assert!(steps.contains(&crate::ReadyStep::Placing { done: 0, total: 2 }));
        assert!(steps.contains(&crate::ReadyStep::MakingPreviews { done: 2, total: 2 }));

        let d = discover(&library, &catalog).unwrap();
        assert_eq!(d.waiting_to_gather, 2, "made ready is not backed up");
        let cornwall = &d.collections[1];
        let thumb = cornwall.cover.thumbnail.as_deref().unwrap();
        assert_eq!(thumb, format!("renditions/thumb/{}.webp", cornwall.cover.id));
        assert!(library.join(thumb).is_file());
        assert!(!library.join("originals").exists(), "no copy of anyone's photos");
    }

    #[test]
    fn single_photos_in_a_collection_can_be_listed_and_chosen() {
        let (_dir, library, catalog) = world();
        let photos = collection_photos(&library, &catalog, "Holidays / Cornwall").unwrap();
        assert_eq!(photos.len(), 2);
        assert!(photos.iter().all(|p| p.kept && p.junk_reason.is_none()));
        assert_eq!(photos[0].taken.as_deref(), Some("2008-08-12T10:15:00"), "in date order");
        let json = serde_json::to_value(&photos[0]).unwrap();
        assert_eq!(
            json["name"], "IMG_20080812_101500.jpg",
            "the photo's own fields sit alongside"
        );
        assert_eq!(json["kept"], true);

        set_photo_kept(&catalog, &photos[1].photo.id, false).unwrap();
        let after = collection_photos(&library, &catalog, "Holidays / Cornwall").unwrap();
        assert_eq!((after[0].kept, after[1].kept), (true, false));
        let d = discover(&library, &catalog).unwrap();
        assert!(d.collections[1].kept, "one photo skipped does not skip the collection");
        assert_eq!(d.waiting_to_gather, 2);

        assert!(collection_photos(&library, &catalog, "Narnia").is_err());
        let err = set_photo_kept(&catalog, "0000", true).unwrap_err();
        assert_eq!(err.to_string(), "That photo isn't known to this backup.");
    }

    #[test]
    fn pictures_set_aside_are_listed_with_reasons_and_can_be_kept() {
        let dir = tempfile::tempdir().unwrap();
        let drive = dir.path().join("drive");
        fs::create_dir_all(drive.join("Grandad")).unwrap();
        fs::write(drive.join("Grandad/portrait.jpg"), real_jpeg(30)).unwrap();
        let tiny = image::RgbImage::from_pixel(240, 180, image::Rgb([90, 90, 90]));
        tiny.save(drive.join("Grandad/1952.png")).unwrap();
        let library = dir.path().join("library");
        let mut catalog = Catalog::open_in_library(&library).unwrap();
        scan_into(&mut catalog, &drive, None, &mut |_| {}).unwrap();

        assert_eq!(discover(&library, &catalog).unwrap().junk, 1);
        let aside = set_aside(&library, &catalog).unwrap();
        assert_eq!(aside.len(), 1);
        assert_eq!(aside[0].photo.name, "1952.png");
        assert_eq!(aside[0].junk_reason.as_deref(), Some("Tiny images (under 300 px)"));
        assert!(!aside[0].kept, "set aside means not kept until the user says so");

        set_photo_kept(&catalog, &aside[0].photo.id, true).unwrap();
        assert!(set_aside(&library, &catalog).unwrap().is_empty());
        let d = discover(&library, &catalog).unwrap();
        assert_eq!((d.junk, d.photos), (0, 2), "it is back among the photos");
        assert_eq!(collection_photos(&library, &catalog, "Grandad").unwrap().len(), 2);
    }

    #[test]
    fn a_backup_whose_own_folder_is_inside_a_source_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let drive = dir.path().join("drive");
        fs::create_dir_all(&drive).unwrap();
        fs::write(drive.join("a.jpg"), real_jpeg(1)).unwrap();
        let home = drive.join("Keevoro backup");
        let mut catalog = Catalog::open_in_library(&home).unwrap();
        scan_into(&mut catalog, &drive, None, &mut |_| {}).unwrap();
        let err = crate::check_and_place(&home, &catalog, &|| true, &mut |_| {}).unwrap_err();
        assert!(err.to_string().contains("Keevoro never writes to those"), "{err}");
        assert!(catalog.collected().unwrap().is_empty());
        assert_eq!(
            catalog.sources().unwrap()[0].name,
            "drive",
            "an unnamed drive is called after its folder"
        );
    }
}
