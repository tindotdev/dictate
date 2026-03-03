//! Cancellation primitives shared across transcription and post-processing.

use std::thread;
use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;

use crate::error::TranscriptionError;

const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// A core-owned cancellation context for a single dictate session.
///
/// This wraps [`CancellationToken`] so downstream crates do not depend on
/// `tokio-util` directly while the core transitions toward async transport.
#[derive(Debug, Clone, Default)]
pub struct CancellationContext {
    token: CancellationToken,
}

impl CancellationContext {
    /// Create a new, uncancelled context.
    #[must_use]
    pub fn new() -> Self {
        Self {
            token: CancellationToken::new(),
        }
    }

    /// Mark this context as cancelled.
    pub fn cancel(&self) {
        self.token.cancel();
    }

    /// Whether cancellation has been requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }

    /// Return a cancellation error if the session has been cancelled.
    ///
    /// # Errors
    ///
    /// Returns [`TranscriptionError::Cancelled`] once cancellation is observed.
    pub fn check(&self) -> Result<(), TranscriptionError> {
        if self.is_cancelled() {
            return Err(TranscriptionError::Cancelled);
        }

        Ok(())
    }

    /// Sleep for up to `duration`, returning early if cancellation is observed.
    ///
    /// # Errors
    ///
    /// Returns [`TranscriptionError::Cancelled`] if cancellation is requested
    /// before the full delay elapses.
    pub fn sleep(&self, duration: Duration) -> Result<(), TranscriptionError> {
        let deadline = Instant::now() + duration;

        loop {
            self.check()?;

            let now = Instant::now();
            if now >= deadline {
                return Ok(());
            }

            let remaining = deadline.saturating_duration_since(now);
            thread::sleep(remaining.min(CANCELLATION_POLL_INTERVAL));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_returns_cancelled_after_cancel() {
        let context = CancellationContext::new();
        context.cancel();

        assert!(matches!(
            context.check(),
            Err(TranscriptionError::Cancelled)
        ));
    }

    #[test]
    fn sleep_returns_cancelled_when_context_is_cancelled() {
        let context = CancellationContext::new();
        context.cancel();

        assert!(matches!(
            context.sleep(Duration::from_millis(10)),
            Err(TranscriptionError::Cancelled)
        ));
    }
}
