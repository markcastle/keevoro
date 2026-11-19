//! One backup, and the operations that work on it.
//!
//! A backup lives in its home folder: the folder that holds Keevoro's
//! record of it (`.keevoro/catalog.sqlite`) and the previews it makes. It
//! is working space, never a copy of anyone's photos.
//!
//! A backup is opened by its home folder. Named backups ("Drone footage")
//! are listed by [`crate::Backups`], which finds the home folder for a name
//! and hands back this same [`Backup`].

use crate::error::Result;
use keevoro_catalog::Catalog;
use keevoro_engine::discover::DestinationView;
use keevoro_engine::history::Sent;
use keevoro_engine::{CollectionView, Discover, DuplicateGroup, PhotoChoice, SourceView};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The catalog's name for what the last backup said when it ended.
const LAST_SUMMARY: &str = "last_backup_said";

/// One backup, open.
pub struct Backup {
    home: PathBuf,
    catalog: Catalog,
}

/// What a backup holds, in numbers: the top of the Discover screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    pub photos: usize,
    pub videos: usize,
    /// The size of everything found, in bytes.
    pub bytes: u64,
    /// Extra copies of files that are kept once.
    pub duplicates: usize,
    /// The room those extra copies would have taken, in bytes.
    pub duplicate_bytes: u64,
    /// Pictures set aside as probably not photos.
    pub set_aside: usize,
    /// Files being kept that are not yet safely backed up.
    pub waiting: usize,
    /// The room those waiting files need, in bytes.
    pub waiting_bytes: u64,
}

/// One kind of file found on a source, for the "File types" list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTypeView {
    /// Lower case, without the dot: `jpg`, `mp4`. Empty for files whose
    /// names have no ending.
    pub ending: String,
    pub files: u64,
    pub bytes: u64,
    /// False when the person has left this kind out of this source.
    pub included: bool,
}

impl Backup {
    /// Opens the backup whose home folder this is, making the folder and
    /// its record if they are not there yet.
    pub fn open(home: &Path) -> Result<Self> {
        Ok(Self {
            home: home.to_path_buf(),
            catalog: Catalog::open_in_library(home)?,
        })
    }

    pub(crate) fn with_catalog(home: &Path, catalog: Catalog) -> Self {
        Self {
            home: home.to_path_buf(),
            catalog,
        }
    }

    /// The backup's home folder.
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// Keevoro's record of this backup.
    pub(crate) fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// Everything the Discover screen shows, read in one go: what
    /// `found.summary`, `sources.list`, `destinations.list` and
    /// `collections.list` give separately.
    pub fn discover(&self) -> Result<Discover> {
        Ok(keevoro_engine::discover(&self.home, &self.catalog)?)
    }

    /// `found.summary`
    pub fn found(&self) -> Result<Found> {
        let d = self.discover()?;
        Ok(Found {
            photos: d.photos,
            videos: d.videos,
            bytes: d.bytes,
            duplicates: d.duplicates,
            duplicate_bytes: d.duplicate_bytes,
            set_aside: d.junk,
            waiting: d.waiting_to_gather,
            waiting_bytes: d.waiting_bytes,
        })
    }

    /// `sources.list`
    pub fn sources(&self) -> Result<Vec<SourceView>> {
        Ok(self.discover()?.sources)
    }

    /// `sources.forget`: true when there was such a source. Nothing on it is
    /// touched, and what is already backed up stays.
    pub fn forget_source(&mut self, source: i64) -> Result<bool> {
        Ok(keevoro_engine::forget_source(&mut self.catalog, source)?)
    }

    /// `sources.file_types`, most numerous first.
    pub fn file_types(&self, source: i64) -> Result<Vec<FileTypeView>> {
        Ok(self
            .catalog
            .file_types(source)?
            .into_iter()
            .map(|kind| FileTypeView {
                ending: kind.ending,
                files: kind.files,
                bytes: kind.bytes,
                included: kind.included,
            })
            .collect())
    }

    /// `sources.include_type`. Nothing on the source is touched; a kind left
    /// out is simply not offered or backed up.
    pub fn include_type(&self, source: i64, ending: &str, included: bool) -> Result<()> {
        Ok(self.catalog.set_type_included(source, ending, included)?)
    }

    /// `collections.list`, newest first.
    pub fn collections(&self) -> Result<Vec<CollectionView>> {
        Ok(self.discover()?.collections)
    }

    /// `collections.photos`, in date order.
    pub fn collection_photos(&self, collection: &str) -> Result<Vec<PhotoChoice>> {
        Ok(keevoro_engine::collection_photos(
            &self.home,
            &self.catalog,
            collection,
        )?)
    }

    /// `collections.keep`: how many files the choice was recorded for.
    pub fn keep_collection(&self, collection: &str, keep: bool) -> Result<usize> {
        Ok(keevoro_engine::set_collection_kept(&self.catalog, collection, keep)?)
    }

    /// `photos.keep`, by the photo's short id.
    pub fn keep_photo(&self, photo: &str, keep: bool) -> Result<()> {
        Ok(keevoro_engine::set_photo_kept(&self.catalog, photo, keep)?)
    }

    /// `set_aside.list`
    pub fn set_aside(&self) -> Result<Vec<PhotoChoice>> {
        Ok(keevoro_engine::set_aside(&self.home, &self.catalog)?)
    }

    /// `duplicates.list`
    pub fn duplicates(&self) -> Result<Vec<DuplicateGroup>> {
        Ok(keevoro_engine::duplicates(&self.home, &self.catalog)?)
    }

    /// `destinations.list`, in the order they were added.
    pub fn destinations(&self) -> Result<Vec<DestinationView>> {
        Ok(self.discover()?.destinations)
    }

    /// `backup.history`, newest first.
    pub fn history(&self) -> Result<Vec<Sent>> {
        Ok(keevoro_engine::history::recent(&self.catalog)?)
    }

    /// `backup.previews`, read: whether this backup also makes and sends
    /// previews for a gallery.
    pub fn previews(&self) -> Result<bool> {
        Ok(keevoro_engine::send::wants_previews(&self.catalog)?)
    }

    /// `backup.previews`, set.
    pub fn set_previews(&self, wanted: bool) -> Result<()> {
        Ok(keevoro_engine::send::set_wants_previews(&self.catalog, wanted)?)
    }

    /// `backup.last_summary`: what the last backup said when it ended, or
    /// nothing if it has never run or what it said no longer applies.
    pub fn last_summary(&self) -> Result<Option<serde_json::Value>> {
        Ok(self
            .catalog
            .setting(LAST_SUMMARY)?
            .and_then(|json| serde_json::from_str(&json).ok())
            .filter(|said: &serde_json::Value| said.is_object()))
    }

    /// Keeps what a backup said when it ended, for `backup.last_summary`.
    /// Until running a backup moves here, the app's shell calls this.
    pub fn remember_last_summary(&self, said: &impl Serialize) -> Result<()> {
        Ok(self.catalog.set_setting(LAST_SUMMARY, &serde_json::to_string(said)?)?)
    }

    /// Forgets what the last backup said, because it no longer applies: a
    /// new backup has started, or the destinations have changed.
    pub fn forget_last_summary(&self) -> Result<()> {
        Ok(self.catalog.set_setting(LAST_SUMMARY, "null")?)
    }
}

/// The record kept in `home`, if there is one there. Unlike
/// [`Backup::open`], this never makes a folder: a backup whose drive is
/// unplugged is simply not there.
pub(crate) fn existing_catalog(home: &Path) -> Option<Catalog> {
    let file = keevoro_catalog::CATALOG_PATH
        .split('/')
        .fold(home.to_path_buf(), |path, part| path.join(part));
    file.is_file().then(|| Catalog::open(&file).ok()).flatten()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use keevoro_engine::connect::{self, testing, World};
    use std::fs;

    fn real_jpeg(shade: u8) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(640, 480, image::Rgb([shade, 120, 60]));
        let mut bytes = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
            .unwrap();
        bytes
    }

    /// A source with an album, a dated photo, a duplicate and a picture
    /// too small to be a photo, made in `dir` and looked through into the
    /// backup whose home is `home`. The way it is looked through is not an
    /// API operation yet, so the engine does it directly.
    pub(crate) fn card_looked_through_into(dir: &Path, home: &Path) -> PathBuf {
        let card = card_in(dir);
        let mut catalog = Catalog::open_in_library(home).unwrap();
        keevoro_engine::scan_into(&mut catalog, &card, Some("Card"), &mut |_| {}).unwrap();
        card
    }

    /// That source, made in `dir` and not yet looked through.
    pub(crate) fn card_in(dir: &Path) -> PathBuf {
        let card = dir.join("Card");
        for (rel, shade) in [
            ("Holidays/Cornwall/IMG_20080812_101500.jpg", 10),
            ("Holidays/Cornwall/IMG_20080813_090000.jpg", 90),
            ("DCIM/100CANON/IMG_20120304_153012.jpg", 170),
            ("Backup/IMG_20080812_101500.jpg", 10),
        ] {
            let p = card.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, real_jpeg(shade)).unwrap();
        }
        let tiny = image::RgbImage::from_pixel(240, 180, image::Rgb([90, 90, 90]));
        tiny.save(card.join("DCIM/1952.png")).unwrap();
        card
    }

    /// A photo large enough that a quick search only samples it.
    pub(crate) fn large_photo(shade: u8) -> Vec<u8> {
        let mut bytes = real_jpeg(shade);
        bytes.resize(bytes.len() + 300 * 1024, 0);
        bytes
    }

    /// That source, in a backup whose home is somewhere else.
    pub(crate) fn one_backup() -> (tempfile::TempDir, PathBuf, Backup) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let card = card_looked_through_into(dir.path(), &home);
        let backup = Backup::open(&home).unwrap();
        (dir, card, backup)
    }

    #[test]
    fn a_backup_is_opened_by_its_home_folder() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("new backup");
        let backup = Backup::open(&home).unwrap();
        assert_eq!(backup.home(), home);
        assert!(home.join(".keevoro").join("catalog.sqlite").is_file());
        assert!(backup.sources().unwrap().is_empty());
    }

    #[test]
    fn the_discover_screen_gets_exactly_what_the_engine_gives() {
        let (_dir, _card, backup) = one_backup();
        let catalog = Catalog::open_in_library(backup.home()).unwrap();
        let engine = keevoro_engine::discover(backup.home(), &catalog).unwrap();
        assert_eq!(
            serde_json::to_value(backup.discover().unwrap()).unwrap(),
            serde_json::to_value(engine).unwrap()
        );
    }

    #[test]
    fn found_summary_counts_what_a_backup_holds() {
        let (_dir, _card, backup) = one_backup();
        let found = backup.found().unwrap();
        assert_eq!((found.photos, found.videos), (3, 0));
        assert_eq!((found.duplicates, found.set_aside, found.waiting), (1, 1, 3));
        assert!(found.duplicate_bytes > 0 && found.waiting_bytes > 0);
        assert!(found.bytes >= found.waiting_bytes);
        let json = serde_json::to_value(&found).unwrap();
        assert_eq!(json["setAside"], 1);
        assert_eq!(json["waitingBytes"], found.waiting_bytes);
    }

    #[test]
    fn sources_are_listed_and_forgotten_without_touching_them() {
        let (_dir, card, mut backup) = one_backup();
        let sources = backup.sources().unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!((sources[0].name.as_str(), sources[0].items), ("Card", 5));

        assert!(backup.forget_source(sources[0].id).unwrap());
        assert!(backup.sources().unwrap().is_empty());
        assert_eq!(backup.found().unwrap().photos, 0);
        assert!(card.join("Holidays/Cornwall/IMG_20080812_101500.jpg").is_file());
        assert!(card.join("DCIM/1952.png").is_file());
        assert!(
            !backup.forget_source(sources[0].id).unwrap(),
            "asking twice is harmless"
        );
    }

    #[test]
    fn kinds_of_file_on_a_source_can_be_left_out_and_brought_back() {
        let (_dir, card, backup) = one_backup();
        let id = backup.sources().unwrap()[0].id;
        let types = backup.file_types(id).unwrap();
        let endings: Vec<(&str, u64, bool)> = types.iter().map(|t| (t.ending.as_str(), t.files, t.included)).collect();
        assert_eq!(endings, [("jpg", 4, true), ("png", 1, true)]);
        // What the app has always received.
        let json = serde_json::to_value(&types[0]).unwrap();
        assert_eq!(json["ending"], "jpg");
        assert_eq!(json["included"], true);

        backup.include_type(id, ".JPG", false).unwrap();
        assert!(!backup.file_types(id).unwrap()[0].included);
        assert_eq!(backup.found().unwrap().photos, 0);
        assert!(card.join("DCIM/100CANON/IMG_20120304_153012.jpg").is_file());
        backup.include_type(id, "jpg", true).unwrap();
        assert_eq!(backup.found().unwrap().photos, 3);

        assert!(backup.file_types(9999).unwrap().is_empty(), "no such source, no kinds");
    }

    #[test]
    fn collections_are_listed_and_kept_or_left_out() {
        let (_dir, _card, backup) = one_backup();
        let titles: Vec<String> = backup.collections().unwrap().into_iter().map(|c| c.title).collect();
        assert!(titles.contains(&"Cornwall".to_string()), "{titles:?}");

        let photos = backup.collection_photos("Holidays / Cornwall").unwrap();
        assert_eq!(photos.len(), 2);
        assert!(photos.iter().all(|p| p.kept));

        assert_eq!(backup.keep_collection("Holidays / Cornwall", false).unwrap(), 2);
        let cornwall = backup
            .collections()
            .unwrap()
            .into_iter()
            .find(|c| c.key == "Holidays / Cornwall")
            .unwrap();
        assert!(!cornwall.kept);
        assert_eq!(backup.found().unwrap().waiting, 1);

        let err = backup.keep_collection("Narnia", true).unwrap_err();
        assert!(
            err.message.starts_with("That collection isn't there any more."),
            "{err}"
        );
        assert!(backup.collection_photos("Narnia").is_err());
    }

    #[test]
    fn one_photo_is_kept_or_left_out_by_its_id() {
        let (_dir, _card, backup) = one_backup();
        let photos = backup.collection_photos("Holidays / Cornwall").unwrap();
        backup.keep_photo(&photos[1].photo.id, false).unwrap();
        let after = backup.collection_photos("Holidays / Cornwall").unwrap();
        assert_eq!((after[0].kept, after[1].kept), (true, false));

        let err = backup.keep_photo("0000", true).unwrap_err();
        assert_eq!(err.kind, crate::Kind::Failed);
        assert_eq!(err.message, "That photo isn't known to this library.");
    }

    #[test]
    fn pictures_set_aside_are_listed_with_the_reason() {
        let (_dir, _card, backup) = one_backup();
        let aside = backup.set_aside().unwrap();
        assert_eq!(aside.len(), 1);
        assert_eq!(aside[0].photo.name, "1952.png");
        assert_eq!(aside[0].junk_reason.as_deref(), Some("Tiny images (under 300 px)"));
        backup.keep_photo(&aside[0].photo.id, true).unwrap();
        assert!(backup.set_aside().unwrap().is_empty());
    }

    #[test]
    fn duplicates_show_the_copy_kept_and_the_copy_left_out() {
        let (_dir, _card, backup) = one_backup();
        let groups = backup.duplicates().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].photo.name, "IMG_20080812_101500.jpg");
        assert_eq!(groups[0].copies.len(), 1);
    }

    #[test]
    fn destinations_say_what_has_reached_them() {
        let (dir, _card, backup) = one_backup();
        assert!(backup.destinations().unwrap().is_empty());

        let to = dir.path().join("USB stick");
        let catalog = Catalog::open_in_library(backup.home()).unwrap();
        let world = World {
            secrets: &testing::MemorySecrets::default(),
            ask: &testing::nobody_to_ask,
        };
        let folder = connect::Saved {
            kind: "folder".into(),
            place: to.display().to_string(),
            bucket: None,
            base_url: None,
            target: None,
        };
        connect::add(&mut Vec::new(), &catalog, &folder, &world).unwrap();

        let all = backup.destinations().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, format!("the folder {}", to.display()));
        assert_eq!((all[0].there, all[0].waiting), (0, 3));
    }

    #[test]
    fn history_lists_earlier_backups_newest_first() {
        let (_dir, _card, backup) = one_backup();
        assert!(backup.history().unwrap().is_empty());
        let catalog = Catalog::open_in_library(backup.home()).unwrap();
        for bytes in [100, 200] {
            let sent = Sent {
                finished_at: 1_794_751_331,
                seconds: 60,
                bytes,
                to: "the folder Z:\\Backups".into(),
                stopped: false,
            };
            keevoro_engine::history::record(&catalog, &sent).unwrap();
        }
        let history = backup.history().unwrap();
        assert_eq!(history.iter().map(|s| s.bytes).collect::<Vec<_>>(), [200, 100]);
    }

    #[test]
    fn previews_are_made_unless_switched_off() {
        let (_dir, _card, backup) = one_backup();
        assert!(backup.previews().unwrap());
        backup.set_previews(false).unwrap();
        assert!(!backup.previews().unwrap());
        // It is the same choice the engine reads when backing up.
        let catalog = Catalog::open_in_library(backup.home()).unwrap();
        assert!(!keevoro_engine::send::wants_previews(&catalog).unwrap());
        backup.set_previews(true).unwrap();
        assert!(backup.previews().unwrap());
    }

    #[test]
    fn the_last_summary_is_kept_until_it_no_longer_applies() {
        let (_dir, _card, backup) = one_backup();
        assert_eq!(backup.last_summary().unwrap(), None, "it has never run");

        let said = serde_json::json!({ "finished": true, "said": ["Safely backed up 3 files."] });
        backup.remember_last_summary(&said).unwrap();
        assert_eq!(backup.last_summary().unwrap(), Some(said));

        backup.forget_last_summary().unwrap();
        assert_eq!(backup.last_summary().unwrap(), None);

        // Anything that is not what a backup says is not passed on.
        backup.remember_last_summary(&"just words").unwrap();
        assert_eq!(backup.last_summary().unwrap(), None);
    }
}
