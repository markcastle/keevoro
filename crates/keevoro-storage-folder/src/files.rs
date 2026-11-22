//! The few things a copy asks of the disk or share it writes to, behind a
//! trait, so that tests can stand in a share that drops out part-way: a
//! move into place that seems to work but leaves a short file, a file that
//! has gone by the time it is read back, a flush that fails. Keevoro's own
//! folder store always uses [`RealFiles`].

use std::io::{Read, Write};
use std::path::Path;
use std::time::SystemTime;

/// A file being written at a destination.
pub trait Written: Write + Send {
    /// Past the computer's own memory and onto the disk or the far end.
    fn sync(&mut self) -> std::io::Result<()>;
    /// Gives the copy a modified time (the original's).
    fn set_modified(&mut self, when: SystemTime) -> std::io::Result<()>;
}

/// The file operations a copy makes at a destination.
pub trait Files: Send + Sync {
    /// Makes a new file, never over one already there.
    fn create_new(&self, path: &Path) -> std::io::Result<Box<dyn Written>>;
    /// Opens a file to read it, afresh: never a handle already open.
    fn open(&self, path: &Path) -> std::io::Result<Box<dyn Read + Send>>;
    /// The size of a file, from a fresh question to the disk or share;
    /// `None` when nothing is there.
    fn size(&self, path: &Path) -> std::io::Result<Option<u64>>;
    /// A second name for a file, refused if the name is taken.
    fn hard_link(&self, from: &Path, to: &Path) -> std::io::Result<()>;
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()>;
    fn remove(&self, path: &Path) -> std::io::Result<()>;
    fn exists(&self, path: &Path) -> std::io::Result<bool>;
    fn make_folders(&self, path: &Path) -> std::io::Result<()>;
}

/// The computer's own files.
#[derive(Debug, Default, Clone, Copy)]
pub struct RealFiles;

struct RealWritten(std::fs::File);

impl Write for RealWritten {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

impl Written for RealWritten {
    fn sync(&mut self) -> std::io::Result<()> {
        self.0.sync_all()
    }
    fn set_modified(&mut self, when: SystemTime) -> std::io::Result<()> {
        self.0.set_modified(when)
    }
}

impl Files for RealFiles {
    fn create_new(&self, path: &Path) -> std::io::Result<Box<dyn Written>> {
        let file = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
        Ok(Box::new(RealWritten(file)))
    }
    fn open(&self, path: &Path) -> std::io::Result<Box<dyn Read + Send>> {
        Ok(Box::new(std::fs::File::open(path)?))
    }
    fn size(&self, path: &Path) -> std::io::Result<Option<u64>> {
        match std::fs::metadata(path) {
            Ok(meta) if meta.is_file() => Ok(Some(meta.len())),
            Ok(_) => Ok(None),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }
    fn hard_link(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        std::fs::hard_link(from, to)
    }
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        std::fs::rename(from, to)
    }
    fn remove(&self, path: &Path) -> std::io::Result<()> {
        std::fs::remove_file(path)
    }
    fn exists(&self, path: &Path) -> std::io::Result<bool> {
        path.try_exists()
    }
    fn make_folders(&self, path: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(path)
    }
}
