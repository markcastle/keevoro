//! Cloud storage as a source: a bucket, or a folder in one, that a backup
//! reads from, so an old cloud backup can be moved to a cheaper cloud, two
//! merged into one, or a bucket brought down to a drive.
//!
//! A bucket source is only ever read: listed, and each object read when it
//! is backed up. Nothing in it is ever written, replaced or deleted (the
//! engine is given it as `keevoro_storage::ReadOnly`, which cannot write).
//!
//! It goes through everything a folder source does by the paths of its
//! files, so it has a root that reads like a place, `s3://family-photos/Photos`,
//! and each object's path is that root, `/`, and the rest of its key. A
//! drive letter is one letter and a Windows folder name cannot hold `:`,
//! so such a path can never be a real folder, and [`is_bucket_path`] tells
//! the two apart wherever a file would be opened.
//!
//! Pure: everything here is tested without a network.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// The schemes a bucket source's root starts with, one for each kind of
/// cloud storage a destination can be.
const SCHEMES: [&str; 4] = ["r2://", "s3://", "gcs://", "endpoint://"];

/// The kinds of cloud storage Keevoro knows, as they are kept.
pub const KINDS: [&str; 4] = ["r2", "s3", "gcs", "endpoint"];

/// Google Cloud Storage's place. It has one address for every bucket, and
/// no account or region to ask for, so Keevoro fills this in itself and
/// the person is asked only for the bucket and the keys.
pub const GOOGLE_PLACE: &str = "storage.googleapis.com";

/// The place a kind of storage is kept with: what was asked for, tidied,
/// or for Google Cloud Storage, its one address whatever was given.
pub fn place_for(kind: &str, asked: &str) -> String {
    if kind == "gcs" {
        GOOGLE_PLACE.to_string()
    } else {
        asked.trim().to_string()
    }
}

/// Where a bucket source is. Holds nothing secret: the keys are in the
/// computer's keychain, under [`BucketPlace::secret_name`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BucketPlace {
    /// `r2`, `s3`, `gcs` or `endpoint`, as for a destination.
    pub kind: String,
    /// The Cloudflare account ID, Amazon region or server address; for
    /// Google Cloud Storage, always [`GOOGLE_PLACE`].
    pub place: String,
    pub bucket: String,
    /// The folder in the bucket, without a `/` at either end; empty for
    /// the whole bucket.
    #[serde(default)]
    pub prefix: String,
}

impl BucketPlace {
    /// A place as it is asked for, tidied: the prefix without slashes at
    /// either end. `None` when the kind is not one Keevoro knows, or the
    /// place or bucket is empty. Google Cloud Storage needs no place: it is
    /// filled in.
    pub fn new(kind: &str, place: &str, bucket: &str, prefix: &str) -> Option<Self> {
        let (place, bucket) = (place_for(kind, place), bucket.trim());
        let known = KINDS.contains(&kind);
        (known && !place.is_empty() && !bucket.is_empty()).then(|| Self {
            kind: kind.to_string(),
            place,
            bucket: bucket.to_string(),
            prefix: prefix
                .trim()
                .trim_matches('/')
                .split('/')
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("/"),
        })
    }

    /// Its root, as its files' paths start: `s3://family-photos/Photos`.
    pub fn root(&self) -> String {
        let mut root = format!("{}://{}", self.kind, self.bucket);
        if !self.prefix.is_empty() {
            root.push('/');
            root.push_str(&self.prefix);
        }
        root
    }

    /// What to list: the prefix and a `/`, or everything.
    pub fn listing_prefix(&self) -> String {
        if self.prefix.is_empty() {
            String::new()
        } else {
            format!("{}/", self.prefix)
        }
    }

    /// The service, as a person knows it.
    pub fn provider(&self) -> &'static str {
        match self.kind.as_str() {
            "r2" => "Cloudflare R2",
            "s3" => "Amazon S3",
            "gcs" => "Google Cloud Storage",
            _ => "cloud storage",
        }
    }

    /// In words: "the Amazon S3 bucket family-photos", "the folder Photos
    /// in the Cloudflare R2 bucket old-backup".
    pub fn describe(&self) -> String {
        let bucket = match self.kind.as_str() {
            "r2" | "s3" | "gcs" => format!("the {} bucket {}", self.provider(), self.bucket),
            _ => format!("the bucket {} at {}", self.bucket, self.place),
        };
        if self.prefix.is_empty() {
            bucket
        } else {
            format!("the folder {} in {bucket}", self.prefix)
        }
    }

    /// What to call it as a source: its folder's name, or the bucket's.
    pub fn name(&self) -> String {
        self.prefix
            .rsplit('/')
            .next()
            .filter(|n| !n.is_empty())
            .unwrap_or(&self.bucket)
            .to_string()
    }

    /// The keychain entry its keys are kept under: one per bucket and
    /// folder, apart from any destination's.
    pub fn secret_name(&self) -> String {
        format!("source:{}/{}/{}", self.kind, self.place, self.root())
    }

    /// The key of the object a file of this source is at, from its path.
    pub fn key_of(&self, path: &Path) -> Option<String> {
        let path = path.to_string_lossy().replace('\\', "/");
        let rest = path.strip_prefix(&self.root())?.strip_prefix('/')?;
        (!rest.is_empty()).then(|| format!("{}{rest}", self.listing_prefix()))
    }

    /// The path a file at `key` goes by, from the key.
    pub fn path_of(&self, key: &str) -> Option<String> {
        let rest = key.strip_prefix(&self.listing_prefix())?;
        (!rest.is_empty()).then(|| format!("{}/{rest}", self.root()))
    }

    /// Does this source read from the same bucket a destination writes to,
    /// in a folder a backup writes (`originals`, `renditions`, `index`) or
    /// the whole bucket? A backup must never read what it is writing.
    pub fn overlaps_a_backup_at(&self, kind: &str, place: &str, bucket: &str) -> bool {
        let same = self.kind == kind && self.place.trim() == place.trim() && self.bucket == bucket.trim();
        let top = self.prefix.split('/').next().unwrap_or_default();
        same && (self.prefix.is_empty() || ["originals", "renditions", "index"].contains(&top))
    }
}

/// True for a path that is a file in a bucket source, never a file on a
/// disk. Nothing may open one as a file.
pub fn is_bucket_path(path: &Path) -> bool {
    let text = path.to_string_lossy();
    SCHEMES.iter().any(|s| text.starts_with(s))
}

/// The service a file in a bucket source is kept by, from its path, as a
/// person knows it: "Amazon S3", "Cloudflare R2", "Google Cloud Storage",
/// or for any other, "the storage". `None` for a file on a disk.
pub fn provider_of(path: &Path) -> Option<&'static str> {
    let text = path.to_string_lossy();
    if text.starts_with("r2://") {
        Some("Cloudflare R2")
    } else if text.starts_with("s3://") {
        Some("Amazon S3")
    } else if text.starts_with("gcs://") {
        Some("Google Cloud Storage")
    } else if text.starts_with("endpoint://") {
        Some("the storage")
    } else {
        None
    }
}

/// The quick fingerprint of an object, from what its listing says: where
/// it is, its size, its tag and when it was written. It changes whenever
/// the object does, so a second look recognises an unchanged object and
/// keeps the whole fingerprint learnt while it was backed up, reading
/// nothing. It is unique to the object, so no two objects ever look alike
/// by it, and it can never equal a file's fingerprint, quick or whole. It
/// says nothing about the contents: copies are found once the whole
/// fingerprint is learnt, as each object is read to be backed up.
pub fn quick_fingerprint(root: &str, key: &str, bytes: u64, etag: Option<&str>, modified: Option<i64>) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"keevoro bucket fingerprint 1\0");
    for part in [
        root,
        key,
        &bytes.to_string(),
        etag.unwrap_or_default(),
        &modified.unwrap_or_default().to_string(),
    ] {
        hasher.update(part.as_bytes());
        hasher.update(b"\0");
    }
    hasher.finalize().to_hex().to_string()
}

/// True for a storage class whose objects cannot be read until they are
/// restored (Amazon's Glacier Flexible Retrieval and Deep Archive). Glacier
/// Instant Retrieval, R2's Infrequent Access, and Google's Nearline,
/// Coldline and Archive can be read as they are, at a charge for each
/// gigabyte read, which [`crate::egress`] says.
pub fn needs_restoring(class: Option<&str>) -> bool {
    matches!(class, Some("GLACIER" | "DEEP_ARCHIVE"))
}

/// True for one of Google Cloud Storage's colder classes (Nearline, Coldline
/// and Archive), which are read at once like any other, but at a charge
/// for each gigabyte read. Files in them are counted and said, as files in
/// Amazon's archive storage are, and backed up all the same.
pub fn costs_more_to_read(class: Option<&str>) -> bool {
    matches!(class, Some("NEARLINE" | "COLDLINE" | "ARCHIVE"))
}

/// A whole fingerprint Keevoro itself wrote on an object it backed up
/// (`blake3`, as `x-amz-meta-blake3`), when it looks like one: 64
/// lower-case hexadecimal letters. Anything else is not used.
pub fn label(metadata_value: Option<&str>) -> Option<String> {
    let value = metadata_value?.trim();
    (value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()))
        .then(|| value.to_string())
}

/// Is `prefix` (`backup/originals/2019/`) inside a Keevoro backup's
/// `originals`, where each object carries the fingerprint Keevoro labelled
/// it with? Only then are labels asked for: asking costs a request for
/// each object.
pub fn inside_originals(prefix: &str) -> bool {
    prefix.split('/').any(|s| s == "originals")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn photos() -> BucketPlace {
        BucketPlace::new("s3", "eu-west-2", "family-photos", "/Photos/2019/").unwrap()
    }

    #[test]
    fn a_bucket_source_has_a_root_that_reads_like_a_place_and_is_never_a_folder() {
        let p = photos();
        assert_eq!(p.prefix, "Photos/2019");
        assert_eq!(p.root(), "s3://family-photos/Photos/2019");
        assert_eq!(p.listing_prefix(), "Photos/2019/");
        assert_eq!(p.name(), "2019");
        assert_eq!(
            p.describe(),
            "the folder Photos/2019 in the Amazon S3 bucket family-photos"
        );
        let whole = BucketPlace::new("r2", "0123", "old-backup", "").unwrap();
        assert_eq!(
            (whole.root(), whole.listing_prefix()),
            ("r2://old-backup".into(), String::new())
        );
        assert_eq!(whole.name(), "old-backup");
        assert_eq!(whole.describe(), "the Cloudflare R2 bucket old-backup");
        assert!(BucketPlace::new("ftp", "x", "y", "").is_none());
        assert!(BucketPlace::new("s3", "eu-west-2", " ", "").is_none());

        assert!(is_bucket_path(Path::new("s3://family-photos/Photos/a.jpg")));
        assert!(is_bucket_path(Path::new("endpoint://b/a.jpg")));
        assert!(!is_bucket_path(Path::new("/home/you/s3://x")));
        assert!(!is_bucket_path(Path::new("C:\\Photos\\a.jpg")));
        // Its files' paths work like any other's.
        let file = Path::new("s3://family-photos/Photos/2019/Spain/IMG_1.jpg");
        assert_eq!(file.file_name().unwrap(), "IMG_1.jpg");
        assert_eq!(file.extension().unwrap(), "jpg");
        assert!(crate::places::within(file, Path::new(&p.root())));
    }

    #[test]
    fn google_cloud_storage_needs_only_a_bucket_and_fills_in_its_own_place() {
        for asked in ["", "  ", "anything typed"] {
            let p = BucketPlace::new("gcs", asked, "family-archive", "Scans").unwrap();
            assert_eq!(p.place, GOOGLE_PLACE, "{asked:?}");
        }
        let p = BucketPlace::new("gcs", "", "family-archive", "").unwrap();
        assert_eq!(p.root(), "gcs://family-archive");
        assert_eq!(p.provider(), "Google Cloud Storage");
        assert_eq!(p.describe(), "the Google Cloud Storage bucket family-archive");
        assert!(is_bucket_path(Path::new("gcs://family-archive/a.jpg")));
        assert_eq!(
            provider_of(Path::new("gcs://family-archive/a.jpg")),
            Some("Google Cloud Storage")
        );
        assert_eq!(
            p.secret_name(),
            "source:gcs/storage.googleapis.com/gcs://family-archive"
        );
        assert!(p.overlaps_a_backup_at("gcs", GOOGLE_PLACE, "family-archive"));
        assert!(!p.overlaps_a_backup_at("s3", GOOGLE_PLACE, "family-archive"));
        assert_eq!(place_for("s3", " eu-west-2 "), "eu-west-2");
    }

    #[test]
    fn a_file_is_known_by_its_key_and_its_key_by_its_path() {
        let p = photos();
        let path = p.path_of("Photos/2019/Spain/IMG_1.jpg").unwrap();
        assert_eq!(path, "s3://family-photos/Photos/2019/Spain/IMG_1.jpg");
        assert_eq!(p.key_of(Path::new(&path)).unwrap(), "Photos/2019/Spain/IMG_1.jpg");
        assert_eq!(p.path_of("Other/IMG_1.jpg"), None, "outside the source");
        assert_eq!(p.key_of(Path::new("s3://family-photos/Photos")), None);
        let whole = BucketPlace::new("r2", "0123", "old", "").unwrap();
        assert_eq!(whole.key_of(Path::new("r2://old/a/b.jpg")).unwrap(), "a/b.jpg");
    }

    #[test]
    fn the_quick_fingerprint_changes_with_the_object_and_is_never_shared() {
        let a = quick_fingerprint("s3://b", "a.jpg", 10, Some("\"e1\""), Some(5));
        assert_eq!(
            a,
            quick_fingerprint("s3://b", "a.jpg", 10, Some("\"e1\""), Some(5)),
            "the same each look"
        );
        assert_ne!(
            a,
            quick_fingerprint("s3://b", "b.jpg", 10, Some("\"e1\""), Some(5)),
            "another object"
        );
        assert_ne!(
            a,
            quick_fingerprint("s3://b", "a.jpg", 10, Some("\"e2\""), Some(5)),
            "written again"
        );
        assert_ne!(
            a,
            quick_fingerprint("s3://b", "a.jpg", 11, Some("\"e1\""), Some(5)),
            "another size"
        );
        assert_ne!(
            a,
            quick_fingerprint("r2://b", "a.jpg", 10, Some("\"e1\""), Some(5)),
            "another bucket"
        );
        assert_ne!(a, blake3::hash(b"").to_hex().to_string());
    }

    #[test]
    fn archived_objects_need_restoring_and_others_can_be_read() {
        assert!(needs_restoring(Some("GLACIER")));
        assert!(needs_restoring(Some("DEEP_ARCHIVE")));
        for readable in [
            None,
            Some("STANDARD"),
            Some("GLACIER_IR"),
            Some("STANDARD_IA"),
            Some("INTELLIGENT_TIERING"),
            // Google's colder classes are read at once, at a charge.
            Some("NEARLINE"),
            Some("COLDLINE"),
            Some("ARCHIVE"),
        ] {
            assert!(!needs_restoring(readable), "{readable:?}");
        }
        for colder in ["NEARLINE", "COLDLINE", "ARCHIVE"] {
            assert!(costs_more_to_read(Some(colder)), "{colder}");
        }
        for plain in [None, Some("STANDARD"), Some("GLACIER"), Some("REGIONAL")] {
            assert!(!costs_more_to_read(plain), "{plain:?}");
        }
    }

    #[test]
    fn only_a_fingerprint_keevoro_wrote_is_taken_as_a_label() {
        let good = blake3::hash(b"photo").to_hex().to_string();
        assert_eq!(label(Some(&good)), Some(good.clone()));
        assert_eq!(label(Some(&good.to_uppercase())), None);
        assert_eq!(
            label(Some("d41d8cd98f00b204e9800998ecf8427e")),
            None,
            "an MD5 is not one"
        );
        assert_eq!(label(None), None);
        assert!(inside_originals("backup/originals/2019/"));
        assert!(!inside_originals("Photos/"));
    }

    #[test]
    fn a_source_in_the_folders_a_backup_writes_is_where_it_goes() {
        for prefix in ["", "originals", "originals/2019", "index", "renditions"] {
            let p = BucketPlace::new("r2", "acct", "family", prefix).unwrap();
            assert!(p.overlaps_a_backup_at("r2", "acct", "family"), "{prefix:?}");
        }
        let photos = BucketPlace::new("r2", "acct", "family", "Phone uploads").unwrap();
        assert!(!photos.overlaps_a_backup_at("r2", "acct", "family"));
        let whole = BucketPlace::new("r2", "acct", "family", "").unwrap();
        assert!(!whole.overlaps_a_backup_at("r2", "acct", "other"));
        assert!(!whole.overlaps_a_backup_at("s3", "acct", "family"));
    }
}
