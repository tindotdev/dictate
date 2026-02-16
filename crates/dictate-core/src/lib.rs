//! dictate-core: audio capture, chunking, and transcription pipeline.

pub mod audio;
pub mod clipboard;
pub mod dictionary;
pub mod encoder;
pub mod error;
mod groq_error;
pub mod model_id;
pub mod pipeline;
pub mod postprocess;
pub mod prompt;
pub mod provider;
pub mod resampler;
pub mod token;
pub mod vocabulary;

pub use audio::{
    AudioChunk, AudioReceiver, AudioRecorder, ChunkerConfig, DeviceInfo, DeviceSelection,
    ProgressiveChunker, RecorderConfig, RecorderInfo, RecorderStats, RecorderStatsSnapshot,
    RecvResult, list_input_devices,
};
pub use clipboard::{ClipboardError, check_clipboard_available};
pub use dictionary::{Dictionary, DictionaryError, DictionaryStore};
pub use encoder::{AudioEncoder, EncodedAudio, WavEncoder};
pub use error::{AudioError, ModelIdError, TranscriptionError};
pub use model_id::{LLAMA_3_1_8B, LLAMA_3_3_70B, ModelId};
pub use pipeline::{PipelineConfig, PostProcessOutcome, TranscriptionPipeline};
pub use postprocess::{
    DEFAULT_POST_PROCESS_MODEL, GroqPostProcessor, PostProcessConfig, PostProcessor,
};
pub use prompt::{PromptHint, format_hint_within_budget, merge_prompt_hints};
pub use provider::{
    GroqProvider, ResponseFormat, Segment, TimestampGranularity, TranscriptionProvider,
    TranscriptionResult, WhisperModel, Word,
};
pub use resampler::TRANSCRIPTION_SAMPLE_RATE;
pub use vocabulary::{Vocabulary, VocabularyError, VocabularyStore};
