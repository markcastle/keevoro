//! A share that drops out part-way, and what a copy does about it.
//!
//! The owner's backup to a network share recorded 79 files as safely there
//! that a look later found missing or another size. Before 0.43 a copy was
//! read back from its working name, before it was moved into place, and the
//! move was trusted. Each fault here is one way a share that loses its
//! connection and comes back can leave a file short or absent at its real
//! name while everything before seemed to work. Every one of them would
//! have been counted by the copy before 0.43 (it read back the working
//! file and believed the move); none is now.

use super::*;
use std::collections::HashMap;
use std::io::{Cursor, Read, Write};
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fault {
    /// The move into place answers that it worked, but the file at its real
    /// name is shorter: the session was lost while it was being moved.
    ShortAfterMove,
    /// The move answers that it worked, and nothing is at the real name.
    GoneAfterMove,
    /// Flushing the copy to the far end fails.
    FlushFails,
    /// The copy cannot be opened again to be read back.
    ReadBackFails,
    /// The far end only ever received the first half, but reading the file
    /// by the name it was written under answers from this computer's own
    /// memory of what was written: a read back that proves nothing.
    FarEndLosesTheTail,
    /// The working name cannot be removed once the copy is proved.
    WorkingNameStays,
    /// Hard links refused for a reason other than the name being taken, as
    /// on FAT drives and some shares.
    NoHardLinks,
}

struct Dropping {
    fault: Fault,
    /// What this computer remembers having written, by the name it was
    /// written under.
    cache: Mutex<HashMap<PathBuf, Vec<u8>>>,
}

impl Dropping {
    fn new(fault: Fault) -> Arc<Self> {
        Arc::new(Self {
            fault,
            cache: Mutex::new(HashMap::new()),
        })
    }
}

struct Writing {
    path: PathBuf,
    held: Vec<u8>,
    fault: Fault,
    cache: Arc<Dropping>,
}

impl Write for Writing {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.held.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Written for Writing {
    fn sync(&mut self) -> std::io::Result<()> {
        if self.fault == Fault::FlushFails {
            return Err(std::io::Error::other(
                "The specified network name is no longer available. (os error 64)",
            ));
        }
        let reached = if self.fault == Fault::FarEndLosesTheTail {
            &self.held[..self.held.len() / 2]
        } else {
            &self.held[..]
        };
        std::fs::write(&self.path, reached)?;
        self.cache
            .cache
            .lock()
            .unwrap()
            .insert(self.path.clone(), self.held.clone());
        Ok(())
    }
    fn set_modified(&mut self, _when: SystemTime) -> std::io::Result<()> {
        Ok(())
    }
}

/// So a test can hand the stand-in out and still look at it.
struct Shared(Arc<Dropping>);

impl Files for Shared {
    fn create_new(&self, path: &Path) -> std::io::Result<Box<dyn Written>> {
        // Made at once, as a real file would be.
        std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
        Ok(Box::new(Writing {
            path: path.to_path_buf(),
            held: Vec::new(),
            fault: self.0.fault,
            cache: self.0.clone(),
        }))
    }
    fn open(&self, path: &Path) -> std::io::Result<Box<dyn Read + Send>> {
        if self.0.fault == Fault::ReadBackFails && !path.to_string_lossy().contains("tmp-") {
            return Err(std::io::Error::other("The network path was not found. (os error 53)"));
        }
        if let Some(remembered) = self.0.cache.lock().unwrap().get(path) {
            return Ok(Box::new(Cursor::new(remembered.clone())));
        }
        RealFiles.open(path)
    }
    fn size(&self, path: &Path) -> std::io::Result<Option<u64>> {
        RealFiles.size(path)
    }
    fn hard_link(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        if self.0.fault == Fault::NoHardLinks {
            return Err(std::io::Error::other("hard links are not supported here"));
        }
        RealFiles.hard_link(from, to)?;
        self.after_move(to)
    }
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        RealFiles.rename(from, to)?;
        self.after_move(to)
    }
    fn remove(&self, path: &Path) -> std::io::Result<()> {
        if self.0.fault == Fault::WorkingNameStays {
            return Err(std::io::Error::other(
                "The specified network name is no longer available. (os error 64)",
            ));
        }
        RealFiles.remove(path)
    }
    fn exists(&self, path: &Path) -> std::io::Result<bool> {
        RealFiles.exists(path)
    }
    fn make_folders(&self, path: &Path) -> std::io::Result<()> {
        RealFiles.make_folders(path)
    }
}

impl Shared {
    fn after_move(&self, to: &Path) -> std::io::Result<()> {
        match self.0.fault {
            Fault::ShortAfterMove => {
                let whole = std::fs::read(to)?;
                // A link shares its data: cut the file itself short.
                std::fs::OpenOptions::new()
                    .write(true)
                    .open(to)?
                    .set_len((whole.len() / 3) as u64)?;
            }
            Fault::GoneAfterMove => {
                std::fs::remove_file(to)?;
            }
            _ => {}
        }
        Ok(())
    }
}

struct Setup {
    _dir: tempfile::TempDir,
    store: FolderStore,
    photo: PathBuf,
    bytes: Vec<u8>,
    root: PathBuf,
}

async fn setup(fault: Fault) -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("share");
    let store = FolderStore::open(&root)
        .await
        .unwrap()
        .with_files(Arc::new(Shared(Dropping::new(fault))));
    let photo = dir.path().join("IMG_0415.JPG");
    let bytes: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(&photo, &bytes).unwrap();
    Setup {
        _dir: dir,
        store,
        photo,
        bytes,
        root,
    }
}

fn original() -> ObjectKey {
    ObjectKey::new("originals/By date/2016/2016-12/IMG_0415.JPG").unwrap()
}

async fn send(s: &Setup) -> Result<PutReceipt, StorageError> {
    let opts = PutOptions {
        create_only: true,
        expect_blake3: Some(blake3::hash(&s.bytes).to_hex().to_string()),
        ..Default::default()
    };
    s.store.put(&original(), ByteSource::File(s.photo.clone()), &opts).await
}

fn working_files(s: &Setup) -> usize {
    std::fs::read_dir(s.root.join(WORK_DIR)).map_or(0, Iterator::count)
}

fn untouched(s: &Setup) {
    assert_eq!(
        std::fs::read(&s.photo).unwrap(),
        s.bytes,
        "the source is only ever read"
    );
}

#[tokio::test]
async fn a_move_into_place_that_leaves_a_short_file_is_never_counted() {
    let s = setup(Fault::ShortAfterMove).await;
    let sent = send(&s).await;
    assert!(matches!(sent, Err(StorageError::NotTheSame(_))), "{sent:?}");
    // Left where it is, as nothing in a backup is deleted: it is simply
    // never counted, and the next backup finds it the wrong size.
    let there = s.store.head(&original()).await.unwrap().unwrap();
    assert_eq!(there.bytes, 100_000);
    assert_eq!(working_files(&s), 0, "the working name is cleared");
    untouched(&s);
}

#[tokio::test]
async fn a_copy_that_is_not_there_once_moved_into_place_is_never_counted() {
    let s = setup(Fault::GoneAfterMove).await;
    let sent = send(&s).await;
    assert!(matches!(sent, Err(StorageError::NotTheSame(_))), "{sent:?}");
    assert!(s.store.head(&original()).await.unwrap().is_none());
    untouched(&s);
}

#[tokio::test]
async fn a_flush_that_fails_is_never_counted_and_said_as_the_destinations() {
    let s = setup(Fault::FlushFails).await;
    let sent = send(&s).await;
    assert!(
        matches!(
            &sent,
            Err(StorageError::At {
                step: Step::Flushing,
                ..
            })
        ),
        "{sent:?}"
    );
    assert!(sent.unwrap_err().step().unwrap().at_the_destination());
    assert!(
        s.store.head(&original()).await.unwrap().is_none(),
        "never moved into place"
    );
    assert_eq!(working_files(&s), 0);
    untouched(&s);
}

#[tokio::test]
async fn a_read_back_that_fails_is_never_counted() {
    let s = setup(Fault::ReadBackFails).await;
    let sent = send(&s).await;
    assert!(
        matches!(
            &sent,
            Err(StorageError::At {
                step: Step::ReadingBack,
                ..
            })
        ),
        "{sent:?}"
    );
    untouched(&s);
}

#[tokio::test]
async fn a_read_back_answered_from_this_computers_memory_is_not_what_proves_a_copy() {
    // Read back by the name it was written under, as before 0.43, this
    // copy would have matched; at its real name, read afresh, it does not.
    let s = setup(Fault::FarEndLosesTheTail).await;
    let sent = send(&s).await;
    assert!(matches!(sent, Err(StorageError::NotTheSame(_))), "{sent:?}");
    untouched(&s);
}

#[tokio::test]
async fn a_working_name_that_cannot_be_removed_does_not_unprove_a_good_copy() {
    let s = setup(Fault::WorkingNameStays).await;
    let receipt = send(&s).await.unwrap();
    assert_eq!(receipt.bytes, s.bytes.len() as u64);
    assert_eq!(s.store.get(&original()).await.unwrap(), s.bytes);
    // Left for the sweep, which clears it an hour later.
    assert_eq!(working_files(&s), 1);
}

#[tokio::test]
async fn without_hard_links_a_copy_is_moved_and_still_proved_at_its_real_name() {
    let s = setup(Fault::NoHardLinks).await;
    send(&s).await.unwrap();
    assert_eq!(s.store.get(&original()).await.unwrap(), s.bytes);
    assert_eq!(working_files(&s), 0);
    // And never over a file already there.
    let again = send(&s).await;
    assert!(matches!(again, Err(StorageError::AlreadyExists(_))), "{again:?}");
}

#[tokio::test]
async fn a_source_that_cannot_be_read_is_said_as_the_sources() {
    let s = setup(Fault::ShortAfterMove).await;
    std::fs::remove_file(&s.photo).unwrap();
    let sent = send(&s).await;
    let step = sent.unwrap_err().step();
    assert_eq!(step, Some(Step::ReadingTheSource));
    assert!(!step.unwrap().at_the_destination());
}
