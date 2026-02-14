//! Groq chat completion post-processor.
//!
//! Sends transcribed text to Groq's OpenAI-compatible chat completion API
//! for punctuation, capitalization, and filler-word cleanup.

use std::sync::OnceLock;
use std::time::Duration;

use backon::{BlockingRetryable, ExponentialBuilder};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

use super::{PostProcessConfig, PostProcessor};
use crate::error::TranscriptionError;
use crate::groq_error::api_error_from_failed_response;

// ─── Constants ───────────────────────────────────────────────────────────────

const DEFAULT_CHAT_URL: &str = "https://api.groq.com/openai/v1/chat/completions";
const DEFAULT_MODEL: &str = "llama-3.1-8b-instant";

/// HTTP timeout for chat completions (60s — text-only, much faster than audio).
const CHAT_TIMEOUT: Duration = Duration::from_secs(60);

// ─── Retry configuration (mirrors Whisper provider) ─────────────────────────

const MAX_RETRIES: u32 = 3;
const BASE_DELAY: Duration = Duration::from_secs(1);
const MAX_DELAY: Duration = Duration::from_secs(16);

const SYSTEM_PROMPT: &str = "\
Clean up this voice transcript. Fix punctuation and capitalization. \
Remove filler words (um, uh, like, you know). Keep the original \
meaning and all technical terms intact. Output only the cleaned \
text, nothing else.";

// ─── Post-processor ─────────────────────────────────────────────────────────

/// Groq-based post-processor using chat completions.
#[derive(Debug, Default, Clone, Copy)]
pub struct GroqPostProcessor;

/// Lazily-initialised HTTP client for chat completions (separate from Whisper).
fn chat_client() -> Result<&'static Client, TranscriptionError> {
    static CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            Client::builder()
                .timeout(CHAT_TIMEOUT)
                .build()
                .map_err(|e| format!("failed to initialize chat HTTP client: {e}"))
        })
        .as_ref()
        .map_err(|e| TranscriptionError::HttpClientInitialization(e.clone()))
}

impl PostProcessor for GroqPostProcessor {
    fn name(&self) -> &'static str {
        "groq-chat"
    }

    fn process(
        &self,
        text: &str,
        config: PostProcessConfig<'_>,
    ) -> Result<String, TranscriptionError> {
        if text.is_empty() {
            return Ok(String::new());
        }

        let url = config.base_url.unwrap_or(DEFAULT_CHAT_URL);
        let model = config.model.unwrap_or(DEFAULT_MODEL);
        let client = chat_client()?;

        retry_chat_request(
            || send_chat_request(client, url, config.api_key, model, text),
            retry_builder(),
            |err, dur| {
                eprintln!("[dictate] post-process retrying after {dur:?}: {err}");
            },
        )
    }
}

fn retry_builder() -> ExponentialBuilder {
    ExponentialBuilder::default()
        .with_min_delay(BASE_DELAY)
        .with_max_delay(MAX_DELAY)
        .with_max_times(MAX_RETRIES as usize)
}

fn retry_chat_request<Op, Notify>(
    mut operation: Op,
    retry: ExponentialBuilder,
    mut notify: Notify,
) -> Result<String, TranscriptionError>
where
    Op: FnMut() -> Result<String, TranscriptionError>,
    Notify: FnMut(&TranscriptionError, Duration),
{
    (|| operation())
        .retry(retry)
        .when(super::super::error::TranscriptionError::is_retryable)
        .notify(|err, dur| {
            notify(err, dur);
        })
        .call()
        .map_err(|e| {
            if e.is_rate_limit_error() {
                TranscriptionError::RateLimitExhausted {
                    retries: MAX_RETRIES,
                }
            } else {
                e
            }
        })
}

// ─── HTTP request ────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(Deserialize)]
struct ChatChoiceMessage {
    content: String,
}

/// Perform a single chat completion request.
fn send_chat_request(
    client: &Client,
    url: &str,
    api_key: &str,
    model: &str,
    text: &str,
) -> Result<String, TranscriptionError> {
    let body = ChatRequest {
        model,
        messages: vec![
            ChatMessage {
                role: "system",
                content: SYSTEM_PROMPT,
            },
            ChatMessage {
                role: "user",
                content: text,
            },
        ],
    };

    let response = client
        .post(url)
        .bearer_auth(api_key)
        .json(&body)
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
        return Err(api_error_from_failed_response(
            response,
            "Groq post-process error",
        ));
    }

    let body = response
        .text()
        .map_err(|e| TranscriptionError::InvalidResponse(e.to_string()))?;

    let parsed: ChatResponse = serde_json::from_str(&body)
        .map_err(|e| TranscriptionError::InvalidResponse(format!("{e}: {body}")))?;

    parsed
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content.trim().to_string())
        .ok_or_else(|| {
            TranscriptionError::InvalidResponse("chat response contained no choices".to_string())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fast_retry_builder() -> ExponentialBuilder {
        ExponentialBuilder::default()
            .with_min_delay(Duration::from_millis(1))
            .with_max_delay(Duration::from_millis(2))
            .with_max_times(MAX_RETRIES as usize)
    }

    #[test]
    fn retry_exhaustion_retries_then_returns_last_retryable_error() {
        let mut attempts = 0;
        let mut notifications = 0;

        let result = retry_chat_request(
            || {
                attempts += 1;
                Err(TranscriptionError::Api {
                    status: 503,
                    message: "service unavailable".to_string(),
                })
            },
            fast_retry_builder(),
            |_, _| {
                notifications += 1;
            },
        );

        assert!(matches!(
            result,
            Err(TranscriptionError::Api { status: 503, .. })
        ));
        assert_eq!(attempts, (MAX_RETRIES + 1) as usize);
        assert_eq!(notifications, MAX_RETRIES as usize);
    }

    #[test]
    fn rate_limit_retry_exhaustion_converts_to_rate_limit_exhausted() {
        let mut attempts = 0;
        let mut notifications = 0;

        let result = retry_chat_request(
            || {
                attempts += 1;
                Err(TranscriptionError::Api {
                    status: 429,
                    message: "rate limited".to_string(),
                })
            },
            fast_retry_builder(),
            |_, _| {
                notifications += 1;
            },
        );

        assert!(matches!(
            result,
            Err(TranscriptionError::RateLimitExhausted {
                retries: MAX_RETRIES
            })
        ));
        assert_eq!(attempts, (MAX_RETRIES + 1) as usize);
        assert_eq!(notifications, MAX_RETRIES as usize);
    }

    #[test]
    fn non_retryable_error_skips_retry_and_notify() {
        let mut attempts = 0;
        let mut notifications = 0;

        let result = retry_chat_request(
            || {
                attempts += 1;
                Err(TranscriptionError::Api {
                    status: 401,
                    message: "invalid key".to_string(),
                })
            },
            fast_retry_builder(),
            |_, _| {
                notifications += 1;
            },
        );

        assert!(matches!(
            result,
            Err(TranscriptionError::Api { status: 401, .. })
        ));
        assert_eq!(attempts, 1);
        assert_eq!(notifications, 0);
    }

    #[test]
    fn retry_notify_receives_each_retryable_error() {
        let mut notifications = Vec::new();

        let result = retry_chat_request(
            || {
                Err(TranscriptionError::Network(
                    "connection reset by peer".to_string(),
                ))
            },
            fast_retry_builder(),
            |err, dur| {
                notifications.push((err.to_string(), dur));
            },
        );

        assert!(matches!(result, Err(TranscriptionError::Network(_))));
        assert_eq!(notifications.len(), MAX_RETRIES as usize);
        assert!(
            notifications
                .iter()
                .all(|(msg, _)| msg.contains("network error: connection reset by peer"))
        );
    }

    #[test]
    fn empty_text_skips_api() {
        let pp = GroqPostProcessor;
        let config = PostProcessConfig {
            api_key: "test",
            base_url: None,
            model: None,
        };
        let result = pp.process("", config).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn successful_post_processing() {
        use httpmock::prelude::*;
        use serde_json::json;

        let server = MockServer::start();

        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/v1/chat/completions")
                .header("Authorization", "Bearer test-key");
            then.status(200).json_body(json!({
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": "Hello, how are you doing today?"
                    }
                }]
            }));
        });

        let pp = GroqPostProcessor;
        let url = format!("{}/v1/chat/completions", server.base_url());
        let config = PostProcessConfig {
            api_key: "test-key",
            base_url: Some(&url),
            model: None,
        };

        let result = pp.process("um hello how are you uh doing today", config);
        assert_eq!(result.unwrap(), "Hello, how are you doing today?");
        mock.assert();
    }

    #[test]
    fn api_error_returns_error() {
        use httpmock::prelude::*;
        use serde_json::json;

        let server = MockServer::start();

        server.mock(|when, then| {
            when.method(POST).path("/v1/chat/completions");
            then.status(401)
                .json_body(json!({"error": {"message": "invalid key"}}));
        });

        let pp = GroqPostProcessor;
        let url = format!("{}/v1/chat/completions", server.base_url());
        let config = PostProcessConfig {
            api_key: "bad-key",
            base_url: Some(&url),
            model: None,
        };

        let result = pp.process("hello", config);
        assert!(matches!(
            result,
            Err(TranscriptionError::Api { status: 401, .. })
        ));
    }

    #[test]
    fn custom_model_sent_in_request() {
        use httpmock::prelude::*;
        use serde_json::json;

        let server = MockServer::start();

        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/v1/chat/completions")
                .body_matches("(?s).*llama-3\\.1-8b-instant.*");
            then.status(200).json_body(json!({
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": "Cleaned text."
                    }
                }]
            }));
        });

        let pp = GroqPostProcessor;
        let url = format!("{}/v1/chat/completions", server.base_url());
        let config = PostProcessConfig {
            api_key: "test-key",
            base_url: Some(&url),
            model: Some("llama-3.1-8b-instant"),
        };

        let result = pp.process("some text", config);
        assert!(result.is_ok());
        mock.assert();
    }
}
