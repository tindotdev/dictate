//! Groq chat completion post-processor.

use super::openai_compatible::SharedOpenAiCompatiblePostProcessor;
use super::{PostProcessConfig, PostProcessor};
use crate::cancellation::{CancellationContext, CancellationResult};
use crate::error::TranscriptionError;
use crate::request_policy::RequestPolicy;

/// Default LLM model used for Groq post-processing when no override is provided.
pub const DEFAULT_POST_PROCESS_MODEL: &str = "openai/gpt-oss-20b";

/// Groq-based post-processor using chat completions.
#[derive(Debug, Default, Clone, Copy)]
pub struct GroqPostProcessor;

impl PostProcessor for GroqPostProcessor {
    fn name(&self) -> &'static str {
        "groq-chat"
    }

    fn process(
        &self,
        text: &str,
        config: PostProcessConfig<'_>,
    ) -> Result<String, TranscriptionError> {
        SharedOpenAiCompatiblePostProcessor::groq().process(text, config)
    }

    fn process_with_context(
        &self,
        text: &str,
        context: Option<&str>,
        config: PostProcessConfig<'_>,
    ) -> Result<String, TranscriptionError> {
        SharedOpenAiCompatiblePostProcessor::groq().process_with_context(text, context, config)
    }

    fn process_with_cancellation(
        &self,
        text: &str,
        config: PostProcessConfig<'_>,
        cancellation: &CancellationContext,
    ) -> CancellationResult<String, TranscriptionError> {
        SharedOpenAiCompatiblePostProcessor::groq().process_with_cancellation(
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
        SharedOpenAiCompatiblePostProcessor::groq().process_with_cancellation_and_request_policy(
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
        SharedOpenAiCompatiblePostProcessor::groq().process_with_context_and_request_policy(
            text,
            context,
            config,
            request_policy,
            cancellation,
        )
    }
}
