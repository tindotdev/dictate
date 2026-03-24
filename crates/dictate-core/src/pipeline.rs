//! Transcription pipeline: `AudioChunk` -> encode -> upload -> text.

use std::str::FromStr;

use crate::audio::AudioChunk;
use crate::audio::chunker::OVERLAP_SAMPLES;
use crate::cancellation::{CancellationContext, CancellationError, CancellationResult};
use crate::encoder::{AudioEncoder, WavEncoder};
use crate::error::TranscriptionError;
use crate::model_id::ModelId;
use crate::postprocess::{
    PostProcessConfig, PostProcessProviderKind, PostProcessor, ResolvedPostProcessTarget,
};
use crate::provider::{
    ResolvedTranscriptionTarget, ResponseFormat, TimestampGranularity, TranscriptionProvider,
    TranscriptionProviderKind, TranscriptionResult, WhisperModel,
};
use crate::request_policy::RequestPolicies;
use crate::resampler::TRANSCRIPTION_SAMPLE_RATE;

pub use crate::provider::{
    ResponseFormat as PipelineResponseFormat, TimestampGranularity as PipelineTimestampGranularity,
    WhisperModel as PipelineWhisperModel,
};

impl FromStr for ResponseFormat {
    type Err = String;

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

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s.to_lowercase().replace("whisper-", "");
        match normalized.as_str() {
            "large-v3-turbo" => Ok(Self::LargeV3Turbo),
            "large-v3" => Ok(Self::LargeV3),
            _ => Err(format!(
                "invalid whisper model: {s} (valid: large-v3-turbo, large-v3)"
            )),
        }
    }
}

/// Configuration for transcription requests.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Selected transcription provider.
    pub transcription_provider: TranscriptionProviderKind,
    /// Optional transcription endpoint override.
    pub base_url: Option<String>,
    /// Optional ISO-639-1 language code.
    pub language: Option<String>,
    /// Optional text prompt for ASR.
    pub prompt: Option<String>,
    /// Response format.
    pub response_format: ResponseFormat,
    /// Semantic Whisper preset when the user selected one.
    pub transcription_model: Option<WhisperModel>,
    /// Raw ASR model id that should be replayed on retry.
    pub transcription_model_id: Option<String>,
    /// Optional sampling temperature.
    pub temperature: Option<f32>,
    /// Optional timestamp granularities.
    pub timestamp_granularities: Vec<TimestampGranularity>,
    /// Whether post-processing is enabled.
    pub post_process: bool,
    /// Selected post-process provider.
    pub post_process_provider: PostProcessProviderKind,
    /// Optional LLM model for post-processing.
    pub post_process_model: Option<ModelId>,
    /// Optional chat endpoint override.
    pub post_process_base_url: Option<String>,
    /// Timeout and retry settings.
    pub request_policies: RequestPolicies,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            transcription_provider: TranscriptionProviderKind::Groq,
            base_url: None,
            language: None,
            prompt: None,
            response_format: ResponseFormat::Json,
            transcription_model: None,
            transcription_model_id: None,
            temperature: None,
            timestamp_granularities: Vec::new(),
            post_process: false,
            post_process_provider: PostProcessProviderKind::Groq,
            post_process_model: None,
            post_process_base_url: None,
            request_policies: RequestPolicies::default(),
        }
    }
}

/// Orchestrates audio encoding and transcription for a recording session.
pub struct TranscriptionPipeline {
    provider: Box<dyn TranscriptionProvider>,
    transcription_target: ResolvedTranscriptionTarget,
    post_processor: Option<Box<dyn PostProcessor>>,
    post_process_target: Option<ResolvedPostProcessTarget>,
    encoder: WavEncoder,
    config: PipelineConfig,
}

/// Outcome of optional post-processing for a transcription result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostProcessOutcome {
    NotConfigured,
    SkippedEmptyText,
    SkippedVerboseJson,
    Applied,
    FailedFallback,
}

impl TranscriptionPipeline {
    /// Create a new pipeline with a resolved transcription target.
    #[must_use]
    pub fn new(
        provider: Box<dyn TranscriptionProvider>,
        transcription_target: ResolvedTranscriptionTarget,
        config: PipelineConfig,
    ) -> Self {
        Self {
            provider,
            transcription_target,
            post_processor: None,
            post_process_target: None,
            encoder: WavEncoder,
            config,
        }
    }

    /// Attach an optional post-processor with its resolved target.
    #[must_use]
    pub fn with_post_processor(
        mut self,
        post_processor: Box<dyn PostProcessor>,
        post_process_target: ResolvedPostProcessTarget,
    ) -> Self {
        self.post_processor = Some(post_processor);
        self.post_process_target = Some(post_process_target);
        self
    }

    #[must_use]
    pub fn post_process_result(&self, result: TranscriptionResult) -> TranscriptionResult {
        self.post_process_result_with_outcome(result).0
    }

    #[must_use]
    /// # Panics
    ///
    /// Panics if a fresh cancellation context is already cancelled or if
    /// post-processing returns an unexpected hard failure instead of using the
    /// normal fallback path.
    pub fn post_process_result_with_outcome(
        &self,
        result: TranscriptionResult,
    ) -> (TranscriptionResult, PostProcessOutcome) {
        crate::runtime::block_on(
            self.post_process_result_with_cancellation_async(result, &CancellationContext::new()),
        )
        .unwrap_or_else(|err| match err {
            CancellationError::Cancelled => {
                panic!("fresh cancellation context cannot be cancelled")
            }
            CancellationError::Error(err) => panic!("post-processing unexpectedly failed: {err}"),
        })
    }

    /// # Errors
    ///
    /// Returns an error when the operation is cancelled or when post-processing
    /// fails before the pipeline can apply its raw-text fallback behavior.
    pub fn post_process_result_with_cancellation(
        &self,
        result: TranscriptionResult,
        cancellation: &CancellationContext,
    ) -> CancellationResult<(TranscriptionResult, PostProcessOutcome), TranscriptionError> {
        crate::runtime::block_on(
            self.post_process_result_with_cancellation_async(result, cancellation),
        )
    }

    #[allow(clippy::unused_async)]
    /// # Errors
    ///
    /// Returns an error when the operation is cancelled or when post-processing
    /// fails before the pipeline can apply its raw-text fallback behavior.
    pub async fn post_process_result_with_cancellation_async(
        &self,
        mut result: TranscriptionResult,
        cancellation: &CancellationContext,
    ) -> CancellationResult<(TranscriptionResult, PostProcessOutcome), TranscriptionError> {
        cancellation.check()?;

        if !self.config.post_process {
            return Ok((result, PostProcessOutcome::NotConfigured));
        }

        let Some(ref pp) = self.post_processor else {
            return Ok((result, PostProcessOutcome::NotConfigured));
        };
        let Some(ref target) = self.post_process_target else {
            return Ok((result, PostProcessOutcome::NotConfigured));
        };

        if result.text.is_empty() {
            return Ok((result, PostProcessOutcome::SkippedEmptyText));
        }

        if self.config.response_format == ResponseFormat::VerboseJson {
            eprintln!("[dictate] skipping post-processing (incompatible with verbose_json format)");
            return Ok((result, PostProcessOutcome::SkippedVerboseJson));
        }

        let config = PostProcessConfig {
            api_key: &target.api_key,
            base_url: Some(&target.endpoint),
            model: Some(&target.model),
            system_prompt: None,
            temperature: None,
            request_policy: target.request_policy,
        };

        match pp.process_with_cancellation_and_request_policy(
            &result.text,
            config,
            target.request_policy,
            cancellation,
        ) {
            Ok(processed) if processed.trim().is_empty() => {
                cancellation.check()?;
                eprintln!(
                    "[dictate] post-processing returned empty text, keeping raw transcription"
                );
                Ok((result, PostProcessOutcome::FailedFallback))
            }
            Ok(processed) => {
                cancellation.check()?;
                result.text = processed;
                Ok((result, PostProcessOutcome::Applied))
            }
            Err(CancellationError::Cancelled) => Err(CancellationError::Cancelled),
            Err(CancellationError::Error(err)) => {
                cancellation.check()?;
                eprintln!("[dictate] post-processing failed, using raw transcription: {err}");
                Ok((result, PostProcessOutcome::FailedFallback))
            }
        }
    }

    /// # Errors
    ///
    /// Returns an error if audio encoding or transcription fails.
    pub fn transcribe_chunk(
        &self,
        chunk: &AudioChunk,
    ) -> Result<TranscriptionResult, TranscriptionError> {
        crate::runtime::block_on(
            self.transcribe_chunk_with_cancellation_async(chunk, &CancellationContext::new()),
        )
        .map_err(|err| match err {
            CancellationError::Cancelled => {
                unreachable!("fresh cancellation context cannot be cancelled")
            }
            CancellationError::Error(err) => err,
        })
    }

    /// # Errors
    ///
    /// Returns an error when the operation is cancelled or when audio encoding
    /// or transcription fails.
    pub fn transcribe_chunk_with_cancellation(
        &self,
        chunk: &AudioChunk,
        cancellation: &CancellationContext,
    ) -> CancellationResult<TranscriptionResult, TranscriptionError> {
        crate::runtime::block_on(self.transcribe_chunk_with_cancellation_async(chunk, cancellation))
    }

    #[allow(clippy::unused_async)]
    /// # Errors
    ///
    /// Returns an error when the operation is cancelled or when audio encoding
    /// or transcription fails.
    pub async fn transcribe_chunk_with_cancellation_async(
        &self,
        chunk: &AudioChunk,
        cancellation: &CancellationContext,
    ) -> CancellationResult<TranscriptionResult, TranscriptionError> {
        cancellation.check()?;

        let samples = if chunk.has_leading_overlap {
            let overlap_start = OVERLAP_SAMPLES.min(chunk.samples.len());
            &chunk.samples[overlap_start..]
        } else {
            &chunk.samples
        };

        if chunk.has_leading_overlap && samples.is_empty() {
            return Ok(TranscriptionResult {
                text: String::new(),
                segments: None,
                words: None,
            });
        }

        let encoded = self
            .encoder
            .encode(samples, TRANSCRIPTION_SAMPLE_RATE)
            .map_err(CancellationError::Error)?;

        let mut provider_config =
            crate::provider::TranscriptionConfig::new(&self.transcription_target.api_key, encoded)
                .with_base_url(&self.transcription_target.endpoint)
                .with_response_format(self.config.response_format)
                .with_model(&self.transcription_target.model)
                .with_request_policy(self.transcription_target.request_policy);

        if let Some(ref lang) = self.config.language {
            provider_config = provider_config.with_language(lang);
        }
        if let Some(ref prompt) = self.config.prompt {
            provider_config = provider_config.with_prompt(prompt);
        }
        if let Some(temp) = self.config.temperature {
            provider_config = provider_config.with_temperature(temp);
        }
        if !self.config.timestamp_granularities.is_empty() {
            provider_config = provider_config
                .with_timestamp_granularities(self.config.timestamp_granularities.clone());
        }

        let result = self
            .provider
            .transcribe_with_cancellation_and_request_policy(
                provider_config,
                self.transcription_target.request_policy,
                cancellation,
            )?;
        cancellation.check()?;
        Ok(result)
    }
}

impl std::fmt::Debug for TranscriptionPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TranscriptionPipeline")
            .field("transcription_target", &"[REDACTED]")
            .field(
                "post_process_target",
                &self.post_process_target.as_ref().map(|_| "[REDACTED]"),
            )
            .field("encoder", &self.encoder)
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::EncodedAudio;
    use crate::postprocess::ResolvedPostProcessTarget;
    use crate::request_policy::RequestPolicies;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    type TranscriptionCalls = Arc<Mutex<Vec<(String, String)>>>;

    struct MockProvider {
        response: String,
        calls: Option<TranscriptionCalls>,
        encoded_sample_counts: Option<Arc<Mutex<Vec<usize>>>>,
    }

    impl MockProvider {
        fn new(response: impl Into<String>) -> Self {
            Self {
                response: response.into(),
                calls: None,
                encoded_sample_counts: None,
            }
        }

        fn with_calls(response: impl Into<String>, calls: TranscriptionCalls) -> Self {
            Self {
                response: response.into(),
                calls: Some(calls),
                encoded_sample_counts: None,
            }
        }

        fn with_encoded_sample_counts(
            response: impl Into<String>,
            encoded_sample_counts: Arc<Mutex<Vec<usize>>>,
        ) -> Self {
            Self {
                response: response.into(),
                calls: None,
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
            if let Some(calls) = &self.calls {
                calls.lock().unwrap().push((
                    config.base_url.unwrap().to_string(),
                    config.model.unwrap().to_string(),
                ));
            }
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

    struct CountingProvider {
        calls: Arc<AtomicUsize>,
    }

    impl TranscriptionProvider for CountingProvider {
        fn name(&self) -> &'static str {
            "counting"
        }

        fn transcribe(
            &self,
            _config: crate::provider::TranscriptionConfig<'_>,
        ) -> Result<TranscriptionResult, TranscriptionError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(TranscriptionResult {
                text: "counted".into(),
                segments: None,
                words: None,
            })
        }
    }

    type PostProcessCalls = Arc<Mutex<Vec<(String, String, String)>>>;

    struct MockPostProcessor {
        calls: PostProcessCalls,
        output: Option<String>,
        failure_message: Option<String>,
    }

    impl MockPostProcessor {
        fn success(calls: PostProcessCalls, output: impl Into<String>) -> Self {
            Self {
                calls,
                output: Some(output.into()),
                failure_message: None,
            }
        }

        fn failure(calls: PostProcessCalls, message: &str) -> Self {
            Self {
                calls,
                output: None,
                failure_message: Some(message.to_string()),
            }
        }
    }

    impl PostProcessor for MockPostProcessor {
        fn name(&self) -> &'static str {
            "mock-post"
        }

        fn process(
            &self,
            text: &str,
            config: PostProcessConfig<'_>,
        ) -> Result<String, TranscriptionError> {
            self.calls.lock().unwrap().push((
                text.to_string(),
                config.base_url.unwrap().to_string(),
                config.model.unwrap().to_string(),
            ));
            self.output.as_ref().map_or_else(
                || {
                    Err(TranscriptionError::InvalidResponse(
                        self.failure_message
                            .clone()
                            .unwrap_or_else(|| "mock post-process failure".to_string()),
                    ))
                },
                |output| Ok(output.clone()),
            )
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

    fn target() -> ResolvedTranscriptionTarget {
        ResolvedTranscriptionTarget {
            provider: TranscriptionProviderKind::Groq,
            endpoint: "https://transcribe.example.com/v1/audio/transcriptions".into(),
            api_key: "test-key".into(),
            model: "whisper-large-v3-turbo".into(),
            request_policy: RequestPolicies::default().transcription,
        }
    }

    fn post_target() -> ResolvedPostProcessTarget {
        ResolvedPostProcessTarget {
            provider: PostProcessProviderKind::Groq,
            endpoint: "https://chat.example.com/v1/chat/completions".into(),
            api_key: "test-key".into(),
            model: "openai/gpt-oss-20b".into(),
            request_policy: RequestPolicies::default().post_process,
        }
    }

    #[test]
    fn transcribe_single_chunk() {
        let pipeline = TranscriptionPipeline::new(
            Box::new(MockProvider::new("hello world")),
            target(),
            PipelineConfig::default(),
        );

        let result = pipeline.transcribe_chunk(&test_chunk(0, 5.0)).unwrap();
        assert_eq!(result.text, "hello world");
    }

    #[test]
    fn transcribe_overlap_chunk_trims_leading_overlap() {
        let encoded_sample_counts = Arc::new(Mutex::new(Vec::new()));
        let pipeline = TranscriptionPipeline::new(
            Box::new(MockProvider::with_encoded_sample_counts(
                "hello",
                Arc::clone(&encoded_sample_counts),
            )),
            target(),
            PipelineConfig::default(),
        );

        let _ = pipeline.transcribe_chunk(&test_chunk(1, 5.0)).unwrap();
        assert_eq!(encoded_sample_counts.lock().unwrap().as_slice(), &[48_000]);
    }

    #[test]
    fn overlap_only_chunk_returns_empty_without_provider_call() {
        let provider_calls = Arc::new(AtomicUsize::new(0));
        let pipeline = TranscriptionPipeline::new(
            Box::new(CountingProvider {
                calls: Arc::clone(&provider_calls),
            }),
            target(),
            PipelineConfig::default(),
        );

        let result = pipeline.transcribe_chunk(&test_chunk(1, 2.0)).unwrap();

        assert_eq!(result.text, "");
        assert_eq!(provider_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn pipeline_uses_resolved_transcription_target() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let pipeline = TranscriptionPipeline::new(
            Box::new(MockProvider::with_calls("hello", Arc::clone(&calls))),
            target(),
            PipelineConfig::default(),
        );

        let _ = pipeline.transcribe_chunk(&test_chunk(0, 1.0)).unwrap();

        assert_eq!(
            calls.lock().unwrap().as_slice(),
            &[(
                "https://transcribe.example.com/v1/audio/transcriptions".into(),
                "whisper-large-v3-turbo".into()
            )]
        );
    }

    #[test]
    fn cancelled_chunk_skips_provider_call() {
        let provider_calls = Arc::new(AtomicUsize::new(0));
        let pipeline = TranscriptionPipeline::new(
            Box::new(CountingProvider {
                calls: Arc::clone(&provider_calls),
            }),
            target(),
            PipelineConfig::default(),
        );
        let cancellation = CancellationContext::new();
        cancellation.cancel();

        let result =
            pipeline.transcribe_chunk_with_cancellation(&test_chunk(0, 1.0), &cancellation);

        assert!(matches!(result, Err(CancellationError::Cancelled)));
        assert_eq!(provider_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn debug_redacts_targets() {
        let pipeline = TranscriptionPipeline::new(
            Box::new(MockProvider::new("test")),
            target(),
            PipelineConfig::default(),
        );
        let debug_output = format!("{pipeline:?}");
        assert!(!debug_output.contains("test-key"));
        assert!(debug_output.contains("REDACTED"));
    }

    #[test]
    fn response_format_from_str() {
        assert_eq!("json".parse(), Ok(ResponseFormat::Json));
        assert_eq!("verbose_json".parse(), Ok(ResponseFormat::VerboseJson));
        assert!("invalid".parse::<ResponseFormat>().is_err());
    }

    #[test]
    fn timestamp_granularity_from_str() {
        assert_eq!("segment".parse(), Ok(TimestampGranularity::Segment));
        assert_eq!("word".parse(), Ok(TimestampGranularity::Word));
        assert!("invalid".parse::<TimestampGranularity>().is_err());
    }

    #[test]
    fn whisper_model_from_str_accepts_semantic_and_legacy_names() {
        assert_eq!("large-v3-turbo".parse(), Ok(WhisperModel::LargeV3Turbo));
        assert_eq!("whisper-large-v3".parse(), Ok(WhisperModel::LargeV3));
        assert!("invalid".parse::<WhisperModel>().is_err());
    }

    #[test]
    fn post_process_uses_resolved_target() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let pipeline = TranscriptionPipeline::new(
            Box::new(MockProvider::new("raw")),
            target(),
            PipelineConfig {
                post_process: true,
                ..PipelineConfig::default()
            },
        )
        .with_post_processor(
            Box::new(MockPostProcessor::success(Arc::clone(&calls), "clean")),
            post_target(),
        );

        let (processed, outcome) = pipeline.post_process_result_with_outcome(TranscriptionResult {
            text: "raw".into(),
            segments: None,
            words: None,
        });

        assert_eq!(outcome, PostProcessOutcome::Applied);
        assert_eq!(processed.text, "clean");
        assert_eq!(
            calls.lock().unwrap().as_slice(),
            &[(
                "raw".into(),
                "https://chat.example.com/v1/chat/completions".into(),
                "openai/gpt-oss-20b".into()
            )]
        );
    }

    #[test]
    fn post_process_failure_falls_back_to_raw_text() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let pipeline = TranscriptionPipeline::new(
            Box::new(MockProvider::new("raw")),
            target(),
            PipelineConfig {
                post_process: true,
                ..PipelineConfig::default()
            },
        )
        .with_post_processor(
            Box::new(MockPostProcessor::failure(Arc::clone(&calls), "boom")),
            post_target(),
        );

        let (processed, outcome) = pipeline.post_process_result_with_outcome(TranscriptionResult {
            text: "raw".into(),
            segments: None,
            words: None,
        });

        assert_eq!(outcome, PostProcessOutcome::FailedFallback);
        assert_eq!(processed.text, "raw");
        assert_eq!(calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn verbose_json_skips_post_process() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let pipeline = TranscriptionPipeline::new(
            Box::new(MockProvider::new("raw")),
            target(),
            PipelineConfig {
                post_process: true,
                response_format: ResponseFormat::VerboseJson,
                ..PipelineConfig::default()
            },
        )
        .with_post_processor(
            Box::new(MockPostProcessor::success(Arc::clone(&calls), "clean")),
            post_target(),
        );

        let (processed, outcome) = pipeline.post_process_result_with_outcome(TranscriptionResult {
            text: "raw".into(),
            segments: None,
            words: None,
        });

        assert_eq!(outcome, PostProcessOutcome::SkippedVerboseJson);
        assert_eq!(processed.text, "raw");
        assert!(calls.lock().unwrap().is_empty());
    }
}
