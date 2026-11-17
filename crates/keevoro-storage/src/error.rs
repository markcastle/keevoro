use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("invalid object key {key:?}: {reason}")]
    InvalidKey { key: String, reason: &'static str },

    #[error("object not found: {0}")]
    NotFound(String),

    #[error("object already exists: {0}")]
    AlreadyExists(String),

    #[error("upload not found: {0}")]
    UploadNotFound(String),

    #[error("invalid multipart request: {0}")]
    InvalidParts(String),

    #[error("not supported by this provider: {0}")]
    Unsupported(&'static str),

    #[error("access denied: {0}")]
    AccessDenied(String),

    #[error("network problem: {0}")]
    Network(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("provider error: {0}")]
    Provider(String),

    /// What was read, or what was written, is not the file that was
    /// expected. Nothing is left stored under the key.
    #[error("not the same as the original: {0}")]
    NotTheSame(String),
}

impl StorageError {
    /// Worth retrying after a pause. The engine owns the retry policy.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Network(_) => true,
            Self::Io(e) => matches!(
                e.kind(),
                std::io::ErrorKind::Interrupted | std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            ),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_transient_errors_are_retryable() {
        assert!(StorageError::Network("reset".into()).is_retryable());
        assert!(StorageError::Io(std::io::Error::from(std::io::ErrorKind::TimedOut)).is_retryable());
        assert!(!StorageError::Io(std::io::Error::from(std::io::ErrorKind::PermissionDenied)).is_retryable());
        assert!(!StorageError::AccessDenied("bad key".into()).is_retryable());
        assert!(!StorageError::AlreadyExists("a".into()).is_retryable());
    }

    #[test]
    fn errors_read_plainly() {
        let e = StorageError::InvalidKey {
            key: "../x".into(),
            reason: "contains '..'",
        };
        assert_eq!(e.to_string(), "invalid object key \"../x\": contains '..'");
    }
}
