use crate::StorageError;
use std::fmt;

/// A validated object key, such as `originals/Holidays/Spain 2009/IMG_1234.jpg`.
///
/// Keys use `/` between segments and are safe to map onto a folder tree:
/// no empty, `.` or `..` segments, no backslashes and no control characters.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectKey(String);

impl ObjectKey {
    /// S3's limit is 1024 bytes of UTF-8.
    pub const MAX_BYTES: usize = 1024;

    pub fn new(key: impl Into<String>) -> Result<Self, StorageError> {
        let key = key.into();
        let bad = |reason| {
            Err(StorageError::InvalidKey {
                key: key.clone(),
                reason,
            })
        };
        if key.is_empty() {
            return bad("is empty");
        }
        if key.len() > Self::MAX_BYTES {
            return bad("is longer than 1024 bytes");
        }
        if key.contains('\\') {
            return bad("contains a backslash");
        }
        if key.chars().any(char::is_control) {
            return bad("contains a control character");
        }
        for seg in key.split('/') {
            match seg {
                "" => return bad("has an empty segment"),
                "." | ".." => return bad("contains '.' or '..'"),
                _ => {}
            }
        }
        Ok(Self(key))
    }

    /// Builds a key from path segments, for example a library-relative path.
    pub fn from_segments<I, S>(segments: I) -> Result<Self, StorageError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let parts: Vec<String> = segments.into_iter().map(|s| s.as_ref().to_string()).collect();
        Self::new(parts.join("/"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }
}

impl fmt::Display for ObjectKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_library_paths_with_spaces_and_unicode() {
        let k = ObjectKey::new("originals/Holidays/Spain 2009/Día 2/IMG_1234.jpg").unwrap();
        assert_eq!(k.segments().count(), 5);
        assert_eq!(k.to_string(), "originals/Holidays/Spain 2009/Día 2/IMG_1234.jpg");
    }

    #[test]
    fn rejects_unsafe_keys() {
        for key in [
            "",
            "/abs",
            "trailing/",
            "a//b",
            "a/../b",
            "./a",
            "a\\b",
            "a\u{0}b",
            "a\nb",
        ] {
            assert!(ObjectKey::new(key).is_err(), "{key:?}");
        }
        assert!(ObjectKey::new("a".repeat(1025)).is_err());
        assert!(ObjectKey::new("a".repeat(1024)).is_ok());
    }

    #[test]
    fn builds_from_segments() {
        let k = ObjectKey::from_segments(["renditions", "thumb", "9f2c4be1a07d33e8.webp"]).unwrap();
        assert_eq!(k.as_str(), "renditions/thumb/9f2c4be1a07d33e8.webp");
        assert!(ObjectKey::from_segments(["a", ".."]).is_err());
    }
}
