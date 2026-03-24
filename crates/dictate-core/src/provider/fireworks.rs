//! Fireworks Whisper transcription provider.

use super::openai_compatible::SharedOpenAiCompatibleProvider;
use super::{TranscriptionConfig, TranscriptionProvider, TranscriptionResult};
use crate::cancellation::{CancellationContext, CancellationResult};
use crate::error::TranscriptionError;
use crate::request_policy::RequestPolicy;

/// Fireworks Whisper transcription provider.
#[derive(Debug, Default, Clone, Copy)]
pub struct FireworksProvider;

impl TranscriptionProvider for FireworksProvider {
    fn name(&self) -> &'static str {
        "fireworks"
    }

    fn transcribe(
        &self,
        config: TranscriptionConfig<'_>,
    ) -> Result<TranscriptionResult, TranscriptionError> {
        SharedOpenAiCompatibleProvider::fireworks().transcribe(config)
    }

    fn transcribe_with_cancellation(
        &self,
        config: TranscriptionConfig<'_>,
        cancellation: &CancellationContext,
    ) -> CancellationResult<TranscriptionResult, TranscriptionError> {
        SharedOpenAiCompatibleProvider::fireworks()
            .transcribe_with_cancellation(config, cancellation)
    }

    fn transcribe_with_cancellation_and_request_policy(
        &self,
        config: TranscriptionConfig<'_>,
        request_policy: RequestPolicy,
        cancellation: &CancellationContext,
    ) -> CancellationResult<TranscriptionResult, TranscriptionError> {
        SharedOpenAiCompatibleProvider::fireworks().transcribe_with_cancellation_and_request_policy(
            config,
            request_policy,
            cancellation,
        )
    }
}
