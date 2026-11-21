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
    /// Fail every question with a dropped connection, as a server out of
    /// reach does.
    out_of_reach: bool,
    puts: AtomicU32,
    parts: AtomicU32,
    /// Makes each write take a moment, as a far-off server does.
    slow: bool,
    writing: AtomicU32,
    /// The most writes that were ever under way together.
    most_at_once: AtomicU32,
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
            out_of_reach: false,
            puts: AtomicU32::new(0),
            parts: AtomicU32::new(0),
            slow: false,
            writing: AtomicU32::new(0),
            most_at_once: AtomicU32::new(0),
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
        if self.out_of_reach {
            return Err(StorageError::Network("no route to host".into()));
        }
        self.inner.head(key).await
    }
    async fn holds_exactly(&self, key: &ObjectKey, blake3: &str) -> Result<Option<bool>, StorageError> {
        self.inner.holds_exactly(key, blake3).await
    }
    async fn get(&self, key: &ObjectKey) -> Result<Vec<u8>, StorageError> {
        self.inner.get(key).await
    }
    async fn put(&self, key: &ObjectKey, body: ByteSource, opts: &PutOptions) -> Result<PutReceipt, StorageError> {
        self.trouble()?;
        self.puts.fetch_add(1, Ordering::SeqCst);
        let now = self.writing.fetch_add(1, Ordering::SeqCst) + 1;
        self.most_at_once.fetch_max(now, Ordering::SeqCst);
        if self.slow {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let done = self.inner.put(key, body, opts).await;
        self.writing.fetch_sub(1, Ordering::SeqCst);
        done
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
            learn_fingerprint: false,
            fetch: None,
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
        at_once: 1,
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
    let first = publish(&w.store, &w.catalog, "test", &uploads, &quick(), &mut |p| {
        seen.push((p.done, p.total))
    })
    .await
    .unwrap();
    assert_eq!((first.uploaded, first.already_there, first.failed.len()), (2, 0, 0));
    assert_eq!(first.uploaded_bytes, 14);
    assert_eq!(seen.last(), Some(&(2, 2)));
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
async fn a_file_missing_from_its_source_is_reported() {
    let w = World::new().await;
    let upload = w.file("originals/a.jpg", b"photo", true);
    std::fs::remove_file(&upload.file).unwrap();
    let r = w.publish(&[upload], &quick()).await;
    assert!(
        r.failed[0].reason.starts_with("Couldn't read it from its source"),
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
async fn several_files_go_at_once_when_asked_and_all_arrive() {
    let mut w = World::new().await;
    w.store.slow = true;
    let files: Vec<Vec<u8>> = (0..12u8).map(|n| vec![n; 1000 + usize::from(n)]).collect();
    let uploads: Vec<Upload> = files
        .iter()
        .enumerate()
        .map(|(n, bytes)| w.file(&format!("originals/{n}.jpg"), bytes, true))
        .collect();
    let mut seen = Vec::new();
    let opts = PublishOptions { at_once: 4, ..quick() };
    let r = publish(&w.store, &w.catalog, "test", &uploads, &opts, &mut |p| {
        seen.push(p.done)
    })
    .await
    .unwrap();
    assert_eq!((r.uploaded, r.failed.len(), r.stopped), (12, 0, false));
    assert_eq!(
        w.store.most_at_once.load(Ordering::SeqCst),
        4,
        "four at a time, never more"
    );
    assert_eq!(seen, (1..=12).collect::<Vec<_>>(), "progress counts each one once");
    for (n, bytes) in files.iter().enumerate() {
        assert!(&w.stored(&format!("originals/{n}.jpg")).await == bytes);
    }
    // Sending again sends nothing: each was recorded as it landed.
    w.store.puts.store(0, Ordering::SeqCst);
    let again = w.publish(&uploads, &opts).await;
    assert_eq!((again.uploaded, again.already_there), (0, 12));
    assert_eq!(w.store.puts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn one_at_a_time_is_still_one_at_a_time() {
    let mut w = World::new().await;
    w.store.slow = true;
    let uploads: Vec<Upload> = (0..5u8)
        .map(|n| w.file(&format!("originals/{n}.jpg"), &[n; 100], true))
        .collect();
    w.publish(&uploads, &quick()).await;
    assert_eq!(w.store.most_at_once.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn stopping_lets_the_files_under_way_finish_and_starts_no_more() {
    let mut w = World::new().await;
    w.store.slow = true;
    let uploads: Vec<Upload> = (0..10u8)
        .map(|n| w.file(&format!("originals/{n}.jpg"), &[n; 100], true))
        .collect();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let opts = PublishOptions {
        at_once: 3,
        stop: Some(stop.clone()),
        ..quick()
    };
    let r = publish(&w.store, &w.catalog, "test", &uploads, &opts, &mut |p| {
        if p.done == 1 {
            stop.store(true, Ordering::SeqCst);
        }
    })
    .await
    .unwrap();
    assert!(r.stopped);
    assert!(r.uploaded >= 3 && r.uploaded < 10, "{} were sent", r.uploaded);
    assert_eq!(r.failed.len(), 0, "a file under way is finished, not abandoned");
}

#[tokio::test]
async fn a_different_file_of_the_same_name_and_size_is_never_counted_as_the_original() {
    let w = World::new().await;
    // Something else is already in the backup under this name, the same size.
    let other = vec![1u8; 5000];
    let already = w.file("originals/photo.jpg", &other, true);
    w.publish(std::slice::from_ref(&already), &quick()).await;

    // A fresh record, as on another computer, and a different photo of that name.
    let fresh = Catalog::in_memory().unwrap();
    let mine = vec![2u8; 5000];
    let mut upload = w.file("originals/photo.jpg", &mine, true);
    upload
        .metadata
        .insert("blake3".into(), blake3::hash(&mine).to_hex().to_string());
    let r = publish(&w.store, &fresh, "test", &[upload.clone()], &quick(), &mut |_| {})
        .await
        .unwrap();
    assert_eq!(
        (r.uploaded, r.already_there, r.failed.len()),
        (0, 0, 1),
        "refused, not counted as there"
    );
    assert!(
        w.stored("originals/photo.jpg").await == other,
        "and what was there is untouched"
    );

    // The very same file already there is recognised as itself.
    let mut same = w.file("originals/photo.jpg", &other, true);
    same.metadata
        .insert("blake3".into(), blake3::hash(&other).to_hex().to_string());
    let r = publish(&w.store, &fresh, "test", &[same], &quick(), &mut |_| {})
        .await
        .unwrap();
    assert_eq!((r.already_there, r.failed.len()), (1, 0));
}

#[tokio::test]
async fn a_file_not_yet_fingerprinted_is_not_counted_as_there_on_its_size_alone() {
    let w = World::new().await;
    let other = vec![1u8; 5000];
    let already = w.file("originals/photo.jpg", &other, true);
    w.publish(std::slice::from_ref(&already), &quick()).await;

    // A file a search only sampled, of that name and size, but different.
    let fresh = Catalog::in_memory().unwrap();
    let mut upload = w.file("originals/photo.jpg", &[2u8; 5000], true);
    upload.learn_fingerprint = true;
    let r = publish(&w.store, &fresh, "test", &[upload], &quick(), &mut |_| {})
        .await
        .unwrap();
    assert_eq!(
        (r.uploaded, r.already_there, r.failed.len()),
        (0, 0, 1),
        "a store that can read itself is asked, not believed on size"
    );
    assert!(w.stored("originals/photo.jpg").await == other);

    // The very same file is recognised as itself.
    let mut same = w.file("originals/photo.jpg", &other, true);
    same.learn_fingerprint = true;
    let r = publish(&w.store, &fresh, "test", &[same], &quick(), &mut |_| {})
        .await
        .unwrap();
    assert_eq!((r.already_there, r.failed.len()), (1, 0));
}

#[tokio::test]
async fn a_file_not_yet_fingerprinted_is_fingerprinted_as_it_is_sent() {
    let w = World::new().await;
    let small = vec![5u8; 4000];
    let big = pattern(3 * MIB);
    let mut uploads = vec![
        w.file("originals/small.jpg", &small, true),
        w.file("originals/big.mov", &big, true),
        w.file("originals/known.jpg", &[1u8; 100], true),
    ];
    uploads[0].learn_fingerprint = true;
    uploads[1].learn_fingerprint = true;
    let r = w.publish(&uploads, &quick()).await;
    assert_eq!((r.uploaded, r.failed.len()), (3, 0));
    let mut learned = r.learned.clone();
    learned.sort();
    // Whole and in parts alike, and only for the ones that asked.
    assert_eq!(
        learned,
        vec![
            ("originals/big.mov".to_string(), blake3::hash(&big).to_hex().to_string()),
            (
                "originals/small.jpg".to_string(),
                blake3::hash(&small).to_hex().to_string()
            ),
        ]
    );
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
    let quick_check = check_backup(&w.store, &want, false, &quick(), &mut |p| seen.push((p.done, p.total))).await;
    assert_eq!(
        (quick_check.present, quick_check.read_back, quick_check.problems.len()),
        (2, 0, 0)
    );
    assert_eq!(seen.last(), Some(&(2, 2)));

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
            "The backup's copy is a different size from the original"
        ]
    );

    let deep = check_backup(&w.store, &want, true, &quick(), &mut |_| {}).await;
    assert_eq!((deep.present, deep.read_back), (1, 1));
    assert_eq!(deep.problems.len(), 3);
    assert_eq!(deep.problems[2].key, "originals/swapped.jpg");
    assert_eq!(deep.problems[2].reason, "The backup's copy does not match the original");
    // What was found of each, for Keevoro's record.
    use keevoro_catalog::Seen;
    assert_eq!(
        deep.seen,
        [
            ("originals/gone.jpg".to_string(), Seen::Missing),
            ("originals/shrunk.jpg".to_string(), Seen::NotAsSent),
            ("originals/swapped.jpg".to_string(), Seen::NotAsSent),
            ("originals/fine.jpg".to_string(), Seen::There),
        ]
    );
}

#[tokio::test]
async fn a_check_that_cannot_reach_the_storage_finds_nothing_gone() {
    let mut w = World::new().await;
    let upload = w.file("originals/a.jpg", b"photo a", true);
    w.publish(std::slice::from_ref(&upload), &quick()).await;
    w.store.out_of_reach = true;
    let check = check_backup(&w.store, &[expected(&upload)], false, &quick(), &mut |_| {}).await;
    assert_eq!(check.problems.len(), 1, "it is said");
    assert!(check.seen.is_empty(), "but nothing is taken for gone");
}

#[tokio::test]
async fn a_file_found_gone_is_sent_again_and_counted_as_restored() {
    let w = World::new().await;
    let uploads = [
        w.file("originals/a.jpg", b"photo a", true),
        w.file("originals/b.jpg", b"photo b", true),
        w.file("renditions/thumb/a.webp", b"thumb", false),
    ];
    w.publish(&uploads, &quick()).await;
    // Someone deleted one original and its preview; a look found them gone.
    for key in ["originals/a.jpg", "renditions/thumb/a.webp"] {
        w.store.inner.delete(&ObjectKey::new(key).unwrap()).await.unwrap();
    }
    w.catalog
        .set_seen(
            "test",
            &[
                ("originals/a.jpg".into(), keevoro_catalog::Seen::Missing),
                ("renditions/thumb/a.webp".into(), keevoro_catalog::Seen::Missing),
            ],
        )
        .unwrap();
    let again = w.publish(&uploads, &quick()).await;
    assert_eq!((again.uploaded, again.restored, again.already_there), (2, 2, 1));
    assert_eq!(w.stored("originals/a.jpg").await, b"photo a");
    assert!(w.catalog.is_uploaded("test", "originals/a.jpg").unwrap());
    // And next time, nothing.
    let third = w.publish(&uploads, &quick()).await;
    assert_eq!((third.uploaded, third.restored, third.already_there), (0, 0, 3));
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
                learn_fingerprint: false,
                fetch: None,
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

/// Stands in for a file in cloud storage: each fetch writes it into a
/// staging folder here, and drops the connection first as often as asked.
#[derive(Debug)]
struct FromTheCloud {
    contents: Vec<u8>,
    staging: PathBuf,
    drops: AtomicU32,
    fetches: AtomicU32,
}

#[async_trait]
impl Fetch for FromTheCloud {
    async fn fetch(&self) -> Result<Fetched, StorageError> {
        let n = self.fetches.fetch_add(1, Ordering::SeqCst);
        if self.drops.load(Ordering::SeqCst) > 0 {
            self.drops.fetch_sub(1, Ordering::SeqCst);
            return Err(StorageError::Network("connection reset".into()));
        }
        std::fs::create_dir_all(&self.staging)?;
        // Each fetch has a file of its own, as the real one has.
        static EVERY: AtomicU32 = AtomicU32::new(0);
        let file = self
            .staging
            .join(format!("fetch-{n}-{}", EVERY.fetch_add(1, Ordering::SeqCst)));
        std::fs::write(&file, &self.contents)?;
        Ok(Fetched {
            file,
            bytes: self.contents.len() as u64,
            blake3: blake3_hex(&self.contents),
        })
    }
}

impl World {
    /// A file in cloud storage, to be fetched when its turn comes.
    fn in_the_cloud(&self, key: &str, contents: &[u8], drops: u32) -> (Upload, std::sync::Arc<FromTheCloud>) {
        let cloud = std::sync::Arc::new(FromTheCloud {
            contents: contents.to_vec(),
            staging: self.dir.path().join("staging"),
            drops: AtomicU32::new(drops),
            fetches: AtomicU32::new(0),
        });
        let upload = Upload {
            key: ObjectKey::new(key).unwrap(),
            // Never opened: the file is fetched.
            file: PathBuf::from(format!("s3://bucket/{key}")),
            bytes: contents.len() as u64,
            write_once: true,
            content_type: Some("image/jpeg".into()),
            metadata: BTreeMap::new(),
            learn_fingerprint: true,
            fetch: Some(cloud.clone()),
        };
        (upload, cloud)
    }

    fn staged(&self) -> usize {
        std::fs::read_dir(self.dir.path().join("staging")).map_or(0, |d| d.count())
    }
}

#[tokio::test]
async fn a_file_in_cloud_storage_is_fetched_sent_proved_and_its_copy_here_removed() {
    let w = World::new().await;
    let body = pattern(300_000);
    // The connection drops twice while it is fetched: tried again.
    let (upload, cloud) = w.in_the_cloud("originals/Spain/IMG_1.jpg", &body, 2);
    let report = w.publish(std::slice::from_ref(&upload), &quick()).await;
    assert_eq!((report.uploaded, report.failed.len()), (1, 0), "{:?}", report.failed);
    assert_eq!(w.stored("originals/Spain/IMG_1.jpg").await, body);
    assert_eq!(
        report.learned,
        [("originals/Spain/IMG_1.jpg".to_string(), blake3_hex(&body))]
    );
    assert_eq!(cloud.fetches.load(Ordering::SeqCst), 3);
    assert_eq!(w.staged(), 0, "nothing is left on this computer");
    // Once it is there it is never fetched again: nothing already safely
    // sent is read a second time.
    let again = w.publish(&[upload], &quick()).await;
    assert_eq!((again.already_there, cloud.fetches.load(Ordering::SeqCst)), (1, 3));
}

#[tokio::test]
async fn a_fetched_file_that_is_a_copy_of_one_already_placed_is_not_sent_again() {
    let w = World::new().await;
    let body = pattern(5_000);
    // The same photo, already given its place from a folder source.
    w.catalog
        .mark_placed(&blake3_hex(&body), "originals/Card/IMG_1.jpg", 5_000)
        .unwrap();
    let (upload, _) = w.in_the_cloud("originals/Bucket/IMG_1.jpg", &body, 0);
    let report = w.publish(&[upload], &quick()).await;
    assert_eq!((report.uploaded, report.copies_left_out), (0, 1));
    assert_eq!(
        report.learned.len(),
        1,
        "its fingerprint is learnt, so it goes by it from now on"
    );
    assert!(!w.catalog.is_uploaded("test", "originals/Bucket/IMG_1.jpg").unwrap());
    assert_eq!(w.staged(), 0);

    // Two copies fetched in one pass: the second is not sent.
    let w = World::new().await;
    let (one, _) = w.in_the_cloud("originals/A/x.jpg", &body, 0);
    let (two, _) = w.in_the_cloud("originals/B/x.jpg", &body, 0);
    let report = w.publish(&[one, two], &quick()).await;
    assert_eq!((report.uploaded, report.copies_left_out), (1, 1));
}

#[tokio::test]
async fn a_fetched_file_that_is_not_what_keevoro_labelled_it_is_refused() {
    let w = World::new().await;
    let (mut upload, _) = w.in_the_cloud("originals/IMG_1.jpg", b"changed since", 0);
    upload
        .metadata
        .insert("blake3".into(), blake3_hex(b"what was labelled"));
    upload.learn_fingerprint = false;
    let report = w.publish(&[upload], &quick()).await;
    assert_eq!((report.uploaded, report.failed.len()), (0, 1));
    assert!(w
        .store
        .head(&ObjectKey::new("originals/IMG_1.jpg").unwrap())
        .await
        .unwrap()
        .is_none());
    assert_eq!(w.staged(), 0);
}

#[tokio::test]
async fn never_more_than_two_files_from_cloud_storage_are_on_this_computer_at_once() {
    let w = World::new().await;
    let mut uploads = Vec::new();
    let mut clouds = Vec::new();
    for n in 0..8 {
        let (upload, cloud) = w.in_the_cloud(&format!("originals/{n}.jpg"), &pattern(20_000 + n), 0);
        uploads.push(upload);
        clouds.push(cloud);
    }
    let opts = PublishOptions { at_once: 6, ..quick() };
    let mut most = 0;
    let report = publish(&w.store, &w.catalog, "test", &uploads, &opts, &mut |_| {
        most = most.max(w.staged());
    })
    .await
    .unwrap();
    assert_eq!(report.uploaded, 8);
    assert!(most <= FETCHED_AT_ONCE, "{most} were here at once");
    assert_eq!(w.staged(), 0);
}
