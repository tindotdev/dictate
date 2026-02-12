use std::io::IsTerminal;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use dictate_core::token::{MAX_PROMPT_TOKENS, estimate_token_count};
use dictate_core::{
    AudioChunk, AudioError, AudioReceiver, AudioRecorder, ChunkerConfig, ClipboardError,
    DeviceSelection, DictionaryStore, GroqProvider, PipelineConfig, ProgressiveChunker,
    RecorderConfig, RecvResult, ResponseFormat, Segment, TimestampGranularity, TranscriptionError,
    TranscriptionPipeline, TranscriptionResult, WhisperModel, Word,
};
use thiserror::Error;

const RECV_TIMEOUT: Duration = Duration::from_millis(100);
const QUIESCENT_TIMEOUTS: u8 = 3;

/// Environment variable for the Groq API key.
const GROQ_API_KEY_VAR: &str = "GROQ_API_KEY";

/// Environment variable for an optional Groq API base URL override.
const GROQ_BASE_URL_VAR: &str = "GROQ_BASE_URL";

#[derive(Debug, Error)]
pub enum RecordError {
    #[error("audio error: {0}")]
    Audio(#[from] AudioError),

    #[error("transcription error: {0}")]
    Transcription(#[from] TranscriptionError),

    #[error("clipboard error: {0}")]
    Clipboard(#[from] ClipboardError),

    #[error("transcription worker disconnected unexpectedly")]
    TranscriptionWorkerDisconnected,
}

// ══════════════════════════════════════════════════════════════════════════════
//  Builder: RecordOptions
// ══════════════════════════════════════════════════════════════════════════════

/// Configuration options for audio recording and transcription.
/// Use the builder pattern to construct with only the options you need.
#[derive(Default, Debug)]
pub struct RecordOptions {
    device: Option<String>,
    base_url: Option<String>,
    language: Option<String>,
    prompt: Option<String>,
    response_format: Option<ResponseFormat>,
    model: Option<WhisperModel>,
    temperature: Option<f32>,
    timestamp_granularities: Option<Vec<TimestampGranularity>>,
    stdout: bool,
    no_clipboard: bool,
}

impl RecordOptions {
    /// Create a new builder with default options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Select a specific audio input device.
    pub fn device(mut self, device: impl Into<String>) -> Self {
        self.device = Some(device.into());
        self
    }

    /// Override the Groq transcription API base URL.
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    /// Set the ISO-639-1 language code (e.g., "en", "es", "fr").
    pub fn language(mut self, lang: impl Into<String>) -> Self {
        self.language = Some(lang.into());
        self
    }

    /// Set a text prompt to guide transcription style or spelling.
    pub fn prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = Some(prompt.into());
        self
    }

    /// Set the response format: json, `verbose_json`, or text.
    pub const fn response_format(mut self, format: ResponseFormat) -> Self {
        self.response_format = Some(format);
        self
    }

    /// Set the Whisper model (`LargeV3Turbo` or `LargeV3`).
    pub const fn model(mut self, model: WhisperModel) -> Self {
        self.model = Some(model);
        self
    }

    /// Set the sampling temperature (0.0-1.0).
    pub const fn temperature(mut self, temp: f32) -> Self {
        self.temperature = Some(temp);
        self
    }

    /// Set timestamp granularities: segment, word, or both.
    pub fn timestamp_granularities(mut self, granularities: Vec<TimestampGranularity>) -> Self {
        self.timestamp_granularities = Some(granularities);
        self
    }

    /// Print transcript to stdout instead of clipboard.
    pub const fn stdout(mut self, enabled: bool) -> Self {
        self.stdout = enabled;
        self
    }

    /// Skip clipboard entirely (headless/scripted use).
    pub const fn no_clipboard(mut self, enabled: bool) -> Self {
        self.no_clipboard = enabled;
        self
    }
}

// ══════════════════════════════════════════════════════════════════════════════
//  Main Entry Point
// ══════════════════════════════════════════════════════════════════════════════

pub fn run(options: &RecordOptions) -> Result<(), RecordError> {
    // Parse and validate configuration
    let (effective_format, pipeline) = parse_and_create_pipeline(options)?;

    // Fail fast if clipboard is requested but unavailable (missing tool / headless)
    if !options.stdout && !options.no_clipboard {
        dictate_core::check_clipboard_available()?;
    }

    // Set up interrupt handling
    let running = Arc::new(AtomicBool::new(true));
    install_stop_handlers(Arc::clone(&running));

    // Record audio chunks
    let chunks = record_audio_chunks(options.device.as_deref(), &running)?;

    if chunks.is_empty() {
        eprintln!("[dictate] no audio captured");
        return Ok(());
    }

    // Transcribe chunks
    running.store(true, Ordering::Relaxed); // Re-arm for transcription phase
    let (results, interrupted) = transcribe_chunks(&pipeline, chunks, &running)?;

    // Output results
    let merged = merge_results(results);
    output_result(&merged, effective_format, interrupted, options);

    Ok(())
}

// ══════════════════════════════════════════════════════════════════════════════
//  Helper Functions
// ══════════════════════════════════════════════════════════════════════════════

/// Create the transcription pipeline from validated configuration options.
fn parse_and_create_pipeline(
    options: &RecordOptions,
) -> Result<(Option<ResponseFormat>, Arc<TranscriptionPipeline>), RecordError> {
    // Auto-upgrade format when timestamps are requested
    let response_format = auto_upgrade_format(
        options.response_format,
        options.timestamp_granularities.as_ref(),
    );
    let effective_format = response_format;

    // Validate API key upfront (fail fast)
    let api_key =
        std::env::var(GROQ_API_KEY_VAR).map_err(|_| TranscriptionError::MissingApiKey {
            env_var: GROQ_API_KEY_VAR,
        })?;

    let base_url = options
        .base_url
        .clone()
        .or_else(|| std::env::var(GROQ_BASE_URL_VAR).ok());

    // Load dictionary for prompt injection (best-effort: warn and continue on error)
    let effective_prompt = load_dictionary_prompt(options.prompt.as_deref());

    let config = PipelineConfig {
        base_url,
        language: options.language.clone(),
        prompt: effective_prompt,
        response_format: response_format.unwrap_or_default(),
        model: options.model,
        temperature: options.temperature,
        timestamp_granularities: options.timestamp_granularities.clone().unwrap_or_default(),
    };

    let pipeline = Arc::new(TranscriptionPipeline::new(
        Box::new(GroqProvider),
        api_key,
        config,
    ));

    Ok((effective_format, pipeline))
}

/// Record audio from the specified device and collect chunks.
fn record_audio_chunks(
    device: Option<&str>,
    running: &AtomicBool,
) -> Result<Vec<AudioChunk>, RecordError> {
    eprintln!("[dictate] recording... press Enter to stop (Ctrl+C also works)");

    let mut config = RecorderConfig::default();
    if let Some(query) = device {
        config.device = DeviceSelection::Query(query.to_string());
    }

    let (mut recorder, mut rx, info) = AudioRecorder::start(config)?;
    eprintln!(
        "[dictate] device: {} ({} Hz, {}ch) -> resampling to {} Hz mono",
        info.device_name,
        info.device_sample_rate_hz,
        info.device_channels,
        info.target_sample_rate_hz
    );

    // Collect audio chunks
    let chunker_config = ChunkerConfig::default();
    let mut chunker = ProgressiveChunker::new(chunker_config);
    let mut chunks: Vec<AudioChunk> = Vec::new();

    consume_until_stopped(&mut rx, running, &mut chunker, &mut chunks);

    recorder.stop()?;

    // Check recording stats and warn about any issues.
    let stats = recorder.stats().snapshot();
    if stats.resample_errors > 0 {
        eprintln!(
            "[dictate] warning: {} audio samples lost due to resampling errors",
            stats.resample_errors
        );
    }
    if stats.dropped_samples > 0 {
        eprintln!(
            "[dictate] warning: {} audio samples dropped (processing too slow)",
            stats.dropped_samples
        );
    }
    if stats.stream_errors > 0 {
        eprintln!(
            "[dictate] warning: {} audio stream errors occurred",
            stats.stream_errors
        );
    }

    drain_remaining(&mut rx, &mut chunker, &mut chunks);

    let tail = recorder.take_flushed_tail();
    if !tail.is_empty() {
        push_and_collect(&mut chunker, &tail, &mut chunks);
    }

    if let Some(chunk) = chunker.flush() {
        eprintln!(
            "[dictate] final chunk {} ready ({:.1}s)",
            chunk.index,
            chunk.duration_secs()
        );
        chunks.push(chunk);
    }

    Ok(chunks)
}

/// Transcribe all audio chunks, respecting interrupt signals.
///
/// Tracks the recording timeline so each chunk's timestamps are offset to
/// reflect their absolute position. The overlap between consecutive chunks
/// is subtracted so that timestamps align at the splice point.
fn transcribe_chunks(
    pipeline: &Arc<TranscriptionPipeline>,
    chunks: Vec<AudioChunk>,
    running: &AtomicBool,
) -> Result<(Vec<TranscriptionResult>, bool), RecordError> {
    eprintln!(
        "[dictate] transcribing {} chunk{}...",
        chunks.len(),
        if chunks.len() == 1 { "" } else { "s" }
    );

    let mut results: Vec<TranscriptionResult> = Vec::new();
    let mut interrupted = false;
    let mut timeline_offset: f64 = 0.0;

    // Re-arm is handled by the caller before calling this function
    for chunk in chunks {
        if !running.load(Ordering::Relaxed) {
            eprintln!("[dictate] interrupted, skipping remaining chunks");
            interrupted = true;
            break;
        }

        eprintln!(
            "[dictate]   chunk {} ({:.1}s)...",
            chunk.index,
            chunk.duration_secs()
        );

        let chunk_offset = timeline_offset;
        let chunk_duration = f64::from(chunk.duration_secs());
        let leading_overlap = f64::from(chunk.leading_overlap_secs());

        let maybe_result = transcribe_chunk_interruptible(Arc::clone(pipeline), chunk, running)?;
        let Some(mut result) = maybe_result else {
            eprintln!("[dictate] interrupted, canceled in-flight transcription");
            interrupted = true;
            break;
        };

        if chunk_offset > 0.0 {
            offset_timestamps(&mut result, chunk_offset);
        }

        timeline_offset += chunk_duration - leading_overlap;
        results.push(result);
    }

    Ok((results, interrupted))
}

/// Shift all segment and word timestamps by the given offset (seconds).
fn offset_timestamps(result: &mut TranscriptionResult, offset: f64) {
    if let Some(segments) = &mut result.segments {
        for seg in segments {
            seg.start += offset;
            seg.end += offset;
            for word in &mut seg.words {
                word.start += offset;
                word.end += offset;
            }
        }
    }
    if let Some(words) = &mut result.words {
        for word in words {
            word.start += offset;
            word.end += offset;
        }
    }
}

/// Format and output the transcription result.
///
/// Output behavior:
/// - If `--stdout` or `--no-clipboard`: print to stdout
/// - Otherwise: copy to clipboard (with stderr fallback on failure)
fn output_result(
    merged: &TranscriptionResult,
    format: Option<ResponseFormat>,
    interrupted: bool,
    options: &RecordOptions,
) {
    if merged.text.is_empty() && !interrupted {
        eprintln!("[dictate] no speech detected");
        return;
    }

    if merged.text.is_empty() {
        // Interrupted with no text captured
        eprintln!("[dictate] interrupted (no text captured)");
        return;
    }

    // Determine output destination based on flags
    let use_clipboard = !options.stdout && !options.no_clipboard;

    // Format the result according to --format flag
    let formatted = format_to_string(merged, format);

    if use_clipboard {
        // Default behavior: copy to clipboard
        match dictate_core::clipboard::copy_to_clipboard(&formatted) {
            Ok(()) => {
                if interrupted {
                    eprintln!(
                        "[dictate] interrupted (partial transcript: {} chars, copied to clipboard)",
                        merged.text.len()
                    );
                } else {
                    eprintln!(
                        "[dictate] done ({} chars, copied to clipboard)",
                        merged.text.len()
                    );
                }
            }
            Err(err) => {
                // Failure safety: never lose transcribed text
                eprintln!("[dictate] clipboard failed: {err}");
                eprintln!("[dictate] transcript (saved to stderr to prevent data loss):");
                eprintln!("{formatted}");
                // Return success since text was not lost (important for shell scripts)
            }
        }
    } else {
        // --stdout or --no-clipboard: print to stdout
        println!("{formatted}");

        if interrupted {
            eprintln!(
                "[dictate] interrupted (partial transcript: {} chars)",
                merged.text.len()
            );
        } else {
            eprintln!("[dictate] done ({} chars)", merged.text.len());
        }
    }
}

fn transcribe_chunk_interruptible(
    pipeline: Arc<TranscriptionPipeline>,
    chunk: AudioChunk,
    running: &AtomicBool,
) -> Result<Option<TranscriptionResult>, RecordError> {
    let (tx, rx) = mpsc::sync_channel(1);

    std::thread::spawn(move || {
        let result = pipeline.transcribe_chunk(&chunk);
        if tx.send(result).is_err() {
            eprintln!("[dictate] warning: transcription result dropped (receiver disconnected)");
        }
    });

    loop {
        if !running.load(Ordering::Relaxed) {
            return Ok(None);
        }

        match rx.recv_timeout(RECV_TIMEOUT) {
            Ok(result) => return result.map(Some).map_err(RecordError::from),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(RecordError::TranscriptionWorkerDisconnected);
            }
        }
    }
}

fn install_stop_handlers(running: Arc<AtomicBool>) {
    let running_ctrlc = Arc::clone(&running);
    if let Err(err) = ctrlc::set_handler(move || {
        // First press: cooperative shutdown. Second press: force exit.
        if !running_ctrlc.swap(false, Ordering::Relaxed) {
            eprintln!("\n[dictate] forced exit");
            std::process::exit(130);
        }
    }) {
        eprintln!("[dictate] warning: failed to set Ctrl+C handler: {err}");
    }

    // Only listen for Enter when stdin is interactive (not piped / closed).
    if std::io::stdin().is_terminal() {
        std::thread::spawn(move || {
            let mut input = String::new();
            let _ = std::io::stdin().read_line(&mut input);
            running.store(false, Ordering::Relaxed);
        });
    }
}

/// Push samples to chunker and collect any produced chunk.
fn push_and_collect(
    chunker: &mut ProgressiveChunker,
    samples: &[f32],
    chunks: &mut Vec<AudioChunk>,
) {
    if let Some(chunk) = chunker.push_samples(samples) {
        eprintln!(
            "[dictate] chunk {} ready ({:.1}s)",
            chunk.index,
            chunk.duration_secs()
        );
        chunks.push(chunk);
    }
}

fn consume_until_stopped(
    rx: &mut AudioReceiver,
    running: &AtomicBool,
    chunker: &mut ProgressiveChunker,
    chunks: &mut Vec<AudioChunk>,
) {
    while running.load(Ordering::Relaxed) {
        match rx.recv_timeout(RECV_TIMEOUT) {
            RecvResult::Data(samples) => push_and_collect(chunker, samples, chunks),
            RecvResult::Timeout => {}
            RecvResult::Disconnected => break,
        }
    }
}

/// Drain all remaining samples from the ring buffer after the stream has stopped.
///
/// Loops: non-blocking drain → disconnected check → blocking wait with timeout
/// counting, until the producer is dropped and the buffer is empty.
fn drain_remaining(
    rx: &mut AudioReceiver,
    chunker: &mut ProgressiveChunker,
    chunks: &mut Vec<AudioChunk>,
) {
    let mut consecutive_timeouts = 0_u8;

    loop {
        while let Some(samples) = rx.try_recv() {
            consecutive_timeouts = 0;
            push_and_collect(chunker, samples, chunks);
        }

        if rx.is_disconnected() {
            break;
        }

        match rx.recv_timeout(RECV_TIMEOUT) {
            RecvResult::Data(samples) => {
                consecutive_timeouts = 0;
                push_and_collect(chunker, samples, chunks);
            }
            RecvResult::Timeout => {
                consecutive_timeouts += 1;
                if consecutive_timeouts >= QUIESCENT_TIMEOUTS {
                    break;
                }
            }
            RecvResult::Disconnected => break,
        }
    }
}

/// Load the dictionary and compose the effective prompt.
///
/// This is best-effort: if the dictionary cannot be loaded, a warning is printed
/// and the user's prompt (if any) is returned unchanged.
fn load_dictionary_prompt(user_prompt: Option<&str>) -> Option<String> {
    let store = match DictionaryStore::open() {
        Ok(s) => s,
        Err(err) => {
            eprintln!("[dictate] warning: could not open dictionary store: {err}");
            return user_prompt.map(String::from);
        }
    };

    let dict = match store.load() {
        Ok(d) => d,
        Err(err) => {
            eprintln!("[dictate] warning: could not load dictionary: {err}");
            return user_prompt.map(String::from);
        }
    };

    if dict.is_empty() {
        return user_prompt.map(String::from);
    }

    // Calculate remaining token budget after the user's prompt.
    // Reserve 2 tokens for the ". " joiner inserted by build_effective_prompt
    // when both a dictionary hint and a user prompt are present.
    let user_tokens = user_prompt.map_or(0, estimate_token_count);
    let joiner_cost = if user_prompt.is_some() { 2 } else { 0 };
    let remaining_budget = MAX_PROMPT_TOKENS.saturating_sub(user_tokens + joiner_cost);

    let hint = dict.as_prompt_hint_within(remaining_budget);

    if let Some(ref h) = hint {
        if h.included < h.total {
            eprintln!(
                "[dictate] dictionary: using {}/{} entries (prompt token limit)",
                h.included, h.total
            );
        } else {
            eprintln!(
                "[dictate] dictionary loaded ({} {})",
                h.included,
                if h.included == 1 { "entry" } else { "entries" }
            );
        }
    }

    build_effective_prompt(user_prompt, hint.as_ref().map(|h| h.text.as_str()))
}

/// Compose dictionary hint and user prompt into a single effective prompt.
///
/// Dictionary hint comes first (primes vocabulary), then user prompt (style/context).
fn build_effective_prompt(
    user_prompt: Option<&str>,
    dictionary_hint: Option<&str>,
) -> Option<String> {
    match (dictionary_hint, user_prompt) {
        (Some(hint), Some(user)) => Some(format!("{hint}. {user}")),
        (Some(hint), None) => Some(hint.to_string()),
        (None, Some(user)) => Some(user.to_string()),
        (None, None) => None,
    }
}

/// If `--timestamps` is set but `--format` isn't `verbose_json`, upgrade the format.
///
/// Timestamps require `verbose_json` to carry segment/word metadata. Without the
/// upgrade the API would either ignore the granularity flags or error out.
fn auto_upgrade_format(
    explicit_format: Option<ResponseFormat>,
    timestamps: Option<&Vec<TimestampGranularity>>,
) -> Option<ResponseFormat> {
    let has_timestamps = timestamps.is_some_and(|ts| !ts.is_empty());

    if !has_timestamps {
        return explicit_format;
    }

    match explicit_format {
        Some(ResponseFormat::VerboseJson) => explicit_format,
        Some(other) => {
            eprintln!(
                "[dictate] warning: --timestamps requires verbose_json; \
                 overriding --format {}",
                other.as_str()
            );
            Some(ResponseFormat::VerboseJson)
        }
        None => Some(ResponseFormat::VerboseJson),
    }
}

/// Merge multiple chunk transcription results into a single result.
///
/// - Text is space-joined (skipping empty chunks).
/// - Segment IDs are re-indexed contiguously across chunks.
/// - Words are concatenated in order.
fn merge_results(results: Vec<TranscriptionResult>) -> TranscriptionResult {
    if results.is_empty() {
        return TranscriptionResult {
            text: String::new(),
            segments: None,
            words: None,
        };
    }

    if results.len() == 1 {
        // Avoid unnecessary allocation for the single-chunk common case.
        return results.into_iter().next().expect("checked non-empty");
    }

    let mut text = String::new();
    let mut all_segments: Vec<Segment> = Vec::new();
    let mut all_words: Vec<Word> = Vec::new();
    let mut has_segments = false;
    let mut has_words = false;

    for result in results {
        // Join text with spaces between non-empty chunks.
        if !result.text.is_empty() {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&result.text);
        }

        if let Some(segments) = result.segments {
            has_segments = true;
            let base_id = u32::try_from(all_segments.len()).unwrap_or(u32::MAX);
            for mut seg in segments {
                seg.id = base_id.saturating_add(seg.id);
                all_segments.push(seg);
            }
        }

        if let Some(words) = result.words {
            has_words = true;
            all_words.extend(words);
        }
    }

    TranscriptionResult {
        text,
        segments: if has_segments {
            Some(all_segments)
        } else {
            None
        },
        words: if has_words { Some(all_words) } else { None },
    }
}

/// Format the transcription result according to the requested format.
///
/// Returns the formatted string ready for output (clipboard or stdout).
fn format_to_string(result: &TranscriptionResult, format: Option<ResponseFormat>) -> String {
    match format {
        Some(ResponseFormat::VerboseJson) => {
            // Full structured JSON with segments and words.
            match serde_json::to_string_pretty(result) {
                Ok(json) => json,
                Err(err) => {
                    eprintln!("[dictate] warning: JSON serialization failed: {err}");
                    result.text.clone()
                }
            }
        }
        Some(ResponseFormat::Json) => {
            // Simple JSON with text only.
            match serde_json::to_string_pretty(&serde_json::json!({"text": result.text})) {
                Ok(json) => json,
                Err(err) => {
                    eprintln!("[dictate] warning: JSON serialization failed: {err}");
                    result.text.clone()
                }
            }
        }
        Some(ResponseFormat::Text) | None => {
            // Plain text (default): preserves existing behavior exactly.
            result.text.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_result(text: &str) -> TranscriptionResult {
        TranscriptionResult {
            text: text.to_string(),
            segments: None,
            words: None,
        }
    }

    fn make_verbose_result(text: &str, seg_id: u32, start: f64, end: f64) -> TranscriptionResult {
        TranscriptionResult {
            text: text.to_string(),
            segments: Some(vec![Segment {
                id: seg_id,
                start,
                end,
                text: text.to_string(),
                words: vec![],
            }]),
            words: Some(vec![Word {
                word: text.to_string(),
                start,
                end,
            }]),
        }
    }

    // ── merge_results tests ──────────────────────────────────────────

    #[test]
    fn merge_empty_produces_empty() {
        let merged = merge_results(vec![]);
        assert_eq!(merged.text, "");
        assert!(merged.segments.is_none());
        assert!(merged.words.is_none());
    }

    #[test]
    fn merge_single_returns_as_is() {
        let result = make_verbose_result("hello", 0, 0.0, 1.0);
        let merged = merge_results(vec![result.clone()]);
        assert_eq!(merged, result);
    }

    #[test]
    fn merge_multiple_joins_text_with_spaces() {
        let merged = merge_results(vec![make_result("hello"), make_result("world")]);
        assert_eq!(merged.text, "hello world");
    }

    #[test]
    fn merge_skips_empty_text_chunks() {
        let merged = merge_results(vec![
            make_result("hello"),
            make_result(""),
            make_result("world"),
        ]);
        assert_eq!(merged.text, "hello world");
    }

    #[test]
    fn merge_reindexes_segment_ids() {
        let merged = merge_results(vec![
            make_verbose_result("first", 0, 0.0, 1.0),
            make_verbose_result("second", 0, 1.0, 2.0),
        ]);
        let segments = merged.segments.unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].id, 0);
        assert_eq!(segments[1].id, 1);
    }

    #[test]
    fn merge_concatenates_words() {
        let merged = merge_results(vec![
            make_verbose_result("hello", 0, 0.0, 0.5),
            make_verbose_result("world", 0, 0.5, 1.0),
        ]);
        let words = merged.words.unwrap();
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].word, "hello");
        assert_eq!(words[1].word, "world");
    }

    #[test]
    fn merge_partial_segments_preserves_none() {
        // First chunk has segments, second does not.
        let r1 = make_verbose_result("hello", 0, 0.0, 1.0);
        let r2 = make_result("world");
        let merged = merge_results(vec![r1, r2]);
        // At least one chunk had segments, so merged has segments.
        assert!(merged.segments.is_some());
        assert_eq!(merged.segments.unwrap().len(), 1);
    }

    // ── auto_upgrade_format tests ────────────────────────────────────

    #[test]
    fn no_timestamps_preserves_format() {
        assert_eq!(auto_upgrade_format(None, None), None);
        assert_eq!(
            auto_upgrade_format(Some(ResponseFormat::Json), None),
            Some(ResponseFormat::Json)
        );
        let empty: Vec<TimestampGranularity> = vec![];
        assert_eq!(
            auto_upgrade_format(Some(ResponseFormat::Text), Some(&empty)),
            Some(ResponseFormat::Text)
        );
    }

    #[test]
    fn timestamps_upgrade_none_format() {
        let ts = vec![TimestampGranularity::Word];
        assert_eq!(
            auto_upgrade_format(None, Some(&ts)),
            Some(ResponseFormat::VerboseJson)
        );
    }

    #[test]
    fn timestamps_override_explicit_non_verbose_format() {
        let ts = vec![TimestampGranularity::Segment];
        assert_eq!(
            auto_upgrade_format(Some(ResponseFormat::Json), Some(&ts)),
            Some(ResponseFormat::VerboseJson)
        );
    }

    #[test]
    fn timestamps_keep_verbose_json() {
        let ts = vec![TimestampGranularity::Word];
        assert_eq!(
            auto_upgrade_format(Some(ResponseFormat::VerboseJson), Some(&ts)),
            Some(ResponseFormat::VerboseJson)
        );
    }

    // ── offset_timestamps tests ─────────────────────────────────────

    #[test]
    #[allow(clippy::float_cmp)]
    fn offset_timestamps_shifts_segments_and_words() {
        let mut result = TranscriptionResult {
            text: "hello world".to_string(),
            segments: Some(vec![Segment {
                id: 0,
                start: 0.0,
                end: 5.0,
                text: "hello world".to_string(),
                words: vec![
                    Word {
                        word: "hello".to_string(),
                        start: 0.0,
                        end: 2.5,
                    },
                    Word {
                        word: "world".to_string(),
                        start: 2.5,
                        end: 5.0,
                    },
                ],
            }]),
            words: Some(vec![
                Word {
                    word: "hello".to_string(),
                    start: 0.0,
                    end: 2.5,
                },
                Word {
                    word: "world".to_string(),
                    start: 2.5,
                    end: 5.0,
                },
            ]),
        };

        offset_timestamps(&mut result, 88.0);

        let segments = result.segments.unwrap();
        assert_eq!(segments[0].start, 88.0);
        assert_eq!(segments[0].end, 93.0);
        assert_eq!(segments[0].words[0].start, 88.0);
        assert_eq!(segments[0].words[0].end, 90.5);
        assert_eq!(segments[0].words[1].start, 90.5);
        assert_eq!(segments[0].words[1].end, 93.0);

        let words = result.words.unwrap();
        assert_eq!(words[0].start, 88.0);
        assert_eq!(words[0].end, 90.5);
        assert_eq!(words[1].start, 90.5);
        assert_eq!(words[1].end, 93.0);
    }

    #[test]
    fn clipboard_error_converts_to_record_error() {
        let clip_err = ClipboardError::NoDisplay;
        let record_err: RecordError = clip_err.into();
        assert!(
            matches!(record_err, RecordError::Clipboard(_)),
            "expected RecordError::Clipboard variant"
        );
        let msg = record_err.to_string();
        assert!(msg.contains("clipboard error"));
        assert!(msg.contains("no display environment"));
    }

    // ── build_effective_prompt tests ──────────────────────────────────

    #[test]
    fn effective_prompt_both_hint_and_user() {
        let result = build_effective_prompt(Some("use formal English"), Some("Claude, Tin"));
        assert_eq!(result, Some("Claude, Tin. use formal English".to_string()));
    }

    #[test]
    fn effective_prompt_hint_only() {
        let result = build_effective_prompt(None, Some("Claude, Tin"));
        assert_eq!(result, Some("Claude, Tin".to_string()));
    }

    #[test]
    fn effective_prompt_user_only() {
        let result = build_effective_prompt(Some("use formal English"), None);
        assert_eq!(result, Some("use formal English".to_string()));
    }

    #[test]
    fn effective_prompt_neither() {
        let result = build_effective_prompt(None, None);
        assert!(result.is_none());
    }

    #[test]
    fn offset_timestamps_noop_on_text_only_result() {
        let mut result = make_result("hello");
        offset_timestamps(&mut result, 100.0);
        assert!(result.segments.is_none());
        assert!(result.words.is_none());
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn merge_multi_chunk_preserves_offset_timestamps() {
        // Simulate two chunks where timestamps have already been offset
        // (as transcribe_chunks would do after fix 3c).
        //
        // Chunk 0: 90s, no overlap → offset = 0.0
        // Chunk 1: 90s, 2s overlap → offset = 90 - 2 = 88.0
        let chunk0 = make_verbose_result("first chunk", 0, 0.0, 90.0);
        let mut chunk1 = make_verbose_result("second chunk", 0, 0.0, 90.0);
        offset_timestamps(&mut chunk1, 88.0);

        let merged = merge_results(vec![chunk0, chunk1]);

        let segments = merged.segments.unwrap();
        assert_eq!(segments.len(), 2);
        // Chunk 0 timestamps unchanged
        assert_eq!(segments[0].start, 0.0);
        assert_eq!(segments[0].end, 90.0);
        // Chunk 1 timestamps offset by 88.0
        assert_eq!(segments[1].start, 88.0);
        assert_eq!(segments[1].end, 178.0);

        // Verify monotonic ordering across chunks
        assert!(segments[0].end <= segments[1].start + 2.0); // Allow overlap window

        let words = merged.words.unwrap();
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].start, 0.0);
        assert_eq!(words[1].start, 88.0);
    }
}
