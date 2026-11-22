use super::*;
use async_trait::async_trait;
use keevoro_storage::{Capabilities, ConnectionReport, ListPage, ObjectInfo, PutReceipt, StorageError};
use keevoro_storage_folder::FolderStore;
use std::sync::Mutex;

/// Cloud storage in memory: no folders and no moving, a tag that changes
/// with every write, as S3's does.
struct Bucket {
    objects: Mutex<BTreeMap<String, (Vec<u8>, u64)>>,
    writes: Mutex<u64>,
    caps: Capabilities,
}

impl Default for Bucket {
    fn default() -> Self {
        Self {
            objects: Mutex::default(),
            writes: Mutex::default(),
            caps: Capabilities {
                multipart: None,
                conditional_create: true,
                server_checksums: false,
                custom_metadata: false,
                whole_files_preferred: false,
                counts_its_own_bytes: false,
            },
        }
    }
}

#[async_trait]
impl ObjectStore for Bucket {
    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }
    async fn check_connection(&self) -> std::result::Result<ConnectionReport, StorageError> {
        Ok(ConnectionReport {
            can_write: true,
            can_read: true,
            publicly_listable: Some(false),
        })
    }
    async fn head(&self, key: &ObjectKey) -> std::result::Result<Option<ObjectInfo>, StorageError> {
        Ok(self
            .objects
            .lock()
            .unwrap()
            .get(key.as_str())
            .map(|(body, tag)| ObjectInfo {
                etag: Some(format!("\"{tag}\"")),
                ..ObjectInfo::new(key.clone(), body.len() as u64)
            }))
    }
    async fn get(&self, key: &ObjectKey) -> std::result::Result<Vec<u8>, StorageError> {
        self.objects
            .lock()
            .unwrap()
            .get(key.as_str())
            .map(|(body, _)| body.clone())
            .ok_or_else(|| StorageError::NotFound(key.to_string()))
    }
    async fn put(
        &self,
        key: &ObjectKey,
        body: ByteSource,
        opts: &PutOptions,
    ) -> std::result::Result<PutReceipt, StorageError> {
        let ByteSource::Bytes(body) = body else {
            panic!("only bytes here")
        };
        let mut objects = self.objects.lock().unwrap();
        if opts.create_only && objects.contains_key(key.as_str()) {
            return Err(StorageError::AlreadyExists(key.to_string()));
        }
        let mut writes = self.writes.lock().unwrap();
        *writes += 1;
        let bytes = body.len() as u64;
        objects.insert(key.to_string(), (body, *writes));
        Ok(PutReceipt {
            key: key.clone(),
            bytes,
            etag: Some(format!("\"{writes}\"")),
            blake3: None,
        })
    }
    async fn list_page(
        &self,
        prefix: &str,
        _continuation: Option<String>,
    ) -> std::result::Result<ListPage, StorageError> {
        let objects = self
            .objects
            .lock()
            .unwrap()
            .iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .map(|(k, (body, _))| ObjectInfo::new(ObjectKey::new(k.clone()).unwrap(), body.len() as u64))
            .collect();
        Ok(ListPage { objects, next: None })
    }
    async fn delete(&self, key: &ObjectKey) -> std::result::Result<(), StorageError> {
        self.objects.lock().unwrap().remove(key.as_str());
        Ok(())
    }
}

impl Bucket {
    fn writes(&self) -> u64 {
        *self.writes.lock().unwrap()
    }
    fn set(&self, name: &str, body: Vec<u8>) {
        let mut writes = self.writes.lock().unwrap();
        *writes += 1;
        self.objects.lock().unwrap().insert(name.to_string(), (body, *writes));
    }
}

const TARGET: &str = "bucket:family-photos";

fn block<T>(work: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(work)
}

/// A catalog that has sent `n` originals to [`TARGET`].
fn sent(n: usize) -> Catalog {
    let catalog = Catalog::in_memory().unwrap();
    add_sent(&catalog, 0..n);
    catalog
}

fn add_sent(catalog: &Catalog, which: std::ops::Range<usize>) {
    catalog
        .in_one_go(|| -> keevoro_catalog::Result<()> {
            for i in which {
                let blake3 = format!("{i:064x}");
                let key = format!("originals/2009/07/IMG_{i:04}.jpg");
                catalog.set_choice(&blake3, true)?;
                catalog.mark_placed(&blake3, &key, 2_483_312)?;
                catalog.mark_uploaded(TARGET, &key, 2_483_312)?;
            }
            Ok(())
        })
        .unwrap();
}

fn gzip(text: &[u8]) -> Vec<u8> {
    let mut packed = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    packed.write_all(text).unwrap();
    packed.finish().unwrap()
}

fn gunzip(file: &[u8]) -> String {
    let mut text = String::new();
    flate2::read::GzDecoder::new(file).read_to_string(&mut text).unwrap();
    text
}

#[test]
fn notes_read_back_exactly_as_written_and_say_what_they_are_on_their_first_line() {
    let catalog = sent(3);
    let noted = catalog.notes_for(TARGET).unwrap();
    let (file, _) = encode(&noted, "Family photos", 1_795_219_200).unwrap();
    let text = gunzip(&file);
    let first = text.lines().next().unwrap();
    assert!(first.contains("\"keevoroNotes\":1"), "{first}");
    assert!(first.contains("Keevoro's notes about the backup in this folder"));
    assert!(first.contains(&format!("\"madeBy\":\"Keevoro {}\"", env!("CARGO_PKG_VERSION"))));
    assert_eq!(text.lines().count(), 5, "about, three files, the end");
    let notes = decode(&file).unwrap();
    assert_eq!(notes.noted, noted);
    assert_eq!(notes.about.name, "Family photos");
    assert_eq!((notes.about.files, notes.about.bytes), (3, 3 * 2_483_312));
    assert_eq!(notes.about.written_at, 1_795_219_200);
}

#[test]
fn the_fingerprint_of_what_they_say_ignores_when_they_were_written() {
    let noted = sent(2).notes_for(TARGET).unwrap();
    let (_, a) = encode(&noted, "Family photos", 1).unwrap();
    let (_, b) = encode(&noted, "Family photos", 2).unwrap();
    let (_, c) = encode(&noted, "Holiday videos", 2).unwrap();
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn notes_cut_short_changed_or_not_keevoros_are_called_damaged_never_half_read() {
    let (file, _) = encode(&sent(3).notes_for(TARGET).unwrap(), "Family photos", 1).unwrap();
    let text = gunzip(&file);
    let damaged = |text: &str| matches!(decode(&gzip(text.as_bytes())), Err(Unusable::Damaged(_)));
    // The last file and the end line gone, as a copy stopped part-way.
    let cut: Vec<&str> = text.lines().take(3).collect();
    assert!(damaged(&(cut.join("\n") + "\n")));
    assert!(damaged(&text.replacen("IMG_0001", "IMG_0009", 1)), "a changed line");
    assert!(damaged("{\"hello\":1}\n"));
    assert!(damaged(""));
    assert!(matches!(decode(b"not even packed"), Err(Unusable::Damaged(_))));
    assert!(matches!(decode(&file[..file.len() / 2]), Err(Unusable::Damaged(_))));
}

#[test]
fn notes_from_a_newer_keevoro_are_refused_plainly_and_a_new_kind_of_line_in_this_format_is_passed_over() {
    let newer = "{\"keevoroNotes\":2,\"madeBy\":\"Keevoro 9.0.0\"}\nwhatever comes next\n";
    let refused = decode(&gzip(newer.as_bytes())).unwrap_err();
    assert_eq!(
        refused,
        Unusable::Newer {
            made_by: "Keevoro 9.0.0".into()
        }
    );
    assert!(refused.to_string().contains("made by a newer Keevoro (Keevoro 9.0.0)"));
    assert!(refused.to_string().contains("Update Keevoro first"));

    // A line of a kind this version does not know, with the end's count
    // and fingerprint made to fit, as a later 1.x would write it.
    let (file, _) = encode(&sent(1).notes_for(TARGET).unwrap(), "Family photos", 1).unwrap();
    let text = gunzip(&file);
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    lines.pop();
    lines.push("{\"album\":{\"name\":\"Spain 2009\"}}".into());
    let body = lines.join("\n") + "\n";
    let end = format!(
        "{{\"end\":{{\"lines\":{},\"blake3\":\"{}\"}}}}\n",
        lines.len(),
        blake3::hash(body.as_bytes()).to_hex()
    );
    let notes = decode(&gzip((body + &end).as_bytes())).unwrap();
    assert_eq!(notes.noted.files.len(), 1);
}

#[test]
fn notes_are_written_read_back_and_not_written_again_when_nothing_changed() {
    let bucket = Bucket::default();
    let catalog = sent(2);
    let kept = block(keep_at(&bucket, &catalog, TARGET, "Family photos", 10)).unwrap();
    assert_eq!(
        kept,
        Kept::Written {
            merged: 0,
            damaged_before: false
        }
    );
    assert_eq!(bucket.writes(), 1, "nothing earlier to keep the first time");
    let id = catalog.setting(NOTES_ID).unwrap().unwrap();
    assert_eq!(id.len(), 32);

    let kept = block(keep_at(&bucket, &catalog, TARGET, "Family photos", 20)).unwrap();
    assert_eq!(kept, Kept::Unchanged);
    assert_eq!(bucket.writes(), 1, "nothing written when nothing changed");

    add_sent(&catalog, 2..3);
    let kept = block(keep_at(&bucket, &catalog, TARGET, "Family photos", 30)).unwrap();
    assert_eq!(
        kept,
        Kept::Written {
            merged: 0,
            damaged_before: false
        }
    );
    let newest = decode(&block(bucket.get(&ObjectKey::new(NOTES).unwrap())).unwrap()).unwrap();
    let earlier = decode(&block(bucket.get(&ObjectKey::new(EARLIER).unwrap())).unwrap()).unwrap();
    assert_eq!(
        (newest.about.files, earlier.about.files),
        (3, 2),
        "the notes before are kept one generation"
    );
    assert_eq!(newest.id(), Some(id.as_str()));
    assert_eq!(catalog.setting(NOTES_ID).unwrap().unwrap(), id, "made once");
}

#[test]
fn in_a_folder_the_notes_sit_beside_the_guide_and_nothing_else_is_left_there() {
    let dir = tempfile::tempdir().unwrap();
    let store = block(FolderStore::open(dir.path())).unwrap();
    let catalog = sent(2);
    block(keep_at(&store, &catalog, TARGET, "Family photos", 10)).unwrap();
    add_sent(&catalog, 2..3);
    block(keep_at(&store, &catalog, TARGET, "Family photos", 20)).unwrap();
    let mut names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort();
    assert_eq!(names, [EARLIER, NOTES]);
    assert_eq!(
        block(keep_at(&store, &catalog, TARGET, "Family photos", 30)).unwrap(),
        Kept::Unchanged
    );
}

#[test]
fn notes_another_computer_wrote_there_are_learnt_from_before_being_written_again() {
    let bucket = Bucket::default();
    let here = sent(2);
    block(keep_at(&bucket, &here, TARGET, "Family photos", 10)).unwrap();
    // The other computer opened the same backup, sent two more files, and
    // wrote its own notes.
    let there = sent(4);
    block(keep_at(&bucket, &there, TARGET, "Family photos", 20)).unwrap();
    // This computer sent one the other does not know.
    add_sent(&here, 10..11);
    let kept = block(keep_at(&bucket, &here, TARGET, "Family photos", 30)).unwrap();
    assert_eq!(
        kept,
        Kept::Written {
            merged: 2,
            damaged_before: false
        }
    );
    let said = kept_words(&kept, "the Amazon S3 bucket family-photos (eu-west-2)").unwrap();
    assert!(
        said.starts_with("Another computer has also backed up to the Amazon S3 bucket"),
        "{said}"
    );
    assert!(said.contains("the 2 files it sent there"), "{said}");
    let newest = decode(&block(bucket.get(&ObjectKey::new(NOTES).unwrap())).unwrap()).unwrap();
    assert_eq!(newest.about.files, 5, "the union: nothing either knew is lost");
    let earlier = decode(&block(bucket.get(&ObjectKey::new(EARLIER).unwrap())).unwrap()).unwrap();
    assert_eq!(
        earlier.about.files, 4,
        "the other computer's notes are the earlier generation"
    );
    // And the other computer learns this one's file in turn.
    let kept = block(keep_at(&bucket, &there, TARGET, "Family photos", 40)).unwrap();
    assert_eq!(
        kept,
        Kept::Written {
            merged: 1,
            damaged_before: false
        }
    );
}

#[test]
fn damaged_notes_there_are_written_over_but_never_kept_as_the_earlier_copy() {
    let bucket = Bucket::default();
    let catalog = sent(2);
    block(keep_at(&bucket, &catalog, TARGET, "Family photos", 10)).unwrap();
    add_sent(&catalog, 2..3);
    block(keep_at(&bucket, &catalog, TARGET, "Family photos", 20)).unwrap();
    bucket.set(NOTES, b"scrambled".to_vec());
    add_sent(&catalog, 3..4);
    let kept = block(keep_at(&bucket, &catalog, TARGET, "Family photos", 30)).unwrap();
    assert_eq!(
        kept,
        Kept::Written {
            merged: 0,
            damaged_before: true
        }
    );
    let earlier = decode(&block(bucket.get(&ObjectKey::new(EARLIER).unwrap())).unwrap()).unwrap();
    assert_eq!(earlier.about.files, 2, "the good earlier copy stays");
    assert_eq!(
        block(find_at(&bucket)).unwrap(),
        Found::Notes {
            notes: Box::new(decode(&block(bucket.get(&ObjectKey::new(NOTES).unwrap())).unwrap()).unwrap()),
            from_earlier: false
        }
    );
}

#[test]
fn a_newer_keevoros_notes_are_left_as_they_are() {
    let bucket = Bucket::default();
    let newer = gzip(b"{\"keevoroNotes\":7,\"madeBy\":\"Keevoro 9.0.0\"}\n");
    bucket.set(NOTES, newer.clone());
    let kept = block(keep_at(&bucket, &sent(2), TARGET, "Family photos", 10)).unwrap();
    assert_eq!(
        kept,
        Kept::LeftForNewer {
            made_by: "Keevoro 9.0.0".into()
        }
    );
    assert!(kept_words(&kept, "the folder F:\\Family Backup")
        .unwrap()
        .contains("left them as they are"));
    assert_eq!(block(bucket.get(&ObjectKey::new(NOTES).unwrap())).unwrap(), newer);
    assert_eq!(
        block(find_at(&bucket)).unwrap(),
        Found::Newer {
            made_by: "Keevoro 9.0.0".into()
        }
    );
}

#[test]
fn looking_for_notes_falls_back_to_the_earlier_copy_and_says_when_none_can_be_used() {
    let bucket = Bucket::default();
    assert_eq!(block(find_at(&bucket)).unwrap(), Found::Nothing);
    bucket.set(crate::layout::GUIDE_NAME, b"What is in this backup".to_vec());
    assert_eq!(
        block(find_at(&bucket)).unwrap(),
        Found::NoUsableNotes { damaged: false },
        "a backup from before notes were kept"
    );
    let catalog = sent(2);
    block(keep_at(&bucket, &catalog, TARGET, "Family photos", 10)).unwrap();
    add_sent(&catalog, 2..3);
    block(keep_at(&bucket, &catalog, TARGET, "Family photos", 20)).unwrap();
    bucket.set(NOTES, b"scrambled".to_vec());
    match block(find_at(&bucket)).unwrap() {
        Found::Notes { notes, from_earlier } => {
            assert!(from_earlier);
            assert_eq!(notes.about.files, 2);
        }
        other => panic!("{other:?}"),
    }
    bucket.set(EARLIER, b"scrambled too".to_vec());
    assert_eq!(block(find_at(&bucket)).unwrap(), Found::NoUsableNotes { damaged: true });
}

#[test]
fn the_notes_never_hold_a_key_or_a_token() {
    let catalog = sent(1);
    catalog
        .set_setting("destinations", "[{\"kind\":\"r2\",\"place\":\"marker-account\"}]")
        .unwrap();
    catalog.set_setting("index_key:x", "index/marker-index.json").unwrap();
    catalog.set_setting("history", "[\"marker-history\"]").unwrap();
    let (file, _) = encode(&catalog.notes_for(TARGET).unwrap(), "Family photos", 1).unwrap();
    assert!(!gunzip(&file).contains("marker"));
}

/// Run by hand: `cargo test -p keevoro-engine how_long_notes_for_100_000_files_take -- --ignored --nocapture`.
#[test]
#[ignore]
fn how_long_notes_for_100_000_files_take() {
    let catalog = sent(100_000);
    let started = std::time::Instant::now();
    let noted = catalog.notes_for(TARGET).unwrap();
    let read = started.elapsed();
    let (file, _) = encode(&noted, "Family photos", 1).unwrap();
    let written = started.elapsed();
    let back = decode(&file).unwrap();
    println!(
        "read from the catalog in {read:?}, written in {:?}, read back in {:?}: {} bytes",
        written - read,
        started.elapsed() - written,
        file.len()
    );
    assert_eq!(back.noted.files.len(), 100_000);
}

mod a_new_computer {
    use super::*;
    use crate::connect::{self, testing::MemorySecrets, Flags, Saved, World};
    use crate::send::{self, Stop};
    use keevoro_core::estimate::Tracker;
    use std::path::{Path, PathBuf};

    fn world(secrets: &MemorySecrets) -> World<'_> {
        World {
            secrets,
            environment: &connect::testing::no_environment,
        }
    }

    fn folder(at: &Path) -> Saved {
        Saved::from_flags(&Flags {
            to_folder: Some(at.to_path_buf()),
            ..Flags::default()
        })
        .unwrap()
    }

    /// Backs up everything kept to every destination, and says how it went.
    fn back_up(catalog: &Catalog, home: &Path, secrets: &MemorySecrets) -> (String, send::Went) {
        let mut said = Vec::new();
        let went = send::everywhere(
            &mut said,
            catalog,
            home,
            &world(secrets),
            None,
            &Stop::default(),
            &Tracker::new(keevoro_core::estimate::Remembered::default()),
            &mut |_| {},
            &mut |_| {},
        )
        .unwrap();
        (String::from_utf8(said).unwrap(), went)
    }

    fn a_card(dir: &Path) -> PathBuf {
        let card = dir.join("Mum's card");
        for (n, shade) in [10u8, 70, 130].iter().enumerate() {
            let p = card.join("DCIM").join(format!("IMG_004{}.jpg", n + 8));
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, crate::discover::tests::real_jpeg(*shade)).unwrap();
        }
        card
    }

    #[test]
    fn a_backup_opened_from_its_folder_on_a_new_computer_carries_on_without_sending_anything_again() {
        let dir = tempfile::tempdir().unwrap();
        let card = a_card(dir.path());
        let usb = dir.path().join("Family Backup");
        let secrets = MemorySecrets::default();

        // The old computer backs the card up, and its notes are written.
        let old_home = dir.path().join("old computer");
        {
            let mut catalog = Catalog::open_in_library(&old_home).unwrap();
            catalog.set_setting(NAME, "Family photos").unwrap();
            crate::scan_into(&mut catalog, &card, Some("Mum's card"), &mut |_| {}).unwrap();
            connect::add(&mut Vec::new(), &catalog, &folder(&usb), None, &world(&secrets)).unwrap();
            let (said, went) = back_up(&catalog, &old_home, &secrets);
            assert_eq!(went.destinations[0].originals.sent, 3, "{said}");
            assert!(usb.join(NOTES).is_file());
        }
        let card_before = std::fs::read_dir(card.join("DCIM")).unwrap().count();
        // It dies: everything Keevoro kept on it is gone.
        std::fs::remove_dir_all(&old_home).unwrap();

        // The new computer finds the notes at the destination.
        let store = keevoro_storage_folder::FolderStore::look_at(&usb).unwrap();
        let Found::Notes { notes, from_earlier } = block(find_at(&store)).unwrap() else {
            panic!("notes are there")
        };
        assert!(!from_earlier);
        assert_eq!((notes.about.name.as_str(), notes.about.files), ("Family photos", 3));

        // It opens the backup: a new record, with the destination attached.
        let home = dir.path().join("new computer");
        let mut catalog = Catalog::open_in_library(&home).unwrap();
        connect::add(&mut Vec::new(), &catalog, &folder(&usb), None, &world(&secrets)).unwrap();
        let target = connect::destinations(&catalog).unwrap()[0].target.clone().unwrap();
        open_into(&mut catalog, &home, &notes, &target).unwrap();
        crate::confirm::look(&catalog, &connect::destinations(&catalog).unwrap()[0], &world(&secrets)).unwrap();

        // Its source waits to be connected; backing up meanwhile sends
        // nothing, loses nothing from the index, and keeps the notes.
        let source = catalog.sources().unwrap().remove(0);
        let waiting = catalog.elsewhere(&source.root).unwrap().unwrap();
        assert_eq!(waiting.was_at, card.to_string_lossy());
        assert!(waiting_words(&waiting.was_at).starts_with("Keevoro does not know where these photos live"));
        let (said, went) = back_up(&catalog, &home, &secrets);
        assert_eq!(went.destinations[0].originals.sent, 0, "{said}");
        assert_eq!(went.destinations[0].indexed, Some(3), "{said}");
        assert!(!said.contains("Another computer"), "{said}");

        // The person connects the card again, where it is on this computer.
        connect_again(&mut catalog, source.id, &card).unwrap();
        assert!(connect_again(&mut catalog, source.id, &card).is_err(), "only once");
        crate::scan_into(&mut catalog, &card, None, &mut |_| {}).unwrap();
        let sources = catalog.sources().unwrap();
        assert_eq!(sources.len(), 1, "the same source, not a second");
        assert_eq!(sources[0].name, "Mum's card");
        assert!(catalog.elsewhere(&sources[0].root).unwrap().is_none());

        // Backing up again sends nothing: every file is known to be there.
        let (said, went) = back_up(&catalog, &home, &secrets);
        assert_eq!(went.destinations[0].originals.sent, 0, "{said}");
        assert_eq!(went.destinations[0].originals.already_there, 3, "{said}");
        let mut names: Vec<String> = std::fs::read_dir(usb.join("originals"))
            .unwrap()
            .flat_map(|e| walk(&e.unwrap().path()))
            .collect();
        names.sort();
        assert_eq!(names.len(), 3, "no second copy of anything: {names:?}");

        // And proving every copy by reading it back passes.
        let wrong = send::check(
            &mut Vec::new(),
            &catalog,
            send::Destination::Folder(usb.clone()),
            Some(&target),
            true,
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(wrong, 0);
        assert_eq!(
            std::fs::read_dir(card.join("DCIM")).unwrap().count(),
            card_before,
            "the card untouched"
        );
    }

    fn walk(at: &Path) -> Vec<String> {
        if at.is_dir() {
            std::fs::read_dir(at)
                .unwrap()
                .flat_map(|e| walk(&e.unwrap().path()))
                .collect()
        } else {
            vec![at.file_name().unwrap().to_string_lossy().into_owned()]
        }
    }

    #[test]
    fn the_same_backup_opened_from_cloud_storage_sends_nothing_again_either() {
        // Cloud storage in memory: the notes and the originals as the old
        // computer left them, read and written the way a bucket is.
        let dir = tempfile::tempdir().unwrap();
        let card = a_card(dir.path());
        let usb = dir.path().join("Family Backup");
        let secrets = MemorySecrets::default();
        let old_home = dir.path().join("old computer");
        let mut catalog = Catalog::open_in_library(&old_home).unwrap();
        crate::scan_into(&mut catalog, &card, Some("Mum's card"), &mut |_| {}).unwrap();
        connect::add(&mut Vec::new(), &catalog, &folder(&usb), None, &world(&secrets)).unwrap();
        back_up(&catalog, &old_home, &secrets);
        let bucket = Bucket::default();
        let folder_target = connect::destinations(&catalog).unwrap()[0].target.clone().unwrap();
        // The old computer's record of the bucket is its record of the folder.
        let noted = catalog.notes_for(&folder_target).unwrap();
        for file in &noted.files {
            let mut path = usb.clone();
            path.extend(file.key.split('/'));
            bucket.set(&file.key, std::fs::read(path).unwrap());
        }
        let (file, _) = encode(&noted, "Family photos", 1).unwrap();
        bucket.set(NOTES, file);
        drop(catalog);
        std::fs::remove_dir_all(&old_home).unwrap();

        let Found::Notes { notes, .. } = block(find_at(&bucket)).unwrap() else {
            panic!("notes are there")
        };
        let home = dir.path().join("new computer");
        let mut catalog = Catalog::open_in_library(&home).unwrap();
        open_into(&mut catalog, &home, &notes, TARGET).unwrap();
        let source = catalog.sources().unwrap().remove(0);
        connect_again(&mut catalog, source.id, &card).unwrap();
        crate::scan_into(&mut catalog, &card, None, &mut |_| {}).unwrap();

        // Every file the card holds has its place, already in the bucket.
        let reports = catalog.load_reports().unwrap();
        let kept = send::kept_now(&catalog, &reports).unwrap();
        assert_eq!(kept.len(), 3);
        for k in &kept {
            assert!(
                catalog.is_uploaded(TARGET, &k.path).unwrap(),
                "{} would be sent again",
                k.path
            );
            let stored = block(bucket.get(&ObjectKey::new(k.path.clone()).unwrap())).unwrap();
            assert_eq!(
                blake3::hash(&stored).to_hex().as_str(),
                k.hash,
                "the same file, by fingerprint"
            );
        }
        // Its notes, written here, know the same files.
        let kept = block(keep_at(&bucket, &catalog, TARGET, "Family photos", 5)).unwrap();
        assert_eq!(
            kept,
            Kept::Written {
                merged: 0,
                damaged_before: false
            }
        );
    }
}
