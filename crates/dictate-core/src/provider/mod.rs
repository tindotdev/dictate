//! Transcription provider abstraction and implementations.

mod fireworks;
mod groq;
mod openai_compatible;

pub use fireworks::FireworksProvider;
pub use groq::GroqProvider;
pub use openai_compatible::OpenAiCompatibleProvider;

use std::fmt;
use std::str::FromStr;

use crate::cancellation::{CancellationContext, CancellationError, CancellationResult};
use crate::encoder::EncodedAudio;
use crate::error::TranscriptionError;
use crate::request_policy::{RequestPolicies, RequestPolicy};
use serde::{Deserialize, Serialize};

/// Response format for transcription output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResponseFormat {
    /// Simple JSON with only text field (default).
    #[default]
    Json,
    /// Detailed JSON with segments, words, and metadata.
    VerboseJson,
    /// Plain text response (no JSON).
    Text,
}

impl ResponseFormat {
    /// Convert to API parameter string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::VerboseJson => "verbose_json",
            Self::Text => "text",
        }
    }
}

/// Timestamp granularity level for transcription metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimestampGranularity {
    /// Word-level timestamps.
    Word,
    /// Segment-level timestamps.
    Segment,
}

impl TimestampGranularity {
    /// Convert to API parameter string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Word => "word",
            Self::Segment => "segment",
        }
    }
}

/// User-facing semantic Whisper presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WhisperModel {
    /// Whisper Large V3 Turbo - fastest, recommended for most use cases (default).
    #[default]
    LargeV3Turbo,
    /// Whisper Large V3 - slower but potentially more accurate.
    LargeV3,
}

impl WhisperModel {
    /// Convert to the stable user-facing preset string.
    #[must_use]
    pub const fn preset(self) -> &'static str {
        match self {
            Self::LargeV3Turbo => "large-v3-turbo",
            Self::LargeV3 => "large-v3",
        }
    }
}

/// Provider kind for transcription requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TranscriptionProviderKind {
    /// Groq hosted OpenAI-compatible Whisper.
    Groq,
    /// Fireworks hosted OpenAI-compatible Whisper.
    Fireworks,
    /// Arbitrary OpenAI-compatible transcription endpoint.
    OpenAiCompatible,
}

impl TranscriptionProviderKind {
    /// Return the CLI/storage string for this provider.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Groq => "groq",
            Self::Fireworks => "fireworks",
            Self::OpenAiCompatible => "openai-compatible",
        }
    }
}

impl fmt::Display for TranscriptionProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TranscriptionProviderKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "groq" => Ok(Self::Groq),
            "fireworks" => Ok(Self::Fireworks),
            "openai-compatible" => Ok(Self::OpenAiCompatible),
            _ => Err(format!(
                "invalid transcription provider: {value} (valid: groq, fireworks, openai-compatible)"
            )),
        }
    }
}

/// Fully resolved transcription target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTranscriptionTarget {
    /// Provider identity for this target.
    pub provider: TranscriptionProviderKind,
    /// Final endpoint used for the request.
    pub endpoint: String,
    /// API key used for bearer authentication.
    pub api_key: String,
    /// Final wire model identifier sent to the provider.
    pub model: String,
    /// Timeout and retry settings for this request.
    pub request_policy: RequestPolicy,
}

/// Configuration for a transcription request.
#[derive(Debug, Clone)]
pub struct TranscriptionConfig<'a> {
    /// API key for authentication (required).
    pub api_key: &'a str,
    /// Encoded audio to transcribe (required).
    pub audio: EncodedAudio,
    /// Optional override for the API endpoint.
    pub base_url: Option<&'a str>,
    /// Optional ISO-639-1 language code.
    pub language: Option<&'a str>,
    /// Optional text to guide transcription style or spelling.
    pub prompt: Option<&'a str>,
    /// Response format (default: Json).
    pub response_format: ResponseFormat,
    /// Optional model selection.
    pub model: Option<&'a str>,
    /// Optional sampling temperature (0.0-1.0).
    pub temperature: Option<f32>,
    /// Optional timestamp granularities.
    pub timestamp_granularities: Vec<TimestampGranularity>,
    /// Timeout and retry settings for this request.
    pub request_policy: RequestPolicy,
}

impl<'a> TranscriptionConfig<'a> {
    /// Create a new transcription configuration with required parameters.
    pub const fn new(api_key: &'a str, audio: EncodedAudio) -> Self {
        Self {
            api_key,
            audio,
            base_url: None,
            language: None,
            prompt: None,
            response_format: ResponseFormat::Json,
            model: None,
            temperature: None,
            timestamp_granularities: Vec::new(),
            request_policy: RequestPolicies::persistent().transcription,
        }
    }

    /// Set the base URL for the API endpoint.
    #[must_use]
    pub const fn with_base_url(mut self, url: &'a str) -> Self {
        self.base_url = Some(url);
        self
    }

    /// Set the language code.
    #[must_use]
    pub const fn with_language(mut self, lang: &'a str) -> Self {
        self.language = Some(lang);
        self
    }

    /// Set the prompt text.
    #[must_use]
    pub const fn with_prompt(mut self, prompt: &'a str) -> Self {
        self.prompt = Some(prompt);
        self
    }

    /// Set the response format.
    #[must_use]
    pub const fn with_response_format(mut self, format: ResponseFormat) -> Self {
        self.response_format = format;
        self
    }

    /// Set the model.
    #[must_use]
    pub const fn with_model(mut self, model: &'a str) -> Self {
        self.model = Some(model);
        self
    }

    /// Set the temperature.
    #[must_use]
    pub const fn with_temperature(mut self, temp: f32) -> Self {
        self.temperature = Some(temp);
        self
    }

    /// Set the timestamp granularities.
    #[must_use]
    pub fn with_timestamp_granularities(
        mut self,
        granularities: Vec<TimestampGranularity>,
    ) -> Self {
        self.timestamp_granularities = granularities;
        self
    }

    /// Set timeout and retry settings for this request.
    #[must_use]
    pub const fn with_request_policy(mut self, request_policy: RequestPolicy) -> Self {
        self.request_policy = request_policy;
        self
    }
}

/// A single word in a transcription with timing information.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Word {
    /// The transcribed word text.
    pub word: String,
    /// Start time in seconds.
    pub start: f64,
    /// End time in seconds.
    pub end: f64,
}

/// A segment of transcribed audio with timing and optional word-level details.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Segment {
    /// Segment ID (0-indexed).
    pub id: u32,
    /// Start time in seconds.
    pub start: f64,
    /// End time in seconds.
    pub end: f64,
    /// The transcribed text for this segment.
    pub text: String,
    /// Optional word-level breakdown.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<Word>,
}

/// Result of a transcription request.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TranscriptionResult {
    /// The full transcribed text.
    pub text: String,
    /// Optional segment-level timestamps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segments: Option<Vec<Segment>>,
    /// Optional word-level timestamps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub words: Option<Vec<Word>>,
}

/// A speech-to-text backend that can transcribe encoded audio into text.
pub trait TranscriptionProvider: Send + Sync {
    /// Human-readable name of this provider (e.g. `"groq"`).
    fn name(&self) -> &'static str;

    /// Send encoded audio to the provider and return the transcribed text.
    ///
    /// # Errors
    ///
    /// Returns [`TranscriptionError`] on network, API, or parsing failures.
    fn transcribe(
        &self,
        config: TranscriptionConfig<'_>,
    ) -> Result<TranscriptionResult, TranscriptionError>;

    /// Send encoded audio to the provider while observing cancellation.
    ///
    /// # Errors
    ///
    /// Returns [`CancellationError`] with [`TranscriptionError`] for network,
    /// API, or parsing failures, or `Cancelled` when the session is aborted.
    fn transcribe_with_cancellation(
        &self,
        config: TranscriptionConfig<'_>,
        cancellation: &CancellationContext,
    ) -> CancellationResult<TranscriptionResult, TranscriptionError> {
        cancellation.check()?;
        let result = self.transcribe(config).map_err(CancellationError::Error)?;
        cancellation.check()?;
        Ok(result)
    }

    /// Send encoded audio using the supplied transport policy while observing
    /// cancellation.
    ///
    /// # Errors
    ///
    /// Returns [`CancellationError`] with [`TranscriptionError`] for network,
    /// API, or parsing failures, or `Cancelled` when the session is aborted.
    fn transcribe_with_cancellation_and_request_policy(
        &self,
        config: TranscriptionConfig<'_>,
        request_policy: RequestPolicy,
        cancellation: &CancellationContext,
    ) -> CancellationResult<TranscriptionResult, TranscriptionError> {
        self.transcribe_with_cancellation(config.with_request_policy(request_policy), cancellation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whisper_model_preset_strings_are_semantic() {
        assert_eq!(WhisperModel::LargeV3Turbo.preset(), "large-v3-turbo");
        assert_eq!(WhisperModel::LargeV3.preset(), "large-v3");
    }

    #[test]
    fn transcription_provider_kind_roundtrips() {
        assert_eq!(
            "openai-compatible"
                .parse::<TranscriptionProviderKind>()
                .unwrap(),
            TranscriptionProviderKind::OpenAiCompatible
        );
        assert_eq!(
            TranscriptionProviderKind::Fireworks.to_string(),
            "fireworks"
        );
    }

    #[test]
    fn text_only_result_omits_optional_fields() {
        let result = TranscriptionResult {
            text: "hello world".to_string(),
            segments: None,
            words: None,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json, serde_json::json!({"text": "hello world"}));
    }

    #[test]
    fn verbose_result_includes_segments_and_words() {
        let result = TranscriptionResult {
            text: "hello".to_string(),
            segments: Some(vec![Segment {
                id: 0,
                start: 0.0,
                end: 1.0,
                text: "hello".to_string(),
                words: vec![],
            }]),
            words: Some(vec![Word {
                word: "hello".to_string(),
                start: 0.0,
                end: 0.5,
            }]),
        };
        let json = serde_json::to_value(&result).unwrap();
        assert!(json.get("segments").is_some());
        assert!(json.get("words").is_some());
    }
}
