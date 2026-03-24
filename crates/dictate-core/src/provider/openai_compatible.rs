use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;

use super::{ResponseFormat, TranscriptionConfig, TranscriptionProvider, TranscriptionResult};
use crate::cancellation::{CancellationContext, CancellationError, CancellationResult};
use crate::error::TranscriptionError;
use crate::openai_error::api_error_from_failed_response;
use crate::request_policy::RequestPolicy;
use crate::retry::retry_with_cancellation;
use crate::token::{MAX_PROMPT_TOKENS, estimate_token_count};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Preset {
    Groq,
    Fireworks,
    OpenAiCompatible,
}

#[derive(Debug, Clone, Copy)]
pub struct SharedOpenAiCompatibleProvider {
    preset: Preset,
}

impl SharedOpenAiCompatibleProvider {
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

    const fn default_model(self) -> Option<&'static str> {
        match self.preset {
            Preset::Groq => Some("whisper-large-v3-turbo"),
            Preset::Fireworks => Some("whisper-v3-turbo"),
            Preset::OpenAiCompatible => None,
        }
    }

    fn default_url(self, model: &str) -> Option<&'static str> {
        match self.preset {
            Preset::Groq => Some("https://api.groq.com/openai/v1/audio/transcriptions"),
            Preset::Fireworks => match model {
                "whisper-v3-turbo" => {
                    Some("https://audio-turbo.api.fireworks.ai/v1/audio/transcriptions")
                }
                _ => Some("https://audio-prod.api.fireworks.ai/v1/audio/transcriptions"),
            },
            Preset::OpenAiCompatible => None,
        }
    }

    const fn error_label(self) -> &'static str {
        match self.preset {
            Preset::Groq => "Groq transcription error",
            Preset::Fireworks => "Fireworks transcription error",
            Preset::OpenAiCompatible => "OpenAI-compatible transcription error",
        }
    }

    const fn provider_name(self) -> &'static str {
        match self.preset {
            Preset::Groq => "groq",
            Preset::Fireworks => "fireworks",
            Preset::OpenAiCompatible => "openai-compatible",
        }
    }
}

/// Generic OpenAI-compatible transcription provider.
#[derive(Debug, Default, Clone, Copy)]
pub struct OpenAiCompatibleProvider;

impl TranscriptionProvider for OpenAiCompatibleProvider {
    fn name(&self) -> &'static str {
        SharedOpenAiCompatibleProvider::generic().provider_name()
    }

    fn transcribe(
        &self,
        config: TranscriptionConfig<'_>,
    ) -> Result<TranscriptionResult, TranscriptionError> {
        SharedOpenAiCompatibleProvider::generic().transcribe(config)
    }

    fn transcribe_with_cancellation(
        &self,
        config: TranscriptionConfig<'_>,
        cancellation: &CancellationContext,
    ) -> CancellationResult<TranscriptionResult, TranscriptionError> {
        SharedOpenAiCompatibleProvider::generic().transcribe_with_cancellation(config, cancellation)
    }

    fn transcribe_with_cancellation_and_request_policy(
        &self,
        config: TranscriptionConfig<'_>,
        request_policy: RequestPolicy,
        cancellation: &CancellationContext,
    ) -> CancellationResult<TranscriptionResult, TranscriptionError> {
        SharedOpenAiCompatibleProvider::generic().transcribe_with_cancellation_and_request_policy(
            config,
            request_policy,
            cancellation,
        )
    }
}

impl TranscriptionProvider for SharedOpenAiCompatibleProvider {
    fn name(&self) -> &'static str {
        self.provider_name()
    }

    fn transcribe(
        &self,
        config: TranscriptionConfig<'_>,
    ) -> Result<TranscriptionResult, TranscriptionError> {
        let request_policy = config.request_policy;
        crate::runtime::block_on(transcribe_request_async(
            *self,
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
            *self,
            config,
            request_policy,
            cancellation,
        ))
    }
}

fn http_client(timeout: Duration) -> Result<Client, TranscriptionError> {
    Client::builder().timeout(timeout).build().map_err(|e| {
        TranscriptionError::HttpClientInitialization(format!(
            "failed to initialize HTTP client: {e}"
        ))
    })
}

async fn transcribe_request_async(
    provider: SharedOpenAiCompatibleProvider,
    config: TranscriptionConfig<'_>,
    request_policy: RequestPolicy,
    cancellation: &CancellationContext,
) -> CancellationResult<TranscriptionResult, TranscriptionError> {
    let model = config
        .model
        .or_else(|| provider.default_model())
        .ok_or_else(|| {
            CancellationError::Error(TranscriptionError::InvalidResponse(
                "transcription model not configured".to_string(),
            ))
        })?;
    let url = config
        .base_url
        .or_else(|| provider.default_url(model))
        .ok_or_else(|| {
            CancellationError::Error(TranscriptionError::InvalidResponse(
                "transcription endpoint not configured".to_string(),
            ))
        })?;
    let client = http_client(request_policy.timeout).map_err(CancellationError::Error)?;

    retry_with_cancellation(
        request_policy,
        cancellation,
        || send_request(&client, url, &config, model, provider, cancellation),
        |err, dur| {
            eprintln!("[dictate] retrying after {dur:?}: {err}");
        },
    )
    .await
}

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

async fn send_request(
    client: &Client,
    url: &str,
    config: &TranscriptionConfig<'_>,
    model: &str,
    provider: SharedOpenAiCompatibleProvider,
    cancellation: &CancellationContext,
) -> CancellationResult<TranscriptionResult, TranscriptionError> {
    cancellation.check()?;

    let filename = format!("audio.{}", config.audio.extension());
    let data = config.audio.data().clone();
    let len = data.len() as u64;
    let file_part = reqwest::multipart::Part::stream_with_length(data, len)
        .file_name(filename)
        .mime_str(config.audio.mime_type())
        .map_err(|e| CancellationError::Error(TranscriptionError::EncodingFailed(e.to_string())))?;

    let mut form = reqwest::multipart::Form::new()
        .text("model", model.to_string())
        .part("file", file_part);

    if let Some(lang) = config.language {
        form = form.text("language", lang.to_string());
    }

    if let Some(prompt) = config.prompt {
        if provider.preset == Preset::Groq {
            validate_prompt_length(prompt).map_err(CancellationError::Error)?;
        }
        form = form.text("prompt", prompt.to_string());
    }

    form = form.text("response_format", config.response_format.as_str());

    if let Some(temp) = config.temperature {
        form = form.text("temperature", temp.to_string());
    }

    for granularity in &config.timestamp_granularities {
        form = form.text("timestamp_granularities[]", granularity.as_str());
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

    match config.response_format {
        ResponseFormat::Text => Ok(TranscriptionResult {
            text: body.trim().to_string(),
            segments: None,
            words: None,
        }),
        ResponseFormat::Json | ResponseFormat::VerboseJson => {
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

    #[test]
    fn semantic_model_mapping_matches_provider_defaults() {
        assert_eq!(
            SharedOpenAiCompatibleProvider::groq().default_model(),
            Some("whisper-large-v3-turbo")
        );
        assert_eq!(
            SharedOpenAiCompatibleProvider::fireworks().default_model(),
            Some("whisper-v3-turbo")
        );
        assert_eq!(
            SharedOpenAiCompatibleProvider::generic().default_model(),
            None
        );
    }

    #[test]
    fn fireworks_default_endpoint_depends_on_model() {
        assert_eq!(
            SharedOpenAiCompatibleProvider::fireworks().default_url("whisper-v3-turbo"),
            Some("https://audio-turbo.api.fireworks.ai/v1/audio/transcriptions")
        );
        assert_eq!(
            SharedOpenAiCompatibleProvider::fireworks().default_url("whisper-v3"),
            Some("https://audio-prod.api.fireworks.ai/v1/audio/transcriptions")
        );
    }

    #[test]
    fn groq_prompt_validation_is_enforced() {
        let prompt = "a".repeat(MAX_PROMPT_TOKENS + 1);
        let result = validate_prompt_length(&prompt);
        assert!(matches!(
            result,
            Err(TranscriptionError::PromptTooLong { .. })
        ));
    }
}
