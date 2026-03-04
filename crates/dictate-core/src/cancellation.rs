//! Cancellation primitives shared across transcription and post-processing.

use std::future::Future;
use std::thread;
use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;

const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Marker error returned when an operation is cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("cancelled")]
pub struct Cancelled;

/// Result type for operations that may either fail normally or be cancelled.
pub type CancellationResult<T, E> = Result<T, CancellationError<E>>;

/// Wrapper error for APIs that preserve the underlying error type while also
/// surfacing user cancellation.
#[derive(Debug, thiserror::Error)]
pub enum CancellationError<E> {
    /// The operation was cancelled before it completed.
    #[error("cancelled")]
    Cancelled,

    /// The underlying operation failed normally.
    #[error("{0}")]
    Error(E),
}

impl<E> CancellationError<E> {
    /// Whether this value represents cancellation rather than an underlying error.
    #[must_use]
    pub const fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }

    /// Return the underlying error, if present.
    #[must_use]
    pub fn into_error(self) -> Option<E> {
        match self {
            Self::Cancelled => None,
            Self::Error(err) => Some(err),
        }
    }
}

impl<E> From<Cancelled> for CancellationError<E> {
    fn from(_: Cancelled) -> Self {
        Self::Cancelled
    }
}

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
    /// Returns [`Cancelled`] once cancellation is observed.
    pub fn check(&self) -> Result<(), Cancelled> {
        if self.is_cancelled() {
            return Err(Cancelled);
        }

        Ok(())
    }

    /// Sleep for up to `duration`, returning early if cancellation is observed.
    ///
    /// # Errors
    ///
    /// Returns [`Cancelled`] if cancellation is requested before the full
    /// delay elapses.
    pub fn sleep(&self, duration: Duration) -> Result<(), Cancelled> {
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

    /// Sleep asynchronously for up to `duration`, returning early on cancellation.
    ///
    /// # Errors
    ///
    /// Returns [`Cancelled`] if cancellation is requested before the full
    /// delay elapses.
    pub async fn sleep_async(&self, duration: Duration) -> Result<(), Cancelled> {
        self.check()?;

        let token = self.token.clone();
        tokio::select! {
            () = token.cancelled() => Err(Cancelled),
            () = tokio::time::sleep(duration) => Ok(()),
        }
    }

    /// Resolve `future`, unless cancellation is requested first.
    ///
    /// # Errors
    ///
    /// Returns [`Cancelled`] when the cancellation token wins the race before
    /// `future` completes.
    pub(crate) async fn run_until_cancelled<T, F>(&self, future: F) -> Result<T, Cancelled>
    where
        F: Future<Output = T>,
    {
        self.check()?;

        let token = self.token.clone();
        tokio::select! {
            () = token.cancelled() => Err(Cancelled),
            output = future => Ok(output),
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

        assert!(matches!(context.check(), Err(Cancelled)));
    }

    #[test]
    fn sleep_returns_cancelled_when_context_is_cancelled() {
        let context = CancellationContext::new();
        context.cancel();

        assert!(matches!(
            context.sleep(Duration::from_millis(10)),
            Err(Cancelled)
        ));
    }
}
