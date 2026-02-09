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

    #[error(
        "Audio device permission denied: {reason}\n\nTroubleshooting:\n  • Check that PipeWire/PulseAudio is running: `systemctl --user status pipewire`\n  • Add your user to the audio group: `sudo usermod -aG audio $USER` (then log out and back in)\n  • Check if another application has exclusive access to the device\n  • Try a different device with `dictate devices` and `dictate --device <name>`"
    )]
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
            return Self::DevicePermissionDenied { reason: msg };
        }
        Self::RecordingFailed(msg)
    }

    pub(crate) fn from_play_stream(err: &cpal::PlayStreamError) -> Self {
        let msg = err.to_string();
        if is_permission_error(&msg) {
            return Self::DevicePermissionDenied { reason: msg };
        }
        Self::RecordingFailed(msg)
    }

    pub(crate) fn from_devices(err: &cpal::DevicesError) -> Self {
        Self::DeviceNotFound(err.to_string())
    }
}

/// Check if an error message indicates a permission or access issue.
///
/// cpal wraps OS-specific errors (ALSA/PipeWire) into opaque strings.
/// We heuristically match known patterns to surface actionable guidance.
fn is_permission_error(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    lower.contains("permission")
        || lower.contains("denied")
        || lower.contains("access")
        || lower.contains("eacces")
        || lower.contains("eperm")
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
