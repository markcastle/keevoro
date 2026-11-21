//! The shared test suite every storage provider must pass.
//!
//! This is how keevoro keeps providers substitutable: the folder provider,
//! MinIO in Docker, R2 and S3 all run exactly these checks. A provider ships
//! only when it passes.
//!
//! Everything is written under a unique prefix and cleaned up afterwards, so
//! the suite is safe to run against a real bucket.
//!
//! ```ignore
//! #[tokio::test]
//! async fn conforms() {
//!     let store = MyProvider::new(...);
//!     keevoro_storage::conformance::run_all(&store).await;
//! }
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::{ByteSource, MultipartStore, ObjectKey, PartReceipt, PutOptions, StorageError};
use std::time::{SystemTime, UNIX_EPOCH};

/// Runs every check. Panics with a description on the first failure.
pub async fn run_all(store: &dyn MultipartStore) {
    let prefix = unique_prefix();
    connection_check_passes(store).await;
    put_get_head_roundtrip(store, &prefix).await;
    missing_objects(store, &prefix).await;
    create_only_refuses_overwrite(store, &prefix).await;
    put_from_file(store, &prefix).await;
    metadata_roundtrip_when_supported(store, &prefix).await;
    list_by_prefix_in_key_order(store, &prefix).await;
    delete_removes(store, &prefix).await;
    if store.capabilities().multipart.is_some() {
        multipart_roundtrip_and_resume(store, &prefix).await;
        multipart_abort_leaves_nothing(store, &prefix).await;
        multipart_rejects_bad_part_lists(store, &prefix).await;
    }
    cleanup(store, &prefix).await;
}

fn unique_prefix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("conformance/{nanos}-{}", std::process::id())
}

fn key(prefix: &str, rest: &str) -> ObjectKey {
    ObjectKey::new(format!("{prefix}/{rest}")).expect("test key is valid")
}

async fn connection_check_passes(store: &dyn MultipartStore) {
    let r = store.check_connection().await.expect("check_connection");
    assert!(
        r.can_read && r.can_write,
        "connection check should report read and write access"
    );
}

async fn put_get_head_roundtrip(store: &dyn MultipartStore, p: &str) {
    let k = key(p, "Holidays/Spain 2009/IMG 0001.jpg");
    let body = b"not really a jpeg".to_vec();
    let r = store
        .put(&k, ByteSource::Bytes(body.clone()), &PutOptions::default())
        .await
        .expect("put");
    assert_eq!(r.key, k);
    assert_eq!(r.bytes, body.len() as u64);
    assert_eq!(store.get(&k).await.expect("get"), body);
    let info = store.head(&k).await.expect("head").expect("object should exist");
    assert_eq!(info.bytes, body.len() as u64);
    assert_eq!(info.key, k);
}

async fn missing_objects(store: &dyn MultipartStore, p: &str) {
    let k = key(p, "missing.jpg");
    assert!(
        store.head(&k).await.expect("head").is_none(),
        "head of a missing key should be None"
    );
    assert!(
        matches!(store.get(&k).await, Err(StorageError::NotFound(_))),
        "get of a missing key should be NotFound"
    );
}

async fn create_only_refuses_overwrite(store: &dyn MultipartStore, p: &str) {
    let k = key(p, "write-once.jpg");
    let once = PutOptions {
        create_only: true,
        ..Default::default()
    };
    store
        .put(&k, ByteSource::Bytes(b"first".to_vec()), &once)
        .await
        .expect("first create");
    let second = store.put(&k, ByteSource::Bytes(b"second".to_vec()), &once).await;
    assert!(
        matches!(second, Err(StorageError::AlreadyExists(_))),
        "create_only must refuse to overwrite"
    );
    assert_eq!(
        store.get(&k).await.expect("get"),
        b"first",
        "original must be untouched"
    );
    // A normal put may overwrite.
    store
        .put(&k, ByteSource::Bytes(b"third".to_vec()), &PutOptions::default())
        .await
        .expect("overwrite");
    assert_eq!(store.get(&k).await.expect("get"), b"third");
}

async fn put_from_file(store: &dyn MultipartStore, p: &str) {
    // Unique per call, not just per process: two providers are often checked
    // by two tests in the same process, and they must not share this file.
    static CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("keevoro-conformance-{}-{call}.bin", std::process::id()));
    let body: Vec<u8> = (0..=255u8).cycle().take(70_000).collect();
    std::fs::write(&path, &body).expect("write temp file");
    let k = key(p, "from-file.bin");
    let r = store
        .put(&k, ByteSource::File(path.clone()), &PutOptions::default())
        .await;
    let _ = std::fs::remove_file(&path);
    assert_eq!(r.expect("put from file").bytes, body.len() as u64);
    assert_eq!(store.get(&k).await.expect("get"), body);
}

async fn metadata_roundtrip_when_supported(store: &dyn MultipartStore, p: &str) {
    if !store.capabilities().custom_metadata {
        return;
    }
    let k = key(p, "with-metadata.jpg");
    let mut opts = PutOptions::default();
    opts.metadata.insert("blake3".into(), "9f2c4be1a07d33e8".into());
    opts.metadata.insert("taken".into(), "2009-07-14T21:03:11".into());
    store
        .put(&k, ByteSource::Bytes(b"x".to_vec()), &opts)
        .await
        .expect("put");
    let info = store.head(&k).await.expect("head").expect("exists");
    assert_eq!(info.metadata, opts.metadata, "metadata must round-trip");
}

async fn list_by_prefix_in_key_order(store: &dyn MultipartStore, p: &str) {
    for name in ["list/b.jpg", "list/a.jpg", "list/sub/c.jpg", "listing-not-me.jpg"] {
        store
            .put(&key(p, name), ByteSource::Bytes(b"x".to_vec()), &PutOptions::default())
            .await
            .expect("put");
    }
    let mut keys = Vec::new();
    let mut next = None;
    loop {
        let page = store.list_page(&format!("{p}/list/"), next).await.expect("list");
        keys.extend(page.objects.into_iter().map(|o| o.key.to_string()));
        match page.next {
            Some(n) => next = Some(n),
            None => break,
        }
    }
    let want: Vec<String> = ["list/a.jpg", "list/b.jpg", "list/sub/c.jpg"]
        .iter()
        .map(|n| format!("{p}/{n}"))
        .collect();
    assert_eq!(keys, want, "list must return exactly the prefix's keys, in order");

    // Everything at once says the same, with each one's size.
    store
        .put(
            &key(p, "list/sub/longer.jpg"),
            ByteSource::Bytes(b"twelve bytes".to_vec()),
            &PutOptions::default(),
        )
        .await
        .expect("put");
    let mut all: Vec<(String, u64)> = store
        .list_all(&format!("{p}/list/"))
        .await
        .expect("list_all")
        .into_iter()
        .map(|o| (o.key.to_string(), o.bytes))
        .collect();
    all.sort();
    let want: Vec<(String, u64)> = [
        ("list/a.jpg", 1),
        ("list/b.jpg", 1),
        ("list/sub/c.jpg", 1),
        ("list/sub/longer.jpg", 12),
    ]
    .iter()
    .map(|(n, bytes)| (format!("{p}/{n}"), *bytes))
    .collect();
    assert_eq!(
        all, want,
        "list_all must return exactly the prefix's keys and their sizes"
    );
    assert!(
        store
            .list_all(&format!("{p}/nothing-here/"))
            .await
            .expect("list_all of nothing")
            .is_empty(),
        "a prefix with nothing under it lists nothing"
    );
}

async fn delete_removes(store: &dyn MultipartStore, p: &str) {
    let k = key(p, "delete-me.jpg");
    store
        .put(&k, ByteSource::Bytes(b"x".to_vec()), &PutOptions::default())
        .await
        .expect("put");
    store.delete(&k).await.expect("delete");
    assert!(store.head(&k).await.expect("head").is_none());
    // Deleting something already gone is not an error.
    store.delete(&k).await.expect("delete again");
}

async fn multipart_roundtrip_and_resume(store: &dyn MultipartStore, p: &str) {
    let limits = store.capabilities().multipart.expect("multipart");
    let part = usize::try_from(limits.min_part_bytes).expect("part size fits in memory");
    let k = key(p, "big/video.mov");
    let id = store.start(&k, &PutOptions::default()).await.expect("start");

    let one: Vec<u8> = vec![1; part];
    let two: Vec<u8> = vec![2; part];
    let three: Vec<u8> = vec![3; 1000];
    let r1 = store
        .put_part(&id, 1, ByteSource::Bytes(one.clone()))
        .await
        .expect("part 1");
    let r2 = store
        .put_part(&id, 2, ByteSource::Bytes(two.clone()))
        .await
        .expect("part 2");

    // Simulate a crash: the engine asks which parts the provider already has.
    let held = store.list_parts(&id).await.expect("list_parts");
    assert_eq!(
        held,
        vec![r1.clone(), r2.clone()],
        "list_parts must report uploaded parts in order"
    );

    let r3 = store
        .put_part(&id, 3, ByteSource::Bytes(three.clone()))
        .await
        .expect("part 3");
    let done = store.complete(&id, &[r1, r2, r3]).await.expect("complete");
    let total = (one.len() + two.len() + three.len()) as u64;
    assert_eq!(done.bytes, total);

    let got = store.get(&k).await.expect("get");
    assert_eq!(got.len() as u64, total);
    assert!(got[..part].iter().all(|b| *b == 1) && got[part..2 * part].iter().all(|b| *b == 2));
    assert!(got[2 * part..].iter().all(|b| *b == 3));
}

async fn multipart_abort_leaves_nothing(store: &dyn MultipartStore, p: &str) {
    let k = key(p, "big/aborted.mov");
    let id = store.start(&k, &PutOptions::default()).await.expect("start");
    store
        .put_part(&id, 1, ByteSource::Bytes(vec![9; 10]))
        .await
        .expect("part");
    store.abort(&id).await.expect("abort");
    assert!(
        store.head(&k).await.expect("head").is_none(),
        "aborted upload must not create the object"
    );
    assert!(store.list_parts(&id).await.is_err(), "aborted upload id must be gone");
}

async fn multipart_rejects_bad_part_lists(store: &dyn MultipartStore, p: &str) {
    let k = key(p, "big/bad.mov");
    let id = store.start(&k, &PutOptions::default()).await.expect("start");
    let r1 = store
        .put_part(&id, 1, ByteSource::Bytes(vec![1; 10]))
        .await
        .expect("part");
    let gap = PartReceipt {
        number: 3,
        ..r1.clone()
    };
    assert!(
        store.complete(&id, &[]).await.is_err(),
        "empty part list must be rejected"
    );
    assert!(store.complete(&id, &[r1, gap]).await.is_err(), "gaps must be rejected");
    store.abort(&id).await.expect("abort");
}

async fn cleanup(store: &dyn MultipartStore, p: &str) {
    let mut next = None;
    loop {
        let page = store.list_page(&format!("{p}/"), next).await.expect("list for cleanup");
        for o in page.objects {
            store.delete(&o.key).await.expect("cleanup delete");
        }
        match page.next {
            Some(n) => next = Some(n),
            None => break,
        }
    }
}
