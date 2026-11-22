//! Two kinds of test live here.
//!
//! The pure ones (presets, upload ids, error classification) always run.
//!
//! The ones that talk to a server run only when these are set, as they are
//! in CI where MinIO is started first:
//!
//! ```text
//! KEEVORO_S3_TEST_ENDPOINT=http://127.0.0.1:9000
//! KEEVORO_S3_TEST_BUCKET=keevoro-test
//! KEEVORO_S3_TEST_ACCESS_KEY=...
//! KEEVORO_S3_TEST_SECRET_KEY=...
//! ```
//!
//! Point them at a throwaway bucket. The suite cleans up after itself, but
//! it is not something to aim at a family archive.

use super::*;

fn settings(preset: Preset) -> S3Settings {
    S3Settings {
        preset,
        bucket: "family-photos".into(),
        access_key_id: " AKIAEXAMPLE ".into(),
        secret_access_key: "secret".into(),
    }
}

#[test]
fn r2_needs_only_an_account_id() {
    let id = "0123456789abcdef0123456789ABCDEF";
    let r = Preset::CloudflareR2 {
        account_id: format!(" {id} "),
    }
    .resolve()
    .unwrap();
    assert_eq!(
        r.endpoint.as_deref(),
        Some("https://0123456789abcdef0123456789abcdef.r2.cloudflarestorage.com")
    );
    assert_eq!((r.region.as_str(), r.path_style), ("auto", false));

    let err = Preset::CloudflareR2 {
        account_id: "my-account".into(),
    }
    .resolve()
    .unwrap_err();
    assert!(err.to_string().contains("Cloudflare account ID"), "{err}");
}

#[test]
fn amazon_needs_only_a_region() {
    let r = Preset::AmazonS3 {
        region: "EU-West-2".into(),
    }
    .resolve()
    .unwrap();
    assert_eq!((r.endpoint, r.region.as_str()), (None, "eu-west-2"));
    assert!(Preset::AmazonS3 {
        region: "london please".into()
    }
    .resolve()
    .is_err());
    assert!(Preset::AmazonS3 { region: String::new() }.resolve().is_err());
}

#[test]
fn other_servers_take_an_address() {
    let r = Preset::Custom {
        endpoint: "http://nas.local:9000/".into(),
        region: Some("  ".into()),
        path_style: true,
    }
    .resolve()
    .unwrap();
    assert_eq!(r.endpoint.as_deref(), Some("http://nas.local:9000"));
    assert_eq!((r.region.as_str(), r.path_style), ("us-east-1", true));

    let err = Preset::Custom {
        endpoint: "nas.local".into(),
        region: None,
        path_style: true,
    }
    .resolve()
    .unwrap_err();
    assert!(err.to_string().contains("should start with https://"));
}

#[test]
fn google_needs_nothing_but_the_bucket_and_its_keys() {
    let r = Preset::GoogleCloud.resolve().unwrap();
    assert_eq!(r.endpoint.as_deref(), Some("https://storage.googleapis.com"));
    assert_eq!((r.region.as_str(), r.path_style, r.google), ("auto", true, true));
    for other in [
        Preset::CloudflareR2 {
            account_id: "0123456789abcdef0123456789abcdef".into(),
        },
        Preset::AmazonS3 {
            region: "eu-west-2".into(),
        },
    ] {
        assert!(!other.resolve().unwrap().google, "{other:?}");
    }
    let mut keys = settings(Preset::GoogleCloud);
    keys.access_key_id = "GOOGEXAMPLE1234567890AB".into();
    let store = S3Store::connect(&keys).unwrap();
    assert_eq!(store.target_name(), "s3:https://storage.googleapis.com/family-photos");
    assert!(!store.target_name().contains("GOOG"));
}

#[test]
fn connecting_checks_the_bucket_name_and_names_the_target_without_secrets() {
    let aws = S3Store::connect(&settings(Preset::AmazonS3 {
        region: "eu-west-2".into(),
    }))
    .unwrap();
    assert_eq!(aws.target_name(), "s3:aws:eu-west-2/family-photos");
    let minio = S3Store::connect(&settings(Preset::Custom {
        endpoint: "http://127.0.0.1:9000".into(),
        region: None,
        path_style: true,
    }))
    .unwrap();
    assert_eq!(minio.target_name(), "s3:http://127.0.0.1:9000/family-photos");
    assert!(!minio.target_name().contains("secret") && !minio.target_name().contains("AKIA"));
    assert!(minio.capabilities().conditional_create && minio.capabilities().custom_metadata);

    for bad in ["", "two words", "a/b"] {
        let mut s = settings(Preset::AmazonS3 {
            region: "eu-west-2".into(),
        });
        s.bucket = bad.into();
        assert!(S3Store::connect(&s).is_err(), "{bad:?}");
    }
    assert!(S3Store::connect(&settings(Preset::AmazonS3 { region: "?".into() })).is_err());
}

#[test]
fn upload_ids_carry_their_key_and_write_once_flag() {
    let packed = PackedUploadId {
        create_only: true,
        upload_id: "VXBsb2FkIElE.abc-123".into(),
        key: "originals/Holidays/Spain 2009/big video.mov".into(),
    };
    let id = packed.pack();
    assert_eq!(PackedUploadId::unpack(&id).unwrap(), packed);
    let plain = PackedUploadId {
        create_only: false,
        ..packed
    };
    assert_eq!(PackedUploadId::unpack(&plain.pack()).unwrap(), plain);

    for nonsense in ["", "deadbeef", "1 key", "2abc key", "1abc ", " "] {
        let err = PackedUploadId::unpack(&UploadId(nonsense.into())).unwrap_err();
        assert!(matches!(err, StorageError::UploadNotFound(_)), "{nonsense:?}");
    }
}

#[test]
fn server_answers_become_the_right_kind_of_error() {
    let c = |code: Option<&str>, status: u16| classify(code, Some(status), "k", "detail".into());
    assert!(matches!(c(Some("NoSuchKey"), 404), StorageError::NotFound(_)));
    assert!(matches!(c(None, 404), StorageError::NotFound(_)), "head gives no code");
    assert!(matches!(c(Some("NoSuchUpload"), 404), StorageError::UploadNotFound(_)));
    assert!(matches!(
        c(Some("PreconditionFailed"), 412),
        StorageError::AlreadyExists(_)
    ));
    assert!(matches!(
        c(Some("ConditionalRequestConflict"), 409),
        StorageError::AlreadyExists(_)
    ));
    assert!(matches!(c(Some("AccessDenied"), 403), StorageError::AccessDenied(_)));
    assert!(matches!(
        c(Some("SignatureDoesNotMatch"), 403),
        StorageError::AccessDenied(_)
    ));
    assert!(matches!(c(Some("InvalidPart"), 400), StorageError::InvalidParts(_)));
    assert!(matches!(c(Some("EntityTooSmall"), 400), StorageError::InvalidParts(_)));
    assert!(matches!(c(Some("Weird"), 400), StorageError::Provider(_)));

    // Only a busy or unwell server is worth trying again.
    for (code, status) in [
        (Some("SlowDown"), 503),
        (Some("InternalError"), 500),
        (None, 502),
        (None, 429),
    ] {
        assert!(c(code, status).is_retryable(), "{code:?} {status}");
    }
    assert!(!c(Some("AccessDenied"), 403).is_retryable());
    assert!(matches!(
        classify(None, None, "k", "d".into()),
        StorageError::Provider(_)
    ));
}

#[test]
fn part_lists_with_gaps_are_refused_before_they_reach_the_server() {
    let part = |number| PartReceipt {
        number,
        bytes: 10,
        etag: "e".into(),
    };
    assert!(check_parts(&[part(1), part(2), part(3)]).is_ok());
    assert!(matches!(check_parts(&[]), Err(StorageError::InvalidParts(_))));
    assert!(matches!(
        check_parts(&[part(1), part(3)]),
        Err(StorageError::InvalidParts(_))
    ));
    assert!(matches!(check_parts(&[part(2)]), Err(StorageError::InvalidParts(_))));
}

#[tokio::test]
async fn a_missing_file_is_an_error_before_anything_is_sent() {
    let missing = ByteSource::File("/definitely/not/here.jpg".into());
    assert!(matches!(body_of(missing).await, Err(StorageError::Io(_))));
    let (_, len) = body_of(ByteSource::Bytes(vec![1, 2, 3])).await.unwrap();
    assert_eq!(len, 3);
}

/// A stand-in for an S3-compatible server on this computer, which answers
/// just enough for one upload of each kind and a listing, and keeps every
/// request it was sent so a test can see exactly what went out.
mod stand_in {
    use std::collections::BTreeMap;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    #[derive(Debug, Clone)]
    pub struct Seen {
        pub method: String,
        pub target: String,
        /// Header names in lower case.
        pub headers: BTreeMap<String, String>,
        pub body: Vec<u8>,
    }

    pub struct StandIn {
        pub address: String,
        pub seen: Arc<Mutex<Vec<Seen>>>,
    }

    fn answer(seen: &Seen) -> (u16, String) {
        let target = seen.target.as_str();
        match seen.method.as_str() {
            "POST" if target.contains("uploads") => (
                200,
                "<InitiateMultipartUploadResult><Bucket>family-photos</Bucket><Key>k</Key>\
                 <UploadId>example-upload</UploadId></InitiateMultipartUploadResult>"
                    .into(),
            ),
            "POST" => (
                200,
                "<CompleteMultipartUploadResult><Bucket>family-photos</Bucket><Key>k</Key>\
                 <ETag>\"done\"</ETag></CompleteMultipartUploadResult>"
                    .into(),
            ),
            "GET" => (
                200,
                "<ListBucketResult><Name>family-photos</Name><Prefix>Scans/</Prefix><KeyCount>1</KeyCount>\
                 <MaxKeys>1000</MaxKeys><IsTruncated>false</IsTruncated><Contents><Key>Scans/a.jpg</Key>\
                 <LastModified>2026-01-02T03:04:05.000Z</LastModified><ETag>\"e1\"</ETag><Size>7</Size>\
                 <StorageClass>NEARLINE</StorageClass></Contents></ListBucketResult>"
                    .into(),
            ),
            "DELETE" => (204, String::new()),
            _ => (200, String::new()),
        }
    }

    pub fn start() -> StandIn {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&seen);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    continue;
                }
                let mut parts = line.split_whitespace();
                let (method, target) = (parts.next().unwrap_or_default(), parts.next().unwrap_or_default());
                let mut headers = BTreeMap::new();
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).unwrap();
                    let header = header.trim_end();
                    if header.is_empty() {
                        break;
                    }
                    if let Some((name, value)) = header.split_once(':') {
                        headers.insert(name.trim().to_lowercase(), value.trim().to_string());
                    }
                }
                if headers
                    .get("expect")
                    .is_some_and(|e| e.eq_ignore_ascii_case("100-continue"))
                {
                    stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").unwrap();
                }
                let length = headers.get("content-length").and_then(|l| l.parse().ok()).unwrap_or(0);
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let request = Seen {
                    method: method.to_string(),
                    target: target.to_string(),
                    headers,
                    body,
                };
                let (status, reply) = answer(&request);
                kept.lock().unwrap().push(request);
                let head = format!(
                    "HTTP/1.1 {status} OK\r\nETag: \"e1\"\r\nContent-Type: application/xml\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n",
                    reply.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        StandIn { address, seen }
    }
}

/// A store that talks to the stand-in as the given kind would talk to its
/// real address.
fn speaking_to(stand_in: &stand_in::StandIn, kind: Preset) -> S3Store {
    let mut resolved = kind.resolve().unwrap();
    resolved.endpoint = Some(stand_in.address.clone());
    resolved.path_style = true;
    let mut keys = settings(kind);
    keys.access_key_id = "GOOGEXAMPLE1234567890AB".into();
    S3Store::connect_resolved(&resolved, &keys).unwrap()
}

/// Everything one backup sends: a small original, a big one in parts,
/// and a listing.
async fn back_up_one_of_each(store: &S3Store) -> Vec<ObjectInfo> {
    let once = PutOptions {
        create_only: true,
        ..Default::default()
    };
    let small = ObjectKey::new("originals/Scans/a.jpg").unwrap();
    store
        .put(&small, ByteSource::Bytes(b"a photo".to_vec()), &once)
        .await
        .unwrap();
    let big = ObjectKey::new("originals/Scans/film.mov").unwrap();
    let id = store.start(&big, &once).await.unwrap();
    let part = store.put_part(&id, 1, ByteSource::Bytes(vec![7; 1024])).await.unwrap();
    store.complete(&id, &[part]).await.unwrap();
    store.list_page("Scans/", None).await.unwrap().objects
}

/// No request carries the extra checksums or the chunked encoding that
/// Google Cloud Storage (and once R2) refuses.
fn sent_plainly(seen: &[stand_in::Seen]) {
    for request in seen {
        let said = format!("{} {}", request.method, request.target);
        for (name, value) in &request.headers {
            assert!(!name.starts_with("x-amz-checksum"), "{said}: {name}");
            assert!(!name.starts_with("x-amz-sdk-checksum"), "{said}: {name}");
            assert_ne!(name, "x-amz-trailer", "{said}");
            assert_ne!(name, "x-amz-decoded-content-length", "{said}");
            assert!(!value.contains("aws-chunked"), "{said}: {name}: {value}");
            assert!(!value.starts_with("STREAMING-"), "{said}: {name}: {value}");
        }
        assert!(
            !request.headers.contains_key("transfer-encoding"),
            "{said}: a length is always given"
        );
        // The bucket is in the path, and the region is Google's `auto`
        // or the one given.
        assert!(request.target.starts_with("/family-photos/"), "{said}");
    }
}

#[tokio::test]
async fn google_is_sent_plain_requests_and_asked_for_write_once_in_its_own_words() {
    let stand_in = stand_in::start();
    let store = speaking_to(&stand_in, Preset::GoogleCloud);
    let listed = back_up_one_of_each(&store).await;
    let seen = stand_in.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 5, "{seen:#?}");
    sent_plainly(&seen);
    for request in &seen {
        let said = format!("{} {}", request.method, request.target);
        let authorization = &request.headers["authorization"];
        assert!(
            authorization.contains("Credential=GOOGEXAMPLE1234567890AB/") && authorization.contains("/auto/s3/"),
            "{said}: {authorization}"
        );
        assert!(!request.headers.contains_key("if-none-match"), "{said}");
        let writes_an_original =
            request.method == "PUT" && !request.target.contains("partNumber") || request.method == "POST";
        assert_eq!(
            request.headers.get(GOOGLE_WRITE_ONCE.0).map(String::as_str),
            writes_an_original.then_some(GOOGLE_WRITE_ONCE.1),
            "{said}"
        );
        if writes_an_original {
            // Signed with the rest, so it cannot be dropped on the way.
            assert!(authorization.contains(GOOGLE_WRITE_ONCE.0), "{said}: {authorization}");
        }
    }
    assert_eq!(seen[0].body, b"a photo");
    // A colder class is read from the listing as Google names it.
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].class.as_deref(), Some("NEARLINE"));
}

#[tokio::test]
async fn r2_and_amazon_are_asked_for_write_once_with_if_none_match() {
    let stand_in = stand_in::start();
    let store = speaking_to(
        &stand_in,
        Preset::CloudflareR2 {
            account_id: "0123456789abcdef0123456789abcdef".into(),
        },
    );
    back_up_one_of_each(&store).await;
    let seen = stand_in.seen.lock().unwrap().clone();
    sent_plainly(&seen);
    let once: Vec<(&str, Option<&str>)> = seen
        .iter()
        .map(|r| (r.method.as_str(), r.headers.get("if-none-match").map(String::as_str)))
        .collect();
    assert_eq!(
        once,
        [
            ("PUT", Some("*")),
            ("POST", None),
            ("PUT", None),
            ("POST", Some("*")),
            ("GET", None)
        ]
    );
    assert!(seen.iter().all(|r| !r.headers.contains_key(GOOGLE_WRITE_ONCE.0)));
}

/// The test server, if one has been configured.
fn test_server() -> Option<S3Store> {
    let var = |name: &str| std::env::var(format!("KEEVORO_S3_TEST_{name}")).ok();
    let store = S3Store::connect(&S3Settings {
        preset: Preset::Custom {
            endpoint: var("ENDPOINT")?,
            region: var("REGION"),
            path_style: true,
        },
        bucket: var("BUCKET")?,
        access_key_id: var("ACCESS_KEY")?,
        secret_access_key: var("SECRET_KEY")?,
    })
    .expect("test settings are valid");
    Some(store)
}

async fn ensure_bucket(store: &S3Store) {
    // Fails harmlessly when the bucket is already there.
    let _ = store.client.create_bucket().bucket(&store.bucket).send().await;
}

#[tokio::test]
async fn passes_the_shared_conformance_suite() {
    let Some(store) = test_server() else {
        eprintln!("skipped: no KEEVORO_S3_TEST_* server configured");
        return;
    };
    ensure_bucket(&store).await;
    keevoro_storage::conformance::run_all(&store).await;
}

#[tokio::test]
async fn s3_specific_behaviour() {
    let Some(store) = test_server() else {
        eprintln!("skipped: no KEEVORO_S3_TEST_* server configured");
        return;
    };
    ensure_bucket(&store).await;

    // A private bucket is reported as not listable by strangers.
    let report = store.check_connection().await.unwrap();
    assert_eq!(report.publicly_listable, Some(false));

    // A write-once multipart upload refuses to replace an existing object.
    let key = ObjectKey::new(format!("s3-specific/{}/once.bin", std::process::id())).unwrap();
    store
        .put(&key, ByteSource::Bytes(b"first".to_vec()), &PutOptions::default())
        .await
        .unwrap();
    let once = PutOptions {
        create_only: true,
        ..Default::default()
    };
    let id = store.start(&key, &once).await.unwrap();
    let part = store
        .put_part(&id, 1, ByteSource::Bytes(b"second".to_vec()))
        .await
        .unwrap();
    let refused = store.complete(&id, &[part]).await;
    assert!(matches!(refused, Err(StorageError::AlreadyExists(_))), "{refused:?}");
    let _ = store.abort(&id).await;
    assert_eq!(store.get(&key).await.unwrap(), b"first");

    // Listing pages carry on from where the last one stopped.
    let page = store.list_page("s3-specific/", None).await.unwrap();
    assert!(page.objects.iter().any(|o| o.key == key));
    store.delete(&key).await.unwrap();

    // Wrong keys are reported as refused access, not as a network problem.
    let mut wrong = S3Settings {
        preset: Preset::Custom {
            endpoint: std::env::var("KEEVORO_S3_TEST_ENDPOINT").unwrap(),
            region: None,
            path_style: true,
        },
        bucket: store.bucket.clone(),
        access_key_id: "not-the-key".into(),
        secret_access_key: "not-the-secret".into(),
    };
    let denied = S3Store::connect(&wrong).unwrap().check_connection().await;
    assert!(matches!(denied, Err(StorageError::AccessDenied(_))), "{denied:?}");

    // An address nothing answers on is a network problem, worth retrying.
    wrong.preset = Preset::Custom {
        endpoint: "http://127.0.0.1:9".into(),
        region: None,
        path_style: true,
    };
    let unreachable = S3Store::connect(&wrong).unwrap().head(&key).await;
    assert!(unreachable.is_err_and(|e| e.is_retryable()));
}
