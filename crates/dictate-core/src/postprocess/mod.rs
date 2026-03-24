//! Post-processing for transcribed text.

#[cfg(test)]
mod eval;

pub mod fireworks;
pub mod groq;
mod openai_compatible;

pub use fireworks::{FIREWORKS_DEFAULT_POST_PROCESS_MODEL, FireworksPostProcessor};
pub use groq::{DEFAULT_POST_PROCESS_MODEL, GroqPostProcessor};
pub use openai_compatible::OpenAiCompatiblePostProcessor;

use std::fmt;
use std::str::FromStr;

use crate::cancellation::{CancellationContext, CancellationError, CancellationResult};
use crate::error::TranscriptionError;
use crate::request_policy::{RequestPolicies, RequestPolicy};
use serde::{Deserialize, Serialize};

/// Provider kind for post-processing requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PostProcessProviderKind {
    /// Groq hosted chat completions.
    Groq,
    /// Fireworks hosted chat completions.
    Fireworks,
    /// Arbitrary OpenAI-compatible chat completion endpoint.
    OpenAiCompatible,
}

impl PostProcessProviderKind {
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

impl fmt::Display for PostProcessProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for PostProcessProviderKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "groq" => Ok(Self::Groq),
            "fireworks" => Ok(Self::Fireworks),
            "openai-compatible" => Ok(Self::OpenAiCompatible),
            _ => Err(format!(
                "invalid post-process provider: {value} (valid: groq, fireworks, openai-compatible)"
            )),
        }
    }
}

/// Fully resolved post-processing target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPostProcessTarget {
    /// Provider identity for this target.
    pub provider: PostProcessProviderKind,
    /// Final endpoint used for the request.
    pub endpoint: String,
    /// API key used for bearer authentication.
    pub api_key: String,
    /// Final wire model identifier sent to the provider.
    pub model: String,
    /// Timeout and retry settings for this request.
    pub request_policy: RequestPolicy,
}

/// A text post-processor that refines transcribed text.
pub trait PostProcessor: Send + Sync {
    /// Human-readable name of this post-processor.
    fn name(&self) -> &'static str;

    /// Clean up the given transcription text.
    ///
    /// # Errors
    ///
    /// Returns [`TranscriptionError`] on network, API, or parsing failures.
    fn process(
        &self,
        text: &str,
        config: PostProcessConfig<'_>,
    ) -> Result<String, TranscriptionError>;

    /// Clean up the given transcription text while observing cancellation.
    ///
    /// # Errors
    ///
    /// Returns [`CancellationError`] with [`TranscriptionError`] for network,
    /// API, or parsing failures, or `Cancelled` when the session is aborted.
    fn process_with_cancellation(
        &self,
        text: &str,
        config: PostProcessConfig<'_>,
        cancellation: &CancellationContext,
    ) -> CancellationResult<String, TranscriptionError> {
        cancellation.check()?;
        let result = self
            .process(text, config)
            .map_err(CancellationError::Error)?;
        cancellation.check()?;
        Ok(result)
    }

    /// Clean up the given transcription text using the supplied transport
    /// policy while observing cancellation.
    ///
    /// # Errors
    ///
    /// Returns [`CancellationError`] with [`TranscriptionError`] for network,
    /// API, or parsing failures, or `Cancelled` when the session is aborted.
    fn process_with_cancellation_and_request_policy(
        &self,
        text: &str,
        config: PostProcessConfig<'_>,
        request_policy: RequestPolicy,
        cancellation: &CancellationContext,
    ) -> CancellationResult<String, TranscriptionError> {
        self.process_with_cancellation(
            text,
            config.with_request_policy(request_policy),
            cancellation,
        )
    }
}

/// Configuration for a post-processing request.
pub struct PostProcessConfig<'a> {
    /// API key for authentication.
    pub api_key: &'a str,
    /// Optional API endpoint override.
    pub base_url: Option<&'a str>,
    /// Optional model override.
    pub model: Option<&'a str>,
    /// Optional system prompt override.
    pub system_prompt: Option<&'a str>,
    /// Optional temperature for chat completion.
    pub temperature: Option<f32>,
    /// Timeout and retry settings for this request.
    pub request_policy: RequestPolicy,
}

impl<'a> PostProcessConfig<'a> {
    /// Create a post-processing config with default request settings.
    #[must_use]
    pub const fn new(api_key: &'a str) -> Self {
        Self {
            api_key,
            base_url: None,
            model: None,
            system_prompt: None,
            temperature: None,
            request_policy: RequestPolicies::persistent().post_process,
        }
    }

    /// Set timeout and retry settings for this request.
    #[must_use]
    pub const fn with_request_policy(mut self, request_policy: RequestPolicy) -> Self {
        self.request_policy = request_policy;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_process_provider_kind_roundtrips() {
        assert_eq!(
            "openai-compatible"
                .parse::<PostProcessProviderKind>()
                .unwrap(),
            PostProcessProviderKind::OpenAiCompatible
        );
        assert_eq!(PostProcessProviderKind::Groq.to_string(), "groq");
    }
}
