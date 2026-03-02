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
use crate::model_id::ModelId;
use crate::postprocess::{PostProcessConfig, PostProcessor};
use crate::provider::{
    ResponseFormat, TimestampGranularity, TranscriptionProvider, TranscriptionResult, WhisperModel,
};
use crate::request_policy::RequestPolicies;
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
    /// Optional transcription model selection. Defaults to provider-specific default if None.
    pub transcription_model: Option<WhisperModel>,
    /// Optional sampling temperature (0.0-1.0). Default 0.0 recommended for transcription.
    pub temperature: Option<f32>,
    /// Optional timestamp granularities.
    /// Requires `response_format: ResponseFormat::VerboseJson`.
    pub timestamp_granularities: Vec<TimestampGranularity>,
    /// Whether post-processing is enabled.
    pub post_process: bool,
    /// Optional LLM model for post-processing.
    pub post_process_model: Option<ModelId>,
    /// Optional base URL for the post-processing chat endpoint.
    /// Separate from `base_url` (transcription) because they hit different APIs.
    pub post_process_base_url: Option<String>,
    /// Timeout and retry settings for transcription and post-processing requests.
    pub request_policies: RequestPolicies,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            base_url: None,
            language: None,
            prompt: None,
            response_format: ResponseFormat::Json,
            transcription_model: None,
            temperature: None,
            timestamp_granularities: Vec::new(),
            post_process: false,
            post_process_model: None,
            post_process_base_url: None,
            request_policies: RequestPolicies::default(),
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

/// Outcome of optional post-processing for a transcription result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostProcessOutcome {
    /// Post-processing is not configured (disabled and/or no post-processor attached).
    NotConfigured,
    /// Input text was empty, so post-processing was skipped.
    SkippedEmptyText,
    /// Post-processing was skipped because `verbose_json` must preserve raw timestamps.
    SkippedVerboseJson,
    /// Post-processing succeeded and rewrote the output text.
    Applied,
    /// Post-processing failed and the raw transcription was returned.
    FailedFallback,
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
    pub fn post_process_result(&self, result: TranscriptionResult) -> TranscriptionResult {
        self.post_process_result_with_outcome(result).0
    }

    /// Post-process a merged transcription result and return the outcome.
    ///
    /// Fail-safe: returns the original result on error (never loses transcribed text).
    #[must_use]
    pub fn post_process_result_with_outcome(
        &self,
        mut result: TranscriptionResult,
    ) -> (TranscriptionResult, PostProcessOutcome) {
        if !self.config.post_process {
            return (result, PostProcessOutcome::NotConfigured);
        }

        let Some(ref pp) = self.post_processor else {
            return (result, PostProcessOutcome::NotConfigured);
        };

        if result.text.is_empty() {
            return (result, PostProcessOutcome::SkippedEmptyText);
        }

        // VerboseJson includes segments/words with timestamps that correspond
        // to the raw Whisper output. Rewriting only the top-level `text` field
        // would produce self-contradictory JSON, so skip post-processing.
        if self.config.response_format == ResponseFormat::VerboseJson {
            eprintln!("[dictate] skipping post-processing (incompatible with verbose_json format)");
            return (result, PostProcessOutcome::SkippedVerboseJson);
        }

        let config = PostProcessConfig {
            api_key: &self.api_key,
            base_url: self.config.post_process_base_url.as_deref(),
            model: self.config.post_process_model.as_ref().map(ModelId::as_str),
            system_prompt: None,
            temperature: None,
            request_policy: self.config.request_policies.post_process,
        };

        match pp.process(&result.text, config) {
            Ok(processed) if processed.trim().is_empty() => {
                eprintln!(
                    "[dictate] post-processing returned empty text, keeping raw transcription"
                );
                (result, PostProcessOutcome::FailedFallback)
            }
            Ok(processed) => {
                result.text = processed;
                (result, PostProcessOutcome::Applied)
            }
            Err(err) => {
                eprintln!("[dictate] post-processing failed, using raw transcription: {err}");
                (result, PostProcessOutcome::FailedFallback)
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
        if let Some(model) = self.config.transcription_model {
            provider_config = provider_config.with_model(model.as_str());
        }
        if let Some(temp) = self.config.temperature {
            provider_config = provider_config.with_temperature(temp);
        }
        if !self.config.timestamp_granularities.is_empty() {
            provider_config = provider_config
                .with_timestamp_granularities(self.config.timestamp_granularities.clone());
        }
        provider_config =
            provider_config.with_request_policy(self.config.request_policies.transcription);

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

    // ── Post-processing tests ───────────────────────────────────────────

    /// Recorded post-processor calls: (input text, `base_url` passed).
    type PostProcessCalls = Arc<Mutex<Vec<(String, Option<String>)>>>;

    /// Mock post-processor that records calls and returns uppercased text.
    struct MockPostProcessor {
        calls: PostProcessCalls,
    }

    impl MockPostProcessor {
        fn new(calls: PostProcessCalls) -> Self {
            Self { calls }
        }
    }

    impl PostProcessor for MockPostProcessor {
        fn name(&self) -> &'static str {
            "mock-pp"
        }

        fn process(
            &self,
            text: &str,
            config: crate::postprocess::PostProcessConfig<'_>,
        ) -> Result<String, crate::error::TranscriptionError> {
            self.calls
                .lock()
                .unwrap()
                .push((text.to_string(), config.base_url.map(String::from)));
            Ok(text.to_uppercase())
        }
    }

    /// Mock post-processor that returns a fixed string (including empty).
    struct FixedOutputPostProcessor {
        output: String,
    }

    impl FixedOutputPostProcessor {
        fn new(output: &str) -> Self {
            Self {
                output: output.to_string(),
            }
        }
    }

    impl PostProcessor for FixedOutputPostProcessor {
        fn name(&self) -> &'static str {
            "mock-pp-fixed"
        }

        fn process(
            &self,
            _text: &str,
            _config: crate::postprocess::PostProcessConfig<'_>,
        ) -> Result<String, crate::error::TranscriptionError> {
            Ok(self.output.clone())
        }
    }

    /// Mock post-processor that always fails after recording calls.
    struct FailingPostProcessor {
        calls: PostProcessCalls,
    }

    impl FailingPostProcessor {
        fn new(calls: PostProcessCalls) -> Self {
            Self { calls }
        }
    }

    impl PostProcessor for FailingPostProcessor {
        fn name(&self) -> &'static str {
            "mock-pp-failing"
        }

        fn process(
            &self,
            text: &str,
            config: crate::postprocess::PostProcessConfig<'_>,
        ) -> Result<String, crate::error::TranscriptionError> {
            self.calls
                .lock()
                .unwrap()
                .push((text.to_string(), config.base_url.map(String::from)));
            Err(crate::error::TranscriptionError::Network(
                "forced post-process failure".into(),
            ))
        }
    }

    #[test]
    fn post_process_skips_verbose_json_format() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let pipeline = TranscriptionPipeline::new(
            Box::new(MockProvider::new("test")),
            "test-key".into(),
            PipelineConfig {
                response_format: ResponseFormat::VerboseJson,
                post_process: true,
                ..PipelineConfig::default()
            },
        )
        .with_post_processor(Box::new(MockPostProcessor::new(Arc::clone(&calls))));

        let result = TranscriptionResult {
            text: "hello world".into(),
            segments: Some(vec![]),
            words: Some(vec![]),
        };

        let (processed, outcome) = pipeline.post_process_result_with_outcome(result);

        // Text should be unchanged — post-processor was never called
        assert_eq!(processed.text, "hello world");
        assert!(calls.lock().unwrap().is_empty());
        assert_eq!(outcome, PostProcessOutcome::SkippedVerboseJson);
    }

    #[test]
    fn post_process_runs_for_json_and_text_formats() {
        for format in [ResponseFormat::Json, ResponseFormat::Text] {
            let calls = Arc::new(Mutex::new(Vec::new()));
            let pipeline = TranscriptionPipeline::new(
                Box::new(MockProvider::new("test")),
                "test-key".into(),
                PipelineConfig {
                    response_format: format,
                    post_process: true,
                    ..PipelineConfig::default()
                },
            )
            .with_post_processor(Box::new(MockPostProcessor::new(Arc::clone(&calls))));

            let result = TranscriptionResult {
                text: "hello world".into(),
                segments: None,
                words: None,
            };

            let (processed, outcome) = pipeline.post_process_result_with_outcome(result);
            assert_eq!(processed.text, "HELLO WORLD", "format: {}", format.as_str());
            assert_eq!(calls.lock().unwrap().len(), 1);
            assert_eq!(outcome, PostProcessOutcome::Applied);
        }
    }

    #[test]
    fn post_process_failure_falls_back_to_raw_transcription() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let pipeline = TranscriptionPipeline::new(
            Box::new(MockProvider::new("test")),
            "test-key".into(),
            PipelineConfig {
                response_format: ResponseFormat::Json,
                post_process: true,
                ..PipelineConfig::default()
            },
        )
        .with_post_processor(Box::new(FailingPostProcessor::new(Arc::clone(&calls))));

        let result = TranscriptionResult {
            text: "hello world".into(),
            segments: None,
            words: None,
        };

        let (processed, outcome) = pipeline.post_process_result_with_outcome(result);

        assert_eq!(processed.text, "hello world");
        assert_eq!(calls.lock().unwrap().len(), 1);
        assert_eq!(outcome, PostProcessOutcome::FailedFallback);
    }

    #[test]
    fn post_process_empty_output_preserves_raw_transcription() {
        for empty_output in ["", "   ", "\n\t "] {
            let pipeline = TranscriptionPipeline::new(
                Box::new(MockProvider::new("test")),
                "test-key".into(),
                PipelineConfig {
                    response_format: ResponseFormat::Json,
                    post_process: true,
                    ..PipelineConfig::default()
                },
            )
            .with_post_processor(Box::new(FixedOutputPostProcessor::new(empty_output)));

            let result = TranscriptionResult {
                text: "hello world".into(),
                segments: None,
                words: None,
            };

            let (processed, outcome) = pipeline.post_process_result_with_outcome(result);

            assert_eq!(
                processed.text, "hello world",
                "raw text must be preserved when post-processor returns {empty_output:?}"
            );
            assert_eq!(outcome, PostProcessOutcome::FailedFallback);
        }
    }

    #[test]
    fn post_process_outcome_not_configured_when_processor_missing() {
        let pipeline = TranscriptionPipeline::new(
            Box::new(MockProvider::new("test")),
            "test-key".into(),
            PipelineConfig {
                response_format: ResponseFormat::Json,
                post_process: false,
                ..PipelineConfig::default()
            },
        );

        let result = TranscriptionResult {
            text: "hello world".into(),
            segments: None,
            words: None,
        };

        let (processed, outcome) = pipeline.post_process_result_with_outcome(result);

        assert_eq!(processed.text, "hello world");
        assert_eq!(outcome, PostProcessOutcome::NotConfigured);
    }

    #[test]
    fn post_process_disabled_skips_attached_processor() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let pipeline = TranscriptionPipeline::new(
            Box::new(MockProvider::new("test")),
            "test-key".into(),
            PipelineConfig {
                response_format: ResponseFormat::Json,
                post_process: false,
                ..PipelineConfig::default()
            },
        )
        .with_post_processor(Box::new(MockPostProcessor::new(Arc::clone(&calls))));

        let result = TranscriptionResult {
            text: "hello world".into(),
            segments: None,
            words: None,
        };

        let (processed, outcome) = pipeline.post_process_result_with_outcome(result);

        assert_eq!(processed.text, "hello world");
        assert!(calls.lock().unwrap().is_empty());
        assert_eq!(outcome, PostProcessOutcome::NotConfigured);
    }

    #[test]
    #[allow(clippy::significant_drop_tightening)]
    fn post_process_uses_post_process_base_url_not_base_url() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let pipeline = TranscriptionPipeline::new(
            Box::new(MockProvider::new("test")),
            "test-key".into(),
            PipelineConfig {
                base_url: Some("https://whisper.example.com/v1/audio".into()),
                post_process_base_url: Some("https://chat.example.com/v1/chat".into()),
                post_process: true,
                ..PipelineConfig::default()
            },
        )
        .with_post_processor(Box::new(MockPostProcessor::new(Arc::clone(&calls))));

        let result = TranscriptionResult {
            text: "hello".into(),
            segments: None,
            words: None,
        };

        let _ = pipeline.post_process_result(result);

        let recorded = calls.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(
            recorded[0].1.as_deref(),
            Some("https://chat.example.com/v1/chat"),
            "post-processor should receive post_process_base_url, not base_url"
        );
    }

    #[test]
    #[allow(clippy::significant_drop_tightening)]
    fn post_process_base_url_defaults_to_none() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let pipeline = TranscriptionPipeline::new(
            Box::new(MockProvider::new("test")),
            "test-key".into(),
            PipelineConfig {
                base_url: Some("https://whisper.example.com/v1/audio".into()),
                post_process: true,
                ..PipelineConfig::default()
            },
        )
        .with_post_processor(Box::new(MockPostProcessor::new(Arc::clone(&calls))));

        let result = TranscriptionResult {
            text: "hello".into(),
            segments: None,
            words: None,
        };

        let _ = pipeline.post_process_result(result);

        let recorded = calls.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(
            recorded[0].1, None,
            "post-processor should get None when post_process_base_url is unset (not inherit base_url)"
        );
    }
}
