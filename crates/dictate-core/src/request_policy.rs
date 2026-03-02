use std::time::Duration;

/// Provider-agnostic timeout and retry settings for a single request type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestPolicy {
    /// Maximum time allowed for one HTTP request attempt.
    pub timeout: Duration,
    /// Number of retry attempts after the initial request.
    pub max_retries: u32,
    /// Minimum delay before a retry.
    pub base_delay: Duration,
    /// Maximum backoff delay between retries.
    pub max_delay: Duration,
}

impl RequestPolicy {
    /// Create a new request policy.
    #[must_use]
    pub const fn new(
        timeout: Duration,
        max_retries: u32,
        base_delay: Duration,
        max_delay: Duration,
    ) -> Self {
        Self {
            timeout,
            max_retries,
            base_delay,
            max_delay,
        }
    }
}

/// Request policies for the transcription and post-processing stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestPolicies {
    /// Policy for speech-to-text requests.
    pub transcription: RequestPolicy,
    /// Policy for optional post-processing requests.
    pub post_process: RequestPolicy,
}

impl RequestPolicies {
    /// Shorter, interactive budgets for immediate recording flows.
    #[must_use]
    pub const fn interactive() -> Self {
        Self {
            transcription: RequestPolicy::new(
                Duration::from_secs(45),
                1,
                Duration::from_secs(1),
                Duration::from_secs(2),
            ),
            post_process: RequestPolicy::new(
                Duration::from_secs(20),
                1,
                Duration::from_secs(1),
                Duration::from_secs(2),
            ),
        }
    }

    /// Longer, more persistent budgets for explicit retry/reprocessing flows.
    #[must_use]
    pub const fn persistent() -> Self {
        Self {
            transcription: RequestPolicy::new(
                Duration::from_secs(300),
                3,
                Duration::from_secs(1),
                Duration::from_secs(16),
            ),
            post_process: RequestPolicy::new(
                Duration::from_secs(60),
                3,
                Duration::from_secs(1),
                Duration::from_secs(16),
            ),
        }
    }
}

impl Default for RequestPolicies {
    fn default() -> Self {
        Self::persistent()
    }
}
