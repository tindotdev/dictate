//! Persistent storage for the reusable last recording.

mod error;
mod store;

pub use error::SavedRecordingError;
pub use store::{
    SavedBaseUrlSource, SavedPipelineConfig, SavedRecording, SavedRecordingManifest,
    SavedRecordingStore,
};
