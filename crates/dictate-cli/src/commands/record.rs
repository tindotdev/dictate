use std::io::IsTerminal;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dictate_core::token::{MAX_PROMPT_TOKENS, estimate_token_count};
use dictate_core::{
    AudioChunk, AudioError, AudioReceiver, AudioRecorder, CancellationContext, CancellationError,
    ChunkerConfig, ClipboardError, DEFAULT_POST_PROCESS_MODEL, DeviceSelection, GroqPostProcessor,
    GroqProvider, ModelId, PipelineConfig, PostProcessOutcome, PostProcessor, ProgressiveChunker,
    RecorderConfig, RecorderStopHandle, RecvResult, RequestPolicies, ResponseFormat,
    SavedRecording, SavedRecordingManifest, SavedRecordingStore, Segment, TimestampGranularity,
    TranscriptionError, TranscriptionPipeline, TranscriptionResult, Vocabulary, VocabularyStore,
    WhisperModel, Word, format_hint_within_budget,
};
use thiserror::Error;
#[cfg(unix)]
use {signal_hook::consts::signal::SIGUSR1, signal_hook::iterator::Signals};

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunMode {
    Record,
    Retry,
}

impl RunMode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Record => "record",
            Self::Retry => "retry",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionPhase {
    Recording,
    SavingLastAudio,
    Transcribing,
    PostProcessing,
    EmittingOutput,
    Completed,
    Cancelled,
}

impl SessionPhase {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Recording => "recording",
            Self::SavingLastAudio => "saving_last_audio",
            Self::Transcribing => "transcribing",
            Self::PostProcessing => "post_processing",
            Self::EmittingOutput => "emitting_output",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestedAction {
    None,
    StopRecording,
    CancelSession,
    ForceExit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SessionSnapshot {
    phase: SessionPhase,
    requested_action: RequestedAction,
}

#[derive(Debug)]
struct SessionController {
    state: Mutex<SessionSnapshot>,
    cancellation: CancellationContext,
}

impl SessionController {
    fn new(initial_phase: SessionPhase) -> Self {
        Self {
            state: Mutex::new(SessionSnapshot {
                phase: initial_phase,
                requested_action: RequestedAction::None,
            }),
            cancellation: CancellationContext::new(),
        }
    }

    fn request_stop_recording(&self) -> bool {
        let mut state = self.lock();
        if state.phase == SessionPhase::Recording && state.requested_action == RequestedAction::None
        {
            state.requested_action = RequestedAction::StopRecording;
            drop(state);
            return true;
        }

        false
    }

    fn request_cancel_session(&self) -> bool {
        let mut state = self.lock();
        if matches!(
            state.requested_action,
            RequestedAction::CancelSession | RequestedAction::ForceExit
        ) {
            state.requested_action = RequestedAction::ForceExit;
            return true;
        }

        state.requested_action = RequestedAction::CancelSession;
        let should_cancel_work = !matches!(
            state.phase,
            SessionPhase::EmittingOutput | SessionPhase::Completed
        );
        if should_cancel_work {
            state.phase = SessionPhase::Cancelled;
        }
        drop(state);
        if should_cancel_work {
            self.cancellation.cancel();
        }
        false
    }

    fn begin_saving_last_audio(&self) -> bool {
        self.transition_to(SessionPhase::SavingLastAudio)
    }

    fn begin_transcribing(&self) -> bool {
        self.transition_to(SessionPhase::Transcribing)
    }

    fn begin_post_processing(&self) -> bool {
        self.transition_to(SessionPhase::PostProcessing)
    }

    fn begin_output_commit(&self) -> bool {
        self.transition_to(SessionPhase::EmittingOutput)
    }

    fn finish_success(&self) {
        let mut state = self.lock();
        state.phase = SessionPhase::Completed;
        state.requested_action = RequestedAction::None;
    }

    fn is_cancelled(&self) -> bool {
        let state = self.lock();
        state.phase == SessionPhase::Cancelled
            || matches!(
                state.requested_action,
                RequestedAction::CancelSession | RequestedAction::ForceExit
            )
    }

    fn is_recording(&self) -> bool {
        self.lock().phase == SessionPhase::Recording
    }

    const fn cancellation(&self) -> &CancellationContext {
        &self.cancellation
    }

    fn should_continue_recording(&self) -> bool {
        let state = self.lock();
        state.phase == SessionPhase::Recording && state.requested_action == RequestedAction::None
    }

    fn transition_to(&self, next_phase: SessionPhase) -> bool {
        let mut state = self.lock();
        if state.phase == SessionPhase::Cancelled
            || matches!(
                state.requested_action,
                RequestedAction::CancelSession | RequestedAction::ForceExit
            )
        {
            state.phase = SessionPhase::Cancelled;
            return false;
        }

        if state.phase == SessionPhase::Recording
            && state.requested_action == RequestedAction::StopRecording
        {
            state.requested_action = RequestedAction::None;
        }

        state.phase = next_phase;
        true
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, SessionSnapshot> {
        match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Reporter {
    json_events: bool,
}

impl Reporter {
    const fn new(json_events: bool) -> Self {
        Self { json_events }
    }

    fn session_started(self, mode: RunMode, phase: SessionPhase, stop_after: Option<Duration>) {
        if self.json_events {
            Self::emit_json(&serde_json::json!({
                "event": "session",
                "mode": mode.as_str(),
                "phase": phase.as_str(),
                "stop_after_ms": stop_after.map(|duration| duration.as_millis()),
            }));
        }
    }

    fn phase(self, phase: SessionPhase, chunk_count: Option<usize>, model: Option<&str>) {
        if self.json_events {
            Self::emit_json(&serde_json::json!({
                "event": "phase",
                "phase": phase.as_str(),
                "chunk_count": chunk_count,
                "model": model,
            }));
        }
    }

    fn status(self, message: impl std::fmt::Display) {
        if !self.json_events {
            eprintln!("[dictate] {message}");
        }
    }

    fn warning(self, message: impl std::fmt::Display) {
        if self.json_events {
            Self::emit_json(&serde_json::json!({
                "event": "warning",
                "message": message.to_string(),
            }));
        } else {
            eprintln!("[dictate] warning: {message}");
        }
    }

    fn completed(self, char_count: usize, copied_to_clipboard: bool) {
        if self.json_events {
            Self::emit_json(&serde_json::json!({
                "event": "result",
                "status": "completed",
                "char_count": char_count,
                "copied_to_clipboard": copied_to_clipboard,
            }));
        } else {
            let suffix = if copied_to_clipboard {
                ", copied to clipboard"
            } else {
                ""
            };
            eprintln!("[dictate] done ({char_count} chars{suffix})");
        }
    }

    fn completed_without_transcript(self, message: &str) {
        if self.json_events {
            Self::emit_json(&serde_json::json!({
                "event": "result",
                "status": "completed",
                "char_count": 0,
                "copied_to_clipboard": false,
                "message": message,
            }));
        } else {
            eprintln!("[dictate] {message}");
        }
    }

    fn cancelled(self) {
        if self.json_events {
            Self::emit_json(&serde_json::json!({
                "event": "result",
                "status": "cancelled",
            }));
        } else {
            eprintln!("[dictate] cancelled");
        }
    }

    fn emit_json(value: &serde_json::Value) {
        eprintln!(
            "{}",
            serde_json::to_string(value).expect("JSON event should serialize")
        );
    }
}

// ══════════════════════════════════════════════════════════════════════════════
//  Builder: RecordOptions
// ══════════════════════════════════════════════════════════════════════════════

/// Configuration options for audio recording and transcription.
/// Use the builder pattern to construct with only the options you need.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputOptions {
    stdout: bool,
    no_clipboard: bool,
}

impl OutputOptions {
    /// Create a new output configuration with default destinations.
    pub fn new() -> Self {
        Self::default()
    }

    /// Print transcript to stdout while still copying to clipboard.
    pub const fn stdout(mut self, enabled: bool) -> Self {
        self.stdout = enabled;
        self
    }

    /// Skip clipboard entirely (headless/scripted use).
    pub const fn no_clipboard(mut self, enabled: bool) -> Self {
        self.no_clipboard = enabled;
        self
    }

    const fn write_to_stdout(self) -> bool {
        self.stdout || self.no_clipboard
    }

    const fn use_clipboard(self) -> bool {
        !self.no_clipboard
    }
}

#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct PostProcessOptions {
    enabled: Option<bool>,
    model: Option<ModelId>,
    base_url: Option<String>,
}

impl PostProcessOptions {
    /// Create a new post-process configuration with inherited defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable or disable LLM post-processing.
    pub const fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = Some(enabled);
        self
    }

    /// Set the model for post-processing.
    pub fn model(mut self, model: ModelId) -> Self {
        self.model = Some(model);
        self
    }

    /// Override the post-processing chat API base URL.
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }
}

#[derive(Default, Debug)]
pub struct RecordOptions {
    device: Option<String>,
    base_url: Option<String>,
    language: Option<String>,
    prompt: Option<String>,
    response_format: Option<ResponseFormat>,
    transcription_model: Option<WhisperModel>,
    temperature: Option<f32>,
    timestamp_granularities: Option<Vec<TimestampGranularity>>,
    output: OutputOptions,
    post_process: PostProcessOptions,
    pub(crate) stop_after: Option<Duration>,
    save_last_audio: bool,
    json_events: bool,
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

    /// Set grouped output options.
    pub const fn output(mut self, output: OutputOptions) -> Self {
        self.output = output;
        self
    }

    /// Set grouped post-process options.
    pub fn post_process_options(mut self, post_process: PostProcessOptions) -> Self {
        self.post_process = post_process;
        self
    }

    /// Automatically stop recording after the given duration.
    pub const fn stop_after(mut self, duration: Duration) -> Self {
        self.stop_after = Some(duration);
        self
    }

    /// Persist the captured audio locally for later reuse with `dictate retry`.
    pub const fn save_last_audio(mut self, enabled: bool) -> Self {
        self.save_last_audio = enabled;
        self
    }

    /// Emit machine-readable JSONL progress events on stderr.
    pub const fn json_events(mut self, enabled: bool) -> Self {
        self.json_events = enabled;
        self
    }
}

// ══════════════════════════════════════════════════════════════════════════════
//  Main Entry Point
// ══════════════════════════════════════════════════════════════════════════════

pub fn run(options: &RecordOptions) -> Result<RunOutcome, RecordError> {
    let reporter = Reporter::new(options.json_events);
    let resolved = resolve_run_config(options, None, RunMode::Record, reporter)?;

    // Fail fast if clipboard is requested but unavailable (missing tool / headless)
    if options.output.use_clipboard() {
        dictate_core::check_clipboard_available()?;
    }

    let (controller, active_recording_stop) =
        prepare_session_control(SessionPhase::Recording, true, reporter);
    reporter.session_started(RunMode::Record, SessionPhase::Recording, options.stop_after);

    // Record audio chunks
    let session = capture_recording_session(
        options.device.as_deref(),
        options.save_last_audio,
        options.stop_after,
        &controller,
        &active_recording_stop,
        reporter,
    )?;

    if session.chunks.is_empty() {
        if controller.is_cancelled() {
            reporter.cancelled();
            return Ok(RunOutcome::Cancelled);
        }

        reporter.completed_without_transcript("no audio captured");
        return Ok(RunOutcome::Completed);
    }

    if options.save_last_audio && !session.samples.is_empty() {
        if !controller.begin_saving_last_audio() {
            reporter.cancelled();
            return Ok(RunOutcome::Cancelled);
        }

        reporter.phase(SessionPhase::SavingLastAudio, None, None);
        if let Err(outcome) =
            maybe_save_last_audio(options, &session, &resolved, &controller, reporter)
        {
            if outcome == RunOutcome::Cancelled {
                reporter.cancelled();
            }
            return Ok(outcome);
        }
    }

    if !controller.begin_transcribing() {
        reporter.cancelled();
        return Ok(RunOutcome::Cancelled);
    }

    process_transcription_session(options, &resolved, session.chunks, &controller, reporter)
}

/// Reuse the last saved recording and rerun transcription/post-processing.
pub fn run_retry(options: &RecordOptions) -> Result<RunOutcome, RecordError> {
    let reporter = Reporter::new(options.json_events);
    if options.output.use_clipboard() {
        dictate_core::check_clipboard_available()?;
    }

    let saved = SavedRecordingStore::open()?.load()?;
    let defaults = saved_defaults_from_manifest(&saved.manifest)?;
    let resolved = resolve_run_config(options, Some(&defaults), RunMode::Retry, reporter)?;
    let session = rechunk_saved_audio(saved.samples, saved.manifest.chunk_target_duration_secs);

    let (controller, _active_recording_stop) =
        prepare_session_control(SessionPhase::Transcribing, false, reporter);
    reporter.session_started(RunMode::Retry, SessionPhase::Transcribing, None);

    if session.chunks.is_empty() {
        reporter.completed_without_transcript("saved recording contains no audio");
        return Ok(RunOutcome::Completed);
    }

    reporter.status("reusing saved audio from last recording...");
    process_transcription_session(options, &resolved, session.chunks, &controller, reporter)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OutputSummary {
    char_count: usize,
    copied_to_clipboard: bool,
}

struct CaptureCollector<'a> {
    chunker: &'a mut ProgressiveChunker,
    all_samples: &'a mut Option<Vec<f32>>,
    chunks: &'a mut Vec<AudioChunk>,
    reporter: Reporter,
}

impl CaptureCollector<'_> {
    fn push_samples(&mut self, samples: &[f32]) {
        if let Some(all_samples) = self.all_samples.as_mut() {
            all_samples.extend_from_slice(samples);
        }

        if let Some(chunk) = self.chunker.push_samples(samples) {
            self.reporter.status(format!(
                "chunk {} ready ({:.1}s)",
                chunk.index,
                chunk.duration_secs()
            ));
            self.chunks.push(chunk);
        }
    }

    fn flush_chunker(&mut self) {
        if let Some(chunk) = self.chunker.flush() {
            self.reporter.status(format!(
                "final chunk {} ready ({:.1}s)",
                chunk.index,
                chunk.duration_secs()
            ));
            self.chunks.push(chunk);
        }
    }
}

fn process_transcription_session(
    options: &RecordOptions,
    resolved: &ResolvedRunConfig,
    chunks: Vec<AudioChunk>,
    controller: &SessionController,
    reporter: Reporter,
) -> Result<RunOutcome, RecordError> {
    reporter.phase(SessionPhase::Transcribing, Some(chunks.len()), None);
    let Some(results) = transcribe_chunks(&resolved.pipeline, chunks, controller, reporter)? else {
        reporter.cancelled();
        return Ok(RunOutcome::Cancelled);
    };

    // Output results
    let merged = merge_results(results);
    let post_process_requested = resolved.effective_post_process;

    let (merged, post_process_outcome, post_process_requested) =
        if post_process_requested && !merged.text.is_empty() {
            if !controller.begin_post_processing() {
                reporter.cancelled();
                return Ok(RunOutcome::Cancelled);
            }

            let model = resolved
                .pipeline_config
                .post_process_model
                .as_ref()
                .map_or(DEFAULT_POST_PROCESS_MODEL, dictate_core::ModelId::as_str);
            reporter.phase(SessionPhase::PostProcessing, None, Some(model));
            reporter.status(format!("post-processing with {model}..."));

            match resolved
                .pipeline
                .post_process_result_with_cancellation(merged, controller.cancellation())
            {
                Ok((merged, outcome)) => (merged, outcome, true),
                Err(CancellationError::Cancelled) => {
                    reporter.cancelled();
                    return Ok(RunOutcome::Cancelled);
                }
                Err(CancellationError::Error(err)) => return Err(RecordError::from(err)),
            }
        } else {
            (merged, PostProcessOutcome::NotConfigured, false)
        };

    if !controller.begin_output_commit() {
        reporter.cancelled();
        return Ok(RunOutcome::Cancelled);
    }

    let output = output_result(
        &merged,
        resolved.effective_format,
        post_process_requested,
        options,
        post_process_outcome,
        reporter,
    );
    controller.finish_success();
    reporter.completed(output.char_count, output.copied_to_clipboard);

    Ok(RunOutcome::Completed)
}

// ══════════════════════════════════════════════════════════════════════════════
//  Helper Functions
// ══════════════════════════════════════════════════════════════════════════════

/// Resolve the effective transcription configuration and build a pipeline.
fn resolve_run_config(
    options: &RecordOptions,
    defaults: Option<&SavedDefaults>,
    run_mode: RunMode,
    reporter: Reporter,
) -> Result<ResolvedRunConfig, RecordError> {
    let timestamp_granularities = resolve_timestamp_granularities(options, defaults);

    // Auto-upgrade format when timestamps are requested
    let effective_format = auto_upgrade_format(
        options
            .response_format
            .or_else(|| defaults.and_then(|saved| saved.output_format)),
        Some(&timestamp_granularities),
        reporter,
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
        .post_process
        .base_url
        .clone()
        .or_else(|| defaults.and_then(|saved| saved.pipeline_config.post_process_base_url.clone()))
        .or_else(|| std::env::var(GROQ_CHAT_BASE_URL_VAR).ok());

    // Load vocabulary prompt hints for prompt injection.
    // Best-effort: warn and continue on store errors.
    let effective_prompt = if options.prompt.is_some() || defaults.is_none() {
        load_prompt_hints(options.prompt.as_deref(), reporter)
    } else {
        defaults.and_then(|saved| saved.pipeline_config.prompt.clone())
    };

    let inherited_response_format = defaults.map_or(ResponseFormat::Json, |saved| {
        saved.pipeline_config.response_format
    });
    let inherited_post_process = defaults.is_some_and(|saved| saved.pipeline_config.post_process);
    let effective_post_process =
        resolve_post_process_enabled(options.post_process.enabled, inherited_post_process);

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
        post_process_model: options.post_process.model.clone().or_else(|| {
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
    stop_after: Option<Duration>,
    controller: &SessionController,
    active_recording_stop: &Mutex<Option<RecorderStopHandle>>,
    reporter: Reporter,
) -> Result<CapturedSession, RecordError> {
    print_recording_start_message(stop_after, reporter);

    let mut config = RecorderConfig::default();
    if let Some(query) = device {
        config.device = DeviceSelection::Query(query.to_string());
    }

    let (mut recorder, mut rx, info) = AudioRecorder::start(config)?;
    set_active_recording_stop(active_recording_stop, Some(recorder.stop_handle()));
    reporter.status(format!(
        "device: {} ({} Hz, {}ch) -> resampling to {} Hz mono",
        info.device_name,
        info.device_sample_rate_hz,
        info.device_channels,
        info.target_sample_rate_hz
    ));

    // Collect audio chunks
    let chunker_config = ChunkerConfig::default();
    let mut chunker = ProgressiveChunker::new(chunker_config.clone());
    let mut chunks: Vec<AudioChunk> = Vec::new();
    let mut samples = collect_samples.then(Vec::new);
    let mut collector = CaptureCollector {
        chunker: &mut chunker,
        all_samples: &mut samples,
        chunks: &mut chunks,
        reporter,
    };

    consume_until_stopped(
        &mut rx,
        controller,
        active_recording_stop,
        stop_after,
        &mut collector,
    );
    set_active_recording_stop(active_recording_stop, None);

    recorder.stop()?;

    // Check recording stats and warn about any issues.
    let stats = recorder.stats().snapshot();
    if stats.resample_errors > 0 {
        reporter.warning(format!(
            "{} audio samples lost due to resampling errors",
            stats.resample_errors
        ));
    }
    if stats.dropped_samples > 0 {
        reporter.warning(format!(
            "{} audio samples dropped (processing too slow)",
            stats.dropped_samples
        ));
    }
    if stats.stream_errors > 0 {
        reporter.warning(format!(
            "{} audio stream errors occurred",
            stats.stream_errors
        ));
    }

    drain_remaining(&mut rx, &mut collector);

    let tail = recorder.take_flushed_tail();
    if !tail.is_empty() {
        collector.push_samples(&tail);
    }

    collector.flush_chunker();

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
    controller: &SessionController,
    reporter: Reporter,
) -> Result<Option<Vec<TranscriptionResult>>, RecordError> {
    reporter.status(format!(
        "transcribing {} chunk{}...",
        chunks.len(),
        if chunks.len() == 1 { "" } else { "s" }
    ));

    let mut results: Vec<TranscriptionResult> = Vec::new();
    let mut timeline_offset: f64 = 0.0;

    for chunk in chunks {
        if controller.is_cancelled() {
            reporter.status("cancelled, skipping remaining chunks");
            return Ok(None);
        }

        reporter.status(format!(
            "  chunk {} ({:.1}s)...",
            chunk.index,
            chunk.duration_secs()
        ));

        let chunk_offset = timeline_offset;
        let chunk_duration = f64::from(chunk.duration_secs());
        let leading_overlap = f64::from(chunk.leading_overlap_secs());

        let mut result =
            match pipeline.transcribe_chunk_with_cancellation(&chunk, controller.cancellation()) {
                Ok(result) => result,
                Err(CancellationError::Cancelled) => {
                    reporter.status("cancelled, abandoning in-flight transcription");
                    return Ok(None);
                }
                Err(CancellationError::Error(err)) => return Err(RecordError::from(err)),
            };

        if chunk_offset > 0.0 {
            offset_timestamps(&mut result, chunk_offset);
        }

        timeline_offset += chunk_duration - leading_overlap;
        results.push(result);
    }

    Ok(Some(results))
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
/// - If `--stdout`: print to stdout and copy to clipboard
/// - If `--no-clipboard`: print to stdout only
/// - Otherwise: copy to clipboard (with stderr/stdout fallback on failure)
fn output_result(
    merged: &TranscriptionResult,
    format: Option<ResponseFormat>,
    post_process_requested: bool,
    options: &RecordOptions,
    post_process_outcome: PostProcessOutcome,
    reporter: Reporter,
) -> OutputSummary {
    if merged.text.is_empty() {
        if !reporter.json_events {
            eprintln!("[dictate] no speech detected");
        }
        return OutputSummary {
            char_count: 0,
            copied_to_clipboard: false,
        };
    }

    // Determine output destinations based on flags
    let write_to_stdout = options.output.write_to_stdout();
    let use_clipboard = options.output.use_clipboard();

    // Format the result according to --format flag
    let formatted = format_to_string(
        merged,
        format,
        post_process_requested,
        post_process_outcome,
        reporter,
    );

    if write_to_stdout {
        println!("{formatted}");
    }

    if use_clipboard {
        match dictate_core::clipboard::copy_to_clipboard(&formatted) {
            Ok(()) => OutputSummary {
                char_count: merged.text.len(),
                copied_to_clipboard: true,
            },
            Err(err) => {
                reporter.warning(format!("clipboard failed: {err}"));
                if !write_to_stdout && reporter.json_events {
                    println!("{formatted}");
                } else if !write_to_stdout {
                    eprintln!("[dictate] transcript (saved to stderr to prevent data loss):");
                    eprintln!("{formatted}");
                }
                OutputSummary {
                    char_count: merged.text.len(),
                    copied_to_clipboard: false,
                }
            }
        }
    } else {
        OutputSummary {
            char_count: merged.text.len(),
            copied_to_clipboard: false,
        }
    }
}

fn prepare_session_control(
    phase: SessionPhase,
    enable_stdin_stop: bool,
    reporter: Reporter,
) -> (
    Arc<SessionController>,
    Arc<Mutex<Option<RecorderStopHandle>>>,
) {
    let controller = Arc::new(SessionController::new(phase));
    let active_recording_stop = Arc::new(Mutex::new(None));
    install_stop_handlers(
        &controller,
        &active_recording_stop,
        enable_stdin_stop,
        reporter,
    );
    (controller, active_recording_stop)
}

fn install_stop_handlers(
    controller: &Arc<SessionController>,
    active_recording_stop: &Arc<Mutex<Option<RecorderStopHandle>>>,
    enable_stdin_stop: bool,
    reporter: Reporter,
) {
    let controller_ctrlc = Arc::clone(controller);
    let active_recording_stop_ctrlc = Arc::clone(active_recording_stop);
    if let Err(err) = ctrlc::set_handler(move || {
        if controller_ctrlc.request_cancel_session() {
            if !reporter.json_events {
                eprintln!("\n[dictate] forced exit");
            }
            std::process::exit(130);
        }

        if controller_ctrlc.is_recording() {
            request_active_recording_stop(&active_recording_stop_ctrlc, reporter);
        }
    }) {
        reporter.warning(format!("failed to set Ctrl+C handler: {err}"));
    }

    install_sigusr1_stop_handler(controller, active_recording_stop, reporter);

    if enable_stdin_stop && std::io::stdin().is_terminal() {
        let active_recording_stop_stdin = Arc::clone(active_recording_stop);
        let controller_stdin = Arc::clone(controller);
        std::thread::spawn(move || {
            let mut input = String::new();
            let read_result = std::io::stdin().read_line(&mut input);
            if should_stop_after_stdin_read(&read_result, &input) {
                request_recording_stop(
                    &controller_stdin,
                    &active_recording_stop_stdin,
                    None,
                    reporter,
                );
            }
        });
    }
}

fn should_stop_after_stdin_read(read_result: &std::io::Result<usize>, input: &str) -> bool {
    read_result.is_ok() && input.ends_with('\n')
}

#[cfg(unix)]
fn install_sigusr1_stop_handler(
    controller: &Arc<SessionController>,
    active_recording_stop: &Arc<Mutex<Option<RecorderStopHandle>>>,
    reporter: Reporter,
) {
    let controller_sigusr1 = Arc::clone(controller);
    let active_recording_stop_sigusr1 = Arc::clone(active_recording_stop);
    let signals = match Signals::new([SIGUSR1]) {
        Ok(signals) => signals,
        Err(err) => {
            reporter.warning(format!("failed to set SIGUSR1 handler: {err}"));
            return;
        }
    };

    std::thread::spawn(move || {
        let mut signals = signals;
        for _signal in signals.forever() {
            if controller_sigusr1.is_recording() {
                request_recording_stop(
                    &controller_sigusr1,
                    &active_recording_stop_sigusr1,
                    None,
                    reporter,
                );
            } else if !controller_sigusr1.is_cancelled() {
                reporter.warning("ignoring SIGUSR1 outside recording");
            }
        }
    });
}

#[cfg(not(unix))]
fn install_sigusr1_stop_handler(
    _controller: &Arc<SessionController>,
    _active_recording_stop: &Arc<Mutex<Option<RecorderStopHandle>>>,
    _reporter: Reporter,
) {
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

fn request_active_recording_stop(
    active_recording_stop: &Mutex<Option<RecorderStopHandle>>,
    reporter: Reporter,
) {
    let stop_handle = match active_recording_stop.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };

    if let Some(stop_handle) = stop_handle
        && let Err(err) = stop_handle.request_stop()
    {
        reporter.warning(format!("failed to stop recording promptly: {err}"));
    }
}

fn request_recording_stop(
    controller: &SessionController,
    active_recording_stop: &Mutex<Option<RecorderStopHandle>>,
    reason: Option<&str>,
    reporter: Reporter,
) {
    if controller.request_stop_recording() {
        if let Some(reason) = reason {
            reporter.status(reason);
        }
        request_active_recording_stop(active_recording_stop, reporter);
    }
}

fn print_recording_start_message(stop_after: Option<Duration>, reporter: Reporter) {
    let stop_hint = if std::io::stdin().is_terminal() {
        "press Enter to stop"
    } else {
        "use --stop-after or SIGUSR1 to stop"
    };

    let auto_stop_hint = stop_after.map_or_else(String::new, |duration| {
        format!(", auto-stop after {}", format_duration(duration))
    });

    reporter.status(format!(
        "recording... {stop_hint}{auto_stop_hint}, Ctrl+C to cancel"
    ));
}

fn format_duration(duration: Duration) -> String {
    if duration.as_secs() > 0 && duration.subsec_nanos() == 0 {
        return format!("{}s", duration.as_secs());
    }

    if duration.as_secs() > 0 {
        return format!("{:.1}s", duration.as_secs_f64());
    }

    if duration.as_millis() > 0 {
        return format!("{}ms", duration.as_millis());
    }

    format!("{:.3}ms", duration.as_secs_f64() * 1000.0)
}

fn consume_until_stopped(
    rx: &mut AudioReceiver,
    controller: &SessionController,
    active_recording_stop: &Mutex<Option<RecorderStopHandle>>,
    stop_after: Option<Duration>,
    collector: &mut CaptureCollector<'_>,
) {
    let stop_deadline = resolve_stop_deadline(stop_after, collector.reporter);

    loop {
        if stop_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            request_recording_stop(
                controller,
                active_recording_stop,
                Some("stop-after elapsed; finishing capture"),
                collector.reporter,
            );
        }

        if !controller.should_continue_recording() {
            break;
        }

        match rx.recv_timeout(recording_wait_timeout(stop_deadline)) {
            RecvResult::Data(samples) => {
                collector.push_samples(samples);
            }
            RecvResult::Timeout => {}
            RecvResult::Disconnected => break,
        }
    }
}

fn resolve_stop_deadline(stop_after: Option<Duration>, reporter: Reporter) -> Option<Instant> {
    let duration = stop_after?;
    Some(Instant::now().checked_add(duration).unwrap_or_else(|| {
        reporter.warning("--stop-after is too large on this platform; stopping immediately");
        Instant::now()
    }))
}

fn recording_wait_timeout(stop_deadline: Option<Instant>) -> Duration {
    stop_deadline.map_or(RECV_TIMEOUT, |deadline| {
        let now = Instant::now();
        if now >= deadline {
            Duration::ZERO
        } else {
            RECV_TIMEOUT.min(deadline.duration_since(now))
        }
    })
}

/// Drain all remaining samples from the ring buffer after the stream has stopped.
///
/// Loops: non-blocking drain → disconnected check → blocking wait with timeout
/// counting, until the producer is dropped and the buffer is empty.
fn drain_remaining(rx: &mut AudioReceiver, collector: &mut CaptureCollector<'_>) {
    let mut consecutive_timeouts = 0_u8;

    loop {
        while let Some(samples) = rx.try_recv() {
            consecutive_timeouts = 0;
            collector.push_samples(samples);
        }

        if rx.is_disconnected() {
            break;
        }

        match rx.recv_timeout(RECV_TIMEOUT) {
            RecvResult::Data(samples) => {
                consecutive_timeouts = 0;
                collector.push_samples(samples);
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
    controller: &SessionController,
    reporter: Reporter,
) -> Result<(), RunOutcome> {
    if !options.save_last_audio || session.samples.is_empty() {
        return Ok(());
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

    match SavedRecordingStore::open() {
        Ok(store) => match store.save_with_cancellation(&recording, controller.cancellation()) {
            Ok(()) => reporter.status("saved audio for later reuse"),
            Err(CancellationError::Cancelled) => {
                return Err(RunOutcome::Cancelled);
            }
            Err(CancellationError::Error(err)) => {
                reporter.warning(format!("could not save audio for retry: {err}"));
            }
        },
        Err(err) => reporter.warning(format!("could not save audio for retry: {err}")),
    }

    Ok(())
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

/// Load vocabulary hints, then compose the effective prompt.
///
/// This is best-effort: if the store cannot be loaded, a warning is printed
/// and prompt composition continues with the user prompt only.
fn load_prompt_hints(user_prompt: Option<&str>, reporter: Reporter) -> Option<String> {
    let vocabulary = load_vocabulary_best_effort(reporter);
    if vocabulary.is_empty() {
        return user_prompt.map(String::from);
    }

    // Calculate remaining token budget after the user's prompt.
    // Reserve 2 tokens for the ". " joiner inserted by build_effective_prompt
    // when both prompt hints and a user prompt are present.
    let user_tokens = user_prompt.map_or(0, estimate_token_count);
    let joiner_cost = if user_prompt.is_some() { 2 } else { 0 };
    let remaining_budget = MAX_PROMPT_TOKENS.saturating_sub(user_tokens + joiner_cost);

    let hint = format_hint_within_budget(vocabulary.iter(), remaining_budget);

    if let Some(ref h) = hint {
        if h.included < h.total {
            reporter.status(format!(
                "prompt hints: using {}/{} entries (token limit)",
                h.included, h.total
            ));
        } else {
            reporter.status(format!(
                "prompt hints loaded ({} {})",
                h.included,
                if h.included == 1 { "entry" } else { "entries" }
            ));
        }
    }

    build_effective_prompt(user_prompt, hint.as_ref().map(|entry| entry.text.as_str()))
}

fn load_vocabulary_best_effort(reporter: Reporter) -> Vocabulary {
    let store = match VocabularyStore::open() {
        Ok(s) => s,
        Err(err) => {
            reporter.warning(format!("could not open vocabulary store: {err}"));
            return Vocabulary::new();
        }
    };

    match store.load() {
        Ok(v) => v,
        Err(err) => {
            reporter.warning(format!("could not load vocabulary: {err}"));
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
    reporter: Reporter,
) -> Option<ResponseFormat> {
    let has_timestamps = timestamps.is_some_and(|ts| !ts.is_empty());

    if !has_timestamps {
        return explicit_format;
    }

    match explicit_format {
        Some(ResponseFormat::VerboseJson) => explicit_format,
        Some(other) => {
            reporter.warning(format!(
                "--timestamps requires verbose_json; overriding --format {}",
                other.as_str()
            ));
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
    reporter: Reporter,
) -> String {
    match format {
        Some(ResponseFormat::VerboseJson) => {
            // Full structured JSON with segments and words.
            let mut payload = match serde_json::to_value(result) {
                Ok(value) => value,
                Err(err) => {
                    reporter.warning(format!("JSON serialization failed: {err}"));
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
                    reporter.warning(format!("JSON serialization failed: {err}"));
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
                    reporter.warning(format!("JSON serialization failed: {err}"));
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
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::time::{SystemTime, UNIX_EPOCH};

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
        cancel_after_first: Option<Arc<SessionController>>,
    }

    impl StubProvider {
        fn new(response: impl Into<String>) -> Self {
            Self {
                response: response.into(),
                calls: Arc::new(AtomicUsize::new(0)),
                cancel_after_first: None,
            }
        }

        fn with_cancel_after_first(mut self, controller: Arc<SessionController>) -> Self {
            self.cancel_after_first = Some(controller);
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
                && let Some(controller) = &self.cancel_after_first
            {
                let _ = controller.request_cancel_session();
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

    struct CancelledPostProcessor {
        calls: Arc<AtomicUsize>,
    }

    impl CancelledPostProcessor {
        fn new(calls: Arc<AtomicUsize>) -> Self {
            Self { calls }
        }
    }

    impl PostProcessor for CancelledPostProcessor {
        fn name(&self) -> &'static str {
            "cancelled"
        }

        fn process(
            &self,
            _text: &str,
            _config: dictate_core::PostProcessConfig<'_>,
        ) -> Result<String, TranscriptionError> {
            self.calls.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(String::new())
        }

        fn process_with_cancellation(
            &self,
            _text: &str,
            _config: dictate_core::PostProcessConfig<'_>,
            _cancellation: &CancellationContext,
        ) -> dictate_core::CancellationResult<String, TranscriptionError> {
            self.calls.fetch_add(1, AtomicOrdering::SeqCst);
            Err(CancellationError::Cancelled)
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

    fn test_saved_recording(
        samples: Vec<f32>,
        target_duration_secs: u64,
        format: Option<ResponseFormat>,
    ) -> SavedRecording {
        SavedRecording {
            manifest: SavedRecordingManifest::new(
                samples.len(),
                target_duration_secs,
                format,
                &PipelineConfig::default(),
            ),
            samples,
        }
    }

    fn make_temp_saved_recording_store() -> (PathBuf, SavedRecordingStore) {
        let unique_suffix = format!(
            "{}-{}-{}",
            std::process::id(),
            TEST_TEMP_DIR_COUNTER.fetch_add(1, AtomicOrdering::SeqCst),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let dir = std::env::temp_dir().join(format!("dictate-record-tests-{unique_suffix}"));
        (dir.clone(), SavedRecordingStore::open_at(dir))
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
        assert_eq!(auto_upgrade_format(None, None, Reporter::new(false)), None);
        assert_eq!(
            auto_upgrade_format(Some(ResponseFormat::Json), None, Reporter::new(false)),
            Some(ResponseFormat::Json)
        );
        let empty: Vec<TimestampGranularity> = vec![];
        assert_eq!(
            auto_upgrade_format(
                Some(ResponseFormat::Text),
                Some(&empty),
                Reporter::new(false),
            ),
            Some(ResponseFormat::Text)
        );
    }

    #[test]
    fn timestamps_upgrade_none_format() {
        let ts = vec![TimestampGranularity::Word];
        assert_eq!(
            auto_upgrade_format(None, Some(&ts), Reporter::new(false)),
            Some(ResponseFormat::VerboseJson)
        );
    }

    #[test]
    fn timestamps_override_explicit_non_verbose_format() {
        let ts = vec![TimestampGranularity::Segment];
        assert_eq!(
            auto_upgrade_format(Some(ResponseFormat::Json), Some(&ts), Reporter::new(false),),
            Some(ResponseFormat::VerboseJson)
        );
    }

    #[test]
    fn timestamps_keep_verbose_json() {
        let ts = vec![TimestampGranularity::Word];
        assert_eq!(
            auto_upgrade_format(
                Some(ResponseFormat::VerboseJson),
                Some(&ts),
                Reporter::new(false),
            ),
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
        let mut collector = CaptureCollector {
            chunker: &mut chunker,
            all_samples: &mut all_samples,
            chunks: &mut chunks,
            reporter: Reporter::new(false),
        };

        collector.push_samples(&input);

        assert_eq!(all_samples.unwrap(), input);
        assert!(chunks.is_empty());
    }

    #[test]
    fn push_samples_and_collect_skips_buffer_when_disabled() {
        let mut chunker = ProgressiveChunker::new(ChunkerConfig::default());
        let input = vec![0.25_f32, -0.5, 0.75];
        let mut all_samples = None;
        let mut chunks = Vec::new();
        let mut collector = CaptureCollector {
            chunker: &mut chunker,
            all_samples: &mut all_samples,
            chunks: &mut chunks,
            reporter: Reporter::new(false),
        };

        collector.push_samples(&input);

        assert!(all_samples.is_none());
        assert!(chunks.is_empty());
    }

    static TEST_TEMP_DIR_COUNTER: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn cancelled_before_transcription_start_preserves_saved_audio() {
        let (dir, store) = make_temp_saved_recording_store();
        let existing = test_saved_recording(vec![0.1, 0.2, 0.3], 15, Some(ResponseFormat::Text));
        store.save(&existing).unwrap();

        let controller = SessionController::new(SessionPhase::Recording);
        assert!(controller.begin_saving_last_audio());
        assert!(!controller.request_cancel_session());

        let replacement = test_saved_recording(vec![0.9, 0.8], 30, Some(ResponseFormat::Json));
        let result = store.save_with_cancellation(&replacement, controller.cancellation());

        assert!(matches!(result, Err(CancellationError::Cancelled)));
        let loaded = store.load().unwrap();
        assert_eq!(loaded.samples.len(), existing.samples.len());
        for (actual, expected) in loaded.samples.iter().zip(&existing.samples) {
            assert!((actual - expected).abs() < 1.0e-4);
        }
        assert_eq!(
            loaded.manifest.chunk_target_duration_secs,
            existing.manifest.chunk_target_duration_secs
        );
        assert_eq!(loaded.manifest.sample_count, existing.manifest.sample_count);
        assert_eq!(
            loaded.manifest.output_format,
            existing.manifest.output_format
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn cancel_during_output_commit_still_finishes_successfully() {
        let controller = SessionController::new(SessionPhase::Transcribing);

        assert!(controller.begin_output_commit());
        assert!(!controller.request_cancel_session());
        controller.finish_success();

        let state = controller.lock();
        assert_eq!(state.phase, SessionPhase::Completed);
        assert_eq!(state.requested_action, RequestedAction::None);
        drop(state);
    }

    #[test]
    fn cancelled_after_partial_transcription_suppresses_results() {
        let controller = Arc::new(SessionController::new(SessionPhase::Transcribing));
        let provider =
            StubProvider::new("partial").with_cancel_after_first(Arc::clone(&controller));
        let pipeline = test_pipeline(provider, None);
        let chunks = vec![test_chunk(0, 1.0), test_chunk(1, 1.0)];

        let results =
            transcribe_chunks(&pipeline, chunks, &controller, Reporter::new(false)).unwrap();

        assert!(results.is_none());
    }

    #[test]
    fn cancelled_before_post_process_skips_post_processor() {
        let controller = Arc::new(SessionController::new(SessionPhase::Transcribing));
        let provider =
            StubProvider::new("partial").with_cancel_after_first(Arc::clone(&controller));
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
        let options = RecordOptions::new().output(OutputOptions::new().no_clipboard(true));

        let outcome = process_transcription_session(
            &options,
            &resolved,
            vec![test_chunk(0, 1.0), test_chunk(1, 1.0)],
            &controller,
            Reporter::new(false),
        )
        .unwrap();

        assert_eq!(outcome, RunOutcome::Cancelled);
        assert_eq!(post_process_calls.load(AtomicOrdering::SeqCst), 0);
    }

    #[test]
    fn cancelled_post_process_error_returns_cancelled_outcome() {
        let controller = Arc::new(SessionController::new(SessionPhase::Transcribing));
        let post_process_calls = Arc::new(AtomicUsize::new(0));
        let pipeline = test_pipeline(
            StubProvider::new("raw text"),
            Some(Box::new(CancelledPostProcessor::new(Arc::clone(
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
        let options = RecordOptions::new().output(OutputOptions::new().no_clipboard(true));

        let outcome = process_transcription_session(
            &options,
            &resolved,
            vec![test_chunk(0, 1.0)],
            &controller,
            Reporter::new(false),
        )
        .unwrap();

        assert_eq!(outcome, RunOutcome::Cancelled);
        assert_eq!(post_process_calls.load(AtomicOrdering::SeqCst), 1);
    }

    #[test]
    fn stop_recording_request_only_applies_during_recording() {
        let controller = SessionController::new(SessionPhase::Recording);

        assert!(controller.request_stop_recording());
        assert!(!controller.should_continue_recording());
        assert!(controller.begin_transcribing());
        assert!(!controller.request_stop_recording());
    }

    #[test]
    fn helper_request_recording_stop_marks_recording_as_stopped() {
        let controller = SessionController::new(SessionPhase::Recording);
        let active_recording_stop = Mutex::new(None);

        request_recording_stop(
            &controller,
            &active_recording_stop,
            None,
            Reporter::new(false),
        );

        assert!(!controller.should_continue_recording());
    }

    #[test]
    fn stdin_stop_requires_newline_terminated_read() {
        assert!(should_stop_after_stdin_read(&Ok(1), "\n"));
        assert!(should_stop_after_stdin_read(&Ok(5), "stop\n"));
        assert!(!should_stop_after_stdin_read(&Ok(0), ""));
        assert!(!should_stop_after_stdin_read(&Ok(4), "stop"));
    }

    #[test]
    fn stdout_output_keeps_clipboard_enabled() {
        let output = OutputOptions::new().stdout(true);

        assert!(output.write_to_stdout());
        assert!(output.use_clipboard());
    }

    #[test]
    fn no_clipboard_output_prints_without_clipboard() {
        let output = OutputOptions::new().no_clipboard(true);

        assert!(output.write_to_stdout());
        assert!(!output.use_clipboard());
    }

    #[test]
    fn stdin_stop_ignores_interrupted_reads() {
        assert!(!should_stop_after_stdin_read(
            &Err(std::io::Error::from(std::io::ErrorKind::Interrupted)),
            "",
        ));
    }

    #[test]
    fn recording_wait_timeout_defaults_to_recv_timeout_without_deadline() {
        assert_eq!(recording_wait_timeout(None), RECV_TIMEOUT);
    }

    #[test]
    fn recording_wait_timeout_shrinks_to_near_deadline() {
        let deadline = Instant::now() + Duration::from_millis(10);
        assert!(recording_wait_timeout(Some(deadline)) <= Duration::from_millis(10));
    }

    #[test]
    fn resolve_stop_deadline_handles_overflow() {
        let deadline = resolve_stop_deadline(Some(Duration::MAX), Reporter::new(false)).unwrap();
        assert!(deadline <= Instant::now());
    }

    #[test]
    fn format_duration_uses_human_readable_units() {
        assert_eq!(format_duration(Duration::from_secs(30)), "30s");
        assert_eq!(format_duration(Duration::from_secs_f64(2.5)), "2.5s");
        assert_eq!(format_duration(Duration::from_millis(250)), "250ms");
    }

    #[test]
    fn second_cancel_request_forces_exit() {
        let controller = SessionController::new(SessionPhase::Transcribing);

        assert!(!controller.request_cancel_session());
        assert!(controller.is_cancelled());
        assert!(controller.request_cancel_session());
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
            Reporter::new(false),
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
            Reporter::new(false),
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
            Reporter::new(false),
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
