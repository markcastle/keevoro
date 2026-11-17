use super::*;
use async_trait::async_trait;
use keevoro_storage::{Capabilities, ConnectionReport, ListPage, ObjectInfo, PutReceipt};
use keevoro_storage_folder::FolderStore;
use std::sync::atomic::{AtomicU32, Ordering};
use tempfile::TempDir;

const MIB: usize = 1024 * 1024;

/// A real folder store with switches for misbehaving like a network does.
struct Flaky {
    inner: FolderStore,
    caps: Capabilities,
    /// How many of the next writes fail with a dropped connection.
    drop_next: AtomicU32,
    /// Fail every write with "access denied".
    deny: bool,
    puts: AtomicU32,
    parts: AtomicU32,
}

impl Flaky {
    async fn new(dir: &Path) -> Self {
        let inner = FolderStore::open(dir).await.unwrap();
        // It stands in for a network store, where large files go in parts.
        let mut caps = inner.capabilities().clone();
        caps.whole_files_preferred = false;
        Self {
            caps,
            inner,
            drop_next: AtomicU32::new(0),
            deny: false,
            puts: AtomicU32::new(0),
            parts: AtomicU32::new(0),
        }
    }
    fn trouble(&self) -> Result<(), StorageError> {
        if self.deny {
            return Err(StorageError::AccessDenied("no".into()));
        }
        // Use up one planned failure, if any are left.
        let mut left = self.drop_next.load(Ordering::SeqCst);
        while left > 0 {
            match self
                .drop_next
                .compare_exchange(left, left - 1, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => return Err(StorageError::Network("connection reset".into())),
                Err(now) => left = now,
            }
        }
        Ok(())
    }
}

#[async_trait]
impl ObjectStore for Flaky {
    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }
    async fn check_connection(&self) -> Result<ConnectionReport, StorageError> {
        self.inner.check_connection().await
    }
    async fn head(&self, key: &ObjectKey) -> Result<Option<ObjectInfo>, StorageError> {
        self.inner.head(key).await
    }
    async fn get(&self, key: &ObjectKey) -> Result<Vec<u8>, StorageError> {
        self.inner.get(key).await
    }
    async fn put(&self, key: &ObjectKey, body: ByteSource, opts: &PutOptions) -> Result<PutReceipt, StorageError> {
        self.trouble()?;
        self.puts.fetch_add(1, Ordering::SeqCst);
        self.inner.put(key, body, opts).await
    }
    async fn list_page(&self, prefix: &str, continuation: Option<String>) -> Result<ListPage, StorageError> {
        self.inner.list_page(prefix, continuation).await
    }
    async fn delete(&self, key: &ObjectKey) -> Result<(), StorageError> {
        self.inner.delete(key).await
    }
}

#[async_trait]
impl MultipartStore for Flaky {
    async fn start(&self, key: &ObjectKey, opts: &PutOptions) -> Result<UploadId, StorageError> {
        self.inner.start(key, opts).await
    }
    async fn put_part(&self, id: &UploadId, number: u16, body: ByteSource) -> Result<PartReceipt, StorageError> {
        self.trouble()?;
        self.parts.fetch_add(1, Ordering::SeqCst);
        self.inner.put_part(id, number, body).await
    }
    async fn complete(&self, id: &UploadId, parts: &[PartReceipt]) -> Result<PutReceipt, StorageError> {
        self.inner.complete(id, parts).await
    }
    async fn abort(&self, id: &UploadId) -> Result<(), StorageError> {
        self.inner.abort(id).await
    }
    async fn list_parts(&self, id: &UploadId) -> Result<Vec<PartReceipt>, StorageError> {
        self.inner.list_parts(id).await
    }
}

struct World {
    dir: TempDir,
    catalog: Catalog,
    store: Flaky,
}

impl World {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = Flaky::new(&dir.path().join("bucket")).await;
        Self {
            dir,
            catalog: Catalog::in_memory().unwrap(),
            store,
        }
    }
    /// Writes a library file and describes its upload.
    fn file(&self, key: &str, contents: &[u8], write_once: bool) -> Upload {
        let path = self.dir.path().join("library").join(key);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
        Upload {
            key: ObjectKey::new(key).unwrap(),
            file: path,
            bytes: contents.len() as u64,
            write_once,
            content_type: Some("image/jpeg".into()),
            metadata: BTreeMap::new(),
        }
    }
    async fn publish(&self, uploads: &[Upload], opts: &PublishOptions) -> PublishReport {
        publish(&self.store, &self.catalog, "test", uploads, opts, &mut |_| {})
            .await
            .unwrap()
    }
    async fn stored(&self, key: &str) -> Vec<u8> {
        self.store.get(&ObjectKey::new(key).unwrap()).await.unwrap()
    }
}

fn quick() -> PublishOptions {
    PublishOptions {
        multipart_above: (2 * MIB) as u64,
        part_bytes: MIB as u64,
        retries: 3,
        retry_pause: Duration::ZERO,
        stop: None,
        over_network: false,
    }
}

/// Bytes that differ along their length, so a part in the wrong place shows.
fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| ((i / 7919) % 251) as u8).collect()
}

#[tokio::test]
async fn sends_everything_then_nothing_the_second_time() {
    let w = World::new().await;
    let uploads = [
        w.file("originals/Holidays/Spain 2009/IMG_1234.jpg", b"photo one", true),
        w.file("renditions/thumb/9f2c4be1a07d33e8.webp", b"thumb", false),
    ];
    let mut seen = Vec::new();
    let first = publish(&w.store, &w.catalog, "test", &uploads, &quick(), &mut |p| seen.push(*p))
        .await
        .unwrap();
    assert_eq!((first.uploaded, first.already_there, first.failed.len()), (2, 0, 0));
    assert_eq!(first.uploaded_bytes, 14);
    assert_eq!(seen.last(), Some(&PublishProgress { done: 2, total: 2 }));
    assert_eq!(
        w.stored("originals/Holidays/Spain 2009/IMG_1234.jpg").await,
        b"photo one"
    );

    let second = w.publish(&uploads, &quick()).await;
    assert_eq!((second.uploaded, second.already_there), (0, 2));
    assert_eq!(w.store.puts.load(Ordering::SeqCst), 2, "nothing was sent twice");
}

#[tokio::test]
async fn an_original_is_never_replaced() {
    let mut w = World::new().await;
    let original = w.file("originals/a.jpg", b"the real photo", true);
    w.publish(std::slice::from_ref(&original), &quick()).await;

    // A different file turns up under the same name, and the catalog has
    // been lost, so only the storage itself can say no.
    w.catalog = Catalog::in_memory().unwrap();
    let impostor = w.file("originals/a.jpg", b"something else entirely", true);
    let r = w.publish(&[impostor], &quick()).await;
    assert_eq!((r.uploaded, r.failed.len()), (0, 1));
    assert!(
        r.failed[0].reason.contains("never replaces an original"),
        "{}",
        r.failed[0].reason
    );
    assert_eq!(w.stored("originals/a.jpg").await, b"the real photo");

    // The same file again is simply recognised.
    let same = w.file("originals/a.jpg", b"the real photo", true);
    let r = w.publish(&[same], &quick()).await;
    assert_eq!((r.uploaded, r.already_there, r.failed.len()), (0, 1, 0));
}

#[tokio::test]
async fn write_once_still_holds_when_the_provider_cannot_enforce_it() {
    let mut w = World::new().await;
    w.store.caps.conditional_create = false;
    let original = w.file("originals/a.jpg", b"the real photo", true);
    assert_eq!(w.publish(std::slice::from_ref(&original), &quick()).await.uploaded, 1);

    w.catalog = Catalog::in_memory().unwrap();
    let impostor = w.file("originals/a.jpg", b"something else entirely", true);
    assert_eq!(w.publish(&[impostor], &quick()).await.failed.len(), 1);
    assert_eq!(w.stored("originals/a.jpg").await, b"the real photo");
    let r = w.publish(&[original], &quick()).await;
    assert_eq!(r.already_there, 1);
}

#[tokio::test]
async fn previews_can_be_replaced() {
    let mut w = World::new().await;
    w.publish(&[w.file("renditions/thumb/abc.webp", b"old thumb", false)], &quick())
        .await;
    w.catalog = Catalog::in_memory().unwrap();
    let r = w
        .publish(
            &[w.file("renditions/thumb/abc.webp", b"new, better thumb", false)],
            &quick(),
        )
        .await;
    assert_eq!(r.uploaded, 1);
    assert_eq!(w.stored("renditions/thumb/abc.webp").await, b"new, better thumb");
}

#[tokio::test]
async fn short_network_drops_are_retried() {
    let w = World::new().await;
    w.store.drop_next.store(2, Ordering::SeqCst);
    let r = w.publish(&[w.file("originals/a.jpg", b"photo", true)], &quick()).await;
    assert_eq!((r.uploaded, r.failed.len()), (1, 0));
}

#[tokio::test]
async fn a_long_outage_is_reported_and_picked_up_next_time() {
    let w = World::new().await;
    let uploads = [
        w.file("originals/a.jpg", b"photo a", true),
        w.file("originals/b.jpg", b"photo b", true),
    ];
    // Enough failures to exhaust the retries for the first file only.
    w.store.drop_next.store(4, Ordering::SeqCst);
    let r = w.publish(&uploads, &quick()).await;
    assert_eq!(
        (r.uploaded, r.failed.len()),
        (1, 1),
        "one bad file does not stop the rest"
    );
    assert_eq!(r.failed[0].key, "originals/a.jpg");
    assert!(r.failed[0].reason.contains("tried again next time"));

    let r = w.publish(&uploads, &quick()).await;
    assert_eq!((r.uploaded, r.already_there, r.failed.len()), (1, 1, 0));
}

#[tokio::test]
async fn a_refused_key_is_not_retried() {
    let mut w = World::new().await;
    w.store.deny = true;
    let r = w.publish(&[w.file("originals/a.jpg", b"photo", true)], &quick()).await;
    assert_eq!(r.failed.len(), 1);
    assert!(r.failed[0].reason.starts_with("The storage refused access"));
    assert_eq!(w.store.puts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_file_missing_from_the_library_is_reported() {
    let w = World::new().await;
    let upload = w.file("originals/a.jpg", b"photo", true);
    std::fs::remove_file(&upload.file).unwrap();
    let r = w.publish(&[upload], &quick()).await;
    assert!(
        r.failed[0].reason.starts_with("Couldn't read it from the library"),
        "{}",
        r.failed[0].reason
    );
}

#[tokio::test]
async fn large_files_go_up_in_parts_and_arrive_intact() {
    let w = World::new().await;
    let video = pattern(3 * MIB + 12_345);
    let r = w.publish(&[w.file("originals/big.mov", &video, true)], &quick()).await;
    assert_eq!((r.uploaded, r.failed.len()), (1, 0));
    assert_eq!(w.store.parts.load(Ordering::SeqCst), 4);
    assert_eq!(w.store.puts.load(Ordering::SeqCst), 0);
    assert!(w.stored("originals/big.mov").await == video);
    assert_eq!(w.catalog.multipart("test", "originals/big.mov").unwrap(), None);
}

#[tokio::test]
async fn an_interrupted_large_upload_sends_only_the_missing_parts() {
    let w = World::new().await;
    let video = pattern(4 * MIB);
    let upload = w.file("originals/big.mov", &video, true);
    let no_retries = PublishOptions { retries: 0, ..quick() };

    // The connection dies while the third part is going up.
    w.store.drop_next.store(0, Ordering::SeqCst);
    let id = w.store.start(&upload.key, &PutOptions::default()).await.unwrap();
    w.catalog
        .set_multipart("test", "originals/big.mov", Some(&id.0))
        .unwrap();
    for n in 1..=2u16 {
        let start = usize::from(n - 1) * MIB;
        w.store
            .put_part(&id, n, ByteSource::Bytes(video[start..start + MIB].to_vec()))
            .await
            .unwrap();
    }
    w.store.parts.store(0, Ordering::SeqCst);

    let r = w.publish(std::slice::from_ref(&upload), &no_retries).await;
    assert_eq!((r.uploaded, r.failed.len()), (1, 0));
    assert_eq!(
        w.store.parts.load(Ordering::SeqCst),
        2,
        "parts one and two were not sent again"
    );
    assert!(w.stored("originals/big.mov").await == video);
}

#[tokio::test]
async fn a_large_upload_the_provider_has_forgotten_starts_afresh() {
    let w = World::new().await;
    let video = pattern(3 * MIB);
    w.catalog
        .set_multipart("test", "originals/big.mov", Some("deadbeef"))
        .unwrap();
    let r = w.publish(&[w.file("originals/big.mov", &video, true)], &quick()).await;
    assert_eq!((r.uploaded, r.failed.len()), (1, 0));
    assert!(w.stored("originals/big.mov").await == video);
}

#[tokio::test]
async fn a_failed_part_leaves_the_upload_resumable() {
    let w = World::new().await;
    let video = pattern(3 * MIB);
    let upload = w.file("originals/big.mov", &video, true);
    let no_retries = PublishOptions { retries: 0, ..quick() };

    w.store.drop_next.store(1, Ordering::SeqCst);
    let r = w.publish(std::slice::from_ref(&upload), &no_retries).await;
    assert_eq!(r.failed.len(), 1);
    assert!(w.catalog.multipart("test", "originals/big.mov").unwrap().is_some());

    let r = w.publish(&[upload], &no_retries).await;
    assert_eq!(r.uploaded, 1);
    assert!(w.stored("originals/big.mov").await == video);
}

#[tokio::test]
async fn a_large_original_already_stored_is_not_sent_again() {
    let mut w = World::new().await;
    let video = pattern(3 * MIB);
    let upload = w.file("originals/big.mov", &video, true);
    w.publish(std::slice::from_ref(&upload), &quick()).await;
    w.catalog = Catalog::in_memory().unwrap();
    w.store.parts.store(0, Ordering::SeqCst);

    let r = w.publish(&[upload], &quick()).await;
    assert_eq!((r.uploaded, r.already_there), (0, 1));
    assert_eq!(w.store.parts.load(Ordering::SeqCst), 0);

    w.catalog = Catalog::in_memory().unwrap();
    let different = w.file("originals/big.mov", &pattern(3 * MIB + 1), true);
    assert_eq!(w.publish(&[different], &quick()).await.failed.len(), 1);
}

#[tokio::test]
async fn providers_without_multipart_get_one_plain_upload() {
    let mut w = World::new().await;
    w.store.caps.multipart = None;
    let video = pattern(3 * MIB);
    let r = w.publish(&[w.file("originals/big.mov", &video, true)], &quick()).await;
    assert_eq!(r.uploaded, 1);
    assert_eq!(
        (
            w.store.puts.load(Ordering::SeqCst),
            w.store.parts.load(Ordering::SeqCst)
        ),
        (1, 0)
    );
}

#[tokio::test]
async fn a_store_that_prefers_whole_files_is_never_sent_parts() {
    let mut w = World::new().await;
    w.store.caps.whole_files_preferred = true;
    let video = pattern(3 * MIB);
    let r = w.publish(&[w.file("originals/big.mov", &video, true)], &quick()).await;
    assert_eq!(r.uploaded, 1);
    assert_eq!(
        (
            w.store.puts.load(Ordering::SeqCst),
            w.store.parts.load(Ordering::SeqCst)
        ),
        (1, 0)
    );
    assert!(w.stored("originals/big.mov").await == video);
}

#[tokio::test]
async fn part_sizes_respect_the_providers_limits() {
    let mut w = World::new().await;
    // Preferred size below the provider's minimum is raised to it.
    assert_eq!(part_size(&w.store, 10 * MIB as u64, 1), MIB as u64);
    // A provider allowing only three parts gets bigger parts.
    let mut limits = w.store.caps.multipart.unwrap();
    limits.max_parts = 3;
    w.store.caps.multipart = Some(limits);
    assert_eq!(part_size(&w.store, 9 * MIB as u64, MIB as u64), 3 * MIB as u64);
    w.store.caps.multipart = None;
    assert_eq!(part_size(&w.store, 500, MIB as u64), 500);
}

#[tokio::test]
async fn documents_like_the_index_are_stored_and_replaced() {
    let w = World::new().await;
    let key = ObjectKey::new("index/abc.kvx").unwrap();
    w.store.drop_next.store(1, Ordering::SeqCst);
    put_document(&w.store, &key, b"{\"v\":1}".to_vec(), "application/json", &quick())
        .await
        .unwrap();
    put_document(&w.store, &key, b"{\"v\":2}".to_vec(), "application/json", &quick())
        .await
        .unwrap();
    assert_eq!(w.stored("index/abc.kvx").await, b"{\"v\":2}");
}

#[test]
fn reasons_read_plainly() {
    assert!(plain_reason(&StorageError::Provider("500".into())).starts_with("The storage reported a problem"));
    assert!(plain_reason(&StorageError::Network("x".into())).contains("connection kept dropping"));
}

#[tokio::test]
async fn a_large_upload_refused_at_the_last_step_is_tidied_away() {
    let w = World::new().await;
    let video = pattern(3 * MIB);
    let upload = w.file("originals/big.mov", &video, true);
    // An upload is under way from an earlier run...
    let create_only = PutOptions {
        create_only: true,
        ..Default::default()
    };
    let id = w.store.start(&upload.key, &create_only).await.unwrap();
    w.catalog
        .set_multipart("test", "originals/big.mov", Some(&id.0))
        .unwrap();
    // ...and meanwhile a different file has taken the name. The engine asks
    // first, sees it, and refuses without sending anything.
    w.store
        .inner
        .put(
            &upload.key,
            ByteSource::Bytes(b"someone else's file".to_vec()),
            &PutOptions::default(),
        )
        .await
        .unwrap();
    let r = w.publish(std::slice::from_ref(&upload), &quick()).await;
    assert_eq!(r.failed.len(), 1);
    assert_eq!(w.store.parts.load(Ordering::SeqCst), 0);
    assert_eq!(w.stored("originals/big.mov").await, b"someone else's file");

    // The provider itself refusing at completion is also handled: the
    // half-finished upload is aborted so its parts do not linger.
    let receipts = [w
        .store
        .put_part(&id, 1, ByteSource::Bytes(video[..MIB].to_vec()))
        .await
        .unwrap()];
    let err = finish(&w.store, &id, &receipts, &quick()).await.unwrap_err();
    assert!(matches!(err, StorageError::AlreadyExists(_)), "{err:?}");
    assert!(matches!(
        w.store.list_parts(&id).await,
        Err(StorageError::UploadNotFound(_))
    ));
}

#[test]
fn reading_part_of_a_missing_file_is_an_error_not_a_panic() {
    assert!(read_range(Path::new("/definitely/not/here.mov"), 0, 10).is_err());
}

fn expected(upload: &Upload) -> Expected {
    Expected {
        key: upload.key.clone(),
        bytes: upload.bytes,
        blake3: blake3_hex(&std::fs::read(&upload.file).unwrap()),
    }
}

#[tokio::test]
async fn a_good_backup_checks_out_and_reading_back_proves_it() {
    let w = World::new().await;
    let uploads = [
        w.file("originals/a.jpg", b"photo a", true),
        w.file("originals/b.jpg", b"photo b", true),
    ];
    w.publish(&uploads, &quick()).await;
    let want: Vec<Expected> = uploads.iter().map(expected).collect();

    let mut seen = Vec::new();
    let quick_check = check_backup(&w.store, &want, false, &quick(), &mut |p| seen.push(*p)).await;
    assert_eq!(
        (quick_check.present, quick_check.read_back, quick_check.problems.len()),
        (2, 0, 0)
    );
    assert_eq!(seen.last(), Some(&PublishProgress { done: 2, total: 2 }));

    let deep = check_backup(&w.store, &want, true, &quick(), &mut |_| {}).await;
    assert_eq!((deep.present, deep.read_back, deep.problems.len()), (2, 2, 0));
}

#[tokio::test]
async fn missing_and_altered_backup_copies_are_found() {
    let w = World::new().await;
    let uploads = [
        w.file("originals/gone.jpg", b"photo one", true),
        w.file("originals/shrunk.jpg", b"photo two", true),
        w.file("originals/swapped.jpg", b"photo 333", true),
        w.file("originals/fine.jpg", b"photo four", true),
    ];
    w.publish(&uploads, &quick()).await;
    let want: Vec<Expected> = uploads.iter().map(expected).collect();
    let key = |k: &str| ObjectKey::new(k).unwrap();
    w.store.delete(&key("originals/gone.jpg")).await.unwrap();
    let overwrite = PutOptions::default();
    w.store
        .inner
        .put(
            &key("originals/shrunk.jpg"),
            ByteSource::Bytes(b"x".to_vec()),
            &overwrite,
        )
        .await
        .unwrap();
    // Same size, different contents: only reading it back can tell.
    w.store
        .inner
        .put(
            &key("originals/swapped.jpg"),
            ByteSource::Bytes(b"PHOTO 333".to_vec()),
            &overwrite,
        )
        .await
        .unwrap();

    let quick_check = check_backup(&w.store, &want, false, &quick(), &mut |_| {}).await;
    assert_eq!(quick_check.present, 2, "a quick check cannot see a same-size swap");
    let reasons: Vec<&str> = quick_check.problems.iter().map(|p| p.reason.as_str()).collect();
    assert_eq!(
        reasons,
        [
            "It is missing from the backup",
            "The backup's copy is a different size from the one in your library"
        ]
    );

    let deep = check_backup(&w.store, &want, true, &quick(), &mut |_| {}).await;
    assert_eq!((deep.present, deep.read_back), (1, 1));
    assert_eq!(deep.problems.len(), 3);
    assert_eq!(deep.problems[2].key, "originals/swapped.jpg");
    assert_eq!(deep.problems[2].reason, "The backup's copy does not match the original");
}

#[tokio::test]
async fn asked_to_stop_it_finishes_nothing_more_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let store = FolderStore::open(&dir.path().join("to")).await.unwrap();
    let catalog = Catalog::in_memory().unwrap();
    let files: Vec<Upload> = (0..3)
        .map(|n| {
            let file = dir.path().join(format!("{n}.jpg"));
            std::fs::write(&file, format!("photo {n}")).unwrap();
            Upload {
                key: ObjectKey::new(format!("originals/{n}.jpg")).unwrap(),
                file,
                bytes: 7,
                write_once: true,
                content_type: None,
                metadata: BTreeMap::new(),
            }
        })
        .collect();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let opts = PublishOptions {
        stop: Some(stop.clone()),
        ..PublishOptions::default()
    };
    let report = publish(&store, &catalog, "t", &files, &opts, &mut |p| {
        if p.done == 1 {
            stop.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    })
    .await
    .unwrap();
    assert_eq!((report.uploaded, report.stopped), (1, true));
    assert!(catalog.is_uploaded("t", "originals/0.jpg").unwrap());
    assert!(!catalog.is_uploaded("t", "originals/1.jpg").unwrap());
}
