//! Shared token counting utilities for Whisper prompt management.
//!
//! The Groq Whisper API limits the `prompt` parameter to 224 tokens.
//! This module provides a conservative token estimator used by both
//! the provider (for validation) and the dictionary (for budget-aware
//! hint formatting).

/// Maximum allowed tokens for the prompt parameter per Groq API specification.
pub const MAX_PROMPT_TOKENS: usize = 224;

/// Estimate the number of tokens in a text string.
///
/// Uses a language-agnostic upper bound of one Unicode scalar value per token.
/// This intentionally overestimates for many prompts, but avoids undercounting
/// short-token languages (e.g. CJK/emoji) that can otherwise slip past
/// validation and fail server-side.
///
/// # Note
///
/// This is a conservative estimate and may reject prompts that would still fit
/// the true tokenizer limit.
#[must_use]
pub fn estimate_token_count(text: &str) -> usize {
    text.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text() {
        assert_eq!(estimate_token_count("Hello world"), 11);
    }

    #[test]
    fn technical_terms() {
        let prompt = "Technical terms: API, HTTP, JSON";
        assert_eq!(estimate_token_count(prompt), prompt.chars().count());
    }

    #[test]
    fn max_length() {
        let text = "a".repeat(224);
        assert_eq!(estimate_token_count(&text), MAX_PROMPT_TOKENS);
    }

    #[test]
    fn unicode() {
        let text = "Hello 🌍 世界";
        assert_eq!(estimate_token_count(text), text.chars().count());
    }

    #[test]
    fn empty_string() {
        assert_eq!(estimate_token_count(""), 0);
    }
}
