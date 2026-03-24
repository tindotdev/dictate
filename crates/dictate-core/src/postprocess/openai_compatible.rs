use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::{PostProcessConfig, PostProcessor};
use crate::cancellation::{CancellationContext, CancellationError, CancellationResult};
use crate::error::TranscriptionError;
use crate::openai_error::api_error_from_failed_response;
use crate::request_policy::RequestPolicy;
use crate::retry::retry_with_cancellation;

const SYSTEM_PROMPT: &str = include_str!("prompts/cleanup.txt");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Preset {
    Groq,
    Fireworks,
    OpenAiCompatible,
}

#[derive(Debug, Clone, Copy)]
pub struct SharedOpenAiCompatiblePostProcessor {
    preset: Preset,
}

impl SharedOpenAiCompatiblePostProcessor {
    pub const fn groq() -> Self {
        Self {
            preset: Preset::Groq,
        }
    }

    pub const fn fireworks() -> Self {
        Self {
            preset: Preset::Fireworks,
        }
    }

    pub const fn generic() -> Self {
        Self {
            preset: Preset::OpenAiCompatible,
        }
    }

    const fn default_url(self) -> Option<&'static str> {
        match self.preset {
            Preset::Groq => Some("https://api.groq.com/openai/v1/chat/completions"),
            Preset::Fireworks => Some("https://api.fireworks.ai/inference/v1/chat/completions"),
            Preset::OpenAiCompatible => None,
        }
    }

    const fn default_model(self) -> Option<&'static str> {
        match self.preset {
            Preset::Groq => Some(super::groq::DEFAULT_POST_PROCESS_MODEL),
            Preset::Fireworks => Some(super::fireworks::FIREWORKS_DEFAULT_POST_PROCESS_MODEL),
            Preset::OpenAiCompatible => None,
        }
    }

    const fn error_label(self) -> &'static str {
        match self.preset {
            Preset::Groq => "Groq post-process error",
            Preset::Fireworks => "Fireworks post-process error",
            Preset::OpenAiCompatible => "OpenAI-compatible post-process error",
        }
    }

    const fn processor_name(self) -> &'static str {
        match self.preset {
            Preset::Groq => "groq-chat",
            Preset::Fireworks => "fireworks-chat",
            Preset::OpenAiCompatible => "openai-compatible-chat",
        }
    }
}

/// Generic OpenAI-compatible post-processor.
#[derive(Debug, Default, Clone, Copy)]
pub struct OpenAiCompatiblePostProcessor;

impl PostProcessor for OpenAiCompatiblePostProcessor {
    fn name(&self) -> &'static str {
        SharedOpenAiCompatiblePostProcessor::generic().processor_name()
    }

    fn process(
        &self,
        text: &str,
        config: PostProcessConfig<'_>,
    ) -> Result<String, TranscriptionError> {
        SharedOpenAiCompatiblePostProcessor::generic().process(text, config)
    }

    fn process_with_cancellation(
        &self,
        text: &str,
        config: PostProcessConfig<'_>,
        cancellation: &CancellationContext,
    ) -> CancellationResult<String, TranscriptionError> {
        SharedOpenAiCompatiblePostProcessor::generic().process_with_cancellation(
            text,
            config,
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
        SharedOpenAiCompatiblePostProcessor::generic().process_with_cancellation_and_request_policy(
            text,
            config,
            request_policy,
            cancellation,
        )
    }
}

impl PostProcessor for SharedOpenAiCompatiblePostProcessor {
    fn name(&self) -> &'static str {
        self.processor_name()
    }

    fn process(
        &self,
        text: &str,
        config: PostProcessConfig<'_>,
    ) -> Result<String, TranscriptionError> {
        let request_policy = config.request_policy;
        crate::runtime::block_on(process_request_async(
            *self,
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
            *self,
            text,
            config,
            request_policy,
            cancellation,
        ))
    }
}

fn chat_client(timeout: Duration) -> Result<Client, TranscriptionError> {
    Client::builder().timeout(timeout).build().map_err(|e| {
        TranscriptionError::HttpClientInitialization(format!(
            "failed to initialize chat HTTP client: {e}"
        ))
    })
}

async fn process_request_async(
    provider: SharedOpenAiCompatiblePostProcessor,
    text: &str,
    config: PostProcessConfig<'_>,
    request_policy: RequestPolicy,
    cancellation: &CancellationContext,
) -> CancellationResult<String, TranscriptionError> {
    cancellation.check()?;

    if text.is_empty() {
        return Ok(String::new());
    }

    let url = config
        .base_url
        .or_else(|| provider.default_url())
        .ok_or_else(|| {
            CancellationError::Error(TranscriptionError::InvalidResponse(
                "post-process endpoint not configured".to_string(),
            ))
        })?;
    let model = config
        .model
        .or_else(|| provider.default_model())
        .ok_or_else(|| {
            CancellationError::Error(TranscriptionError::InvalidResponse(
                "post-process model not configured".to_string(),
            ))
        })?;
    let system_prompt = config.system_prompt.unwrap_or(SYSTEM_PROMPT);
    let client = chat_client(request_policy.timeout).map_err(CancellationError::Error)?;
    let params = ChatRequestParams {
        api_key: config.api_key,
        model,
        text,
        system_prompt,
        temperature: config.temperature,
    };

    retry_with_cancellation(
        request_policy,
        cancellation,
        || send_chat_request(&client, url, &params, provider, cancellation),
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

async fn send_chat_request(
    client: &Client,
    url: &str,
    params: &ChatRequestParams<'_>,
    provider: SharedOpenAiCompatiblePostProcessor,
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

    if !response.status().is_success() {
        let error = api_error_from_failed_response(response, provider.error_label(), cancellation)
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
        .map(|choice| choice.message.content.trim().to_string())
        .ok_or_else(|| {
            CancellationError::Error(TranscriptionError::InvalidResponse(
                "chat response contained no choices".to_string(),
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_chat_model_resolution_matches_named_providers() {
        assert_eq!(
            SharedOpenAiCompatiblePostProcessor::groq().default_model(),
            Some(super::super::groq::DEFAULT_POST_PROCESS_MODEL)
        );
        assert_eq!(
            SharedOpenAiCompatiblePostProcessor::fireworks().default_model(),
            Some(super::super::fireworks::FIREWORKS_DEFAULT_POST_PROCESS_MODEL)
        );
        assert_eq!(
            SharedOpenAiCompatiblePostProcessor::generic().default_model(),
            None
        );
    }
}
