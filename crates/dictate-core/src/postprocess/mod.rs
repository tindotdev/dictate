//! Post-processing for transcribed text.
//!
//! Provides an optional LLM-based cleanup step that fixes punctuation,
//! capitalization, and filler words in raw Whisper output.

#[cfg(test)]
mod eval;

pub mod groq;

pub use groq::GroqPostProcessor;

use crate::error::TranscriptionError;

/// A text post-processor that refines transcribed text.
pub trait PostProcessor: Send + Sync {
    /// Human-readable name of this post-processor (e.g. `"groq-chat"`).
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
}

/// Configuration for a post-processing request.
pub struct PostProcessConfig<'a> {
    /// API key for authentication.
    pub api_key: &'a str,
    /// Optional API endpoint override.
    pub base_url: Option<&'a str>,
    /// Optional model override (default depends on the implementation).
    pub model: Option<&'a str>,
    /// Optional system prompt override (falls back to built-in `SYSTEM_PROMPT`).
    pub system_prompt: Option<&'a str>,
    /// Optional temperature for chat completion (omitted from request when `None`).
    pub temperature: Option<f32>,
}
