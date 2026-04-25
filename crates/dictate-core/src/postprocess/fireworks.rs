//! Fireworks chat completion post-processor.

use super::openai_compatible::SharedOpenAiCompatiblePostProcessor;
use super::{PostProcessConfig, PostProcessor};
use crate::cancellation::{CancellationContext, CancellationResult};
use crate::error::TranscriptionError;
use crate::request_policy::RequestPolicy;

/// Default Fireworks LLM model used for post-processing.
pub const FIREWORKS_DEFAULT_POST_PROCESS_MODEL: &str = "accounts/fireworks/models/gpt-oss-120b";

/// Fireworks-based post-processor using chat completions.
#[derive(Debug, Default, Clone, Copy)]
pub struct FireworksPostProcessor;

impl PostProcessor for FireworksPostProcessor {
    fn name(&self) -> &'static str {
        "fireworks-chat"
    }

    fn process(
        &self,
        text: &str,
        config: PostProcessConfig<'_>,
    ) -> Result<String, TranscriptionError> {
        SharedOpenAiCompatiblePostProcessor::fireworks().process(text, config)
    }

    fn process_with_context(
        &self,
        text: &str,
        context: Option<&str>,
        config: PostProcessConfig<'_>,
    ) -> Result<String, TranscriptionError> {
        SharedOpenAiCompatiblePostProcessor::fireworks().process_with_context(text, context, config)
    }

    fn process_with_cancellation(
        &self,
        text: &str,
        config: PostProcessConfig<'_>,
        cancellation: &CancellationContext,
    ) -> CancellationResult<String, TranscriptionError> {
        SharedOpenAiCompatiblePostProcessor::fireworks().process_with_cancellation(
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
        SharedOpenAiCompatiblePostProcessor::fireworks()
            .process_with_cancellation_and_request_policy(
                text,
                config,
                request_policy,
                cancellation,
            )
    }

    fn process_with_context_and_request_policy(
        &self,
        text: &str,
        context: Option<&str>,
        config: PostProcessConfig<'_>,
        request_policy: RequestPolicy,
        cancellation: &CancellationContext,
    ) -> CancellationResult<String, TranscriptionError> {
        SharedOpenAiCompatiblePostProcessor::fireworks().process_with_context_and_request_policy(
            text,
            context,
            config,
            request_policy,
            cancellation,
        )
    }
}
