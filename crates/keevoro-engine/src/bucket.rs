//! Cloud storage as a source: adding a bucket (or a folder in one), looking
//! through it by its listing, and bringing each file through this computer
//! when it is backed up.
//!
//! A bucket source is only ever read. Everything here is given it as a
//! [`ReadOnly`], which can list and read and nothing else, so no change
//! here could write to it, replace or delete anything in it without failing
//! to compile. Its keys are tested by listing, never by writing.
//!
//! - **Keys** go in the computer's keychain under the source's own name
//!   ([`BucketPlace::secret_name`]), exactly as a destination's do: given
//!   ones are tested and saved, and only when none are given or saved are
//!   those in the environment used, and then said.
//! - **Looking through** is one paged listing, downloading nothing; what
//!   each object is comes from `keevoro_scan::bucket`. Inside a Keevoro
//!   backup's `originals`, where Keevoro labelled each object with its
//!   fingerprint, the labels are asked for too (a small request each).
//! - **Backing up** fetches each file, as its turn comes, into Keevoro's
//!   own `fetching` folder in the backup's home, fingerprinting it as it
//!   is written, sends it from there and removes it: never more on this
//!   computer at once than are being sent at once, and only when there is
//!   room for it and [`ROOM_TO_SPARE`] besides.

use crate::connect::{keys_to_use, Keys, KeysFrom, Saved, World};
use crate::send::{ACCESS_KEY_VAR, SECRET_KEY_VAR};
use anyhow::{bail, Result};
use keevoro_catalog::Catalog;
use keevoro_core::bucket::{inside_originals, label, BucketPlace};
use keevoro_publish::{Fetch, Fetched};
use keevoro_scan::{ScanProgress, ScanReport};
use keevoro_storage::{ObjectKey, ReadOnly, StorageError};
use keevoro_storage_s3::{S3Settings, S3Store};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Room left free on this computer's disk, beyond the file being fetched,
/// so bringing a large video through never fills it.
pub const ROOM_TO_SPARE: u64 = 2 * 1024 * 1024 * 1024;

/// How many labels are asked for at once, inside a Keevoro backup.
const LABELS_AT_ONCE: usize = 16;

/// The cloud storage a source is in, opened with these keys. Nothing is
/// sent until it is first asked something.
pub fn open(place: &BucketPlace, keys: &Keys) -> Result<ReadOnly> {
    let saved = as_saved(place);
    let Some(preset) = saved.preset() else {
        bail!("That isn't a kind of cloud storage Keevoro knows.");
    };
    let store = S3Store::connect(&S3Settings {
        preset,
        bucket: place.bucket.clone(),
        access_key_id: keys.access().to_string(),
        secret_access_key: keys.secret().to_string(),
    })
    .map_err(|e| anyhow::anyhow!("{}", keevoro_publish::plain_reason(&e)))?;
    Ok(ReadOnly::new(Arc::new(store)))
}

fn as_saved(place: &BucketPlace) -> Saved {
    Saved {
        kind: place.kind.clone(),
        place: place.place.clone(),
        bucket: Some(place.bucket.clone()),
        base_url: None,
        target: None,
        volume: None,
    }
}

/// The keys for a bucket source: those saved for it in the keychain or,
/// only when there are none, those set in the environment.
pub fn keys_for(place: &BucketPlace, world: &World<'_>) -> Result<(Keys, KeysFrom)> {
    let saved = world
        .secrets
        .get(&place.secret_name())?
        .as_deref()
        .and_then(crate::connect::split_keys);
    keys_to_use(saved, KeysFrom::Keychain, world.environment).ok_or_else(|| {
        anyhow::anyhow!(
            "Keevoro needs the storage keys for {}. Add it as a source again to save them, or set \
             {ACCESS_KEY_VAR} and {SECRET_KEY_VAR}.",
            place.describe(),
        )
    })
}

/// What went wrong reaching a bucket source, said for the person.
fn reaching_said(place: &BucketPlace, e: &StorageError) -> String {
    match e {
        StorageError::AccessDenied(_) => format!(
            "{} refused these keys. Check they are right and are allowed to list and read the bucket.",
            capital(&place.describe())
        ),
        StorageError::NotFound(_) => format!("Keevoro can't find {}. Check its name.", place.describe()),
        StorageError::Network(_) => format!(
            "Keevoro couldn't reach {}. Check the internet connection and try again.",
            place.describe()
        ),
        other => keevoro_publish::plain_reason(other),
    }
}

fn capital(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// Adds a bucket, or a folder in one, as a source of this backup: refuses
/// one where this backup itself goes, tests the keys by listing (writing
/// nothing), saves them in the keychain, and remembers where it is. It
/// keeps its folders as they are unless the person chooses otherwise: a
/// bucket's files carry no date of their own, only when they were put
/// there. Adding it again refreshes its keys. Answers its id and where
/// the keys came from; any error has the keys taken out of its words.
pub fn add(
    catalog: &Catalog,
    place: &BucketPlace,
    name: Option<&str>,
    given: Option<Keys>,
    world: &World<'_>,
) -> Result<(i64, KeysFrom)> {
    for to in crate::connect::destinations(catalog)? {
        if place.overlaps_a_backup_at(&to.kind, &to.place, to.bucket.as_deref().unwrap_or_default()) {
            bail!(
                "That is where this backup goes, so it can't be a source of it too. Choose the folder in the \
                 bucket that holds your photos."
            );
        }
    }
    let Some((keys, from)) = keys_to_use(given, KeysFrom::Given, world.environment) else {
        bail!("Enter both keys for the storage.");
    };
    let hide = |said: String| keys.both().iter().fold(said, |said, key| said.replace(key, "[hidden]"));
    let store = open(place, &keys).map_err(|e| anyhow::anyhow!(hide(format!("{e:#}"))))?;
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    runtime
        .block_on(store.check_reading(&place.listing_prefix()))
        .map_err(|e| crate::connect::CannotReach(hide(reaching_said(place, &e))))?;
    world.secrets.set(
        &place.secret_name(),
        &crate::connect::join_keys(keys.access(), keys.secret()),
    )?;
    let root = place.root();
    let new = !catalog.sources()?.iter().any(|s| s.root == Path::new(&root));
    let id = catalog.add_bucket_source(
        name.map(str::trim).filter(|n| !n.is_empty()).unwrap_or(&place.name()),
        &root,
        &serde_json::to_string(place)?,
        crate::confirm::now(),
    )?;
    if new {
        catalog.set_source_arrange(id, Some(keevoro_core::Arrange::Folders))?;
    }
    Ok((id, from))
}

/// Where a bucket source is, from what the catalog keeps of it.
pub fn place_of(kept: Option<&str>) -> Option<BucketPlace> {
    serde_json::from_str(kept?).ok()
}

/// Looks through a bucket source by listing it, a page of up to a thousand
/// objects at a time, and remembers what it holds in `catalog` under its
/// name, as a search does a folder. Nothing is downloaded. `stopped` is
/// asked between pages; a look that is stopped remembers nothing, so what
/// was known stays. A file found before whose kind is no longer looked for
/// is kept while it is still there at the same size, as for a folder.
#[allow(clippy::too_many_arguments)]
pub fn look_through(
    catalog: &mut Catalog,
    name: &str,
    place: &BucketPlace,
    store: &ReadOnly,
    kinds: &keevoro_core::Kinds,
    skip: &keevoro_core::SkipRules,
    stopped: &dyn Fn() -> bool,
    on_progress: &mut dyn FnMut(&ScanProgress),
) -> Result<ScanReport> {
    let _busy = keevoro_core::doing::doing(keevoro_core::doing::Work::LookingThroughASource);
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let prefix = place.listing_prefix();
    let mut listed: Vec<keevoro_scan::bucket::Listed> = Vec::new();
    let mut progress = ScanProgress {
        folder: PathBuf::from(place.root()),
        ..ScanProgress::default()
    };
    let mut next = None;
    loop {
        keevoro_core::pause::wait_while_paused();
        if stopped() {
            return Ok(ScanReport {
                root: PathBuf::from(place.root()),
                stopped: true,
                ..ScanReport::default()
            });
        }
        let page = runtime
            .block_on(store.list_page(&prefix, next))
            .map_err(|e| anyhow::anyhow!(reaching_said(place, &e)))?;
        progress.files_seen += page.objects.len() as u64;
        listed.extend(page.objects.into_iter().map(|o| keevoro_scan::bucket::Listed {
            key: o.key.to_string(),
            bytes: o.bytes,
            modified: o.modified,
            etag: o.etag,
            class: o.class,
            label: None,
        }));
        on_progress(&progress);
        match page.next {
            Some(more) => next = Some(more),
            None => break,
        }
    }
    // Inside a Keevoro backup's originals each object carries the whole
    // fingerprint Keevoro gave it, so copies are known without a download.
    if inside_originals(&prefix) {
        let wanted: Vec<usize> = (0..listed.len())
            .filter(|&i| {
                let ending = listed[i].key.rsplit_once('.').map(|(_, e)| e.to_lowercase());
                ending.is_some_and(|e| kinds.looked_for(&e).is_some())
            })
            .collect();
        let labels = runtime.block_on(labels_of(store, &listed, &wanted));
        for (i, found) in labels {
            listed[i].label = found;
        }
    }
    let mut report = keevoro_scan::bucket::found_in_listing(place, &listed, kinds, skip);
    keep_found_before(catalog, &mut report, &listed, place)?;
    for item in &report.items {
        match item.kind {
            keevoro_core::MediaKind::Photo => progress.photos += 1,
            keevoro_core::MediaKind::Video => progress.videos += 1,
            keevoro_core::MediaKind::Other => progress.others += 1,
        }
        progress.bytes += item.bytes;
    }
    on_progress(&progress);
    catalog.save_scan(name, &report, crate::confirm::now())?;
    Ok(report)
}

/// The labels of the objects at `wanted` in `listed`, a few at a time.
async fn labels_of(
    store: &ReadOnly,
    listed: &[keevoro_scan::bucket::Listed],
    wanted: &[usize],
) -> Vec<(usize, Option<String>)> {
    use futures_util::stream::{self, StreamExt};
    stream::iter(wanted.iter().copied())
        .map(|i| async move {
            let found = match ObjectKey::new(listed[i].key.clone()) {
                Ok(key) => store.head(&key).await.ok().flatten(),
                Err(_) => None,
            };
            (
                i,
                found.and_then(|info| label(info.metadata.get("blake3").map(String::as_str))),
            )
        })
        .buffer_unordered(LABELS_AT_ONCE)
        .collect()
        .await
}

/// Keeps what an earlier look found whose kind is no longer looked for,
/// while the object is still there at the same size.
fn keep_found_before(
    catalog: &Catalog,
    report: &mut ScanReport,
    listed: &[keevoro_scan::bucket::Listed],
    place: &BucketPlace,
) -> Result<()> {
    let Some(before) = catalog
        .load_reports_as_found()?
        .into_iter()
        .find(|r| r.root == report.root)
    else {
        return Ok(());
    };
    let now: std::collections::HashSet<PathBuf> = report.items.iter().map(|i| i.path.clone()).collect();
    let sizes: std::collections::HashMap<&str, u64> = listed.iter().map(|o| (o.key.as_str(), o.bytes)).collect();
    report.items.extend(before.items.into_iter().filter(|item| {
        !now.contains(&item.path)
            && place
                .key_of(&item.path)
                .is_some_and(|key| sizes.get(key.as_str()) == Some(&item.bytes))
    }));
    report.items.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(())
}

/// The bucket sources of one backup, each opened once for a backup with its
/// keys, or with why it could not be, and where files are fetched to.
pub struct Buckets {
    opened: Vec<(BucketPlace, std::result::Result<ReadOnly, String>)>,
    staging: PathBuf,
}

impl Buckets {
    /// No bucket sources: a backup of folders only.
    pub fn none(home: &Path) -> Self {
        Self {
            opened: Vec::new(),
            staging: staging_folder(home),
        }
    }

    /// Opens each bucket source of the backup at `home`. One whose keys are
    /// missing is remembered with why, and its files are said not to be
    /// sent for that reason. Anything a crashed backup left in the staging
    /// folder is cleared first.
    pub fn open(catalog: &Catalog, home: &Path, world: &World<'_>) -> Result<Self> {
        let staging = staging_folder(home);
        let _ = std::fs::remove_dir_all(&staging);
        let mut opened = Vec::new();
        for source in catalog.sources()? {
            let Some(place) = place_of(source.bucket.as_deref()) else {
                continue;
            };
            let store = keys_for(&place, world)
                .and_then(|(keys, _)| open(&place, &keys))
                .map_err(|e| format!("{e:#}"));
            opened.push((place, store));
        }
        Ok(Self { opened, staging })
    }

    /// Bucket sources opened by the caller, each with its store.
    #[cfg(test)]
    pub(crate) fn given(home: &Path, opened: Vec<(BucketPlace, ReadOnly)>) -> Self {
        Self {
            opened: opened.into_iter().map(|(place, store)| (place, Ok(store))).collect(),
            staging: staging_folder(home),
        }
    }

    /// How to bring the file at `path` here, when it is in one of these
    /// buckets.
    pub fn fetch_for(&self, path: &Path, bytes: u64) -> Option<Arc<dyn Fetch>> {
        let (place, store, key) = self
            .opened
            .iter()
            .find_map(|(place, store)| Some((place, store, place.key_of(path)?)))?;
        Some(Arc::new(FromBucket {
            store: store.clone(),
            key,
            bytes,
            staging: self.staging.clone(),
            describe: place.describe(),
        }))
    }
}

/// Where files are fetched to: Keevoro's own folder in the backup's home,
/// never a source or a destination.
fn staging_folder(home: &Path) -> PathBuf {
    home.join(".keevoro").join("fetching")
}

/// One object in a bucket source, to be fetched when its turn comes.
struct FromBucket {
    store: std::result::Result<ReadOnly, String>,
    key: String,
    bytes: u64,
    staging: PathBuf,
    describe: String,
}

impl std::fmt::Debug for FromBucket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} in {}", self.key, self.describe)
    }
}

#[async_trait::async_trait]
impl Fetch for FromBucket {
    async fn fetch(&self) -> std::result::Result<Fetched, StorageError> {
        let store = self.store.as_ref().map_err(|why| StorageError::Provider(why.clone()))?;
        std::fs::create_dir_all(&self.staging)?;
        // Only with room for it, and room to spare.
        if let Some(free) = keevoro_collect::free_space(&self.staging) {
            if free < self.bytes.saturating_add(ROOM_TO_SPARE) {
                return Err(StorageError::Provider(format!(
                    "There isn't room on this computer to bring it through: it needs {} free, and has {}",
                    crate::summary::bytes(self.bytes.saturating_add(ROOM_TO_SPARE)),
                    crate::summary::bytes(free)
                )));
            }
        }
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).map_err(|e| StorageError::Provider(e.to_string()))?;
        let file = self.staging.join(format!(
            "{}.part",
            random.iter().map(|b| format!("{b:02x}")).collect::<String>()
        ));
        let got = store.download_to(&ObjectKey::new(self.key.clone())?, &file).await?;
        Ok(Fetched {
            file,
            bytes: got.bytes,
            blake3: got.blake3,
        })
    }
}

#[cfg(test)]
mod tests;
