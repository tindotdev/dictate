//! Vocabulary-specific error types.

/// Errors that can occur when loading or saving the vocabulary.
#[derive(Debug, thiserror::Error)]
pub enum VocabularyError {
    /// Could not determine the platform config directory.
    #[error("unable to determine config directory")]
    NoConfigDir,

    /// IO error reading or writing the vocabulary file.
    #[error("vocabulary file error: {0}")]
    Io(#[from] std::io::Error),

    /// The vocabulary file contains invalid JSON.
    #[error("vocabulary file is malformed: {0}")]
    InvalidJson(#[from] serde_json::Error),
}
