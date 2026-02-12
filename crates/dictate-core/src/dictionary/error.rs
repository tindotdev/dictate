//! Dictionary-specific error types.

/// Errors that can occur when loading or saving the dictionary.
#[derive(Debug, thiserror::Error)]
pub enum DictionaryError {
    /// Could not determine the platform config directory.
    #[error("unable to determine config directory")]
    NoConfigDir,

    /// IO error reading or writing the dictionary file.
    #[error("dictionary file error: {0}")]
    Io(#[from] std::io::Error),

    /// The dictionary file contains invalid JSON.
    #[error("dictionary file is malformed: {0}")]
    InvalidJson(#[from] serde_json::Error),
}
