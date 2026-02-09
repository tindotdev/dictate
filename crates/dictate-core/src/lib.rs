//! dictate-core: audio capture, chunking, and transcription pipeline.

pub mod audio;
pub mod clipboard;
pub mod encoder;
pub mod error;
pub mod pipeline;
pub mod provider;
pub mod resampler;

pub use audio::{
    AudioChunk, AudioReceiver, AudioRecorder, ChunkerConfig, DeviceInfo, DeviceSelection,
    ProgressiveChunker, RecorderConfig, RecorderInfo, RecorderStats, RecorderStatsSnapshot,
    RecvResult, list_input_devices,
};
pub use clipboard::{ClipboardError, check_clipboard_available};
pub use encoder::{AudioEncoder, EncodedAudio, WavEncoder};
pub use error::{AudioError, TranscriptionError};
pub use pipeline::{PipelineConfig, TranscriptionPipeline};
pub use provider::{
    GroqProvider, ResponseFormat, Segment, TimestampGranularity, TranscriptionProvider,
    TranscriptionResult, WhisperModel, Word,
};
pub use resampler::TRANSCRIPTION_SAMPLE_RATE;
