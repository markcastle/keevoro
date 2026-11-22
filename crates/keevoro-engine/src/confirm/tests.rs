use super::*;
use crate::connect::{self, testing::MemorySecrets, Flags, Saved, World};
use crate::send::{everywhere, Stop, Went};
use async_trait::async_trait;
use keevoro_catalog::Sent;
use keevoro_core::estimate::{Remembered, Tracker};
use keevoro_storage::{
    ByteSource, Capabilities, ConnectionReport, ListPage, ObjectInfo, PutOptions, PutReceipt, StorageError,
};
use std::path::{Path, PathBuf};

fn sent(key: &str, bytes: u64, seen: Seen) -> Sent {
    Sent {
        key: key.into(),
        bytes,
        seen,
    }
}

#[test]
fn a_file_is_there_only_by_its_name_and_the_size_it_was_sent_at() {
    let records = [
        sent("originals/a.jpg", 10, Seen::There),
        sent("originals/b.jpg", 10, Seen::There),
        sent("originals/c.jpg", 10, Seen::There),
        sent("originals/d.jpg", 10, Seen::Missing),
        sent("renditions/thumb/a.webp", 3, Seen::There),
    ];
    let listed: HashMap<String, u64> = [
        ("originals/a.jpg", 10),
        ("originals/c.jpg", 4),
        ("originals/d.jpg", 10),
        // What Keevoro did not send is not its own, and is not counted.
        ("originals/not-mine.jpg", 10),
    ]
    .into_iter()
    .map(|(k, b)| (k.to_string(), b))
    .collect();
    let found = compare(&records, &listed);
    assert_eq!(
        found.originals,
        SeenCounts {
            there: 2,
            missing: 1,
            not_as_sent: 1
        }
    );
    // Only what changes is written down: b gone, c shortened, d back, and
    // the preview gone.
    assert_eq!(
        found.changes,
        [
            ("originals/b.jpg".to_string(), Seen::Missing),
            ("originals/c.jpg".to_string(), Seen::NotAsSent),
            ("originals/d.jpg".to_string(), Seen::There),
            ("renditions/thumb/a.webp".to_string(), Seen::Missing),
        ]
    );
}

#[test]
fn a_destination_looks_emptied_when_its_note_has_gone_or_most_of_it_has() {
    let counts = |there, missing| SeenCounts {
        there,
        missing,
        not_as_sent: 0,
    };
    assert!(!looks_emptied(counts(4, 0), false), "nothing missing is nothing to say");
    assert!(!looks_emptied(counts(39, 1), true), "one file gone is one file gone");
    assert!(looks_emptied(counts(1, 39), true), "most of it gone");
    assert!(looks_emptied(counts(39, 1), false), "the note gone too");
    assert!(!looks_emptied(counts(2, 2), true), "half is not most");
}

#[test]
fn what_has_gone_is_said_plainly_with_what_will_happen() {
    let usb = "the folder /media/USB";
    let gone = |there, missing, not_as_sent| SeenCounts {
        there,
        missing,
        not_as_sent,
    };
    assert_eq!(
        gone_words(usb, gone(0, 40, 0), true, true),
        ["The folder /media/USB looks emptied or replaced: it no longer holds any of the 40 files Keevoro backed up \
          there. They will be sent again."]
    );
    assert_eq!(
        gone_words(usb, gone(1_199, 1, 0), false, true),
        ["The folder /media/USB no longer holds 1 of the 1,200 files Keevoro backed up there. It will be sent again."]
    );
    assert_eq!(
        gone_words(usb, gone(0, 1, 0), true, true),
        [
            "The folder /media/USB looks emptied or replaced: it no longer holds the file Keevoro backed up there. It \
          will be sent again."
        ]
    );
    let shortened = gone_words("the Cloudflare R2 bucket family", gone(3, 0, 1), false, false);
    assert_eq!(
        shortened,
        ["The Cloudflare R2 bucket family has 1 file under the name Keevoro gave it, but not the size it was backed \
          up at, so it is not counted as safe. Keevoro never replaces or deletes a file in a backup. To put it right, \
          delete that copy in your storage's own website and back up again."]
    );
    // The owner's words, read aloud: "gave they" is gone.
    assert_eq!(
        gone_words("the folder Y:\\personal", gone(71_769, 15, 64), false, true),
        [
            "The folder Y:\\personal no longer holds 15 of the 71,848 files Keevoro backed up there. They will be sent \
             again.",
            "The folder Y:\\personal has 64 files under the names Keevoro gave them, but not the sizes they were \
             backed up at, so they are not counted as safe. Keevoro never replaces a file in a backup: \"Put this \
             backup right\" moves those copies aside there, into a folder of their own, then sends them again and \
             proves each one."
        ]
    );
    assert!(gone_words(usb, gone(4, 0, 0), false, true).is_empty());
    for line in gone_words(usb, gone(0, 2, 2), true, true) {
        assert!(!line.contains('\u{2014}'), "{line}");
    }
}

#[test]
fn a_destination_out_of_reach_is_said_as_out_of_reach_with_when_it_was_last_reached() {
    assert_eq!(
        out_of_reach_words(
            "the folder E:\\Backup",
            "Keevoro can't find the folder E:\\Backup. Is it plugged in?",
            Some(1_000),
            1_000 + 3 * 86_400
        ),
        "Keevoro can't find the folder E:\\Backup. Is it plugged in? Until Keevoro can reach the folder E:\\Backup, \
         what is there can't be confirmed: everything is counted as it was when Keevoro last reached it, 3 days ago."
    );
}

#[test]
fn a_look_is_trusted_for_a_few_minutes_and_no_longer() {
    let looked = |at| Looked {
        at,
        reached: true,
        problem: None,
        last_reached: Some(at),
        emptied: false,
        other_disk: false,
    };
    assert!(is_fresh(&looked(1_000), 1_000));
    assert!(is_fresh(&looked(1_000), 1_000 + FRESH_FOR - 1));
    assert!(!is_fresh(&looked(1_000), 1_000 + FRESH_FOR));
    assert!(!is_fresh(&looked(1_000), 900), "a look from the future is not trusted");
}

/// A card of four photos, backed up to a folder called USB, as the tester did.
struct Tester {
    dir: tempfile::TempDir,
    home: PathBuf,
    catalog: Catalog,
    secrets: MemorySecrets,
}

impl Tester {
    fn new() -> Self {
        let tester = Self::card_only();
        let usb = tester.usb();
        connect::add(&mut Vec::new(), &tester.catalog, &folder(&usb), None, &tester.world()).unwrap();
        tester
    }

    /// The card looked through, going nowhere yet.
    fn card_only() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let card = dir.path().join("CARD");
        for (n, shade) in [10u8, 70, 130, 190].iter().enumerate() {
            let p = card.join("DCIM").join(format!("IMG_2024060{}_100000.jpg", n + 1));
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, crate::discover::tests::real_jpeg(*shade)).unwrap();
        }
        let home = dir.path().join("home");
        let mut catalog = Catalog::open_in_library(&home).unwrap();
        crate::scan_into(&mut catalog, &card, Some("CARD"), &mut |_| {}).unwrap();
        Self {
            home,
            catalog,
            secrets: MemorySecrets::default(),
            dir,
        }
    }

    fn usb(&self) -> PathBuf {
        self.dir.path().join("USB")
    }

    fn world(&self) -> World<'_> {
        World {
            secrets: &self.secrets,
            environment: &connect::testing::no_environment,
        }
    }

    fn run(&self) -> (String, Went) {
        let mut said = Vec::new();
        let went = everywhere(
            &mut said,
            &self.catalog,
            &self.home,
            &self.world(),
            None,
            &Stop::default(),
            &Tracker::new(Remembered::default()),
            &mut |_| {},
            &mut |_| {},
        )
        .unwrap();
        (String::from_utf8(said).unwrap(), went)
    }

    fn saved(&self) -> Saved {
        connect::destinations(&self.catalog).unwrap().remove(0)
    }

    fn target(&self) -> String {
        self.saved().target.unwrap()
    }

    fn waiting(&self) -> usize {
        crate::discover(&self.home, &self.catalog).unwrap().waiting_to_gather
    }

    /// What the backup screen would say about the destination now.
    fn view(&self) -> LookedView {
        as_it_stands(&self.catalog, &self.saved(), now()).unwrap().unwrap()
    }

    /// Every original at the USB folder, wherever its moment put it.
    fn originals(&self) -> Vec<PathBuf> {
        let at = self.usb().join("originals");
        if at.is_dir() {
            walk(&at)
        } else {
            Vec::new()
        }
    }

    /// `rm -rf USB/*`: everything but what starts with a dot.
    fn empty_the_folder(&self) {
        for entry in std::fs::read_dir(self.usb()).unwrap().filter_map(Result::ok) {
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                std::fs::remove_dir_all(path).unwrap();
            } else {
                std::fs::remove_file(path).unwrap();
            }
        }
    }

    fn records(&self) -> Vec<Sent> {
        self.catalog.sent_to(&self.target()).unwrap()
    }
}

fn folder(to: &Path) -> Saved {
    Saved::from_flags(&Flags {
        to_folder: Some(to.to_path_buf()),
        ..Flags::default()
    })
    .unwrap()
}

#[test]
fn the_testers_case_a_folder_emptied_is_said_before_anything_is_sent_then_sent_again_and_said_so() {
    let t = Tester::new();
    let (said, _) = t.run();
    assert!(said.contains("Safely backed up 4 files"), "{said}");
    assert_eq!(t.waiting(), 0);

    t.empty_the_folder();
    // The backup screen and the rehearsal look first.
    look_everywhere(&t.catalog, &t.world()).unwrap();
    assert_eq!(t.waiting(), 4, "nothing gone is counted as safe");
    // The map's "Backed up: N of M" goes by the same rule.
    let by_folder = crate::safe_by_folder(&t.catalog).unwrap();
    assert!(by_folder.values().all(|(safe, _)| *safe == 0), "{by_folder:?}");
    let view = t.view();
    assert!(view.fresh && view.reached);
    assert_eq!((view.gone, view.not_as_sent), (4, 0));
    let heads_up = format!(
        "The folder {} looks emptied or replaced: it no longer holds any of the 4 files Keevoro backed up there. \
         They will be sent again.",
        t.usb().display()
    );
    assert_eq!(view.said, std::slice::from_ref(&heads_up));

    // The backup says so first, then sends them, and says it did.
    let (said, went) = t.run();
    assert!(said.starts_with(&heads_up), "{said}");
    assert!(said.contains("Safely backed up 4 files"), "{said}");
    assert!(said.contains("Restored 4 files that had gone from there."), "{said}");
    assert!(!said.contains("already there"), "{said}");
    assert_eq!(went.destinations[0].originals.restored, 4);
    assert_eq!(t.originals().len(), 4, "the photos are back");
    assert!(t.usb().join(crate::layout::GUIDE_NAME).is_file());
    assert_eq!(t.waiting(), 0);
    assert!(t.records().iter().all(|r| r.seen == Seen::There));
    // And the next look has nothing to say.
    look_everywhere(&t.catalog, &t.world()).unwrap();
    assert!(t.view().said.is_empty());
    let (said, _) = t.run();
    assert!(said.contains("4 files already there."), "{said}");
}

#[test]
fn an_unattended_backup_sends_nothing_to_a_folder_that_looks_emptied_and_says_so() {
    let t = Tester::new();
    t.run();
    t.empty_the_folder();
    let mut said = Vec::new();
    let went = crate::send::everywhere_as(
        &crate::send::HowToSend { unattended: true },
        &mut said,
        &t.catalog,
        &t.home,
        &t.world(),
        None,
        &Stop::default(),
        &Tracker::new(Remembered::default()),
        &mut |_| {},
        &mut |_| {},
    )
    .unwrap();
    let said = String::from_utf8(said).unwrap();
    assert!(t.originals().is_empty(), "nothing was sent there");
    assert!(said.contains(crate::send::NOT_SENT_THERE), "{said}");
    assert!(!said.contains("They will be sent again"), "{said}");
    assert_eq!(
        went.held,
        [format!(
            "The folder {} looks emptied or replaced. {}",
            t.usb().display(),
            crate::send::NOT_SENT_THERE
        )]
    );
    assert_eq!((went.reached, went.unreachable, went.failed_files), (0, 0, 0));
    assert!(went.refused.is_none());
    assert_eq!(t.waiting(), 4, "still waiting, for the person's say");
}

#[test]
fn an_unattended_backup_still_sends_again_a_few_files_gone_from_a_destination() {
    let t = Tester::new();
    t.run();
    std::fs::remove_file(&t.originals()[1]).unwrap();
    let mut said = Vec::new();
    let went = crate::send::everywhere_as(
        &crate::send::HowToSend { unattended: true },
        &mut said,
        &t.catalog,
        &t.home,
        &t.world(),
        None,
        &Stop::default(),
        &Tracker::new(Remembered::default()),
        &mut |_| {},
        &mut |_| {},
    )
    .unwrap();
    let said = String::from_utf8(said).unwrap();
    assert!(said.contains("Restored 1 file that had gone from there."), "{said}");
    assert!(went.held.is_empty());
    assert_eq!(t.originals().len(), 4);
}

#[test]
fn one_file_deleted_is_found_said_and_sent_again() {
    let t = Tester::new();
    t.run();
    std::fs::remove_file(&t.originals()[1]).unwrap();
    let (said, went) = t.run();
    assert!(
        said.starts_with(&format!(
            "The folder {} no longer holds 1 of the 4 files Keevoro backed up there. It will be sent again.",
            t.usb().display()
        )),
        "{said}"
    );
    assert!(said.contains("Safely backed up 1 file"), "{said}");
    assert!(said.contains("Restored 1 file that had gone from there."), "{said}");
    assert!(said.contains("3 files already there."), "{said}");
    assert_eq!(went.destinations[0].originals.restored, 1);
    assert_eq!(t.originals().len(), 4);
}

#[test]
fn a_file_cut_short_is_never_counted_as_safe_never_replaced_and_said_plainly() {
    let t = Tester::new();
    t.run();
    let short = t.originals()[0].clone();
    std::fs::write(&short, b"cut short").unwrap();
    look_everywhere(&t.catalog, &t.world()).unwrap();
    assert_eq!(t.waiting(), 1, "it is not counted as safe");
    let view = t.view();
    assert_eq!((view.gone, view.not_as_sent), (0, 1));
    assert!(
        view.said[0].contains("not the size it was backed up at"),
        "{:?}",
        view.said
    );
    // A backup does not replace it: only what Keevoro makes is ever replaced.
    let (said, went) = t.run();
    assert!(said.contains("Couldn't back up 1 file"), "{said}");
    assert_eq!(went.failed_files, 1);
    assert_eq!(std::fs::read(&short).unwrap(), b"cut short");
    // The index lists only what is really there.
    let index = t
        .catalog
        .setting(&format!("index_key:{}", t.target()))
        .unwrap()
        .unwrap();
    let index: serde_json::Value = serde_json::from_slice(&std::fs::read(t.usb().join(index)).unwrap()).unwrap();
    assert_eq!(index["items"].as_object().unwrap().len(), 3);
}

#[test]
fn a_destination_swapped_for_an_empty_folder_is_backed_up_afresh() {
    let t = Tester::new();
    t.run();
    std::fs::remove_dir_all(t.usb()).unwrap();
    std::fs::create_dir(t.usb()).unwrap();
    let (said, went) = t.run();
    assert!(said.contains("looks emptied or replaced"), "{said}");
    assert_eq!(went.destinations[0].originals.restored, 4);
    assert_eq!(t.originals().len(), 4);
}

#[test]
fn a_destination_unplugged_at_the_start_is_out_of_reach_never_empty_and_nothing_is_forgotten() {
    let t = Tester::new();
    t.run();
    let before = t.records();
    let away = t.dir.path().join("USB unplugged");
    std::fs::rename(t.usb(), &away).unwrap();

    look_everywhere(&t.catalog, &t.world()).unwrap();
    assert_eq!(t.records(), before, "not one record is touched");
    assert_eq!(t.waiting(), 0, "what was there when last looked is still counted");
    let view = t.view();
    assert!(!view.reached && !view.fresh);
    assert_eq!(view.gone, 0);
    assert!(
        view.said[0].starts_with(&format!(
            "Keevoro can't find the folder {}. Is it plugged in? Until Keevoro can reach the folder {}, what is \
             there can't be confirmed: everything is counted as it was when Keevoro last reached it",
            t.usb().display(),
            t.usb().display()
        )),
        "{:?}",
        view.said
    );

    // A backup says it is out of reach in its usual words, and never
    // makes the folder again on whatever disk is left.
    let (said, went) = t.run();
    assert_eq!((went.reached, went.unreachable), (0, 1));
    assert!(
        said.contains(&format!(
            "Couldn't back up to the folder {}. Keevoro can't find the folder {}. Is it plugged in?",
            t.usb().display(),
            t.usb().display()
        )),
        "{said}"
    );
    assert!(!said.contains("no longer holds"), "{said}");
    assert!(!t.usb().exists(), "the folder is not made again");
    assert_eq!(t.records(), before);

    // Plugged back in, everything is there and nothing is sent.
    std::fs::rename(&away, t.usb()).unwrap();
    let (said, _) = t.run();
    assert!(said.contains("4 files already there."), "{said}");
    assert!(!said.contains("Safely backed up"), "{said}");
}

#[test]
fn a_destination_unplugged_part_way_through_a_backup_keeps_every_record() {
    let t = Tester::new();
    t.run();
    let before = t.records();
    // A new photo, so there is something to send when it is pulled out,
    // after the look and before the files go.
    let card = t.dir.path().join("CARD");
    std::fs::write(
        card.join("DCIM").join("IMG_20240610_100000.jpg"),
        crate::discover::tests::real_jpeg(220),
    )
    .unwrap();
    let mut catalog = Catalog::open_in_library(&t.home).unwrap();
    crate::scan_into(&mut catalog, &card, Some("CARD"), &mut |_| {}).unwrap();
    let away = t.dir.path().join("USB unplugged");
    let mut said = Vec::new();
    let pulled = std::cell::Cell::new(false);
    let went = everywhere(
        &mut said,
        &t.catalog,
        &t.home,
        &t.world(),
        None,
        &Stop::default(),
        &Tracker::new(Remembered::default()),
        &mut |line| {
            if !pulled.get() && line.starts_with("Working out where each file goes") {
                std::fs::rename(t.usb(), &away).unwrap();
                pulled.set(true);
            }
        },
        &mut |_| {},
    )
    .unwrap();
    if away.exists() {
        let _ = std::fs::remove_dir_all(t.usb());
        std::fs::rename(&away, t.usb()).unwrap();
    }
    let said = String::from_utf8(said).unwrap();
    assert!(pulled.get(), "{said}");
    assert!(went.failed_files > 0 || went.unreachable > 0, "{said}");
    for record in &before {
        assert!(
            t.catalog.is_uploaded(&t.target(), &record.key).unwrap(),
            "{} is still recorded: {said}",
            record.key
        );
    }
}

/// A store that stands for one out of reach part-way through being
/// listed, or for another program finishing a file just as Keevoro looks.
struct Meddling {
    inner: FolderStore,
    caps: Capabilities,
    /// Listing this prefix fails as a dropped connection does.
    fails_at: Option<&'static str>,
    /// Once the listing is done, another program places this file and
    /// records it in its own hold on the backup's catalog (its home
    /// folder, the name of the records, the file), as a backup finishing a
    /// file at that moment would.
    finishes: Option<(PathBuf, String, PathBuf)>,
    /// What the listing gets wrong, as a share that dropped out and came
    /// back once did: a file left out (`None`), or listed at another size.
    stale: Vec<(String, Option<u64>)>,
    /// Asking about one file by itself fails, as when the connection drops.
    head_fails: bool,
}

#[async_trait]
impl ObjectStore for Meddling {
    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }
    async fn check_connection(&self) -> Result<ConnectionReport, StorageError> {
        self.inner.check_connection().await
    }
    async fn head(&self, key: &ObjectKey) -> Result<Option<ObjectInfo>, StorageError> {
        if self.head_fails {
            return Err(StorageError::Io(std::io::Error::other(
                "The specified network name is no longer available. (os error 64)",
            )));
        }
        self.inner.head(key).await
    }
    async fn get(&self, key: &ObjectKey) -> Result<Vec<u8>, StorageError> {
        self.inner.get(key).await
    }
    async fn put(&self, key: &ObjectKey, body: ByteSource, opts: &PutOptions) -> Result<PutReceipt, StorageError> {
        self.inner.put(key, body, opts).await
    }
    async fn list_page(&self, prefix: &str, continuation: Option<String>) -> Result<ListPage, StorageError> {
        self.inner.list_page(prefix, continuation).await
    }
    async fn list_all(&self, prefix: &str) -> Result<Vec<ObjectInfo>, StorageError> {
        if self.fails_at == Some(prefix) {
            return Err(StorageError::Network("connection reset".into()));
        }
        let mut listed = self.inner.list_all(prefix).await?;
        listed.retain(|o| !self.stale.iter().any(|(k, b)| *k == o.key.as_str() && b.is_none()));
        for o in &mut listed {
            if let Some((_, Some(bytes))) = self.stale.iter().find(|(k, _)| *k == o.key.as_str()) {
                o.bytes = *bytes;
            }
        }
        if let (Some((home, target, file)), "originals/") = (&self.finishes, prefix) {
            std::fs::write(file, b"finished just now").unwrap();
            Catalog::open_in_library(home)
                .unwrap()
                .mark_uploaded(target, "originals/just now.jpg", 17)
                .unwrap();
        }
        Ok(listed)
    }
    async fn delete(&self, key: &ObjectKey) -> Result<(), StorageError> {
        self.inner.delete(key).await
    }
}

fn meddling(usb: &Path) -> Meddling {
    let inner = FolderStore::look_at(usb).unwrap();
    Meddling {
        caps: inner.capabilities().clone(),
        inner,
        fails_at: None,
        finishes: None,
        stale: Vec::new(),
        head_fails: false,
    }
}

/// The keys of the originals at the USB folder, as Keevoro names them.
fn keys_of(t: &Tester) -> Vec<String> {
    let mut keys: Vec<String> = t
        .records()
        .into_iter()
        .filter(|r| r.key.starts_with("originals/"))
        .map(|r| r.key)
        .collect();
    keys.sort();
    keys
}

#[test]
fn a_listing_that_leaves_files_out_or_gives_stale_sizes_is_checked_file_by_file_before_anything_is_believed() {
    // The owner's second run: the listing of a share that had dropped out
    // said 15 files had gone and 64 were another size. Each was in fact
    // there and exact, as the backup that followed proved by reading them.
    let t = Tester::new();
    t.run();
    let keys = keys_of(&t);
    let mut store = meddling(&t.usb());
    store.stale = vec![(keys[0].clone(), None), (keys[1].clone(), Some(3))];
    let looked = look_with(&t.catalog, &t.target(), Ok(&store)).unwrap();
    assert!(looked.reached);
    assert!(t.records().iter().all(|r| r.seen == Seen::There), "{:?}", t.records());
    assert!(t.view().said.is_empty(), "nothing to say: {:?}", t.view().said);
    assert_eq!(t.waiting(), 0);

    // When a file really has gone, asking about it by itself agrees.
    std::fs::remove_file(t.usb().join(&keys[2])).unwrap();
    look_with(&t.catalog, &t.target(), Ok(&store)).unwrap();
    assert_eq!((t.view().gone, t.view().not_as_sent), (1, 0));
}

#[test]
fn a_look_that_cannot_ask_about_a_file_by_itself_changes_no_record() {
    let t = Tester::new();
    t.run();
    let before = t.records();
    let keys = keys_of(&t);
    let mut store = meddling(&t.usb());
    store.stale = vec![(keys[0].clone(), None)];
    store.head_fails = true;
    let looked = look_with(&t.catalog, &t.target(), Ok(&store)).unwrap();
    assert!(!looked.reached, "could not look is never gone");
    assert_eq!(t.records(), before);
    assert_eq!(t.waiting(), 0);
}

#[test]
fn a_destination_that_stops_answering_part_way_through_a_look_changes_no_record() {
    let t = Tester::new();
    t.run();
    let before = t.records();
    // The originals are listed (and would all look gone), then the
    // connection drops before the previews are.
    t.empty_the_folder();
    let mut store = meddling(&t.usb());
    store.fails_at = Some("renditions/");
    let looked = look_with(&t.catalog, &t.target(), Ok(&store)).unwrap();
    assert!(!looked.reached);
    assert_eq!(t.records(), before, "a look that did not finish takes nothing for gone");
    assert_eq!(t.waiting(), 0);
    assert!(
        t.view().said[0].contains("what is there can't be confirmed"),
        "{:?}",
        t.view().said
    );
}

#[test]
fn a_file_another_program_finishes_while_keevoro_looks_is_never_taken_for_gone() {
    let t = Tester::new();
    t.run();
    let target = t.target();
    // Another program's copy under way, in Keevoro's working folder.
    let working = t.usb().join(".keevoro-uploads");
    std::fs::create_dir_all(&working).unwrap();
    std::fs::write(working.join("tmp-0123456789abcdef"), b"half").unwrap();
    let mut store = meddling(&t.usb());
    store.finishes = Some((
        t.home.clone(),
        target.clone(),
        t.usb().join("originals").join("just now.jpg"),
    ));
    let looked = look_with(&t.catalog, &target, Ok(&store)).unwrap();
    assert!(looked.reached);
    assert_eq!(
        t.catalog.sent_state(&target, "originals/just now.jpg").unwrap(),
        Some(Seen::There),
        "recorded after the records were read, so not judged by a listing made before it was there"
    );
    assert!(t.records().iter().all(|r| r.seen == Seen::There), "{:?}", t.records());
    assert!(t.view().said.is_empty());
}

#[test]
fn a_preview_gone_from_the_destination_is_sent_again_quietly() {
    let t = Tester::new();
    t.run();
    let thumbs = t.usb().join("renditions").join("thumb");
    let first = std::fs::read_dir(&thumbs).unwrap().next().unwrap().unwrap().path();
    std::fs::remove_file(&first).unwrap();
    let (said, _) = t.run();
    assert!(first.is_file(), "the preview is back: {said}");
    assert!(
        !said.contains("no longer holds"),
        "only the originals are spoken of: {said}"
    );
}

#[test]
fn nothing_at_a_destination_is_ever_deleted_or_taken_for_keevoros_own() {
    let t = Tester::new();
    t.run();
    // The person's own file beside the backup.
    std::fs::remove_file(&t.originals()[0]).unwrap();
    let mine = t.usb().join("originals").join("my notes.txt");
    std::fs::write(&mine, b"mine").unwrap();
    let before: Vec<PathBuf> = walk(&t.usb());
    look_everywhere(&t.catalog, &t.world()).unwrap();
    assert_eq!(walk(&t.usb()), before, "a look changes nothing there");
    assert_eq!(
        t.catalog.sent_state(&t.target(), "originals/my notes.txt").unwrap(),
        None
    );
    t.run();
    let after = walk(&t.usb());
    assert!(before.iter().all(|p| after.contains(p)), "a backup removes nothing");
    assert_eq!(std::fs::read(&mine).unwrap(), b"mine");
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut all = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(at) = stack.pop() {
        for entry in std::fs::read_dir(&at).unwrap().filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                all.push(path);
            }
        }
    }
    all.sort();
    all
}

#[test]
fn a_check_corrects_the_record_so_backing_up_again_sends_what_it_found_missing() {
    let t = Tester::new();
    t.run();
    let gone = t.originals()[2].clone();
    std::fs::remove_file(&gone).unwrap();
    let saved = t.saved();
    let mut said = Vec::new();
    let wrong = crate::send::check(
        &mut said,
        &t.catalog,
        saved.open(&mut Vec::new(), &t.world()).unwrap(),
        saved.target.as_deref(),
        false,
        &mut |_| {},
    )
    .unwrap();
    let said = String::from_utf8(said).unwrap();
    assert_eq!(wrong, 1, "{said}");
    assert!(
        said.contains("Back up again to send anything that is missing."),
        "{said}"
    );
    // Every screen now says it is waiting, before anything else looks.
    assert_eq!(t.waiting(), 1);
    // And backing up again does just that.
    let (said, _) = t.run();
    assert!(said.contains("Safely backed up 1 file"), "{said}");
    assert!(gone.is_file());
}

#[test]
fn a_check_of_a_folder_that_is_not_there_finds_nothing_gone() {
    let t = Tester::new();
    t.run();
    let before = t.records();
    let away = t.dir.path().join("away");
    std::fs::rename(t.usb(), &away).unwrap();
    let saved = t.saved();
    let e = crate::send::check(
        &mut Vec::new(),
        &t.catalog,
        saved.open(&mut Vec::new(), &t.world()).unwrap(),
        saved.target.as_deref(),
        false,
        &mut |_| {},
    )
    .unwrap_err();
    assert!(e.to_string().contains("Is it plugged in?"), "{e}");
    assert!(!t.usb().exists());
    assert_eq!(t.records(), before);
}

/// How long a look at a folder of 50,000 files takes. Run by hand:
/// `cargo test -p keevoro-engine how_long_a_look_at_50_000_files_takes -- --ignored --nocapture`
#[test]
#[ignore = "run by hand: it makes 50,000 files"]
fn how_long_a_look_at_50_000_files_takes() {
    let dir = tempfile::tempdir().unwrap();
    let usb = dir.path().join("USB");
    let catalog = Catalog::in_memory().unwrap();
    let target = "folder:USB";
    let mut keys = Vec::new();
    for folder in 0..500 {
        let at = usb.join("originals").join(format!("Album {folder:03}"));
        std::fs::create_dir_all(&at).unwrap();
        for file in 0..100 {
            std::fs::write(at.join(format!("IMG_{file:04}.jpg")), b"x").unwrap();
            keys.push(format!("originals/Album {folder:03}/IMG_{file:04}.jpg"));
        }
    }
    for key in &keys {
        catalog.mark_uploaded(target, key, 1).unwrap();
    }
    let store = FolderStore::look_at(&usb).unwrap();
    let started = std::time::Instant::now();
    let looked = look_with(&catalog, target, Ok(&store)).unwrap();
    let took = started.elapsed();
    assert!(looked.reached);
    assert_eq!(catalog.seen_counts(target, "originals/").unwrap().there, 50_000);
    // With some gone, the record is corrected in the same moment.
    for key in keys.iter().take(1_000) {
        std::fs::remove_file(usb.join(key)).unwrap();
    }
    let started = std::time::Instant::now();
    look_with(&catalog, target, Ok(&store)).unwrap();
    let with_gone = started.elapsed();
    assert_eq!(catalog.seen_counts(target, "originals/").unwrap().missing, 1_000);
    eprintln!("A look at 50,000 files took {took:?}; with 1,000 gone, {with_gone:?}.");
    assert!(took < std::time::Duration::from_secs(30));
}

/// The tester's case against cloud storage. Runs only where a test server
/// is set up, as CI's coverage job does with MinIO; see the top of
/// `keevoro-storage-s3/src/tests.rs` for the variables.
#[test]
fn against_a_real_server_a_bucket_emptied_is_said_and_sent_again_and_one_refusing_the_keys_is_out_of_reach() {
    let var = |name: &str| std::env::var(format!("KEEVORO_S3_TEST_{name}")).ok();
    let (Some(endpoint), Some(bucket), Some(access), Some(secret)) =
        (var("ENDPOINT"), var("BUCKET"), var("ACCESS_KEY"), var("SECRET_KEY"))
    else {
        eprintln!("skipped: no test server set up");
        return;
    };
    let store = S3Store::connect(&S3Settings {
        preset: keevoro_storage_s3::Preset::Custom {
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
    // Everything a backup writes, gone, as from a bucket emptied by hand.
    let empty = || {
        runtime.block_on(async {
            for prefix in ["originals/", "renditions/", "index/", crate::layout::GUIDE_NAME] {
                for object in store.list_all(prefix).await.unwrap() {
                    store.delete(&object.key).await.unwrap();
                }
            }
        })
    };
    empty();

    let t = Tester::card_only();
    let to = Saved::from_flags(&Flags {
        to_endpoint: Some(endpoint),
        bucket: Some(bucket),
        ..Flags::default()
    })
    .unwrap();
    connect::add(
        &mut Vec::new(),
        &t.catalog,
        &to,
        connect::Keys::new(&access, &secret),
        &t.world(),
    )
    .unwrap();
    let (said, _) = t.run();
    assert!(said.contains("Safely backed up 4 files"), "{said}");

    empty();
    let (said, went) = t.run();
    let place = capitalised(&to.describe());
    assert!(
        said.starts_with(&format!(
            "{place} looks emptied or replaced: it no longer holds any of the 4 files Keevoro backed up there. \
             They will be sent again."
        )),
        "{said}"
    );
    assert!(said.contains("Restored 4 files that had gone from there."), "{said}");
    assert_eq!(went.destinations[0].originals.restored, 4);
    let listed = runtime.block_on(store.list_all("originals/")).unwrap();
    assert_eq!(listed.len(), 4);

    // Keys that no longer work: out of reach, and nothing is forgotten.
    let before = t.records();
    let saved = t.saved();
    t.secrets
        .0
        .borrow_mut()
        .values_mut()
        .for_each(|keys| *keys = "wrong\nwrong".into());
    let looked = look(&t.catalog, &saved, &t.world()).unwrap();
    assert!(!looked.reached, "{looked:?}");
    assert_eq!(t.records(), before);
    let (said, went) = t.run();
    assert_eq!(went.unreachable, 1, "{said}");
    assert!(said.contains("Couldn't back up to"), "{said}");
    assert!(!said.contains("no longer holds"), "{said}");
    assert_eq!(t.records(), before);
    empty();
}

#[test]
fn only_an_empty_folder_on_another_disk_is_taken_for_a_drive_unplugged() {
    assert!(on_another_disk(Some("device 2049"), Some("device 66306"), false));
    assert!(
        !on_another_disk(Some("device 2049"), Some("device 2049"), false),
        "the same disk"
    );
    assert!(
        !on_another_disk(Some("device 2049"), Some("device 2065"), true),
        "the backup's drive, given another number when plugged in again"
    );
    assert!(
        !on_another_disk(None, Some("device 2049"), false),
        "saved before the disk was kept"
    );
    assert!(
        !on_another_disk(Some("device 2049"), None, false),
        "the system cannot say"
    );
}

/// Remembers the destination as being on another disk than the one its
/// folder is on now, as a folder a drive was mounted on reads once the
/// drive is unplugged.
fn known_on_another_disk(t: &Tester) {
    let mut all = connect::destinations(&t.catalog).unwrap();
    all[0].volume = Some("device 1".to_string());
    t.catalog
        .set_setting("destinations", &serde_json::to_string(&all).unwrap())
        .unwrap();
}

#[test]
fn the_empty_folder_an_unplugged_drive_leaves_behind_is_never_backed_up_into() {
    let t = Tester::new();
    t.run();
    assert!(t.saved().volume.is_some(), "the disk is remembered when it is added");
    // The drive is unplugged, leaving the folder it was mounted on, empty,
    // on the computer's own disk.
    t.empty_the_folder();
    known_on_another_disk(&t);
    let (said, went) = t.run();
    assert!(
        said.contains(&format!(
            "The drive this backup goes to does not seem to be plugged in: {} is there, but on another disk",
            t.usb().display()
        )),
        "{said}"
    );
    assert!(t.originals().is_empty(), "nothing is sent onto the wrong disk");
    assert_eq!(went.reached, 0);
    let view = t.view();
    assert!(!view.reached && view.other_disk, "{view:?}");
    assert_eq!(view.gone, 0, "nothing Keevoro knows about it is forgotten");
    // "Check they all arrived" refuses it in the same words.
    let e = not_on_another_disk(&t.saved(), &crate::send::Destination::Folder(t.usb())).unwrap_err();
    assert!(format!("{e:#}").contains("does not seem to be plugged in"));

    // The person says this disk is where it goes now: added again, it is
    // known by this disk, and backed up to afresh.
    connect::add(&mut Vec::new(), &t.catalog, &folder(&t.usb()), None, &t.world()).unwrap();
    let (said, went) = t.run();
    assert!(said.contains("Safely backed up 4 files"), "{said}");
    assert_eq!(went.destinations[0].originals.restored, 4);
}

#[test]
fn the_backups_own_drive_given_another_number_is_still_the_backups_and_is_known_by_it_again() {
    let t = Tester::new();
    t.run();
    known_on_another_disk(&t);
    let (said, went) = t.run();
    assert_eq!(went.reached, 1, "{said}");
    assert!(said.contains("4 files already there"), "{said}");
    assert_ne!(
        t.saved().volume.as_deref(),
        Some("device 1"),
        "known by the number it has now"
    );
}

#[test]
fn a_folder_saved_before_its_disk_was_kept_goes_on_as_before_and_is_known_from_then_on() {
    let t = Tester::new();
    t.run();
    let mut all = connect::destinations(&t.catalog).unwrap();
    all[0].volume = None;
    t.catalog
        .set_setting("destinations", &serde_json::to_string(&all).unwrap())
        .unwrap();
    t.empty_the_folder();
    let (said, _) = t.run();
    assert!(said.contains("looks emptied or replaced"), "as before: {said}");
    assert!(t.saved().volume.is_some(), "and its disk is kept once reached");
}

/// The four photos' places on the card, in order.
fn card_photos(t: &Tester) -> Vec<PathBuf> {
    let mut photos = walk(&t.dir.path().join("CARD"));
    photos.sort();
    photos
}

#[test]
fn the_owners_second_screen_now_tells_one_truth() {
    // The first run: one file can't be read and is missed. (The owner's
    // 176 were missed at the share, which the history now says; here the
    // card is simplest.)
    let t = Tester::new();
    let photo = card_photos(&t)[3].clone();
    let aside = photo.with_extension("away");
    std::fs::rename(&photo, &aside).unwrap();
    let (first, went) = t.run();
    assert_eq!(went.failed_files, 1);
    assert!(first.contains("Couldn't read it from its source"), "{first}");
    assert!(
        first.contains(&format!(
            "3 of the 4 files you are keeping are safely at the folder {}. 1 still needs attention: 1 couldn't be \
             sent.",
            t.usb().display()
        )),
        "{first}"
    );
    std::fs::rename(&aside, &photo).unwrap();

    // What 0.42.0's look left behind on the owner's computer: records of
    // files that are there and exact, written down as gone and as another
    // size, because the share's listing said so.
    let keys = keys_of(&t);
    t.catalog
        .set_seen(
            &t.target(),
            &[(keys[0].clone(), Seen::Missing), (keys[1].clone(), Seen::NotAsSent)],
        )
        .unwrap();

    // "Back up again": the look asks each of those files by itself, finds
    // them there, and says nothing it would have to take back.
    let (second, went) = t.run();
    assert_eq!(went.failed_files, 0);
    assert!(!second.contains("no longer holds"), "{second}");
    assert!(!second.contains("not the size"), "{second}");
    assert!(second.contains("Safely backed up 1 file"), "{second}");
    assert!(second.contains("3 files already there."), "{second}");
    assert!(
        second.contains(&format!(
            "All 4 files you are keeping are safely at the folder {}.",
            t.usb().display()
        )),
        "{second}"
    );
    assert!(t.records().iter().all(|r| r.seen == Seen::There));
    assert_eq!(t.waiting(), 0);
    assert!(t.view().said.is_empty());
}

#[test]
fn a_file_really_gone_is_sent_again_in_the_same_run_that_says_so() {
    let t = Tester::new();
    t.run();
    let keys = keys_of(&t);
    std::fs::remove_file(t.usb().join(&keys[2])).unwrap();
    let (said, _) = t.run();
    assert!(said.contains("It will be sent again."), "{said}");
    assert!(said.contains("Restored 1 file that had gone from there."), "{said}");
    assert!(t.usb().join(&keys[2]).is_file(), "sent again, in this run");
    assert!(said.contains("All 4 files you are keeping are safely at"), "{said}");
}
