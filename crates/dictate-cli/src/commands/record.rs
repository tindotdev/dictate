use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dictate_core::token::{MAX_PROMPT_TOKENS, estimate_token_count};
use dictate_core::{
    AudioChunk, AudioError, AudioReceiver, AudioRecorder, ChunkerConfig, ClipboardError,
    DEFAULT_POST_PROCESS_MODEL, DeviceSelection, Dictionary, DictionaryStore, GroqPostProcessor,
    GroqProvider, ModelId, PipelineConfig, PostProcessOutcome, PostProcessor, ProgressiveChunker,
    RecorderConfig, RecorderStopHandle, RecvResult, RequestPolicies, ResponseFormat,
    SavedRecording, SavedRecordingManifest, SavedRecordingStore, Segment, TimestampGranularity,
    TranscriptionError, TranscriptionPipeline, TranscriptionResult, Vocabulary, VocabularyStore,
    WhisperModel, Word, format_hint_within_budget, merge_prompt_hints,
};
use thiserror::Error;

const RECV_TIMEOUT: Duration = Duration::from_millis(100);
const QUIESCENT_TIMEOUTS: u8 = 3;
const DEFAULT_RETRY_CHUNK_OVERLAP_SAMPLES: usize =
    2 * dictate_core::TRANSCRIPTION_SAMPLE_RATE as usize;

/// Environment variable for the Groq API key.
const GROQ_API_KEY_VAR: &str = "GROQ_API_KEY";

/// Environment variable for an optional Groq API base URL override.
const GROQ_BASE_URL_VAR: &str = "GROQ_BASE_URL";

/// Environment variable for an optional post-processing chat API base URL override.
const GROQ_CHAT_BASE_URL_VAR: &str = "GROQ_CHAT_BASE_URL";

#[derive(Debug, Error)]
pub enum RecordError {
    #[error("audio error: {0}")]
    Audio(#[from] AudioError),

    #[error("transcription error: {0}")]
    Transcription(#[from] TranscriptionError),

    #[error("clipboard error: {0}")]
    Clipboard(#[from] ClipboardError),

    #[error("saved recording error: {0}")]
    SavedRecording(#[from] dictate_core::SavedRecordingError),

    #[error("transcription worker disconnected unexpectedly")]
    TranscriptionWorkerDisconnected,

    #[error("post-process worker disconnected unexpectedly")]
    PostProcessWorkerDisconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionState {
    Completed,
    InterruptedDuringTranscription,
    InterruptedDuringPostProcess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunMode {
    Record,
    Retry,
}

// ══════════════════════════════════════════════════════════════════════════════
//  Builder: RecordOptions
// ══════════════════════════════════════════════════════════════════════════════

/// Configuration options for audio recording and transcription.
/// Use the builder pattern to construct with only the options you need.
#[derive(Default, Debug)]
#[allow(clippy::struct_excessive_bools)]
pub struct RecordOptions {
    device: Option<String>,
    base_url: Option<String>,
    language: Option<String>,
    prompt: Option<String>,
    response_format: Option<ResponseFormat>,
    transcription_model: Option<WhisperModel>,
    temperature: Option<f32>,
    timestamp_granularities: Option<Vec<TimestampGranularity>>,
    stdout: bool,
    no_clipboard: bool,
    post_process_override: Option<bool>,
    post_process_model: Option<ModelId>,
    post_process_base_url: Option<String>,
    save_last_audio: bool,
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

    /// Set the Whisper transcription model (`LargeV3Turbo` or `LargeV3`).
    pub const fn transcription_model(mut self, model: WhisperModel) -> Self {
        self.transcription_model = Some(model);
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

    /// Enable LLM post-processing for punctuation and formatting cleanup.
    pub const fn post_process(mut self, enabled: bool) -> Self {
        self.post_process_override = Some(enabled);
        self
    }

    /// Set the model for post-processing.
    pub fn post_process_model(mut self, model: ModelId) -> Self {
        self.post_process_model = Some(model);
        self
    }

    /// Override the post-processing chat API base URL.
    pub fn post_process_base_url(mut self, url: impl Into<String>) -> Self {
        self.post_process_base_url = Some(url.into());
        self
    }

    /// Persist the captured audio locally for later reuse with `dictate retry`.
    pub const fn save_last_audio(mut self, enabled: bool) -> Self {
        self.save_last_audio = enabled;
        self
    }
}

// ══════════════════════════════════════════════════════════════════════════════
//  Main Entry Point
// ══════════════════════════════════════════════════════════════════════════════

pub fn run(options: &RecordOptions) -> Result<(), RecordError> {
    let resolved = resolve_run_config(options, None, RunMode::Record)?;

    // Fail fast if clipboard is requested but unavailable (missing tool / headless)
    if !options.stdout && !options.no_clipboard {
        dictate_core::check_clipboard_available()?;
    }

    // Set up interrupt handling
    let running = Arc::new(AtomicBool::new(true));
    let active_recording_stop = Arc::new(Mutex::new(None));
    install_stop_handlers(Arc::clone(&running), &active_recording_stop);

    // Record audio chunks
    let session = capture_recording_session(
        options.device.as_deref(),
        options.save_last_audio,
        &running,
        &active_recording_stop,
    )?;

    if session.chunks.is_empty() {
        eprintln!("[dictate] no audio captured");
        return Ok(());
    }

    maybe_save_last_audio(options, &session, &resolved);

    running.store(true, Ordering::Relaxed); // Re-arm for transcription phase
    process_transcription_session(options, &resolved, session.chunks, &running)
}

/// Reuse the last saved recording and rerun transcription/post-processing.
pub fn run_retry(options: &RecordOptions) -> Result<(), RecordError> {
    if !options.stdout && !options.no_clipboard {
        dictate_core::check_clipboard_available()?;
    }

    let saved = SavedRecordingStore::open()?.load()?;
    let defaults = saved_defaults_from_manifest(&saved.manifest)?;
    let resolved = resolve_run_config(options, Some(&defaults), RunMode::Retry)?;
    let session = rechunk_saved_audio(saved.samples, saved.manifest.chunk_target_duration_secs);

    if session.chunks.is_empty() {
        eprintln!("[dictate] saved recording contains no audio");
        return Ok(());
    }

    eprintln!("[dictate] reusing saved audio from last recording...");
    let running = Arc::new(AtomicBool::new(true));
    let active_recording_stop = Arc::new(Mutex::new(None));
    install_stop_handlers(Arc::clone(&running), &active_recording_stop);
    process_transcription_session(options, &resolved, session.chunks, &running)
}

#[derive(Debug, Clone)]
struct ResolvedRunConfig {
    effective_format: Option<ResponseFormat>,
    effective_post_process: bool,
    pipeline_config: PipelineConfig,
    pipeline: Arc<TranscriptionPipeline>,
}

#[derive(Debug, Clone)]
struct SavedDefaults {
    output_format: Option<ResponseFormat>,
    pipeline_config: PipelineConfig,
}

#[derive(Debug, Clone)]
struct CapturedSession {
    samples: Vec<f32>,
    chunks: Vec<AudioChunk>,
    chunker_config: ChunkerConfig,
}

fn process_transcription_session(
    options: &RecordOptions,
    resolved: &ResolvedRunConfig,
    chunks: Vec<AudioChunk>,
    running: &AtomicBool,
) -> Result<(), RecordError> {
    let (results, interrupted) = transcribe_chunks(&resolved.pipeline, chunks, running)?;
    let mut state = if interrupted {
        SessionState::InterruptedDuringTranscription
    } else {
        SessionState::Completed
    };

    // Output results
    let merged = merge_results(results);
    let post_process_requested = resolved.effective_post_process;

    let (merged, post_process_outcome, post_process_requested) =
        if post_process_requested && !merged.text.is_empty() && state == SessionState::Completed {
            let model = resolved
                .pipeline_config
                .post_process_model
                .as_ref()
                .map_or(DEFAULT_POST_PROCESS_MODEL, dictate_core::ModelId::as_str);
            eprintln!("[dictate] post-processing with {model}...");

            if let Some((merged, outcome)) = post_process_result_interruptible(
                Arc::clone(&resolved.pipeline),
                merged.clone(),
                running,
            )? {
                (merged, outcome, true)
            } else {
                eprintln!("[dictate] interrupted, skipping post-processing");
                state = SessionState::InterruptedDuringPostProcess;
                (merged, PostProcessOutcome::NotConfigured, false)
            }
        } else {
            (merged, PostProcessOutcome::NotConfigured, false)
        };

    output_result(
        &merged,
        resolved.effective_format,
        post_process_requested,
        state,
        options,
        post_process_outcome,
    );

    Ok(())
}

// ══════════════════════════════════════════════════════════════════════════════
//  Helper Functions
// ══════════════════════════════════════════════════════════════════════════════

/// Resolve the effective transcription configuration and build a pipeline.
fn resolve_run_config(
    options: &RecordOptions,
    defaults: Option<&SavedDefaults>,
    run_mode: RunMode,
) -> Result<ResolvedRunConfig, RecordError> {
    let timestamp_granularities = resolve_timestamp_granularities(options, defaults);

    // Auto-upgrade format when timestamps are requested
    let effective_format = auto_upgrade_format(
        options
            .response_format
            .or_else(|| defaults.and_then(|saved| saved.output_format)),
        Some(&timestamp_granularities),
    );

    // Validate API key upfront (fail fast)
    let api_key =
        std::env::var(GROQ_API_KEY_VAR).map_err(|_| TranscriptionError::MissingApiKey {
            env_var: GROQ_API_KEY_VAR,
        })?;

    let base_url = options
        .base_url
        .clone()
        .or_else(|| defaults.and_then(|saved| saved.pipeline_config.base_url.clone()))
        .or_else(|| std::env::var(GROQ_BASE_URL_VAR).ok());

    let post_process_base_url = options
        .post_process_base_url
        .clone()
        .or_else(|| defaults.and_then(|saved| saved.pipeline_config.post_process_base_url.clone()))
        .or_else(|| std::env::var(GROQ_CHAT_BASE_URL_VAR).ok());

    // Load prompt hints (dictionary + vocabulary) for prompt injection.
    // Best-effort: warn and continue on store errors.
    let effective_prompt = if options.prompt.is_some() || defaults.is_none() {
        load_prompt_hints(options.prompt.as_deref())
    } else {
        defaults.and_then(|saved| saved.pipeline_config.prompt.clone())
    };

    let inherited_response_format = defaults.map_or(ResponseFormat::Json, |saved| {
        saved.pipeline_config.response_format
    });
    let inherited_post_process = defaults.is_some_and(|saved| saved.pipeline_config.post_process);
    let effective_post_process =
        resolve_post_process_enabled(options.post_process_override, inherited_post_process);

    let config = PipelineConfig {
        base_url,
        language: options
            .language
            .clone()
            .or_else(|| defaults.and_then(|saved| saved.pipeline_config.language.clone())),
        prompt: effective_prompt,
        response_format: effective_format.unwrap_or(inherited_response_format),
        transcription_model: options
            .transcription_model
            .or_else(|| defaults.and_then(|saved| saved.pipeline_config.transcription_model)),
        temperature: options
            .temperature
            .or_else(|| defaults.and_then(|saved| saved.pipeline_config.temperature)),
        timestamp_granularities,
        post_process: effective_post_process,
        post_process_model: options.post_process_model.clone().or_else(|| {
            defaults.and_then(|saved| saved.pipeline_config.post_process_model.clone())
        }),
        post_process_base_url,
        request_policies: request_policies_for_mode(run_mode),
    };

    let pipeline = build_pipeline(api_key, &config);

    Ok(ResolvedRunConfig {
        effective_format,
        effective_post_process,
        pipeline_config: config,
        pipeline,
    })
}

const fn request_policies_for_mode(run_mode: RunMode) -> RequestPolicies {
    match run_mode {
        RunMode::Record => RequestPolicies::interactive(),
        RunMode::Retry => RequestPolicies::persistent(),
    }
}

fn resolve_timestamp_granularities(
    options: &RecordOptions,
    defaults: Option<&SavedDefaults>,
) -> Vec<TimestampGranularity> {
    if let Some(granularities) = options.timestamp_granularities.clone() {
        return granularities;
    }

    match options.response_format {
        Some(ResponseFormat::Json | ResponseFormat::Text) => Vec::new(),
        Some(ResponseFormat::VerboseJson) | None => defaults
            .map(|saved| saved.pipeline_config.timestamp_granularities.clone())
            .unwrap_or_default(),
    }
}

fn build_pipeline(api_key: String, config: &PipelineConfig) -> Arc<TranscriptionPipeline> {
    let mut pipeline = TranscriptionPipeline::new(Box::new(GroqProvider), api_key, config.clone());

    if config.post_process {
        let pp: Box<dyn PostProcessor> = Box::new(GroqPostProcessor);
        pipeline = pipeline.with_post_processor(pp);
    }

    Arc::new(pipeline)
}

const fn resolve_post_process_enabled(override_value: Option<bool>, inherited_value: bool) -> bool {
    match override_value {
        Some(enabled) => enabled,
        None => inherited_value,
    }
}

fn saved_defaults_from_manifest(
    manifest: &SavedRecordingManifest,
) -> Result<SavedDefaults, RecordError> {
    Ok(SavedDefaults {
        output_format: manifest.output_format()?,
        pipeline_config: manifest.pipeline.to_pipeline_config()?,
    })
}

/// Record audio from the specified device and collect chunks.
fn capture_recording_session(
    device: Option<&str>,
    collect_samples: bool,
    running: &AtomicBool,
    active_recording_stop: &Mutex<Option<RecorderStopHandle>>,
) -> Result<CapturedSession, RecordError> {
    eprintln!("[dictate] recording... press Enter to stop (Ctrl+C also works)");

    let mut config = RecorderConfig::default();
    if let Some(query) = device {
        config.device = DeviceSelection::Query(query.to_string());
    }

    let (mut recorder, mut rx, info) = AudioRecorder::start(config)?;
    set_active_recording_stop(active_recording_stop, Some(recorder.stop_handle()));
    eprintln!(
        "[dictate] device: {} ({} Hz, {}ch) -> resampling to {} Hz mono",
        info.device_name,
        info.device_sample_rate_hz,
        info.device_channels,
        info.target_sample_rate_hz
    );

    // Collect audio chunks
    let chunker_config = ChunkerConfig::default();
    let mut chunker = ProgressiveChunker::new(chunker_config.clone());
    let mut chunks: Vec<AudioChunk> = Vec::new();
    let mut samples = collect_samples.then(Vec::new);

    consume_until_stopped(&mut rx, running, &mut chunker, &mut samples, &mut chunks);
    set_active_recording_stop(active_recording_stop, None);

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

    drain_remaining(&mut rx, &mut chunker, &mut samples, &mut chunks);

    let tail = recorder.take_flushed_tail();
    if !tail.is_empty() {
        push_samples_and_collect(&mut chunker, &tail, &mut samples, &mut chunks);
    }

    if let Some(chunk) = chunker.flush() {
        eprintln!(
            "[dictate] final chunk {} ready ({:.1}s)",
            chunk.index,
            chunk.duration_secs()
        );
        chunks.push(chunk);
    }

    Ok(CapturedSession {
        samples: samples.unwrap_or_default(),
        chunks,
        chunker_config,
    })
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
    post_process_requested: bool,
    state: SessionState,
    options: &RecordOptions,
    post_process_outcome: PostProcessOutcome,
) {
    if merged.text.is_empty() && state == SessionState::Completed {
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
    let formatted = format_to_string(merged, format, post_process_requested, post_process_outcome);

    if use_clipboard {
        // Default behavior: copy to clipboard
        match dictate_core::clipboard::copy_to_clipboard(&formatted) {
            Ok(()) => {
                print_completion_message(state, merged.text.len(), true);
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
        print_completion_message(state, merged.text.len(), false);
    }
}

fn print_completion_message(state: SessionState, char_count: usize, copied_to_clipboard: bool) {
    let suffix = if copied_to_clipboard {
        ", copied to clipboard"
    } else {
        ""
    };

    match state {
        SessionState::Completed => {
            eprintln!("[dictate] done ({char_count} chars{suffix})");
        }
        SessionState::InterruptedDuringTranscription => {
            eprintln!("[dictate] interrupted (partial transcript: {char_count} chars{suffix})");
        }
        SessionState::InterruptedDuringPostProcess => {
            eprintln!(
                "[dictate] interrupted during post-processing (raw transcript: {char_count} chars{suffix})"
            );
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

fn post_process_result_interruptible(
    pipeline: Arc<TranscriptionPipeline>,
    merged: TranscriptionResult,
    running: &AtomicBool,
) -> Result<Option<(TranscriptionResult, PostProcessOutcome)>, RecordError> {
    let (tx, rx) = mpsc::sync_channel(1);

    std::thread::spawn(move || {
        let result = pipeline.post_process_result_with_outcome(merged);
        if tx.send(result).is_err() {
            eprintln!("[dictate] warning: post-process result dropped (receiver disconnected)");
        }
    });

    loop {
        if !running.load(Ordering::Relaxed) {
            return Ok(None);
        }

        match rx.recv_timeout(RECV_TIMEOUT) {
            Ok(result) => return Ok(Some(result)),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(RecordError::PostProcessWorkerDisconnected);
            }
        }
    }
}

fn install_stop_handlers(
    running: Arc<AtomicBool>,
    active_recording_stop: &Arc<Mutex<Option<RecorderStopHandle>>>,
) {
    let running_ctrlc = Arc::clone(&running);
    let active_recording_stop_ctrlc = Arc::clone(active_recording_stop);
    if let Err(err) = ctrlc::set_handler(move || {
        // First press: cooperative shutdown. Second press: force exit.
        if handle_stop_signal(&running_ctrlc, || {
            request_active_recording_stop(&active_recording_stop_ctrlc);
        }) {
            eprintln!("\n[dictate] forced exit");
            std::process::exit(130);
        }
    }) {
        eprintln!("[dictate] warning: failed to set Ctrl+C handler: {err}");
    }

    // Only listen for Enter when stdin is interactive (not piped / closed).
    if std::io::stdin().is_terminal() {
        let active_recording_stop_stdin = Arc::clone(active_recording_stop);
        std::thread::spawn(move || {
            let mut input = String::new();
            let _ = std::io::stdin().read_line(&mut input);
            let _ = handle_stop_signal(&running, || {
                request_active_recording_stop(&active_recording_stop_stdin);
            });
        });
    }
}

fn handle_stop_signal<F>(running: &AtomicBool, request_recording_stop: F) -> bool
where
    F: FnOnce(),
{
    if !running.swap(false, Ordering::Relaxed) {
        return true;
    }

    request_recording_stop();
    false
}

fn set_active_recording_stop(
    active_recording_stop: &Mutex<Option<RecorderStopHandle>>,
    stop_handle: Option<RecorderStopHandle>,
) {
    match active_recording_stop.lock() {
        Ok(mut guard) => *guard = stop_handle,
        Err(poisoned) => {
            let mut guard = poisoned.into_inner();
            *guard = stop_handle;
        }
    }
}

fn request_active_recording_stop(active_recording_stop: &Mutex<Option<RecorderStopHandle>>) {
    let stop_handle = match active_recording_stop.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };

    if let Some(stop_handle) = stop_handle
        && let Err(err) = stop_handle.request_stop()
    {
        eprintln!("[dictate] warning: failed to stop recording promptly: {err}");
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

fn push_samples_and_collect(
    chunker: &mut ProgressiveChunker,
    samples: &[f32],
    all_samples: &mut Option<Vec<f32>>,
    chunks: &mut Vec<AudioChunk>,
) {
    if let Some(all_samples) = all_samples.as_mut() {
        all_samples.extend_from_slice(samples);
    }
    push_and_collect(chunker, samples, chunks);
}

fn consume_until_stopped(
    rx: &mut AudioReceiver,
    running: &AtomicBool,
    chunker: &mut ProgressiveChunker,
    all_samples: &mut Option<Vec<f32>>,
    chunks: &mut Vec<AudioChunk>,
) {
    while running.load(Ordering::Relaxed) {
        match rx.recv_timeout(RECV_TIMEOUT) {
            RecvResult::Data(samples) => {
                push_samples_and_collect(chunker, samples, all_samples, chunks);
            }
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
    all_samples: &mut Option<Vec<f32>>,
    chunks: &mut Vec<AudioChunk>,
) {
    let mut consecutive_timeouts = 0_u8;

    loop {
        while let Some(samples) = rx.try_recv() {
            consecutive_timeouts = 0;
            push_samples_and_collect(chunker, samples, all_samples, chunks);
        }

        if rx.is_disconnected() {
            break;
        }

        match rx.recv_timeout(RECV_TIMEOUT) {
            RecvResult::Data(samples) => {
                consecutive_timeouts = 0;
                push_samples_and_collect(chunker, samples, all_samples, chunks);
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

fn maybe_save_last_audio(
    options: &RecordOptions,
    session: &CapturedSession,
    resolved: &ResolvedRunConfig,
) {
    if !options.save_last_audio || session.samples.is_empty() {
        return;
    }

    let recording = SavedRecording {
        manifest: SavedRecordingManifest::new(
            session.samples.len(),
            session.chunker_config.target_duration_secs,
            resolved.effective_format,
            &resolved.pipeline_config,
        ),
        samples: session.samples.clone(),
    };

    match SavedRecordingStore::open().and_then(|store| store.save(&recording)) {
        Ok(()) => eprintln!("[dictate] saved audio for later reuse"),
        Err(err) => eprintln!("[dictate] warning: could not save audio for retry: {err}"),
    }
}

fn rechunk_saved_audio(samples: Vec<f32>, target_duration_secs: u64) -> CapturedSession {
    let mut chunks = Vec::new();
    let target_samples = usize::try_from(target_duration_secs)
        .ok()
        .and_then(|secs| secs.checked_mul(dictate_core::TRANSCRIPTION_SAMPLE_RATE as usize))
        .unwrap_or(usize::MAX);

    if target_samples == 0 || samples.is_empty() {
        return CapturedSession {
            samples,
            chunks,
            chunker_config: ChunkerConfig {
                target_duration_secs,
            },
        };
    }

    let mut index = 0_usize;
    let mut start = 0_usize;
    while start < samples.len() {
        let end = (start + target_samples).min(samples.len());
        let has_leading_overlap = index > 0;
        chunks.push(AudioChunk {
            index,
            samples: samples[start..end].to_vec(),
            has_leading_overlap,
        });

        if end == samples.len() {
            break;
        }

        start = end.saturating_sub(DEFAULT_RETRY_CHUNK_OVERLAP_SAMPLES);
        index += 1;
    }

    CapturedSession {
        samples,
        chunks,
        chunker_config: ChunkerConfig {
            target_duration_secs,
        },
    }
}

/// Load dictionary and vocabulary hints, then compose the effective prompt.
///
/// This is best-effort: if either store cannot be loaded, a warning is printed
/// and prompt composition continues with the available source(s).
fn load_prompt_hints(user_prompt: Option<&str>) -> Option<String> {
    let dictionary = load_dictionary_best_effort();
    let vocabulary = load_vocabulary_best_effort();

    let merged_hints = merge_prompt_hints(&dictionary, &vocabulary);
    if merged_hints.is_empty() {
        return user_prompt.map(String::from);
    }

    // Calculate remaining token budget after the user's prompt.
    // Reserve 2 tokens for the ". " joiner inserted by build_effective_prompt
    // when both prompt hints and a user prompt are present.
    let user_tokens = user_prompt.map_or(0, estimate_token_count);
    let joiner_cost = if user_prompt.is_some() { 2 } else { 0 };
    let remaining_budget = MAX_PROMPT_TOKENS.saturating_sub(user_tokens + joiner_cost);

    let hint = format_hint_within_budget(merged_hints.iter().map(String::as_str), remaining_budget);

    if let Some(ref h) = hint {
        if h.included < h.total {
            eprintln!(
                "[dictate] prompt hints: using {}/{} entries (token limit)",
                h.included, h.total
            );
        } else {
            eprintln!(
                "[dictate] prompt hints loaded ({} {})",
                h.included,
                if h.included == 1 { "entry" } else { "entries" }
            );
        }
    }

    build_effective_prompt(user_prompt, hint.as_ref().map(|h| h.text.as_str()))
}

fn load_dictionary_best_effort() -> Dictionary {
    let store = match DictionaryStore::open() {
        Ok(s) => s,
        Err(err) => {
            eprintln!("[dictate] warning: could not open dictionary store: {err}");
            return Dictionary::new();
        }
    };

    match store.load() {
        Ok(d) => d,
        Err(err) => {
            eprintln!("[dictate] warning: could not load dictionary: {err}");
            Dictionary::new()
        }
    }
}

fn load_vocabulary_best_effort() -> Vocabulary {
    let store = match VocabularyStore::open() {
        Ok(s) => s,
        Err(err) => {
            eprintln!("[dictate] warning: could not open vocabulary store: {err}");
            return Vocabulary::new();
        }
    };

    match store.load() {
        Ok(v) => v,
        Err(err) => {
            eprintln!("[dictate] warning: could not load vocabulary: {err}");
            Vocabulary::new()
        }
    }
}

/// Compose prompt hint and user prompt into a single effective prompt.
///
/// Prompt hint comes first (primes vocabulary), then user prompt (style/context).
fn build_effective_prompt(user_prompt: Option<&str>, prompt_hint: Option<&str>) -> Option<String> {
    match (prompt_hint, user_prompt) {
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
fn format_to_string(
    result: &TranscriptionResult,
    format: Option<ResponseFormat>,
    post_process_requested: bool,
    post_process_outcome: PostProcessOutcome,
) -> String {
    match format {
        Some(ResponseFormat::VerboseJson) => {
            // Full structured JSON with segments and words.
            let mut payload = match serde_json::to_value(result) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("[dictate] warning: JSON serialization failed: {err}");
                    return result.text.clone();
                }
            };

            if let Some((post_processed, status)) =
                post_process_metadata(post_process_requested, post_process_outcome)
            {
                payload["post_processed"] = serde_json::Value::Bool(post_processed);
                payload["post_process_status"] = serde_json::Value::String(status.to_string());
            }

            match serde_json::to_string_pretty(&payload) {
                Ok(json) => json,
                Err(err) => {
                    eprintln!("[dictate] warning: JSON serialization failed: {err}");
                    result.text.clone()
                }
            }
        }
        Some(ResponseFormat::Json) => {
            // Simple JSON with text only.
            let mut payload = serde_json::json!({"text": result.text});
            if let Some((post_processed, status)) =
                post_process_metadata(post_process_requested, post_process_outcome)
            {
                payload["post_processed"] = serde_json::Value::Bool(post_processed);
                payload["post_process_status"] = serde_json::Value::String(status.to_string());
            }

            match serde_json::to_string_pretty(&payload) {
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

const fn post_process_metadata(
    post_process_requested: bool,
    post_process_outcome: PostProcessOutcome,
) -> Option<(bool, &'static str)> {
    if !post_process_requested {
        return None;
    }

    match post_process_outcome {
        PostProcessOutcome::Applied => Some((true, "applied")),
        PostProcessOutcome::FailedFallback => Some((false, "failed_fallback")),
        PostProcessOutcome::SkippedVerboseJson => Some((false, "skipped_verbose_json")),
        PostProcessOutcome::SkippedEmptyText => Some((false, "skipped_empty_text")),
        PostProcessOutcome::NotConfigured => Some((false, "not_configured")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::thread;

    fn make_result(text: &str) -> TranscriptionResult {
        TranscriptionResult {
            text: text.to_string(),
            segments: None,
            words: None,
        }
    }

    #[derive(Default)]
    struct StubProvider {
        response: String,
        calls: Arc<AtomicUsize>,
        stop_after_first: Option<Arc<AtomicBool>>,
    }

    impl StubProvider {
        fn new(response: impl Into<String>) -> Self {
            Self {
                response: response.into(),
                calls: Arc::new(AtomicUsize::new(0)),
                stop_after_first: None,
            }
        }

        fn with_stop_after_first(mut self, running: Arc<AtomicBool>) -> Self {
            self.stop_after_first = Some(running);
            self
        }
    }

    impl dictate_core::TranscriptionProvider for StubProvider {
        fn name(&self) -> &'static str {
            "stub"
        }

        fn transcribe(
            &self,
            _config: dictate_core::provider::TranscriptionConfig<'_>,
        ) -> Result<TranscriptionResult, TranscriptionError> {
            let call = self.calls.fetch_add(1, AtomicOrdering::SeqCst);
            if call == 0
                && let Some(running) = &self.stop_after_first
            {
                running.store(false, Ordering::Relaxed);
            }

            Ok(make_result(&self.response))
        }
    }

    struct CountingPostProcessor {
        calls: Arc<AtomicUsize>,
    }

    impl CountingPostProcessor {
        fn new(calls: Arc<AtomicUsize>) -> Self {
            Self { calls }
        }
    }

    impl PostProcessor for CountingPostProcessor {
        fn name(&self) -> &'static str {
            "counting"
        }

        fn process(
            &self,
            text: &str,
            _config: dictate_core::PostProcessConfig<'_>,
        ) -> Result<String, TranscriptionError> {
            self.calls.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(format!("{text}!"))
        }
    }

    struct BlockingPostProcessor {
        calls: Arc<AtomicUsize>,
        release: Arc<AtomicBool>,
    }

    impl BlockingPostProcessor {
        fn new(calls: Arc<AtomicUsize>, release: Arc<AtomicBool>) -> Self {
            Self { calls, release }
        }
    }

    impl PostProcessor for BlockingPostProcessor {
        fn name(&self) -> &'static str {
            "blocking"
        }

        fn process(
            &self,
            text: &str,
            _config: dictate_core::PostProcessConfig<'_>,
        ) -> Result<String, TranscriptionError> {
            self.calls.fetch_add(1, AtomicOrdering::SeqCst);
            while !self.release.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(10));
            }

            Ok(format!("{text}!"))
        }
    }

    fn test_pipeline(
        provider: StubProvider,
        post_processor: Option<Box<dyn PostProcessor>>,
    ) -> Arc<TranscriptionPipeline> {
        let config = PipelineConfig {
            post_process: post_processor.is_some(),
            ..PipelineConfig::default()
        };

        let mut pipeline =
            TranscriptionPipeline::new(Box::new(provider), "test-key".into(), config);
        if let Some(post_processor) = post_processor {
            pipeline = pipeline.with_post_processor(post_processor);
        }

        Arc::new(pipeline)
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

    fn assert_f64_eq(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < f64::EPSILON);
    }

    #[allow(
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss
    )]
    fn test_chunk(index: usize, duration_secs: f32) -> AudioChunk {
        let num_samples = (dictate_core::TRANSCRIPTION_SAMPLE_RATE as f32 * duration_secs) as usize;
        AudioChunk {
            index,
            samples: vec![0.0; num_samples],
            has_leading_overlap: index > 0,
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

    // ── resolve_timestamp_granularities tests ──────────────────────

    fn saved_defaults_with_timestamps(
        timestamp_granularities: Vec<TimestampGranularity>,
    ) -> SavedDefaults {
        SavedDefaults {
            output_format: Some(ResponseFormat::VerboseJson),
            pipeline_config: PipelineConfig {
                timestamp_granularities,
                ..PipelineConfig::default()
            },
        }
    }

    #[test]
    fn explicit_retry_format_clears_inherited_timestamps() {
        let options = RecordOptions::new().response_format(ResponseFormat::Json);
        let defaults = saved_defaults_with_timestamps(vec![TimestampGranularity::Word]);

        assert_eq!(
            resolve_timestamp_granularities(&options, Some(&defaults)),
            Vec::<TimestampGranularity>::new()
        );
    }

    #[test]
    fn explicit_retry_timestamps_override_saved_timestamps() {
        let options = RecordOptions::new()
            .response_format(ResponseFormat::Json)
            .timestamp_granularities(vec![TimestampGranularity::Segment]);
        let defaults = saved_defaults_with_timestamps(vec![TimestampGranularity::Word]);

        assert_eq!(
            resolve_timestamp_granularities(&options, Some(&defaults)),
            vec![TimestampGranularity::Segment]
        );
    }

    #[test]
    fn verbose_json_without_retry_timestamps_keeps_saved_timestamps() {
        let options = RecordOptions::new().response_format(ResponseFormat::VerboseJson);
        let defaults = saved_defaults_with_timestamps(vec![TimestampGranularity::Word]);

        assert_eq!(
            resolve_timestamp_granularities(&options, Some(&defaults)),
            vec![TimestampGranularity::Word]
        );
    }

    #[test]
    fn record_mode_uses_interactive_request_policies() {
        assert_eq!(
            request_policies_for_mode(RunMode::Record),
            RequestPolicies::interactive()
        );
    }

    #[test]
    fn retry_mode_uses_persistent_request_policies() {
        assert_eq!(
            request_policies_for_mode(RunMode::Retry),
            RequestPolicies::persistent()
        );
    }

    // ── offset_timestamps tests ─────────────────────────────────────

    #[test]
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
        assert_f64_eq(segments[0].start, 88.0);
        assert_f64_eq(segments[0].end, 93.0);
        assert_f64_eq(segments[0].words[0].start, 88.0);
        assert_f64_eq(segments[0].words[0].end, 90.5);
        assert_f64_eq(segments[0].words[1].start, 90.5);
        assert_f64_eq(segments[0].words[1].end, 93.0);

        let words = result.words.unwrap();
        assert_f64_eq(words[0].start, 88.0);
        assert_f64_eq(words[0].end, 90.5);
        assert_f64_eq(words[1].start, 90.5);
        assert_f64_eq(words[1].end, 93.0);
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

    #[test]
    fn push_samples_and_collect_appends_when_enabled() {
        let mut chunker = ProgressiveChunker::new(ChunkerConfig::default());
        let input = vec![0.25_f32, -0.5, 0.75];
        let mut all_samples = Some(Vec::new());
        let mut chunks = Vec::new();

        push_samples_and_collect(&mut chunker, &input, &mut all_samples, &mut chunks);

        assert_eq!(all_samples.unwrap(), input);
        assert!(chunks.is_empty());
    }

    #[test]
    fn push_samples_and_collect_skips_buffer_when_disabled() {
        let mut chunker = ProgressiveChunker::new(ChunkerConfig::default());
        let input = vec![0.25_f32, -0.5, 0.75];
        let mut all_samples = None;
        let mut chunks = Vec::new();

        push_samples_and_collect(&mut chunker, &input, &mut all_samples, &mut chunks);

        assert!(all_samples.is_none());
        assert!(chunks.is_empty());
    }

    #[test]
    fn interrupted_after_partial_transcription_returns_partial_results() {
        let running = Arc::new(AtomicBool::new(true));
        let provider = StubProvider::new("partial").with_stop_after_first(Arc::clone(&running));
        let pipeline = test_pipeline(provider, None);
        let chunks = vec![test_chunk(0, 1.0), test_chunk(1, 1.0)];

        let (results, interrupted) = transcribe_chunks(&pipeline, chunks, &running).unwrap();

        assert!(interrupted);
        assert_eq!(results, vec![make_result("partial")]);
    }

    #[test]
    fn interrupted_before_post_process_skips_post_processor() {
        let running = Arc::new(AtomicBool::new(true));
        let provider = StubProvider::new("partial").with_stop_after_first(Arc::clone(&running));
        let post_process_calls = Arc::new(AtomicUsize::new(0));
        let pipeline = test_pipeline(
            provider,
            Some(Box::new(CountingPostProcessor::new(Arc::clone(
                &post_process_calls,
            )))),
        );
        let resolved = ResolvedRunConfig {
            effective_format: None,
            effective_post_process: true,
            pipeline_config: PipelineConfig {
                post_process: true,
                ..PipelineConfig::default()
            },
            pipeline,
        };
        let options = RecordOptions::new().no_clipboard(true);

        process_transcription_session(
            &options,
            &resolved,
            vec![test_chunk(0, 1.0), test_chunk(1, 1.0)],
            &running,
        )
        .unwrap();

        assert_eq!(post_process_calls.load(AtomicOrdering::SeqCst), 0);
    }

    #[test]
    fn interrupted_during_post_process_returns_raw_text() {
        let running = Arc::new(AtomicBool::new(true));
        let release = Arc::new(AtomicBool::new(false));
        let post_process_calls = Arc::new(AtomicUsize::new(0));
        let pipeline = test_pipeline(
            StubProvider::new("raw text"),
            Some(Box::new(BlockingPostProcessor::new(
                Arc::clone(&post_process_calls),
                Arc::clone(&release),
            ))),
        );
        let running_for_cancel = Arc::clone(&running);

        let cancel_thread = thread::spawn(move || {
            thread::sleep(Duration::from_millis(150));
            running_for_cancel.store(false, Ordering::Relaxed);
        });

        let result =
            post_process_result_interruptible(pipeline, make_result("raw text"), &running).unwrap();

        release.store(true, Ordering::Relaxed);
        cancel_thread.join().unwrap();

        assert!(result.is_none());
        assert_eq!(post_process_calls.load(AtomicOrdering::SeqCst), 1);
    }

    #[test]
    fn first_stop_signal_requests_recording_stop() {
        let running = AtomicBool::new(true);
        let stop_requests = AtomicUsize::new(0);

        let forced_exit = handle_stop_signal(&running, || {
            stop_requests.fetch_add(1, AtomicOrdering::SeqCst);
        });

        assert!(!forced_exit);
        assert!(!running.load(Ordering::Relaxed));
        assert_eq!(stop_requests.load(AtomicOrdering::SeqCst), 1);
    }

    #[test]
    fn second_stop_signal_forces_exit_without_repeating_stop_request() {
        let running = AtomicBool::new(false);
        let stop_requests = AtomicUsize::new(0);

        let forced_exit = handle_stop_signal(&running, || {
            stop_requests.fetch_add(1, AtomicOrdering::SeqCst);
        });

        assert!(forced_exit);
        assert_eq!(stop_requests.load(AtomicOrdering::SeqCst), 0);
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
        assert_f64_eq(segments[0].start, 0.0);
        assert_f64_eq(segments[0].end, 90.0);
        // Chunk 1 timestamps offset by 88.0
        assert_f64_eq(segments[1].start, 88.0);
        assert_f64_eq(segments[1].end, 178.0);

        // Verify monotonic ordering across chunks
        assert!(segments[0].end <= segments[1].start + 2.0); // Allow overlap window

        let words = merged.words.unwrap();
        assert_eq!(words.len(), 2);
        assert_f64_eq(words[0].start, 0.0);
        assert_f64_eq(words[1].start, 88.0);
    }

    // ── post-process metadata tests ───────────────────────────────────

    #[test]
    fn json_output_omits_post_process_fields_when_not_requested() {
        let result = make_result("hello world");
        let formatted = format_to_string(
            &result,
            Some(ResponseFormat::Json),
            false,
            PostProcessOutcome::NotConfigured,
        );

        let json: serde_json::Value = serde_json::from_str(&formatted).unwrap();
        assert_eq!(json, serde_json::json!({"text": "hello world"}));
    }

    #[test]
    fn json_output_includes_post_process_failure_metadata() {
        let result = make_result("hello world");
        let formatted = format_to_string(
            &result,
            Some(ResponseFormat::Json),
            true,
            PostProcessOutcome::FailedFallback,
        );

        let json: serde_json::Value = serde_json::from_str(&formatted).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "text": "hello world",
                "post_processed": false,
                "post_process_status": "failed_fallback"
            })
        );
    }

    #[test]
    fn verbose_json_output_includes_post_process_metadata() {
        let result = make_verbose_result("hello", 0, 0.0, 1.0);
        let formatted = format_to_string(
            &result,
            Some(ResponseFormat::VerboseJson),
            true,
            PostProcessOutcome::SkippedVerboseJson,
        );

        let json: serde_json::Value = serde_json::from_str(&formatted).unwrap();
        assert_eq!(json["text"], "hello");
        assert_eq!(json["post_processed"], false);
        assert_eq!(json["post_process_status"], "skipped_verbose_json");
        assert!(json.get("segments").is_some());
        assert!(json.get("words").is_some());
    }

    #[test]
    fn post_process_override_inherits_when_unset() {
        assert!(resolve_post_process_enabled(None, true));
        assert!(!resolve_post_process_enabled(None, false));
    }

    #[test]
    fn post_process_override_can_force_disable_or_enable() {
        assert!(!resolve_post_process_enabled(Some(false), true));
        assert!(resolve_post_process_enabled(Some(true), false));
    }
}
