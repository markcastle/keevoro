use thiserror::Error;

/// Which step of storing a file a problem came at, so that it is said as
/// what it was: the source could not be read, or the destination could not
/// be written, moved into place or read back. A problem at a destination is
/// never "couldn't read it from its source".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Reading the original, where it was found.
    ReadingTheSource,
    /// Writing the copy at the destination.
    Writing,
    /// Making sure the copy has reached the disk or the far end.
    Flushing,
    /// Giving the copy its real name at the destination.
    MovingIntoPlace,
    /// Reading the copy back from the destination to prove it.
    ReadingBack,
}

impl Step {
    /// True for every step that happens at the destination.
    pub fn at_the_destination(self) -> bool {
        !matches!(self, Step::ReadingTheSource)
    }
}

impl std::fmt::Display for Step {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Step::ReadingTheSource => "reading the source",
            Step::Writing => "writing the copy",
            Step::Flushing => "finishing writing the copy",
            Step::MovingIntoPlace => "moving the copy into place",
            Step::ReadingBack => "reading the copy back",
        })
    }
}

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

    /// A problem with the disk or the network at one step of storing a
    /// file, said with the side it happened on.
    #[error("{step}: {source}")]
    At { step: Step, source: std::io::Error },

    #[error("provider error: {0}")]
    Provider(String),

    /// What was read, or what was written, is not the file that was
    /// expected. Nothing is left stored under the key.
    #[error("not the same as the original: {0}")]
    NotTheSame(String),
}

impl StorageError {
    /// A problem at `step`.
    pub fn at(step: Step, source: std::io::Error) -> Self {
        Self::At { step, source }
    }

    /// The operating system's own error behind this, if there is one.
    pub fn io(&self) -> Option<&std::io::Error> {
        match self {
            Self::Io(e) | Self::At { source: e, .. } => Some(e),
            _ => None,
        }
    }

    /// The step it happened at, when that is known.
    pub fn step(&self) -> Option<Step> {
        match self {
            Self::At { step, .. } => Some(*step),
            _ => None,
        }
    }

    /// Worth retrying after a pause. The engine owns the retry policy.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Network(_) => true,
            Self::Io(e) | Self::At { source: e, .. } => matches!(
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
