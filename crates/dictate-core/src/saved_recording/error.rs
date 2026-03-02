//! Saved-recording-specific error types.

/// Errors that can occur when loading or saving the reusable last recording.
#[derive(Debug, thiserror::Error)]
pub enum SavedRecordingError {
    /// Could not determine the platform data directory.
    #[error("unable to determine data directory")]
    NoDataDir,

    /// No saved recording is currently available.
    #[error("no saved recording available")]
    NoSavedRecording,

    /// IO error reading or writing saved recording files.
    #[error("saved recording file error: {0}")]
    Io(#[from] std::io::Error),

    /// The manifest JSON is malformed.
    #[error("saved recording manifest is malformed: {0}")]
    ManifestJson(#[from] serde_json::Error),

    /// The manifest contents are semantically invalid.
    #[error("saved recording manifest is invalid: {0}")]
    InvalidManifest(String),

    /// The saved WAV audio is invalid or unsupported.
    #[error("saved recording audio is invalid: {0}")]
    InvalidAudio(String),
}
