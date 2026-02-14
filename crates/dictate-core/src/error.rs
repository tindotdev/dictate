use std::fmt;

/// Errors that can occur during audio operations.
#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("Audio device not found: {0}")]
    DeviceNotFound(String),

    #[error(
        "Audio device selection is ambiguous for {query:?}. Use `--device <index>` or a longer name.\nMatches:\n{matches}"
    )]
    DeviceAmbiguous { query: String, matches: String },

    #[error("Recording failed: {0}")]
    RecordingFailed(String),

    #[error("Audio device permission denied: {reason}")]
    DevicePermissionDenied { reason: String },

    #[error("Resampling error: {0}")]
    ResamplingError(String),

    #[error("Audio I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

impl AudioError {
    pub fn device_not_found(device: impl fmt::Display) -> Self {
        Self::DeviceNotFound(device.to_string())
    }

    pub fn device_ambiguous(query: impl Into<String>, matches: impl Into<String>) -> Self {
        Self::DeviceAmbiguous {
            query: query.into(),
            matches: matches.into(),
        }
    }

    pub fn recording_failed(msg: impl fmt::Display) -> Self {
        Self::RecordingFailed(msg.to_string())
    }
}

impl AudioError {
    pub(crate) fn from_build_stream(err: &cpal::BuildStreamError) -> Self {
        let msg = err.to_string();
        if is_permission_error(&msg) {
            return Self::DevicePermissionDenied {
                reason: format!("{msg}\n\nTroubleshooting:\n{}", permission_denied_help()),
            };
        }
        Self::RecordingFailed(msg)
    }

    pub(crate) fn from_play_stream(err: &cpal::PlayStreamError) -> Self {
        let msg = err.to_string();
        if is_permission_error(&msg) {
            return Self::DevicePermissionDenied {
                reason: format!("{msg}\n\nTroubleshooting:\n{}", permission_denied_help()),
            };
        }
        Self::RecordingFailed(msg)
    }

    pub(crate) fn from_devices(err: &cpal::DevicesError) -> Self {
        Self::DeviceNotFound(err.to_string())
    }
}

/// Platform-specific troubleshooting guidance for audio permission errors.
const fn permission_denied_help() -> &'static str {
    if cfg!(target_os = "macos") {
        "  • Open System Settings → Privacy & Security → Microphone\n  \
         • Ensure your terminal app (Terminal, iTerm2, etc.) is listed and enabled\n  \
         • Try a different device with `dictate devices` and `dictate --device <name>`"
    } else {
        "  • Check that PipeWire/PulseAudio is running: `systemctl --user status pipewire`\n  \
         • Add your user to the audio group: `sudo usermod -aG audio $USER` (then log out and back in)\n  \
         • Check if another application has exclusive access to the device\n  \
         • Try a different device with `dictate devices` and `dictate --device <name>`"
    }
}

/// Check if an error message indicates a permission or access issue.
///
/// cpal wraps OS-specific errors (ALSA/PipeWire on Linux, `CoreAudio` on macOS)
/// into opaque strings. We heuristically match known patterns to surface
/// actionable guidance.
fn is_permission_error(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    lower.contains("permission")
        || lower.contains("denied")
        || lower.contains("access")
        || lower.contains("eacces")
        || lower.contains("eperm")
        || lower.contains("kaudiohardware")
}

// ─── Transcription Errors ────────────────────────────────────────────────────

/// Errors that can occur during transcription (encoding, network, API).
#[derive(Debug, thiserror::Error)]
pub enum TranscriptionError {
    /// The required API key environment variable is not set.
    #[error("API key missing: set {env_var} environment variable")]
    MissingApiKey {
        /// Name of the environment variable (e.g. `GROQ_API_KEY`).
        env_var: &'static str,
    },

    /// Audio encoding (e.g. WAV generation) failed.
    #[error("encoding failed: {0}")]
    EncodingFailed(String),

    /// Network-level transport error (timeout, DNS, connection refused).
    #[error("network error: {0}")]
    Network(String),

    /// HTTP client setup failed before any request was sent.
    #[error("HTTP client initialization failed: {0}")]
    HttpClientInitialization(String),

    /// The API returned a non-success HTTP status.
    #[error("API error ({status}): {message}")]
    Api {
        /// HTTP status code (e.g. 400, 401, 413).
        status: u16,
        /// Human-readable error message from the response body.
        message: String,
    },

    /// Rate limit (429) persisted after all retry attempts.
    #[error("rate limit exceeded after {retries} retries")]
    RateLimitExhausted {
        /// Number of retry attempts made.
        retries: u32,
    },

    /// The API response could not be parsed.
    #[error("invalid API response: {0}")]
    InvalidResponse(String),

    /// The prompt exceeds the maximum token limit.
    #[error(
        "prompt exceeds maximum {max_tokens} tokens (estimated {estimated_tokens} tokens, {char_count} characters)"
    )]
    PromptTooLong {
        /// Estimated number of tokens in the prompt.
        estimated_tokens: usize,
        /// Maximum allowed tokens.
        max_tokens: usize,
        /// Character count of the prompt.
        char_count: usize,
    },
}

// ─── Retry classification ───────────────────────────────────────────────────

impl TranscriptionError {
    /// Whether this error is worth retrying.
    ///
    /// Network errors are pre-classified as retryable at conversion time (timeout/connect only).
    #[must_use]
    pub const fn is_retryable(&self) -> bool {
        match self {
            Self::Network(_) | Self::RateLimitExhausted { .. } => true,
            Self::Api { status, .. } => is_retryable_status(*status),
            _ => false,
        }
    }

    /// Whether this error originated from a 429 rate limit.
    #[must_use]
    pub const fn is_rate_limit_error(&self) -> bool {
        matches!(
            self,
            Self::RateLimitExhausted { .. } | Self::Api { status: 429, .. }
        )
    }
}

/// HTTP status codes worth retrying.
#[must_use]
pub const fn is_retryable_status(status: u16) -> bool {
    matches!(status, 408 | 429 | 500 | 502 | 503 | 504)
}

// ─── Model ID Errors ────────────────────────────────────────────────────────

/// Errors that can occur when constructing a [`ModelId`](crate::model_id::ModelId).
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ModelIdError {
    /// The model ID string was empty.
    #[error("model ID must not be empty")]
    Empty,

    /// The model ID exceeded the maximum length of 128 characters.
    #[error("model ID is too long ({len} chars, max 128)")]
    TooLong {
        /// Actual length of the invalid model ID string.
        len: usize,
    },

    /// The model ID contained characters outside `[a-zA-Z0-9._/-]`.
    #[error("model ID contains invalid characters (allowed: a-zA-Z0-9._/-)")]
    InvalidCharacters,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_statuses() {
        assert!(is_retryable_status(408));
        assert!(is_retryable_status(429));
        assert!(is_retryable_status(500));
        assert!(is_retryable_status(502));
        assert!(is_retryable_status(503));
        assert!(is_retryable_status(504));

        assert!(!is_retryable_status(400));
        assert!(!is_retryable_status(401));
        assert!(!is_retryable_status(403));
        assert!(!is_retryable_status(413));
        assert!(!is_retryable_status(200));
    }

    #[test]
    fn retryable_api_errors() {
        let retryable = TranscriptionError::Api {
            status: 500,
            message: "internal".into(),
        };
        assert!(retryable.is_retryable());

        let non_retryable = TranscriptionError::Api {
            status: 401,
            message: "unauthorized".into(),
        };
        assert!(!non_retryable.is_retryable());

        let encoding = TranscriptionError::EncodingFailed("bad".into());
        assert!(!encoding.is_retryable());
    }

    #[test]
    fn retryable_network_errors() {
        let err = TranscriptionError::Network("timeout".into());
        assert!(err.is_retryable());
    }

    #[test]
    fn http_client_initialization_is_not_retryable() {
        let err = TranscriptionError::HttpClientInitialization("tls backend missing".into());
        assert!(!err.is_retryable());
    }

    #[test]
    fn rate_limit_error_classification() {
        let rate_limit = TranscriptionError::Api {
            status: 429,
            message: "rate limit".into(),
        };
        assert!(rate_limit.is_rate_limit_error());
        assert!(rate_limit.is_retryable());

        let exhausted = TranscriptionError::RateLimitExhausted { retries: 3 };
        assert!(exhausted.is_rate_limit_error());

        let server_error = TranscriptionError::Api {
            status: 500,
            message: "internal".into(),
        };
        assert!(!server_error.is_rate_limit_error());
    }
}
