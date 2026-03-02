//! Transcription provider abstraction and implementations.
//!
//! Defines [`TranscriptionProvider`] — the trait that all speech-to-text
//! backends implement — and ships [`GroqProvider`] as the v3.0 default.

mod groq;

pub use groq::GroqProvider;

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

/// Available Whisper models for Groq transcription.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WhisperModel {
    /// Whisper Large V3 Turbo - fastest, recommended for most use cases (default).
    #[default]
    LargeV3Turbo,
    /// Whisper Large V3 - slower but potentially more accurate.
    LargeV3,
}

impl WhisperModel {
    /// Convert to API parameter string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LargeV3Turbo => "whisper-large-v3-turbo",
            Self::LargeV3 => "whisper-large-v3",
        }
    }
}

/// Configuration for a transcription request.
///
/// Use `TranscriptionConfig::new()` with the required parameters, then set
/// optional fields using field access or builder-style methods.
#[derive(Debug, Clone)]
pub struct TranscriptionConfig<'a> {
    /// API key for authentication (required).
    pub api_key: &'a str,
    /// Encoded audio to transcribe (required).
    pub audio: EncodedAudio,
    /// Optional override for the API endpoint (for testing/staging).
    pub base_url: Option<&'a str>,
    /// Optional ISO-639-1 language code (e.g., "en", "es"). Improves accuracy and latency.
    pub language: Option<&'a str>,
    /// Optional text to guide transcription style or spelling. Max 224 tokens.
    pub prompt: Option<&'a str>,
    /// Response format (default: Json).
    pub response_format: ResponseFormat,
    /// Optional model selection. Defaults to provider-specific default if None.
    pub model: Option<&'a str>,
    /// Optional sampling temperature (0.0-1.0). Default 0.0 recommended for transcription.
    pub temperature: Option<f32>,
    /// Optional timestamp granularities. Requires `ResponseFormat::VerboseJson`.
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
    /// Optional word-level breakdown (only present if word granularity requested).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<Word>,
}

/// Result of a transcription request, containing text and optional structured metadata.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TranscriptionResult {
    /// The full transcribed text.
    pub text: String,
    /// Optional segment-level timestamps (present when segment granularity requested).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segments: Option<Vec<Segment>>,
    /// Optional word-level timestamps (present when word granularity requested).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub words: Option<Vec<Word>>,
}

/// A speech-to-text backend that can transcribe encoded audio into text.
///
/// The trait is intentionally minimal for v3.0 (sync, one method). Async
/// and streaming variants will be added when the daemon (v3.1) needs them.
pub trait TranscriptionProvider: Send + Sync {
    /// Human-readable name of this provider (e.g. `"groq"`).
    fn name(&self) -> &'static str;

    /// Send encoded audio to the provider and return the transcribed text with optional metadata.
    ///
    /// # Errors
    ///
    /// Returns [`TranscriptionError`] on network, API, or parsing failures.
    /// Implementations should retry transient errors internally.
    fn transcribe(
        &self,
        config: TranscriptionConfig<'_>,
    ) -> Result<TranscriptionResult, TranscriptionError>;
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn segment_omits_empty_words() {
        let segment = Segment {
            id: 0,
            start: 0.0,
            end: 1.0,
            text: "hello".to_string(),
            words: vec![],
        };
        let json = serde_json::to_value(&segment).unwrap();
        assert!(json.get("words").is_none());
    }

    #[test]
    fn segment_includes_non_empty_words() {
        let segment = Segment {
            id: 0,
            start: 0.0,
            end: 1.0,
            text: "hello".to_string(),
            words: vec![Word {
                word: "hello".to_string(),
                start: 0.0,
                end: 0.5,
            }],
        };
        let json = serde_json::to_value(&segment).unwrap();
        assert!(json.get("words").is_some());
        assert_eq!(json["words"][0]["word"], "hello");
    }
}
