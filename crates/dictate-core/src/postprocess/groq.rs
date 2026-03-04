//! Groq chat completion post-processor.
//!
//! Sends transcribed text to Groq's OpenAI-compatible chat completion API
//! for punctuation, capitalization, and filler-word cleanup.

use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::{PostProcessConfig, PostProcessor};
use crate::cancellation::{CancellationContext, CancellationError, CancellationResult};
use crate::error::TranscriptionError;
use crate::groq_error::api_error_from_failed_response;
use crate::request_policy::RequestPolicy;
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

    fn process(
        &self,
        text: &str,
        config: PostProcessConfig<'_>,
    ) -> Result<String, TranscriptionError> {
        let request_policy = config.request_policy;
        crate::runtime::block_on(process_request_async(
            text,
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

    fn process_with_cancellation(
        &self,
        text: &str,
        config: PostProcessConfig<'_>,
        cancellation: &CancellationContext,
    ) -> CancellationResult<String, TranscriptionError> {
        let request_policy = config.request_policy;
        self.process_with_cancellation_and_request_policy(
            text,
            config,
            request_policy,
            cancellation,
        )
    }

    fn process_with_cancellation_and_request_policy(
        &self,
        text: &str,
        config: PostProcessConfig<'_>,
        request_policy: RequestPolicy,
        cancellation: &CancellationContext,
    ) -> CancellationResult<String, TranscriptionError> {
        crate::runtime::block_on(process_request_async(
            text,
            config,
            request_policy,
            cancellation,
        ))
    }
}

async fn process_request_async(
    text: &str,
    config: PostProcessConfig<'_>,
    request_policy: RequestPolicy,
    cancellation: &CancellationContext,
) -> CancellationResult<String, TranscriptionError> {
    cancellation.check()?;

    if text.is_empty() {
        return Ok(String::new());
    }

    let url = config.base_url.unwrap_or(DEFAULT_CHAT_URL);
    let model = config.model.unwrap_or(DEFAULT_POST_PROCESS_MODEL);
    let system_prompt = config.system_prompt.unwrap_or(SYSTEM_PROMPT);
    let client = chat_client(request_policy.timeout).map_err(CancellationError::Error)?;
    let params = ChatRequestParams {
        api_key: config.api_key,
        model,
        text,
        system_prompt,
        temperature: config.temperature,
    };

    retry_chat_request(
        request_policy,
        cancellation,
        || send_chat_request(&client, url, &params, cancellation),
        |err, dur| {
            eprintln!("[dictate] post-process retrying after {dur:?}: {err}");
        },
    )
    .await
}

fn request_error(err: &reqwest::Error) -> TranscriptionError {
    if err.is_timeout() || err.is_connect() || err.is_request() {
        TranscriptionError::Network(err.to_string())
    } else {
        TranscriptionError::InvalidResponse(format!("HTTP request failed: {err}"))
    }
}

async fn retry_chat_request<Op, Fut, Notify>(
    request_policy: crate::request_policy::RequestPolicy,
    cancellation: &CancellationContext,
    operation: Op,
    mut notify: Notify,
) -> CancellationResult<String, TranscriptionError>
where
    Op: FnMut() -> Fut,
    Fut: std::future::Future<Output = CancellationResult<String, TranscriptionError>>,
    Notify: FnMut(&TranscriptionError, Duration),
{
    retry_with_cancellation(request_policy, cancellation, operation, |err, dur| {
        notify(err, dur);
    })
    .await
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
async fn send_chat_request(
    client: &Client,
    url: &str,
    params: &ChatRequestParams<'_>,
    cancellation: &CancellationContext,
) -> CancellationResult<String, TranscriptionError> {
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

    let response = cancellation
        .run_until_cancelled(
            client
                .post(url)
                .bearer_auth(params.api_key)
                .json(&body)
                .send(),
        )
        .await?
        .map_err(|err| CancellationError::Error(request_error(&err)))?;

    let status = response.status();

    if !status.is_success() {
        let error =
            api_error_from_failed_response(response, "Groq post-process error", cancellation)
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

    let parsed: ChatResponse = serde_json::from_str(&body).map_err(|e| {
        CancellationError::Error(TranscriptionError::InvalidResponse(format!("{e}: {body}")))
    })?;

    parsed
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content.trim().to_string())
        .ok_or_else(|| {
            CancellationError::Error(TranscriptionError::InvalidResponse(
                "chat response contained no choices".to_string(),
            ))
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

        let result = crate::runtime::block_on(retry_chat_request(
            fast_request_policy(),
            &CancellationContext::new(),
            || {
                attempts += 1;
                std::future::ready(Err(CancellationError::Error(TranscriptionError::Api {
                    status: 503,
                    message: "service unavailable".to_string(),
                })))
            },
            |_, _| {
                notifications += 1;
            },
        ));

        assert!(matches!(
            result,
            Err(CancellationError::Error(TranscriptionError::Api {
                status: 503,
                ..
            }))
        ));
        assert_eq!(attempts, (fast_request_policy().max_retries + 1) as usize);
        assert_eq!(notifications, fast_request_policy().max_retries as usize);
    }

    #[test]
    fn rate_limit_retry_exhaustion_converts_to_rate_limit_exhausted() {
        let mut attempts = 0;
        let mut notifications = 0;

        let result = crate::runtime::block_on(retry_chat_request(
            fast_request_policy(),
            &CancellationContext::new(),
            || {
                attempts += 1;
                std::future::ready(Err(CancellationError::Error(TranscriptionError::Api {
                    status: 429,
                    message: "rate limited".to_string(),
                })))
            },
            |_, _| {
                notifications += 1;
            },
        ));

        assert!(matches!(
            result,
            Err(CancellationError::Error(
                TranscriptionError::RateLimitExhausted { retries: 3 }
            ))
        ));
        assert_eq!(attempts, (fast_request_policy().max_retries + 1) as usize);
        assert_eq!(notifications, fast_request_policy().max_retries as usize);
    }

    #[test]
    fn non_retryable_error_skips_retry_and_notify() {
        let mut attempts = 0;
        let mut notifications = 0;

        let result = crate::runtime::block_on(retry_chat_request(
            fast_request_policy(),
            &CancellationContext::new(),
            || {
                attempts += 1;
                std::future::ready(Err(CancellationError::Error(TranscriptionError::Api {
                    status: 401,
                    message: "invalid key".to_string(),
                })))
            },
            |_, _| {
                notifications += 1;
            },
        ));

        assert!(matches!(
            result,
            Err(CancellationError::Error(TranscriptionError::Api {
                status: 401,
                ..
            }))
        ));
        assert_eq!(attempts, 1);
        assert_eq!(notifications, 0);
    }

    #[test]
    fn retry_notify_receives_each_retryable_error() {
        let mut notifications = Vec::new();

        let result = crate::runtime::block_on(retry_chat_request(
            fast_request_policy(),
            &CancellationContext::new(),
            || {
                std::future::ready(Err(CancellationError::Error(TranscriptionError::Network(
                    "connection reset by peer".to_string(),
                ))))
            },
            |err, dur| {
                notifications.push((err.to_string(), dur));
            },
        ));

        assert!(matches!(
            result,
            Err(CancellationError::Error(TranscriptionError::Network(_)))
        ));
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

        let result = pp.process_with_cancellation_and_request_policy(
            "um hello how are you uh doing today",
            config,
            fast_request_policy(),
            &CancellationContext::new(),
        );
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

        let result = pp.process_with_cancellation_and_request_policy(
            "hello",
            config,
            fast_request_policy(),
            &CancellationContext::new(),
        );
        assert!(matches!(
            result,
            Err(CancellationError::Error(TranscriptionError::Api {
                status: 401,
                ..
            }))
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

        let result = pp.process_with_cancellation_and_request_policy(
            "some text",
            config,
            fast_request_policy(),
            &CancellationContext::new(),
        );
        assert!(result.is_ok());
        mock.assert();
    }
}
