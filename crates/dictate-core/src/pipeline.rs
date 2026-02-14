//! Transcription pipeline: `AudioChunk` → encode → upload → text.
//!
//! Orchestrates the encoder and provider without coupling them directly.
//! The CLI calls [`TranscriptionPipeline::transcribe_chunk`] for each chunk
//! produced by the audio recorder, accumulating text as it goes.

use std::str::FromStr;

use crate::audio::AudioChunk;
use crate::audio::chunker::OVERLAP_SAMPLES;
use crate::encoder::AudioEncoder;
use crate::encoder::WavEncoder;
use crate::error::TranscriptionError;
use crate::postprocess::{PostProcessConfig, PostProcessor};
use crate::provider::{
    ResponseFormat, TimestampGranularity, TranscriptionProvider, TranscriptionResult, WhisperModel,
};
use crate::resampler::TRANSCRIPTION_SAMPLE_RATE;

// Re-export provider types for convenience
pub use crate::provider::{
    ResponseFormat as PipelineResponseFormat, TimestampGranularity as PipelineTimestampGranularity,
    WhisperModel as PipelineWhisperModel,
};

impl FromStr for ResponseFormat {
    type Err = String;

    /// Parse from string (case-insensitive).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "json" => Ok(Self::Json),
            "verbose_json" => Ok(Self::VerboseJson),
            "text" => Ok(Self::Text),
            _ => Err(format!("invalid response format: {s}")),
        }
    }
}

impl FromStr for TimestampGranularity {
    type Err = String;

    /// Parse from string (case-insensitive).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "segment" => Ok(Self::Segment),
            "word" => Ok(Self::Word),
            _ => Err(format!("invalid timestamp granularity: {s}")),
        }
    }
}

impl FromStr for WhisperModel {
    type Err = String;

    /// Parse from string (case-insensitive, with or without 'whisper-' prefix).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s.to_lowercase().replace("whisper-", "");
        match normalized.as_str() {
            "large-v3-turbo" => Ok(Self::LargeV3Turbo),
            "large-v3" => Ok(Self::LargeV3),
            _ => Err(format!(
                "invalid whisper model: {s} (valid: whisper-large-v3-turbo, whisper-large-v3)"
            )),
        }
    }
}

/// Configuration for transcription requests (pipeline-level).
///
/// This struct holds owned configuration values for the lifetime of the pipeline.
/// When transcribing, these are converted to the provider's config with borrowed values.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Optional API endpoint override (for testing / staging).
    pub base_url: Option<String>,
    /// Optional ISO-639-1 language code (e.g., `"en"`, `"es"`). Improves accuracy and latency.
    pub language: Option<String>,
    /// Optional text to guide transcription style or spelling. Max 224 tokens.
    pub prompt: Option<String>,
    /// Response format. Defaults to JSON.
    pub response_format: ResponseFormat,
    /// Optional model selection. Defaults to provider-specific default if None.
    pub model: Option<WhisperModel>,
    /// Optional sampling temperature (0.0-1.0). Default 0.0 recommended for transcription.
    pub temperature: Option<f32>,
    /// Optional timestamp granularities.
    /// Requires `response_format: ResponseFormat::VerboseJson`.
    pub timestamp_granularities: Vec<TimestampGranularity>,
    /// Whether post-processing is enabled.
    pub post_process: bool,
    /// Optional LLM model for post-processing.
    pub post_process_model: Option<String>,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            base_url: None,
            language: None,
            prompt: None,
            response_format: ResponseFormat::Json,
            model: None,
            temperature: None,
            timestamp_granularities: Vec::new(),
            post_process: false,
            post_process_model: None,
        }
    }
}

/// Orchestrates audio encoding and transcription for a recording session.
pub struct TranscriptionPipeline {
    provider: Box<dyn TranscriptionProvider>,
    post_processor: Option<Box<dyn PostProcessor>>,
    api_key: String,
    encoder: WavEncoder,
    config: PipelineConfig,
}

impl TranscriptionPipeline {
    /// Create a new pipeline with the given provider and API key.
    ///
    /// # Arguments
    ///
    /// * `provider` — The transcription backend to use (e.g. [`GroqProvider`]).
    /// * `api_key` — Bearer token for the provider's API.
    /// * `config` — Transcription configuration (language, model, format, etc.).
    ///
    /// [`GroqProvider`]: crate::provider::GroqProvider
    #[must_use]
    pub fn new(
        provider: Box<dyn TranscriptionProvider>,
        api_key: String,
        config: PipelineConfig,
    ) -> Self {
        Self {
            provider,
            post_processor: None,
            api_key,
            encoder: WavEncoder,
            config,
        }
    }

    /// Attach an optional post-processor to refine transcribed text.
    #[must_use]
    pub fn with_post_processor(mut self, post_processor: Box<dyn PostProcessor>) -> Self {
        self.post_processor = Some(post_processor);
        self
    }

    /// Post-process a merged transcription result via LLM.
    ///
    /// Fail-safe: returns the original result on error (never loses transcribed text).
    #[must_use]
    pub fn post_process_result(&self, mut result: TranscriptionResult) -> TranscriptionResult {
        let Some(ref pp) = self.post_processor else {
            return result;
        };

        if result.text.is_empty() {
            return result;
        }

        let config = PostProcessConfig {
            api_key: &self.api_key,
            base_url: self.config.base_url.as_deref(),
            model: self.config.post_process_model.as_deref(),
        };

        match pp.process(&result.text, config) {
            Ok(processed) => {
                result.text = processed;
                result
            }
            Err(err) => {
                eprintln!("[dictate] post-processing failed, using raw transcription: {err}");
                result
            }
        }
    }

    /// Encode and transcribe a single audio chunk.
    ///
    /// # Errors
    ///
    /// Returns [`TranscriptionError`] if encoding or transcription fails.
    pub fn transcribe_chunk(
        &self,
        chunk: &AudioChunk,
    ) -> Result<TranscriptionResult, TranscriptionError> {
        let samples = if chunk.has_leading_overlap {
            let overlap_start = OVERLAP_SAMPLES.min(chunk.samples.len());
            &chunk.samples[overlap_start..]
        } else {
            &chunk.samples
        };

        // If the final chunk only contains overlap from the previous chunk,
        // there is no new audio to transcribe.
        if chunk.has_leading_overlap && samples.is_empty() {
            return Ok(TranscriptionResult {
                text: String::new(),
                segments: None,
                words: None,
            });
        }

        let encoded = self.encoder.encode(samples, TRANSCRIPTION_SAMPLE_RATE)?;

        // Build provider config with borrowed values
        let mut provider_config = crate::provider::TranscriptionConfig::new(&self.api_key, encoded);

        if let Some(ref url) = self.config.base_url {
            provider_config = provider_config.with_base_url(url);
        }
        if let Some(ref lang) = self.config.language {
            provider_config = provider_config.with_language(lang);
        }
        if let Some(ref prompt) = self.config.prompt {
            provider_config = provider_config.with_prompt(prompt);
        }
        provider_config = provider_config.with_response_format(self.config.response_format);
        if let Some(model) = self.config.model {
            provider_config = provider_config.with_model(model.as_str());
        }
        if let Some(temp) = self.config.temperature {
            provider_config = provider_config.with_temperature(temp);
        }
        if !self.config.timestamp_granularities.is_empty() {
            provider_config = provider_config
                .with_timestamp_granularities(self.config.timestamp_granularities.clone());
        }

        self.provider.transcribe(provider_config)
    }
}

impl std::fmt::Debug for TranscriptionPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TranscriptionPipeline")
            .field("api_key", &"[REDACTED]")
            .field("encoder", &self.encoder)
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::EncodedAudio;
    use std::sync::{Arc, Mutex};

    /// Mock provider that returns a fixed response for every call.
    struct MockProvider {
        response: String,
        encoded_sample_counts: Option<Arc<Mutex<Vec<usize>>>>,
    }

    impl MockProvider {
        fn new(response: impl Into<String>) -> Self {
            Self {
                response: response.into(),
                encoded_sample_counts: None,
            }
        }

        fn with_encoded_sample_counts(
            response: impl Into<String>,
            encoded_sample_counts: Arc<Mutex<Vec<usize>>>,
        ) -> Self {
            Self {
                response: response.into(),
                encoded_sample_counts: Some(encoded_sample_counts),
            }
        }
    }

    fn wav_sample_count(audio: &EncodedAudio) -> usize {
        let payload_size = u32::from_le_bytes(audio.data()[40..44].try_into().unwrap()) as usize;
        payload_size / 2
    }

    impl TranscriptionProvider for MockProvider {
        fn name(&self) -> &'static str {
            "mock"
        }

        fn transcribe(
            &self,
            config: crate::provider::TranscriptionConfig<'_>,
        ) -> Result<TranscriptionResult, TranscriptionError> {
            if let Some(encoded_sample_counts) = &self.encoded_sample_counts {
                encoded_sample_counts
                    .lock()
                    .unwrap()
                    .push(wav_sample_count(&config.audio));
            }

            Ok(TranscriptionResult {
                text: self.response.clone(),
                segments: None,
                words: None,
            })
        }
    }

    #[allow(
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss
    )]
    fn test_chunk(index: usize, duration_secs: f32) -> AudioChunk {
        let num_samples = (TRANSCRIPTION_SAMPLE_RATE as f32 * duration_secs) as usize;
        AudioChunk {
            index,
            samples: vec![0.0; num_samples],
            has_leading_overlap: index > 0,
        }
    }

    #[test]
    fn transcribe_single_chunk() {
        let pipeline = TranscriptionPipeline::new(
            Box::new(MockProvider::new("hello world")),
            "test-key".into(),
            PipelineConfig::default(),
        );

        let chunk = test_chunk(0, 5.0);
        let result = pipeline.transcribe_chunk(&chunk).unwrap();
        assert_eq!(result.text, "hello world");
    }

    #[test]
    fn transcribe_multiple_chunks_concatenated() {
        let pipeline = TranscriptionPipeline::new(
            Box::new(MockProvider::new("hello")),
            "test-key".into(),
            PipelineConfig::default(),
        );

        let chunks = [test_chunk(0, 5.0), test_chunk(1, 3.0), test_chunk(2, 4.0)];

        let texts: Vec<String> = chunks
            .iter()
            .map(|c| pipeline.transcribe_chunk(c).unwrap().text)
            .collect();

        assert_eq!(texts, vec!["hello", "hello", "hello"]);
    }

    #[test]
    fn transcribe_first_chunk_encodes_full_audio() {
        let encoded_sample_counts = Arc::new(Mutex::new(Vec::new()));
        let pipeline = TranscriptionPipeline::new(
            Box::new(MockProvider::with_encoded_sample_counts(
                "hello",
                Arc::clone(&encoded_sample_counts),
            )),
            "test-key".into(),
            PipelineConfig::default(),
        );

        let chunk = test_chunk(0, 5.0);
        let _ = pipeline.transcribe_chunk(&chunk).unwrap();

        assert_eq!(encoded_sample_counts.lock().unwrap().as_slice(), &[80_000]);
    }

    #[test]
    fn transcribe_overlap_chunk_trims_leading_overlap() {
        let encoded_sample_counts = Arc::new(Mutex::new(Vec::new()));
        let pipeline = TranscriptionPipeline::new(
            Box::new(MockProvider::with_encoded_sample_counts(
                "hello",
                Arc::clone(&encoded_sample_counts),
            )),
            "test-key".into(),
            PipelineConfig::default(),
        );

        let chunk = test_chunk(1, 5.0);
        let _ = pipeline.transcribe_chunk(&chunk).unwrap();

        assert_eq!(encoded_sample_counts.lock().unwrap().as_slice(), &[48_000]);
    }

    #[test]
    fn overlap_only_chunk_returns_empty_without_provider_call() {
        let encoded_sample_counts = Arc::new(Mutex::new(Vec::new()));
        let pipeline = TranscriptionPipeline::new(
            Box::new(MockProvider::with_encoded_sample_counts(
                "hello",
                Arc::clone(&encoded_sample_counts),
            )),
            "test-key".into(),
            PipelineConfig::default(),
        );

        // 2 seconds at 16kHz == overlap size. This chunk contains no new audio.
        let chunk = test_chunk(1, 2.0);
        let result = pipeline.transcribe_chunk(&chunk).unwrap();

        assert_eq!(result.text, "");
        assert!(encoded_sample_counts.lock().unwrap().is_empty());
    }

    #[test]
    fn overlap_chunk_with_minimal_audio_after_trim() {
        let encoded_sample_counts = Arc::new(Mutex::new(Vec::new()));
        let pipeline = TranscriptionPipeline::new(
            Box::new(MockProvider::with_encoded_sample_counts(
                "tiny",
                Arc::clone(&encoded_sample_counts),
            )),
            "test-key".into(),
            PipelineConfig::default(),
        );

        // Chunk with overlap + 1 extra sample: after trimming OVERLAP_SAMPLES,
        // only 1 sample remains — verify encoding doesn't panic.
        let chunk = AudioChunk {
            index: 1,
            samples: vec![0.0; OVERLAP_SAMPLES + 1],
            has_leading_overlap: true,
        };

        let result = pipeline.transcribe_chunk(&chunk);
        assert!(
            result.is_ok(),
            "single sample after overlap trim should succeed"
        );
        assert_eq!(encoded_sample_counts.lock().unwrap().as_slice(), &[1]);
    }

    #[test]
    fn debug_redacts_api_key() {
        let pipeline = TranscriptionPipeline::new(
            Box::new(MockProvider::new("test")),
            "super-secret-key-12345".into(),
            PipelineConfig::default(),
        );
        let debug_output = format!("{pipeline:?}");
        assert!(!debug_output.contains("super-secret-key-12345"));
        assert!(debug_output.contains("REDACTED"));
    }

    // ── ResponseFormat tests ─────────────────────────────────────────────

    #[test]
    fn response_format_as_str() {
        assert_eq!(ResponseFormat::Json.as_str(), "json");
        assert_eq!(ResponseFormat::VerboseJson.as_str(), "verbose_json");
        assert_eq!(ResponseFormat::Text.as_str(), "text");
    }

    #[test]
    fn response_format_from_str() {
        assert_eq!("json".parse(), Ok(ResponseFormat::Json));
        assert_eq!("verbose_json".parse(), Ok(ResponseFormat::VerboseJson));
        assert_eq!("text".parse(), Ok(ResponseFormat::Text));

        // Case-insensitive
        assert_eq!("VERBOSE_JSON".parse(), Ok(ResponseFormat::VerboseJson));
        assert_eq!("Text".parse(), Ok(ResponseFormat::Text));

        // Invalid
        assert!("invalid".parse::<ResponseFormat>().is_err());
        assert!("".parse::<ResponseFormat>().is_err());
    }

    // ── TimestampGranularity tests ───────────────────────────────────────

    #[test]
    fn timestamp_granularity_as_str() {
        assert_eq!(TimestampGranularity::Segment.as_str(), "segment");
        assert_eq!(TimestampGranularity::Word.as_str(), "word");
    }

    #[test]
    fn timestamp_granularity_from_str() {
        assert_eq!("segment".parse(), Ok(TimestampGranularity::Segment));
        assert_eq!("word".parse(), Ok(TimestampGranularity::Word));

        // Case-insensitive
        assert_eq!("SEGMENT".parse(), Ok(TimestampGranularity::Segment));
        assert_eq!("Word".parse(), Ok(TimestampGranularity::Word));

        // Invalid
        assert!("invalid".parse::<TimestampGranularity>().is_err());
        assert!("".parse::<TimestampGranularity>().is_err());
    }

    // ── WhisperModel tests ───────────────────────────────────────────────

    #[test]
    fn whisper_model_as_str() {
        assert_eq!(
            WhisperModel::LargeV3Turbo.as_str(),
            "whisper-large-v3-turbo"
        );
        assert_eq!(WhisperModel::LargeV3.as_str(), "whisper-large-v3");
    }

    #[test]
    fn whisper_model_from_str() {
        assert_eq!(
            "whisper-large-v3-turbo".parse(),
            Ok(WhisperModel::LargeV3Turbo)
        );
        assert_eq!("whisper-large-v3".parse(), Ok(WhisperModel::LargeV3));

        // Case-insensitive
        assert_eq!(
            "WHISPER-LARGE-V3-TURBO".parse(),
            Ok(WhisperModel::LargeV3Turbo)
        );
        assert_eq!("Whisper-Large-V3".parse(), Ok(WhisperModel::LargeV3));

        // Without 'whisper-' prefix
        assert_eq!("large-v3-turbo".parse(), Ok(WhisperModel::LargeV3Turbo));
        assert_eq!("large-v3".parse(), Ok(WhisperModel::LargeV3));

        // Invalid
        assert!("invalid".parse::<WhisperModel>().is_err());
        assert!("whisper-small".parse::<WhisperModel>().is_err());
        assert!("".parse::<WhisperModel>().is_err());
    }

    #[test]
    fn whisper_model_default() {
        assert_eq!(WhisperModel::default(), WhisperModel::LargeV3Turbo);
    }
}
