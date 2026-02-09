//! Groq Whisper transcription provider.
//!
//! Sends encoded audio to Groq's OpenAI-compatible `/audio/transcriptions`
//! endpoint and returns the transcribed text. Includes exponential-backoff
//! retry for transient failures and rate limits.

use std::io::Cursor;
use std::sync::OnceLock;
use std::time::Duration;

use reqwest::blocking::Client;
use serde::Deserialize;

use super::{ResponseFormat, TranscriptionConfig, TranscriptionProvider, TranscriptionResult};
use crate::error::TranscriptionError;

// ─── Constants ───────────────────────────────────────────────────────────────

const DEFAULT_API_URL: &str = "https://api.groq.com/openai/v1/audio/transcriptions";
const DEFAULT_MODEL: &str = "whisper-large-v3-turbo";

/// HTTP request timeout (5 minutes — large audio uploads can be slow).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

// ─── Retry configuration ─────────────────────────────────────────────────────

const MAX_RETRIES: u32 = 3;
const BASE_DELAY: Duration = Duration::from_secs(1);
const MAX_DELAY: Duration = Duration::from_secs(16);
/// Rate-limited responses (429) multiply the delay by this factor.
const RATE_LIMIT_MULTIPLIER: u32 = 2;

/// Maximum allowed tokens for the prompt parameter per Groq API specification.
const MAX_PROMPT_TOKENS: usize = 224;

// ─── Token Counting ──────────────────────────────────────────────────────────

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
fn estimate_token_count(text: &str) -> usize {
    text.chars().count()
}

/// Validate that a prompt does not exceed the maximum token limit.
///
/// Returns an error if the estimated token count exceeds [`MAX_PROMPT_TOKENS`].
fn validate_prompt_length(prompt: &str) -> Result<(), TranscriptionError> {
    let char_count = estimate_token_count(prompt);
    if char_count > MAX_PROMPT_TOKENS {
        return Err(TranscriptionError::PromptTooLong {
            estimated_tokens: char_count,
            max_tokens: MAX_PROMPT_TOKENS,
            char_count,
        });
    }

    Ok(())
}

// ─── Provider ────────────────────────────────────────────────────────────────

/// Groq Whisper transcription provider.
///
/// Uses Groq's OpenAI-compatible API with the `whisper-large-v3-turbo` model.
/// The HTTP client is lazily initialised and shared across all calls.
#[derive(Debug, Default, Clone, Copy)]
pub struct GroqProvider;

/// Module-level shared HTTP client (created once, reused).
fn http_client() -> Result<&'static Client, TranscriptionError> {
    static CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .build()
                .map_err(|e| format!("failed to initialize HTTP client: {e}"))
        })
        .as_ref()
        .map_err(|e| TranscriptionError::Network(e.clone()))
}

impl TranscriptionProvider for GroqProvider {
    fn name(&self) -> &'static str {
        "groq"
    }

    fn transcribe(
        &self,
        config: TranscriptionConfig<'_>,
    ) -> Result<TranscriptionResult, TranscriptionError> {
        let url = config.base_url.unwrap_or(DEFAULT_API_URL);
        let model = config.model.unwrap_or(DEFAULT_MODEL);
        let client = http_client()?;

        let mut last_err = None;

        for attempt in 0..=MAX_RETRIES {
            if attempt > 0
                && let Some(ref err) = last_err
            {
                let delay = backoff_delay(attempt - 1, is_rate_limit_error(err));
                eprintln!(
                    "[dictate] retry {attempt}/{MAX_RETRIES} after error: {err} (waiting {delay:?})"
                );
                std::thread::sleep(delay);
            }

            match send_request(client, url, &config, model) {
                Ok(text) => {
                    if attempt > 0 {
                        eprintln!("[dictate] request succeeded after {attempt} retries");
                    }
                    return Ok(text);
                }
                Err(err) if is_retryable(&err) && attempt < MAX_RETRIES => {
                    last_err = Some(err);
                }
                Err(err) => {
                    if is_rate_limit_error(&err) {
                        return Err(TranscriptionError::RateLimitExhausted { retries: attempt });
                    }
                    return Err(err);
                }
            }
        }

        // If we exhausted retries, the last error is always set.
        Err(last_err.expect("retry loop completed without setting last_err"))
    }
}

// ─── HTTP request ────────────────────────────────────────────────────────────

/// API response from the Groq (OpenAI-compatible) transcription endpoint.
///
/// Supports both simple JSON (`{"text": "..."}`) and verbose JSON with metadata.
#[derive(Deserialize)]
struct TranscriptionResponse {
    text: String,
    #[serde(default)]
    segments: Option<Vec<super::Segment>>,
    #[serde(default)]
    words: Option<Vec<super::Word>>,
}

/// Perform a single HTTP POST to the transcription API.
fn send_request(
    client: &Client,
    url: &str,
    config: &TranscriptionConfig<'_>,
    model: &str,
) -> Result<TranscriptionResult, TranscriptionError> {
    let filename = format!("audio.{}", config.audio.extension());

    let data = config.audio.data().clone(); // O(1): Bytes refcount bump
    let len = data.len() as u64;
    let file_part = reqwest::blocking::multipart::Part::reader_with_length(Cursor::new(data), len)
        .file_name(filename)
        .mime_str(config.audio.mime_type())
        .map_err(|e| TranscriptionError::EncodingFailed(e.to_string()))?;

    let mut form = reqwest::blocking::multipart::Form::new()
        .text("model", model.to_string())
        .part("file", file_part);

    // Add optional parameters if provided
    if let Some(lang) = config.language {
        form = form.text("language", lang.to_string());
    }

    if let Some(p) = config.prompt {
        validate_prompt_length(p)?;
        form = form.text("prompt", p.to_string());
    }

    form = form.text("response_format", config.response_format.as_str());

    if let Some(temp) = config.temperature {
        form = form.text("temperature", temp.to_string());
    }

    // Add timestamp_granularities as array parameters (e.g., "word", "segment")
    if !config.timestamp_granularities.is_empty() {
        for granularity in &config.timestamp_granularities {
            form = form.text("timestamp_granularities[]", granularity.as_str());
        }
    }

    let response = client
        .post(url)
        .bearer_auth(config.api_key)
        .multipart(form)
        .send()
        .map_err(|e| {
            if e.is_timeout() || e.is_connect() || e.is_request() {
                TranscriptionError::Network(e.to_string())
            } else {
                TranscriptionError::InvalidResponse(format!("HTTP request failed: {e}"))
            }
        })?;

    let status = response.status();

    if !status.is_success() {
        let status_code = status.as_u16();
        let body = response
            .text()
            .unwrap_or_else(|_| String::from("<failed to read body>"));

        let message = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v.get("error")?.get("message")?.as_str().map(String::from))
            .unwrap_or_else(|| {
                let mut truncated: String = body.chars().take(200).collect();
                if body.chars().count() > 200 {
                    truncated.push_str("...");
                }
                truncated
            });

        return Err(TranscriptionError::Api {
            status: status_code,
            message,
        });
    }

    let body = response
        .text()
        .map_err(|e| TranscriptionError::InvalidResponse(e.to_string()))?;

    // Parse response based on requested format
    match config.response_format {
        ResponseFormat::Text => {
            // Plain text response - trim whitespace and return with no metadata
            Ok(TranscriptionResult {
                text: body.trim().to_string(),
                segments: None,
                words: None,
            })
        }
        ResponseFormat::Json | ResponseFormat::VerboseJson => {
            // JSON response (default or verbose) - parse and extract fields
            let parsed: TranscriptionResponse = serde_json::from_str(&body)
                .map_err(|e| TranscriptionError::InvalidResponse(format!("{e}: {body}")))?;
            Ok(TranscriptionResult {
                text: parsed.text.trim().to_string(),
                segments: parsed.segments,
                words: parsed.words,
            })
        }
    }
}

// ─── Retry helpers ───────────────────────────────────────────────────────────

/// Compute the backoff delay for the given attempt (0-indexed).
fn backoff_delay(attempt: u32, is_rate_limited: bool) -> Duration {
    let delay = BASE_DELAY.saturating_mul(2_u32.saturating_pow(attempt));
    let delay = delay.min(MAX_DELAY);

    if is_rate_limited {
        delay.saturating_mul(RATE_LIMIT_MULTIPLIER)
    } else {
        delay
    }
}

/// Whether this error is worth retrying.
///
/// Network errors are pre-classified as retryable at conversion time (timeout/connect only).
const fn is_retryable(err: &TranscriptionError) -> bool {
    match err {
        TranscriptionError::Network(_) | TranscriptionError::RateLimitExhausted { .. } => true,
        TranscriptionError::Api { status, .. } => is_retryable_status(*status),
        _ => false,
    }
}

/// HTTP status codes worth retrying.
const fn is_retryable_status(status: u16) -> bool {
    matches!(status, 408 | 429 | 500 | 502 | 503 | 504)
}

/// Whether this error originated from a 429 rate limit.
const fn is_rate_limit_error(err: &TranscriptionError) -> bool {
    matches!(
        err,
        TranscriptionError::RateLimitExhausted { .. } | TranscriptionError::Api { status: 429, .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoder::EncodedAudio;
    use crate::provider::TimestampGranularity;

    // ──── Token Counting Tests ────────────────────────────────────────────

    #[test]
    fn token_estimate_short_text() {
        // One character is counted as one token (conservative upper bound).
        assert_eq!(estimate_token_count("Hello world"), 11);
    }

    #[test]
    fn token_estimate_technical_terms() {
        // Technical prompt from API spec example.
        let prompt = "Technical terms: API, HTTP, JSON";
        assert_eq!(estimate_token_count(prompt), prompt.chars().count());
    }

    #[test]
    fn token_estimate_max_length() {
        // ASCII stays one token per character in the conservative estimate.
        let text = "a".repeat(224);
        assert_eq!(estimate_token_count(&text), MAX_PROMPT_TOKENS);
    }

    #[test]
    fn token_estimate_unicode() {
        let text = "Hello 🌍 世界"; // 10 chars (including emoji and Chinese)
        assert_eq!(estimate_token_count(text), text.chars().count());
    }

    #[test]
    fn validate_prompt_within_limit() {
        // Valid prompt under 224 tokens
        let prompt = "Transcribe this audio with proper punctuation and capitalization.";
        assert!(validate_prompt_length(prompt).is_ok());
    }

    #[test]
    fn validate_prompt_at_limit() {
        // Prompt at exactly 224 tokens (1 char/token estimate).
        let prompt = "a".repeat(224);
        assert!(validate_prompt_length(&prompt).is_ok());
    }

    #[test]
    fn validate_prompt_exceeds_limit() {
        // Prompt exceeding 224 tokens (225+ chars).
        let prompt = "a".repeat(225);
        let result = validate_prompt_length(&prompt);

        match result {
            Err(TranscriptionError::PromptTooLong {
                estimated_tokens,
                max_tokens,
                char_count,
            }) => {
                assert_eq!(max_tokens, MAX_PROMPT_TOKENS);
                assert_eq!(char_count, 225);
                assert!(estimated_tokens > MAX_PROMPT_TOKENS);
            }
            other => panic!("Expected PromptTooLong error, got: {other:?}"),
        }
    }

    #[test]
    fn validate_prompt_exceeds_limit_for_cjk() {
        let prompt = "你".repeat(225);
        let result = validate_prompt_length(&prompt);
        assert!(matches!(
            result,
            Err(TranscriptionError::PromptTooLong {
                char_count: 225,
                ..
            })
        ));
    }

    #[test]
    fn validate_prompt_exceeds_limit_for_emoji() {
        let prompt = "🙂".repeat(225);
        let result = validate_prompt_length(&prompt);
        assert!(matches!(
            result,
            Err(TranscriptionError::PromptTooLong {
                char_count: 225,
                ..
            })
        ));
    }

    #[test]
    fn validate_prompt_way_too_long() {
        // Prompt way over limit to ensure error handling
        let prompt = "This is a very long prompt. ".repeat(100); // ~2800 chars
        let result = validate_prompt_length(&prompt);

        assert!(matches!(
            result,
            Err(TranscriptionError::PromptTooLong { .. })
        ));
    }

    // ──── Backoff Tests ────────────────────────────────────────────────────

    #[test]
    fn backoff_delay_exponential() {
        // attempt 0: 1s, attempt 1: 2s, attempt 2: 4s
        assert_eq!(backoff_delay(0, false), Duration::from_secs(1));
        assert_eq!(backoff_delay(1, false), Duration::from_secs(2));
        assert_eq!(backoff_delay(2, false), Duration::from_secs(4));
    }

    #[test]
    fn backoff_delay_capped() {
        // attempt 5 would be 32s, but capped at 16s
        assert_eq!(backoff_delay(5, false), Duration::from_secs(16));
    }

    #[test]
    fn backoff_delay_rate_limited_doubles() {
        // attempt 0 rate-limited: 1s * 2 = 2s
        assert_eq!(backoff_delay(0, true), Duration::from_secs(2));
        // attempt 1 rate-limited: 2s * 2 = 4s
        assert_eq!(backoff_delay(1, true), Duration::from_secs(4));
    }

    #[test]
    fn retryable_statuses() {
        assert!(is_retryable_status(408));
        assert!(is_retryable_status(429));
        assert!(is_retryable_status(500));
        assert!(is_retryable_status(502));
        assert!(is_retryable_status(503));
        assert!(is_retryable_status(504));

        // Non-retryable
        assert!(!is_retryable_status(400));
        assert!(!is_retryable_status(401));
        assert!(!is_retryable_status(403));
        assert!(!is_retryable_status(413));
        assert!(!is_retryable_status(200));
    }

    #[test]
    fn retryable_api_errors() {
        let retryable = TranscriptionError::Api {
            status: 500,
            message: "internal".into(),
        };
        assert!(is_retryable(&retryable));

        let non_retryable = TranscriptionError::Api {
            status: 401,
            message: "unauthorized".into(),
        };
        assert!(!is_retryable(&non_retryable));

        let encoding = TranscriptionError::EncodingFailed("bad".into());
        assert!(!is_retryable(&encoding));
    }

    // ──── Test Helpers ────────────────────────────────────────────────────

    /// Create minimal valid WAV audio for testing.
    fn test_audio_wav() -> EncodedAudio {
        use crate::encoder::{AudioEncoder, WavEncoder};

        let encoder = WavEncoder;
        let samples = vec![0.0_f32; 8000]; // 0.5s silence at 16kHz
        encoder
            .encode(&samples, 16000)
            .expect("test audio encoding failed")
    }

    /// Helper for matching patterns anywhere in multipart body.
    ///
    /// Multipart boundaries are randomly generated, so we can't match exact content.
    /// This wraps patterns with `(?s).*pattern.*` to match anywhere in the body,
    /// with dot matching newlines.
    fn contains_anywhere(re: &str) -> String {
        format!(r"(?s).*{re}.*")
    }

    // ──── HTTP Integration Tests ──────────────────────────────────────────

    #[test]
    fn http_successful_transcription() {
        use httpmock::prelude::*;
        use serde_json::json;

        let server = MockServer::start();

        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/openai/v1/audio/transcriptions")
                .header("Authorization", "Bearer test-key")
                .header_exists("content-type");
            then.status(200).json_body(json!({"text": "Hello world"}));
        });

        let provider = GroqProvider;
        let audio = test_audio_wav();

        let url = format!("{}/openai/v1/audio/transcriptions", server.base_url());
        let config = TranscriptionConfig::new("test-key", audio).with_base_url(&url);
        let result = provider.transcribe(config);

        match &result {
            Ok(res) => assert_eq!(res.text, "Hello world"),
            Err(e) => panic!("Expected Ok, got Err: {e:?}"),
        }
        mock.assert();
    }

    #[test]
    fn http_retry_after_transient_error() {
        use httpmock::prelude::*;
        use std::time::Instant;

        let server = MockServer::start();

        // Mock always returns 504 (gateway timeout - retryable error)
        server.mock(|when, then| {
            when.method(POST).path("/openai/v1/audio/transcriptions");
            then.status(504).body("Gateway timeout");
        });

        let provider = GroqProvider;
        let audio = test_audio_wav();

        let url = format!("{}/openai/v1/audio/transcriptions", server.base_url());
        let start = Instant::now();
        let config = TranscriptionConfig::new("test-key", audio).with_base_url(&url);
        let result = provider.transcribe(config);
        let elapsed = start.elapsed();

        // Should eventually fail after retries (4 attempts with backoff: 0s + 1s + 2s = ~3s minimum)
        assert!(result.is_err(), "Expected error after retry exhaustion");
        assert!(
            elapsed.as_secs() >= 3,
            "Expected at least 3s of retry backoff, got {elapsed:?}"
        );
    }

    #[test]
    fn http_rate_limit_retry_exhaustion() {
        use httpmock::prelude::*;
        use serde_json::json;
        use std::time::Instant;

        let server = MockServer::start();

        let _mock = server.mock(|when, then| {
            when.method(POST).path("/openai/v1/audio/transcriptions");
            then.status(429)
                .json_body(json!({"error": "rate limit exceeded"}));
        });

        let provider = GroqProvider;
        let audio = test_audio_wav();

        let url = format!("{}/openai/v1/audio/transcriptions", server.base_url());
        let start = Instant::now();
        let config = TranscriptionConfig::new("test-key", audio).with_base_url(&url);
        let result = provider.transcribe(config);
        let elapsed = start.elapsed();

        // Should fail after 4 attempts (0 + 3 retries)
        match &result {
            Err(TranscriptionError::RateLimitExhausted { retries: 3 }) => {}
            other => panic!("Expected RateLimitExhausted with 3 retries, got: {other:?}"),
        }

        // Verify backoff timing: 2s + 4s + 8s = 14s minimum (rate limit doubled)
        assert!(elapsed >= Duration::from_secs(14));
        assert!(elapsed < Duration::from_secs(16)); // Allow margin

        // Note: We don't call mock.assert() because it expects 1 hit,
        // but we make 4 attempts (initial + 3 retries). The behavior
        // is verified by the error type and timing checks above.
    }

    #[test]
    fn http_non_retryable_error_no_retry() {
        use httpmock::prelude::*;
        use serde_json::json;

        let server = MockServer::start();

        let mock = server.mock(|when, then| {
            when.method(POST).path("/openai/v1/audio/transcriptions");
            then.status(401)
                .json_body(json!({"error": "invalid API key"}));
        });

        let provider = GroqProvider;
        let audio = test_audio_wav();

        let url = format!("{}/openai/v1/audio/transcriptions", server.base_url());
        let config = TranscriptionConfig::new("test-key", audio).with_base_url(&url);
        let result = provider.transcribe(config);

        assert!(matches!(
            result,
            Err(TranscriptionError::Api { status: 401, .. })
        ));
        mock.assert(); // Exactly one hit - no retries for 401
    }

    #[test]
    fn http_retry_exhaustion_on_persistent_500() {
        use httpmock::prelude::*;

        let server = MockServer::start();

        // Mock that always returns 503 (retryable server error)
        server.mock(|when, then| {
            when.method(POST).path("/openai/v1/audio/transcriptions");
            then.status(503).body("Service unavailable");
        });

        let provider = GroqProvider;
        let audio = test_audio_wav();

        let url = format!("{}/openai/v1/audio/transcriptions", server.base_url());
        let config = TranscriptionConfig::new("test-key", audio).with_base_url(&url);
        let result = provider.transcribe(config);

        // Should fail with API error after exhausting retries (4 attempts total)
        match &result {
            Err(TranscriptionError::Api { status: 503, .. }) => {
                // Expected: final attempt returns 503 error
            }
            other => panic!("Expected Api error with 503, got: {other:?}"),
        }
    }

    #[test]
    fn http_invalid_json_response() {
        use httpmock::prelude::*;

        let server = MockServer::start();

        let mock = server.mock(|when, then| {
            when.method(POST).path("/openai/v1/audio/transcriptions");
            then.status(200).body("not json at all");
        });

        let provider = GroqProvider;
        let audio = test_audio_wav();

        let url = format!("{}/openai/v1/audio/transcriptions", server.base_url());
        let config = TranscriptionConfig::new("test-key", audio).with_base_url(&url);
        let result = provider.transcribe(config);

        assert!(matches!(
            result,
            Err(TranscriptionError::InvalidResponse(_))
        ));
        mock.assert();
    }

    #[test]
    fn http_multipart_form_fields() {
        use httpmock::prelude::*;
        use serde_json::json;

        let server = MockServer::start();

        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/openai/v1/audio/transcriptions")
                // Multipart boundaries change per request, so we can't match exact Content-Type
                .header_includes("content-type", "multipart/form-data")
                // Validate model field is present with correct value
                .body_matches(contains_anywhere(
                    r#"Content-Disposition: form-data; name="model"\r\n\r\nwhisper-large-v3-turbo"#,
                ))
                // Validate file field is present with correct filename
                .body_matches(contains_anywhere(
                    r#"Content-Disposition: form-data; name="file"; filename="audio\.wav""#,
                ));
            then.status(200).json_body(json!({"text": "test"}));
        });

        let provider = GroqProvider;
        let audio = test_audio_wav();

        let url = format!("{}/openai/v1/audio/transcriptions", server.base_url());
        let config = TranscriptionConfig::new("test-key", audio).with_base_url(&url);
        let result = provider.transcribe(config);

        assert!(result.is_ok());
        mock.assert(); // Validates multipart structure
    }

    #[test]
    fn http_bearer_auth_header_present() {
        use httpmock::prelude::*;
        use serde_json::json;

        let server = MockServer::start();

        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/openai/v1/audio/transcriptions")
                .header("Authorization", "Bearer my-secret-123");
            then.status(200).json_body(json!({"text": "authenticated"}));
        });

        let provider = GroqProvider;
        let audio = test_audio_wav();

        let url = format!("{}/openai/v1/audio/transcriptions", server.base_url());
        let config = TranscriptionConfig::new("my-secret-123", audio).with_base_url(&url);
        let result = provider.transcribe(config);

        assert!(result.is_ok());
        mock.assert();
    }

    #[test]
    fn http_language_parameter_sent() {
        use httpmock::prelude::*;
        use serde_json::json;

        let server = MockServer::start();

        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/openai/v1/audio/transcriptions")
                .header_includes("content-type", "multipart/form-data")
                .body_matches(contains_anywhere(
                    r#"Content-Disposition: form-data; name="language"\r\n\r\nen"#,
                ));
            then.status(200).json_body(json!({"text": "success"}));
        });

        let provider = GroqProvider;
        let audio = test_audio_wav();
        let url = format!("{}/openai/v1/audio/transcriptions", server.base_url());

        let config = TranscriptionConfig::new("test-key", audio)
            .with_base_url(&url)
            .with_language("en");
        let result = provider.transcribe(config);

        assert!(result.is_ok());
        mock.assert();
    }

    #[test]
    fn http_prompt_parameter_sent() {
        use httpmock::prelude::*;
        use serde_json::json;

        let server = MockServer::start();

        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/openai/v1/audio/transcriptions")
                .header_includes("content-type", "multipart/form-data")
                .body_matches(contains_anywhere(
                    r#"Content-Disposition: form-data; name="prompt"\r\n\r\nTechnical terms: API, HTTP, JSON"#,
                ));
            then.status(200).json_body(json!({"text": "success"}));
        });

        let provider = GroqProvider;
        let audio = test_audio_wav();
        let url = format!("{}/openai/v1/audio/transcriptions", server.base_url());

        let config = TranscriptionConfig::new("test-key", audio)
            .with_base_url(&url)
            .with_prompt("Technical terms: API, HTTP, JSON");
        let result = provider.transcribe(config);

        assert!(result.is_ok());
        mock.assert();
    }

    #[test]
    fn http_response_format_text() {
        use httpmock::prelude::*;

        let server = MockServer::start();

        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/openai/v1/audio/transcriptions")
                .body_matches(contains_anywhere(
                    r#"Content-Disposition: form-data; name="response_format"\r\n\r\ntext"#,
                ));
            then.status(200).body("Plain text transcription");
        });

        let provider = GroqProvider;
        let audio = test_audio_wav();
        let url = format!("{}/openai/v1/audio/transcriptions", server.base_url());

        let config = TranscriptionConfig::new("test-key", audio)
            .with_base_url(&url)
            .with_response_format(ResponseFormat::Text);
        let result = provider.transcribe(config);

        assert_eq!(result.unwrap().text, "Plain text transcription");
        mock.assert();
    }

    #[test]
    fn http_response_format_verbose_json() {
        use httpmock::prelude::*;
        use serde_json::json;

        let server = MockServer::start();

        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/openai/v1/audio/transcriptions")
                .body_matches(contains_anywhere(
                    r#"Content-Disposition: form-data; name="response_format"\r\n\r\nverbose_json"#,
                ));
            then.status(200).json_body(json!({
                "text": "Verbose transcription",
                "language": "en",
                "duration": 2.5,
                "segments": []
            }));
        });

        let provider = GroqProvider;
        let audio = test_audio_wav();
        let url = format!("{}/openai/v1/audio/transcriptions", server.base_url());

        let config = TranscriptionConfig::new("test-key", audio)
            .with_base_url(&url)
            .with_response_format(ResponseFormat::VerboseJson);
        let result = provider.transcribe(config);

        assert_eq!(result.unwrap().text, "Verbose transcription");
        mock.assert();
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn http_timestamp_granularities_word() {
        use httpmock::prelude::*;
        use serde_json::json;

        let server = MockServer::start();

        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/openai/v1/audio/transcriptions")
                .header_includes("content-type", "multipart/form-data")
                .body_matches(contains_anywhere(
                    r#"Content-Disposition: form-data; name="response_format"\r\n\r\nverbose_json"#,
                ))
                .body_matches(contains_anywhere(
                    r#"Content-Disposition: form-data; name="timestamp_granularities\[\]"\r\n\r\nword"#,
                ));
            then.status(200).json_body(json!({
                "text": "Hello world",
                "words": [
                    {"word": "Hello", "start": 0.0, "end": 0.5},
                    {"word": "world", "start": 0.6, "end": 1.0}
                ]
            }));
        });

        let provider = GroqProvider;
        let audio = test_audio_wav();
        let url = format!("{}/openai/v1/audio/transcriptions", server.base_url());

        let config = TranscriptionConfig::new("test-key", audio)
            .with_base_url(&url)
            .with_response_format(ResponseFormat::VerboseJson)
            .with_timestamp_granularities(vec![TimestampGranularity::Word]);
        let result = provider.transcribe(config);

        assert!(result.is_ok());
        let res = result.unwrap();
        assert_eq!(res.text, "Hello world");
        assert!(res.words.is_some());
        let words = res.words.unwrap();
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].word, "Hello");
        assert_eq!(words[0].start, 0.0);
        assert_eq!(words[0].end, 0.5);
        mock.assert();
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn http_timestamp_granularities_segment() {
        use httpmock::prelude::*;
        use serde_json::json;

        let server = MockServer::start();

        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/openai/v1/audio/transcriptions")
                .header_includes("content-type", "multipart/form-data")
                .body_matches(contains_anywhere(
                    r#"Content-Disposition: form-data; name="timestamp_granularities\[\]"\r\n\r\nsegment"#,
                ));
            then.status(200).json_body(json!({
                "text": "Hello world",
                "segments": [
                    {
                        "id": 0,
                        "start": 0.0,
                        "end": 1.0,
                        "text": "Hello world",
                        "words": []
                    }
                ]
            }));
        });

        let provider = GroqProvider;
        let audio = test_audio_wav();
        let url = format!("{}/openai/v1/audio/transcriptions", server.base_url());

        let config = TranscriptionConfig::new("test-key", audio)
            .with_base_url(&url)
            .with_response_format(ResponseFormat::VerboseJson)
            .with_timestamp_granularities(vec![TimestampGranularity::Segment]);
        let result = provider.transcribe(config);

        assert!(result.is_ok());
        let res = result.unwrap();
        assert_eq!(res.text, "Hello world");
        assert!(res.segments.is_some());
        let segments = res.segments.unwrap();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].id, 0);
        assert_eq!(segments[0].text, "Hello world");
        mock.assert();
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn http_timestamp_granularities_both() {
        use httpmock::prelude::*;
        use serde_json::json;

        let server = MockServer::start();

        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/openai/v1/audio/transcriptions")
                .header_includes("content-type", "multipart/form-data")
                // Check for both granularities in the body
                .body_matches(contains_anywhere(
                    r#"Content-Disposition: form-data; name="timestamp_granularities\[\]"\r\n\r\nword"#,
                ))
                .body_matches(contains_anywhere(
                    r#"Content-Disposition: form-data; name="timestamp_granularities\[\]"\r\n\r\nsegment"#,
                ));
            then.status(200).json_body(json!({
                "text": "Hello world",
                "segments": [
                    {
                        "id": 0,
                        "start": 0.0,
                        "end": 1.0,
                        "text": "Hello world",
                        "words": [
                            {"word": "Hello", "start": 0.0, "end": 0.5},
                            {"word": "world", "start": 0.6, "end": 1.0}
                        ]
                    }
                ],
                "words": [
                    {"word": "Hello", "start": 0.0, "end": 0.5},
                    {"word": "world", "start": 0.6, "end": 1.0}
                ]
            }));
        });

        let provider = GroqProvider;
        let audio = test_audio_wav();
        let url = format!("{}/openai/v1/audio/transcriptions", server.base_url());

        let config = TranscriptionConfig::new("test-key", audio)
            .with_base_url(&url)
            .with_response_format(ResponseFormat::VerboseJson)
            .with_timestamp_granularities(vec![
                TimestampGranularity::Word,
                TimestampGranularity::Segment,
            ]);
        let result = provider.transcribe(config);

        assert!(result.is_ok());
        let res = result.unwrap();
        assert_eq!(res.text, "Hello world");
        assert!(res.segments.is_some());
        assert!(res.words.is_some());
        let words = res.words.unwrap();
        assert_eq!(words.len(), 2);
        let segments = res.segments.unwrap();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].words.len(), 2);
        mock.assert();
    }

    #[test]
    fn http_prompt_too_long_rejected_early() {
        use httpmock::prelude::*;

        let server = MockServer::start();

        // Mock should NOT be hit - validation happens before HTTP request
        let mock = server.mock(|when, then| {
            when.method(POST).path("/openai/v1/audio/transcriptions");
            then.status(200).body("should not reach here");
        });

        let provider = GroqProvider;
        let audio = test_audio_wav();
        let url = format!("{}/openai/v1/audio/transcriptions", server.base_url());

        // Create a prompt that exceeds 224 tokens (use 1000 chars to be well over limit)
        let too_long_prompt = "a".repeat(1000);

        let config = TranscriptionConfig::new("test-key", audio)
            .with_base_url(&url)
            .with_prompt(&too_long_prompt);
        let result = provider.transcribe(config);

        // Should fail with PromptTooLong error
        match result {
            Err(TranscriptionError::PromptTooLong {
                estimated_tokens,
                max_tokens,
                char_count,
            }) => {
                assert_eq!(max_tokens, MAX_PROMPT_TOKENS);
                assert_eq!(char_count, 1000);
                assert!(estimated_tokens > MAX_PROMPT_TOKENS);
            }
            other => panic!("Expected PromptTooLong error, got: {other:?}"),
        }

        // Mock should NOT have been called - validation prevented HTTP request
        mock.assert_calls(0);
    }

    #[test]
    fn http_temperature_parameter_sent() {
        use httpmock::prelude::*;
        use serde_json::json;

        let server = MockServer::start();

        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/openai/v1/audio/transcriptions")
                .header_includes("content-type", "multipart/form-data")
                .body_matches(contains_anywhere(
                    r#"Content-Disposition: form-data; name="temperature"\r\n\r\n0.3"#,
                ));
            then.status(200).json_body(json!({"text": "success"}));
        });

        let provider = GroqProvider;
        let audio = test_audio_wav();
        let url = format!("{}/openai/v1/audio/transcriptions", server.base_url());

        let config = TranscriptionConfig::new("test-key", audio)
            .with_base_url(&url)
            .with_temperature(0.3);
        let result = provider.transcribe(config);

        assert!(result.is_ok());
        mock.assert();
    }

    #[test]
    fn http_model_parameter_sent() {
        use httpmock::prelude::*;
        use serde_json::json;

        let server = MockServer::start();

        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/openai/v1/audio/transcriptions")
                .header_includes("content-type", "multipart/form-data")
                .body_matches(contains_anywhere(
                    r#"Content-Disposition: form-data; name="model"\r\n\r\nwhisper-large-v3"#,
                ));
            then.status(200).json_body(json!({"text": "success"}));
        });

        let provider = GroqProvider;
        let audio = test_audio_wav();
        let url = format!("{}/openai/v1/audio/transcriptions", server.base_url());

        let config = TranscriptionConfig::new("test-key", audio)
            .with_base_url(&url)
            .with_model("whisper-large-v3");
        let result = provider.transcribe(config);

        assert!(result.is_ok());
        mock.assert();
    }
}
