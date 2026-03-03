//! Groq chat completion post-processor.
//!
//! Sends transcribed text to Groq's OpenAI-compatible chat completion API
//! for punctuation, capitalization, and filler-word cleanup.

use std::time::Duration;

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

use super::{PostProcessConfig, PostProcessor};
use crate::cancellation::CancellationContext;
use crate::error::TranscriptionError;
use crate::groq_error::api_error_from_failed_response;
use crate::retry::retry_with_cancellation;

// ─── Constants ───────────────────────────────────────────────────────────────

const DEFAULT_CHAT_URL: &str = "https://api.groq.com/openai/v1/chat/completions";

/// Default LLM model used for post-processing when no override is provided.
pub const DEFAULT_POST_PROCESS_MODEL: &str = "openai/gpt-oss-20b";

const SYSTEM_PROMPT: &str = include_str!("prompts/cleanup.txt");

// ─── Post-processor ─────────────────────────────────────────────────────────

/// Groq-based post-processor using chat completions.
#[derive(Debug, Default, Clone, Copy)]
pub struct GroqPostProcessor;

fn chat_client(timeout: Duration) -> Result<Client, TranscriptionError> {
    Client::builder().timeout(timeout).build().map_err(|e| {
        TranscriptionError::HttpClientInitialization(format!(
            "failed to initialize chat HTTP client: {e}"
        ))
    })
}

impl PostProcessor for GroqPostProcessor {
    fn name(&self) -> &'static str {
        "groq-chat"
    }

    fn process_with_cancellation(
        &self,
        text: &str,
        config: PostProcessConfig<'_>,
        cancellation: &CancellationContext,
    ) -> Result<String, TranscriptionError> {
        cancellation.check()?;

        if text.is_empty() {
            return Ok(String::new());
        }

        let url = config.base_url.unwrap_or(DEFAULT_CHAT_URL);
        let model = config.model.unwrap_or(DEFAULT_POST_PROCESS_MODEL);
        let system_prompt = config.system_prompt.unwrap_or(SYSTEM_PROMPT);
        let client = chat_client(config.request_policy.timeout)?;
        let params = ChatRequestParams {
            api_key: config.api_key,
            model,
            text,
            system_prompt,
            temperature: config.temperature,
        };

        retry_chat_request(
            config.request_policy,
            cancellation,
            || send_chat_request(&client, url, &params, cancellation),
            |err, dur| {
                eprintln!("[dictate] post-process retrying after {dur:?}: {err}");
            },
        )
    }
}

fn retry_chat_request<Op, Notify>(
    request_policy: crate::request_policy::RequestPolicy,
    cancellation: &CancellationContext,
    operation: Op,
    mut notify: Notify,
) -> Result<String, TranscriptionError>
where
    Op: FnMut() -> Result<String, TranscriptionError>,
    Notify: FnMut(&TranscriptionError, Duration),
{
    retry_with_cancellation(request_policy, cancellation, operation, |err, dur| {
        notify(err, dur);
    })
}

// ─── HTTP request ────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
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

struct ChatRequestParams<'a> {
    api_key: &'a str,
    model: &'a str,
    text: &'a str,
    system_prompt: &'a str,
    temperature: Option<f32>,
}

/// Perform a single chat completion request.
fn send_chat_request(
    client: &Client,
    url: &str,
    params: &ChatRequestParams<'_>,
    cancellation: &CancellationContext,
) -> Result<String, TranscriptionError> {
    cancellation.check()?;

    let body = ChatRequest {
        model: params.model,
        messages: vec![
            ChatMessage {
                role: "system",
                content: params.system_prompt,
            },
            ChatMessage {
                role: "user",
                content: params.text,
            },
        ],
        temperature: params.temperature,
    };

    let response = client
        .post(url)
        .bearer_auth(params.api_key)
        .json(&body)
        .send()
        .map_err(|e| {
            if e.is_timeout() || e.is_connect() || e.is_request() {
                TranscriptionError::Network(e.to_string())
            } else {
                TranscriptionError::InvalidResponse(format!("HTTP request failed: {e}"))
            }
        })?;
    cancellation.check()?;

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
    cancellation.check()?;

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
    use crate::cancellation::CancellationContext;
    use crate::request_policy::RequestPolicy;

    fn fast_request_policy() -> RequestPolicy {
        RequestPolicy::new(
            Duration::from_millis(200),
            3,
            Duration::from_millis(1),
            Duration::from_millis(2),
        )
    }

    #[test]
    fn retry_exhaustion_retries_then_returns_last_retryable_error() {
        let mut attempts = 0;
        let mut notifications = 0;

        let result = retry_chat_request(
            fast_request_policy(),
            &CancellationContext::new(),
            || {
                attempts += 1;
                Err(TranscriptionError::Api {
                    status: 503,
                    message: "service unavailable".to_string(),
                })
            },
            |_, _| {
                notifications += 1;
            },
        );

        assert!(matches!(
            result,
            Err(TranscriptionError::Api { status: 503, .. })
        ));
        assert_eq!(attempts, (fast_request_policy().max_retries + 1) as usize);
        assert_eq!(notifications, fast_request_policy().max_retries as usize);
    }

    #[test]
    fn rate_limit_retry_exhaustion_converts_to_rate_limit_exhausted() {
        let mut attempts = 0;
        let mut notifications = 0;

        let result = retry_chat_request(
            fast_request_policy(),
            &CancellationContext::new(),
            || {
                attempts += 1;
                Err(TranscriptionError::Api {
                    status: 429,
                    message: "rate limited".to_string(),
                })
            },
            |_, _| {
                notifications += 1;
            },
        );

        assert!(matches!(
            result,
            Err(TranscriptionError::RateLimitExhausted { retries: 3 })
        ));
        assert_eq!(attempts, (fast_request_policy().max_retries + 1) as usize);
        assert_eq!(notifications, fast_request_policy().max_retries as usize);
    }

    #[test]
    fn non_retryable_error_skips_retry_and_notify() {
        let mut attempts = 0;
        let mut notifications = 0;

        let result = retry_chat_request(
            fast_request_policy(),
            &CancellationContext::new(),
            || {
                attempts += 1;
                Err(TranscriptionError::Api {
                    status: 401,
                    message: "invalid key".to_string(),
                })
            },
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
            fast_request_policy(),
            &CancellationContext::new(),
            || {
                Err(TranscriptionError::Network(
                    "connection reset by peer".to_string(),
                ))
            },
            |err, dur| {
                notifications.push((err.to_string(), dur));
            },
        );

        assert!(matches!(result, Err(TranscriptionError::Network(_))));
        assert_eq!(
            notifications.len(),
            fast_request_policy().max_retries as usize
        );
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
            system_prompt: None,
            temperature: None,
            request_policy: fast_request_policy(),
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
            system_prompt: None,
            temperature: None,
            request_policy: fast_request_policy(),
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
            system_prompt: None,
            temperature: None,
            request_policy: fast_request_policy(),
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
            system_prompt: None,
            temperature: None,
            request_policy: fast_request_policy(),
        };

        let result = pp.process("some text", config);
        assert!(result.is_ok());
        mock.assert();
    }
}
