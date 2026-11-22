//! A bucket source, end to end, against a store that refuses every write:
//! looked through by listing alone, backed up to a folder exactly, told
//! apart from copies on a folder source, and never changed.

use super::*;
use crate::connect::{self, testing::MemorySecrets, Saved};
use crate::send::{everywhere_reading, Stop, Went};
use keevoro_core::estimate::{Remembered, Tracker};
use keevoro_storage::{
    ByteSource, Capabilities, ConnectionReport, Downloaded, ListPage, ObjectInfo, ObjectStore, PutOptions, PutReceipt,
};
use keevoro_storage_folder::FolderStore;
use std::sync::atomic::{AtomicU32, Ordering};

/// A bucket that can be read and refuses, and counts, every write.
struct NoWrites {
    inner: FolderStore,
    writes: AtomicU32,
    downloads: AtomicU32,
    /// How many of the next downloads lose the connection part-way.
    drop_next: AtomicU32,
}

#[async_trait::async_trait]
impl ObjectStore for NoWrites {
    fn capabilities(&self) -> &Capabilities {
        self.inner.capabilities()
    }
    async fn check_connection(&self) -> std::result::Result<ConnectionReport, StorageError> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        Err(StorageError::AccessDenied("this bucket is only ever read".into()))
    }
    async fn head(&self, key: &ObjectKey) -> std::result::Result<Option<ObjectInfo>, StorageError> {
        self.inner.head(key).await
    }
    async fn get(&self, key: &ObjectKey) -> std::result::Result<Vec<u8>, StorageError> {
        self.downloads.fetch_add(1, Ordering::SeqCst);
        self.inner.get(key).await
    }
    async fn put(&self, _: &ObjectKey, _: ByteSource, _: &PutOptions) -> std::result::Result<PutReceipt, StorageError> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        Err(StorageError::AccessDenied("this bucket is only ever read".into()))
    }
    async fn list_page(&self, prefix: &str, next: Option<String>) -> std::result::Result<ListPage, StorageError> {
        self.inner.list_page(prefix, next).await
    }
    async fn delete(&self, _: &ObjectKey) -> std::result::Result<(), StorageError> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        Err(StorageError::AccessDenied("this bucket is only ever read".into()))
    }
    async fn download_to(&self, key: &ObjectKey, file: &Path) -> std::result::Result<Downloaded, StorageError> {
        self.downloads.fetch_add(1, Ordering::SeqCst);
        if self.drop_next.load(Ordering::SeqCst) > 0 {
            self.drop_next.fetch_sub(1, Ordering::SeqCst);
            // Part of it arrives, then the connection goes: the part is
            // removed, as the real stores remove theirs.
            std::fs::write(file, b"part of it")?;
            let _ = std::fs::remove_file(file);
            return Err(StorageError::Network("connection reset".into()));
        }
        self.inner.download_to(key, file).await
    }
}

struct Bench {
    dir: tempfile::TempDir,
    home: PathBuf,
    catalog: Catalog,
    secrets: MemorySecrets,
    place: BucketPlace,
    store: Arc<NoWrites>,
    read: ReadOnly,
}

/// What a bucket holds, every byte and when each file was last written.
fn everything_in(folder: &Path) -> Vec<(PathBuf, Vec<u8>, std::time::SystemTime)> {
    let mut all: Vec<_> = walk(folder)
        .into_iter()
        .map(|p| {
            let meta = std::fs::metadata(&p).unwrap();
            (p.clone(), std::fs::read(&p).unwrap(), meta.modified().unwrap())
        })
        .collect();
    all.sort_by(|a, b| a.0.cmp(&b.0));
    all
}

fn walk(folder: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}

impl Bench {
    /// A bucket holding these files, added as a source of a new backup.
    fn new(files: &[(&str, Vec<u8>)]) -> Self {
        Self::called(files, None)
    }

    /// [`Bench::new`], with the source given a name of the person's own
    /// rather than its folder's.
    fn called(files: &[(&str, Vec<u8>)], name: Option<&str>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let bucket = dir.path().join("bucket");
        for (key, body) in files {
            let path = bucket.join(key);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let inner = runtime.block_on(FolderStore::open(&bucket)).unwrap();
        let store = Arc::new(NoWrites {
            inner,
            writes: AtomicU32::new(0),
            downloads: AtomicU32::new(0),
            drop_next: AtomicU32::new(0),
        });
        let read = ReadOnly::new(store.clone());
        let place = BucketPlace::new("endpoint", "https://storage.example", "family", "Photos").unwrap();
        let home = dir.path().join("home");
        let catalog = Catalog::open_in_library(&home).unwrap();
        let id = catalog
            .add_bucket_source(
                name.unwrap_or(&place.name()),
                &place.root(),
                &serde_json::to_string(&place).unwrap(),
                0,
            )
            .unwrap();
        catalog
            .set_source_arrange(id, Some(keevoro_core::Arrange::Folders))
            .unwrap();
        Self {
            dir,
            home,
            catalog,
            secrets: MemorySecrets::default(),
            place,
            store,
            read,
        }
    }

    fn world(&self) -> World<'_> {
        World {
            secrets: &self.secrets,
            environment: &connect::testing::no_environment,
        }
    }

    fn look(&mut self) -> ScanReport {
        look_through(
            &mut self.catalog,
            "Photos",
            &self.place,
            &self.read,
            &keevoro_core::Kinds::default(),
            &keevoro_core::SkipRules::default(),
            &|| false,
            &mut |_| {},
        )
        .unwrap()
    }

    fn usb(&self) -> PathBuf {
        self.dir.path().join("USB")
    }

    fn to_usb(&self) {
        let usb = Saved {
            kind: "folder".into(),
            place: self.usb().display().to_string(),
            bucket: None,
            base_url: None,
            target: None,
            volume: None,
        };
        connect::add(&mut Vec::new(), &self.catalog, &usb, None, &self.world()).unwrap();
    }

    fn back_up(&self) -> (String, Went) {
        let buckets = Buckets::given(&self.home, vec![(self.place.clone(), self.read.clone())]);
        let mut said = Vec::new();
        let went = everywhere_reading(
            &crate::send::HowToSend::default(),
            &mut said,
            &self.catalog,
            &self.home,
            &self.world(),
            &buckets,
            None,
            &Stop::default(),
            &Tracker::new(Remembered::default()),
            &mut |_| {},
            &mut |_| {},
        )
        .unwrap();
        (String::from_utf8(said).unwrap(), went)
    }

    fn originals_at_usb(&self) -> Vec<(String, Vec<u8>)> {
        let originals = self.usb().join("originals");
        let mut found: Vec<(String, Vec<u8>)> = walk(&originals)
            .into_iter()
            .map(|p| {
                let rel = p.strip_prefix(&originals).unwrap().to_string_lossy().replace('\\', "/");
                (rel, std::fs::read(&p).unwrap())
            })
            .collect();
        found.sort();
        found
    }

    fn staged(&self) -> usize {
        walk(&self.home.join(".keevoro").join("fetching")).len()
    }
}

fn photo(shade: u8) -> Vec<u8> {
    crate::discover::tests::real_jpeg(shade)
}

#[test]
fn a_bucket_is_looked_through_by_its_listing_alone_downloading_nothing() {
    let mut b = Bench::new(&[
        ("Photos/Spain 2019/IMG_20190704_101500.jpg", photo(10)),
        ("Photos/Spain 2019/IMG_20190705_101500.jpg", photo(60)),
        ("Photos/notes.txt", b"words".to_vec()),
        ("Elsewhere/IMG_1.jpg", photo(90)),
    ]);
    let report = b.look();
    assert_eq!(report.items.len(), 2);
    assert_eq!(report.other_files, 1);
    assert_eq!(b.store.downloads.load(Ordering::SeqCst), 0, "nothing was read");
    let source = &b.catalog.sources().unwrap()[0];
    assert_eq!(source.items, 2);
    assert!(source.bucket.is_some(), "still cloud storage once looked through");
    // The background check for duplicates never downloads it.
    assert!(b.catalog.unhashed().unwrap().is_empty());
    assert_eq!(b.store.writes.load(Ordering::SeqCst), 0);
}

#[test]
fn a_bucket_is_backed_up_to_a_folder_exactly_and_nothing_in_it_is_ever_written() {
    let mut b = Bench::new(&[
        ("Photos/Spain 2019/IMG_20190704_101500.jpg", photo(10)),
        ("Photos/Spain 2019/IMG_20190705_101500.jpg", photo(60)),
        ("Photos/Cornwall/IMG_20200101_090000.jpg", photo(120)),
    ]);
    let before = everything_in(&b.dir.path().join("bucket"));
    b.look();
    b.to_usb();
    // Before anything starts, what it would read out of the bucket, and
    // about what that costs, is known.
    let total: u64 = [10, 60, 120].iter().map(|s| photo(*s).len() as u64).sum();
    let found = crate::discover(&b.home, &b.catalog).unwrap();
    assert_eq!(found.from_cloud.len(), 1);
    assert_eq!(found.from_cloud[0].bytes, total);
    assert!(
        found.from_cloud[0].cost.contains("so check before you start"),
        "{:?}",
        found.from_cloud
    );
    // The connection drops once part-way through a file: tried again, and
    // nothing half-fetched is left behind.
    b.store.drop_next.store(1, Ordering::SeqCst);
    let (said, went) = b.back_up();
    assert_eq!((went.reached, went.failed_files), (1, 0), "{said}");
    assert!(said.contains("Safely backed up 3 files"), "{said}");
    // Kept in its folders, the source's own name first, byte for byte.
    assert_eq!(
        b.originals_at_usb(),
        [
            ("Photos/Cornwall/IMG_20200101_090000.jpg".to_string(), photo(120)),
            ("Photos/Spain 2019/IMG_20190704_101500.jpg".to_string(), photo(10)),
            ("Photos/Spain 2019/IMG_20190705_101500.jpg".to_string(), photo(60)),
        ]
    );
    assert_eq!(b.staged(), 0, "nothing fetched is left on this computer");
    assert!(
        crate::discover(&b.home, &b.catalog).unwrap().from_cloud.is_empty(),
        "nothing left to read"
    );
    assert_eq!(
        b.store.downloads.load(Ordering::SeqCst),
        4,
        "each read once, one tried again"
    );
    // Each file now goes by its whole fingerprint, learnt as it was read.
    let reports = b.catalog.load_reports().unwrap();
    assert!(reports[0].items.iter().all(|i| i.hashed));
    assert_eq!(
        reports[0].items[0].blake3.as_deref(),
        Some(blake3::hash(&photo(120)).to_hex().as_str())
    );

    // A second look and a second backup read nothing again.
    b.look();
    assert!(b.catalog.load_reports().unwrap()[0].items.iter().all(|i| i.hashed));
    let (said, _) = b.back_up();
    assert!(said.contains("3 files already there"), "{said}");
    assert_eq!(
        b.store.downloads.load(Ordering::SeqCst),
        4,
        "nothing already safe is read again"
    );

    // The bucket is exactly as it was, and nothing ever asked to write to it.
    assert_eq!(b.store.writes.load(Ordering::SeqCst), 0);
    assert_eq!(everything_in(&b.dir.path().join("bucket")), before);
}

#[test]
fn a_photo_in_the_bucket_and_on_a_folder_source_is_backed_up_once() {
    let mut b = Bench::new(&[("Photos/IMG_1.jpg", photo(10)), ("Photos/IMG_2.jpg", photo(200))]);
    let card = b.dir.path().join("CARD");
    std::fs::create_dir_all(card.join("DCIM")).unwrap();
    std::fs::write(card.join("DCIM").join("IMG_1.jpg"), photo(10)).unwrap();
    crate::scan_into(&mut b.catalog, &card, Some("CARD"), &mut |_| {}).unwrap();
    b.look();
    b.to_usb();
    let (said, went) = b.back_up();
    assert_eq!(went.failed_files, 0, "{said}");
    assert!(
        said.contains("1 file read from cloud storage was an exact copy of a file already being backed up, so it was not sent again."),
        "{said}"
    );
    let at_usb = b.originals_at_usb();
    let copies = at_usb.iter().filter(|(_, body)| *body == photo(10)).count();
    assert_eq!(copies, 1, "{at_usb:?}");
    assert_eq!(at_usb.len(), 2, "{at_usb:?}");
    // From now on it is known as the same photo, so it is a duplicate.
    let found = crate::discover(&b.home, &b.catalog).unwrap();
    assert_eq!(found.duplicates, 1);
    assert_eq!(b.store.writes.load(Ordering::SeqCst), 0);
}

#[test]
fn what_is_in_archive_storage_is_said_and_never_read() {
    let mut b = Bench::new(&[("Photos/IMG_1.jpg", photo(10))]);
    // As a listing of Amazon's archive storage would give it.
    let report = keevoro_scan::bucket::found_in_listing(
        &b.place,
        &[keevoro_scan::bucket::Listed {
            key: "Photos/IMG_2.jpg".into(),
            bytes: 9_000_000,
            modified: None,
            etag: None,
            class: Some("DEEP_ARCHIVE".into()),
            label: None,
        }],
        &keevoro_core::Kinds::default(),
        &keevoro_core::SkipRules::default(),
    );
    b.catalog.save_scan("Photos", &report, 1).unwrap();
    let source = crate::discover(&b.home, &b.catalog).unwrap().sources.remove(0);
    assert_eq!(
        source.online_only_said.as_deref(),
        Some(
            "1 file, 8.6 MB, is in archive storage, which can't be read until it is restored. Keevoro can't do \
             that yet; to include it, restore it in the storage first, then look again."
        )
    );
    assert_eq!(b.store.downloads.load(Ordering::SeqCst), 0);
    b.look();
}

#[test]
fn what_is_in_googles_colder_classes_is_counted_said_and_still_backed_up() {
    let mut b = Bench::new(&[("Photos/IMG_1.jpg", photo(10))]);
    let listed = |key: &str, bytes: u64, class: &str| keevoro_scan::bucket::Listed {
        key: key.into(),
        bytes,
        modified: None,
        etag: None,
        class: Some(class.into()),
        label: None,
    };
    // As a listing of Google Cloud Storage would give it.
    let listing = [
        listed("Photos/IMG_2.jpg", 9_000_000, "COLDLINE"),
        listed("Photos/IMG_3.jpg", 1_000, "ARCHIVE"),
        listed("Photos/IMG_4.jpg", 5_000, "STANDARD"),
        listed("Photos/notes.txt", 5_000, "NEARLINE"),
    ];
    let report = keevoro_scan::bucket::found_in_listing(
        &b.place,
        &listing,
        &keevoro_core::Kinds::default(),
        &keevoro_core::SkipRules::default(),
    );
    assert_eq!(report.items.len(), 3, "read at once, so found, not passed over");
    assert!(report.online_only.is_empty());
    let (n, bytes) = in_colder_classes(&b.place, &report, &listing);
    assert_eq!((n, bytes), (2, 9_001_000), "only the photos and videos found count");
    b.catalog.save_scan("Photos", &report, 1).unwrap();
    let said = |b: &Bench| {
        crate::discover(&b.home, &b.catalog)
            .unwrap()
            .sources
            .remove(0)
            .colder_said
    };
    assert_eq!(said(&b), None, "nothing is said before it is counted");
    b.catalog
        .set_setting(&colder_key(&b.place.root()), &format!("{n} {bytes}"))
        .unwrap();
    assert_eq!(
        said(&b).as_deref(),
        Some(
            "2 files, 8.6 MB, are in Google’s Nearline, Coldline or Archive storage, which costs more to read \
             out. Keevoro backs them up all the same."
        )
    );
    // A look that finds none says nothing.
    b.look();
    assert_eq!(said(&b), None);
}

#[test]
fn a_look_that_is_stopped_remembers_nothing_new() {
    let mut b = Bench::new(&[("Photos/IMG_1.jpg", photo(10))]);
    let report = look_through(
        &mut b.catalog,
        "Photos",
        &b.place,
        &b.read,
        &keevoro_core::Kinds::default(),
        &keevoro_core::SkipRules::default(),
        &|| true,
        &mut |_| {},
    )
    .unwrap();
    assert!(report.stopped);
    assert_eq!(b.catalog.sources().unwrap()[0].items, 0);
}

#[test]
fn a_source_where_the_backup_itself_goes_is_refused_and_keys_are_tested_by_reading() {
    let b = Bench::new(&[]);
    let r2 = Saved {
        kind: "r2".into(),
        place: "0123456789abcdef0123456789abcdef".into(),
        bucket: Some("family".into()),
        base_url: None,
        target: None,
        volume: None,
    };
    // As if it had been added as a destination.
    b.catalog
        .set_setting("destinations", &serde_json::to_string(&[r2]).unwrap())
        .unwrap();
    let whole = BucketPlace::new("r2", "0123456789abcdef0123456789abcdef", "family", "").unwrap();
    let keys = Keys::new("AKIAMARKER", "secret-marker");
    let e = add(&b.catalog, &whole, None, keys, &b.world()).unwrap_err();
    assert!(e.to_string().starts_with("That is where this backup goes"), "{e}");
    assert!(b.secrets.0.borrow().is_empty(), "no key was kept");
}

/// Against a real S3-compatible server where one is set up (MinIO in CI):
/// a bucket source is added with keys tested by reading, looked through,
/// and backed up to a folder through `send::everywhere` exactly as a
/// person's backup is, with the keys from the keychain; and what is in the
/// bucket is exactly as it was, tag for tag.
#[test]
fn against_a_real_server_a_bucket_source_is_added_looked_through_and_backed_up_and_never_changed() {
    use keevoro_storage_s3::{Preset, S3Settings, S3Store};
    let var = |name: &str| std::env::var(format!("KEEVORO_S3_TEST_{name}")).ok();
    let (Some(endpoint), Some(bucket), Some(access), Some(secret)) =
        (var("ENDPOINT"), var("BUCKET"), var("ACCESS_KEY"), var("SECRET_KEY"))
    else {
        eprintln!("skipped: no test server set up");
        return;
    };
    let writer = S3Store::connect(&S3Settings {
        preset: Preset::Custom {
            endpoint: endpoint.clone(),
            region: var("REGION"),
            path_style: true,
        },
        bucket: bucket.clone(),
        access_key_id: access.clone(),
        secret_access_key: secret.clone(),
    })
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let _ = runtime.block_on(writer.check_connection());
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let top = format!("bucket-source-{nanos}");
    let files = [("Spain/IMG_1.jpg", photo(10)), ("Spain/IMG_2.jpg", photo(90))];
    runtime.block_on(async {
        for (rest, body) in &files {
            let key = ObjectKey::new(format!("{top}/Photos/{rest}")).unwrap();
            writer
                .put(&key, ByteSource::Bytes(body.clone()), &PutOptions::default())
                .await
                .unwrap();
        }
    });
    let listing = || {
        let mut all: Vec<(String, u64, Option<String>)> = runtime
            .block_on(writer.list_all(&format!("{top}/")))
            .unwrap()
            .into_iter()
            .map(|o| (o.key.to_string(), o.bytes, o.etag))
            .collect();
        all.sort();
        all
    };
    let before = listing();

    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let mut catalog = Catalog::open_in_library(&home).unwrap();
    let secrets = MemorySecrets::default();
    let world = World {
        secrets: &secrets,
        environment: &connect::testing::no_environment,
    };
    let place = BucketPlace::new("endpoint", &endpoint, &bucket, &format!("{top}/Photos")).unwrap();
    let (id, from) = add(&catalog, &place, None, Keys::new(&access, &secret), &world).unwrap();
    assert_eq!(from, KeysFrom::Given);
    assert_eq!(
        catalog.source_arrange(id).unwrap(),
        Some(keevoro_core::Arrange::Folders)
    );
    let (keys, _) = keys_for(&place, &world).unwrap();
    let report = look_through(
        &mut catalog,
        "Photos",
        &place,
        &open(&place, &keys).unwrap(),
        &keevoro_core::Kinds::default(),
        &keevoro_core::SkipRules::default(),
        &|| false,
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(report.items.len(), 2);
    let usb = dir.path().join("USB");
    let to = Saved {
        kind: "folder".into(),
        place: usb.display().to_string(),
        bucket: None,
        base_url: None,
        target: None,
        volume: None,
    };
    connect::add(&mut Vec::new(), &catalog, &to, None, &world).unwrap();
    let mut said = Vec::new();
    let went = crate::send::everywhere(
        &mut said,
        &catalog,
        &home,
        &world,
        None,
        &Stop::default(),
        &Tracker::new(Remembered::default()),
        &mut |_| {},
        &mut |_| {},
    )
    .unwrap();
    let said = String::from_utf8(said).unwrap();
    assert_eq!(went.failed_files, 0, "{said}");
    for (rest, body) in &files {
        assert_eq!(
            std::fs::read(usb.join("originals/Photos").join(rest)).unwrap(),
            *body,
            "{rest}"
        );
    }
    assert!(walk(&home.join(".keevoro").join("fetching")).is_empty());
    assert_eq!(listing(), before, "the bucket is exactly as it was");
    runtime.block_on(async {
        for (key, _, _) in before {
            writer.delete(&ObjectKey::new(key).unwrap()).await.unwrap();
        }
    });
}

/// A name the person gave a bucket source is kept by every look through
/// it, whatever name the look comes with.
#[test]
fn looking_through_a_bucket_source_again_keeps_the_name_the_person_gave_it() {
    let mut bench = Bench::called(
        &[("Photos/IMG_0048.jpg", b"not really a photo".to_vec())],
        Some("Holiday videos"),
    );
    bench.look();
    bench.look();
    let names: Vec<String> = bench.catalog.sources().unwrap().into_iter().map(|s| s.name).collect();
    assert_eq!(names, ["Holiday videos"]);
}
