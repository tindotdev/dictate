//! Groq Whisper transcription provider.
//!
//! Sends encoded audio to Groq's OpenAI-compatible `/audio/transcriptions`
//! endpoint and returns the transcribed text. Includes exponential-backoff
//! retry for transient failures and rate limits.

use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;

use super::{ResponseFormat, TranscriptionConfig, TranscriptionProvider, TranscriptionResult};
use crate::cancellation::{CancellationContext, CancellationError, CancellationResult};
use crate::error::TranscriptionError;
use crate::groq_error::api_error_from_failed_response;
use crate::request_policy::RequestPolicy;
use crate::retry::retry_with_cancellation;
use crate::token::{MAX_PROMPT_TOKENS, estimate_token_count};

// ─── Constants ───────────────────────────────────────────────────────────────

const DEFAULT_API_URL: &str = "https://api.groq.com/openai/v1/audio/transcriptions";
const DEFAULT_MODEL: &str = "whisper-large-v3-turbo";

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
#[derive(Debug, Default, Clone, Copy)]
pub struct GroqProvider;

fn http_client(timeout: Duration) -> Result<Client, TranscriptionError> {
    Client::builder().timeout(timeout).build().map_err(|e| {
        TranscriptionError::HttpClientInitialization(format!(
            "failed to initialize HTTP client: {e}"
        ))
    })
}

impl TranscriptionProvider for GroqProvider {
    fn name(&self) -> &'static str {
        "groq"
    }

    fn transcribe(
        &self,
        config: TranscriptionConfig<'_>,
    ) -> Result<TranscriptionResult, TranscriptionError> {
        let request_policy = config.request_policy;
        crate::runtime::block_on(transcribe_request_async(
            config,
            request_policy,
            &CancellationContext::new(),
        ))
        .map_err(|err| match err {
            CancellationError::Cancelled => {
                unreachable!("fresh cancellation context cannot be cancelled")
            }
            CancellationError::Error(err) => err,
        })
    }

    fn transcribe_with_cancellation(
        &self,
        config: TranscriptionConfig<'_>,
        cancellation: &CancellationContext,
    ) -> CancellationResult<TranscriptionResult, TranscriptionError> {
        let request_policy = config.request_policy;
        self.transcribe_with_cancellation_and_request_policy(config, request_policy, cancellation)
    }

    fn transcribe_with_cancellation_and_request_policy(
        &self,
        config: TranscriptionConfig<'_>,
        request_policy: RequestPolicy,
        cancellation: &CancellationContext,
    ) -> CancellationResult<TranscriptionResult, TranscriptionError> {
        crate::runtime::block_on(transcribe_request_async(
            config,
            request_policy,
            cancellation,
        ))
    }
}

async fn transcribe_request_async(
    config: TranscriptionConfig<'_>,
    request_policy: RequestPolicy,
    cancellation: &CancellationContext,
) -> CancellationResult<TranscriptionResult, TranscriptionError> {
    let url = config.base_url.unwrap_or(DEFAULT_API_URL);
    let model = config.model.unwrap_or(DEFAULT_MODEL);
    let client = http_client(request_policy.timeout).map_err(CancellationError::Error)?;

    retry_with_cancellation(
        request_policy,
        cancellation,
        || send_request(&client, url, &config, model, cancellation),
        |err, dur| {
            eprintln!("[dictate] retrying after {dur:?}: {err}");
        },
    )
    .await
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

fn request_error(err: &reqwest::Error) -> TranscriptionError {
    if err.is_timeout() || err.is_connect() || err.is_request() {
        TranscriptionError::Network(err.to_string())
    } else {
        TranscriptionError::InvalidResponse(format!("HTTP request failed: {err}"))
    }
}

/// Perform a single HTTP POST to the transcription API.
async fn send_request(
    client: &Client,
    url: &str,
    config: &TranscriptionConfig<'_>,
    model: &str,
    cancellation: &CancellationContext,
) -> CancellationResult<TranscriptionResult, TranscriptionError> {
    cancellation.check()?;

    let filename = format!("audio.{}", config.audio.extension());

    let data = config.audio.data().clone(); // O(1): Bytes refcount bump
    let len = data.len() as u64;
    let file_part = reqwest::multipart::Part::stream_with_length(data, len)
        .file_name(filename)
        .mime_str(config.audio.mime_type())
        .map_err(|e| CancellationError::Error(TranscriptionError::EncodingFailed(e.to_string())))?;

    let mut form = reqwest::multipart::Form::new()
        .text("model", model.to_string())
        .part("file", file_part);

    // Add optional parameters if provided
    if let Some(lang) = config.language {
        form = form.text("language", lang.to_string());
    }

    if let Some(p) = config.prompt {
        validate_prompt_length(p).map_err(CancellationError::Error)?;
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

    let response = cancellation
        .run_until_cancelled(
            client
                .post(url)
                .bearer_auth(config.api_key)
                .multipart(form)
                .send(),
        )
        .await?
        .map_err(|err| CancellationError::Error(request_error(&err)))?;

    let status = response.status();

    if !status.is_success() {
        let error = api_error_from_failed_response(response, "Groq error", cancellation)
            .await
            .map_err(CancellationError::from)?;
        return Err(CancellationError::Error(error));
    }

    let body = cancellation
        .run_until_cancelled(response.text())
        .await?
        .map_err(|e| {
            CancellationError::Error(TranscriptionError::InvalidResponse(e.to_string()))
        })?;

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
            let parsed: TranscriptionResponse = serde_json::from_str(&body).map_err(|e| {
                CancellationError::Error(TranscriptionError::InvalidResponse(format!(
                    "{e}: {body}"
                )))
            })?;
            Ok(TranscriptionResult {
                text: parsed.text.trim().to_string(),
                segments: parsed.segments,
                words: parsed.words,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_f64_eq(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < f64::EPSILON);
    }
    use crate::encoder::EncodedAudio;
    use crate::provider::TimestampGranularity;
    use crate::request_policy::RequestPolicy;

    fn fast_request_policy() -> RequestPolicy {
        RequestPolicy::new(
            Duration::from_millis(200),
            3,
            Duration::from_millis(1),
            Duration::from_millis(2),
        )
    }

    // ──── Prompt Validation Tests ────────────────────────────────────────

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

    // ──── Retry classification is tested in error.rs ───────────────────────

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
        let result = provider.transcribe_with_cancellation_and_request_policy(
            config,
            fast_request_policy(),
            &CancellationContext::new(),
        );
        let elapsed = start.elapsed();

        // Should eventually fail after retries using the configured fast backoff.
        assert!(result.is_err(), "Expected error after retry exhaustion");
        assert!(elapsed >= Duration::from_millis(3));
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
        let result = provider.transcribe_with_cancellation_and_request_policy(
            config,
            fast_request_policy(),
            &CancellationContext::new(),
        );
        let elapsed = start.elapsed();

        // Should fail after 4 attempts (0 + 3 retries)
        match &result {
            Err(CancellationError::Error(TranscriptionError::RateLimitExhausted {
                retries: 3,
            })) => {}
            other => panic!("Expected RateLimitExhausted with 3 retries, got: {other:?}"),
        }

        assert!(elapsed >= Duration::from_millis(3));
        assert!(elapsed < Duration::from_secs(1));

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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancelled_failed_response_body_returns_cancelled_promptly() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test listener should bind");
        let address = listener
            .local_addr()
            .expect("test listener should expose local address");

        let server = tokio::spawn(async move {
            let (mut stream, _) = listener
                .accept()
                .await
                .expect("test server should accept one connection");
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).await;

            stream
                .write_all(
                    b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 1024\r\nConnection: close\r\n\r\n",
                )
                .await
                .expect("test server should write headers");

            tokio::time::sleep(Duration::from_secs(5)).await;
        });

        let provider = GroqProvider;
        let audio = test_audio_wav();
        let url = format!("http://{address}");
        let config = TranscriptionConfig::new("test-key", audio).with_base_url(&url);
        let cancellation = CancellationContext::new();
        let cancellation_for_thread = cancellation.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            cancellation_for_thread.cancel();
        });

        let started = std::time::Instant::now();
        let result = provider.transcribe_with_cancellation_and_request_policy(
            config,
            RequestPolicy::new(
                Duration::from_secs(5),
                0,
                Duration::from_millis(1),
                Duration::from_millis(2),
            ),
            &cancellation,
        );
        let elapsed = started.elapsed();

        assert!(matches!(result, Err(CancellationError::Cancelled)));
        assert!(elapsed < Duration::from_secs(1));

        server.abort();
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
        assert_f64_eq(words[0].start, 0.0);
        assert_f64_eq(words[0].end, 0.5);
        mock.assert();
    }

    #[test]
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
