//! Persistent storage for the reusable last recording.
//!
//! Stores one full normalized recording plus a JSON manifest under the
//! platform-standard local app data directory. The manifest names the active
//! WAV file so updates can switch generations by replacing the manifest only.

use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use super::SavedRecordingError;
use crate::cancellation::{CancellationContext, CancellationError, CancellationResult};
use crate::encoder::{AudioEncoder, WavEncoder};
use crate::model_id::ModelId;
use crate::pipeline::PipelineConfig;
use crate::postprocess::PostProcessProviderKind;
use crate::provider::{
    ResponseFormat, TimestampGranularity, TranscriptionProviderKind, WhisperModel,
};
use crate::resampler::TRANSCRIPTION_SAMPLE_RATE;

const MANIFEST_FILENAME: &str = "last-recording.json";
const MANIFEST_TMP_FILENAME: &str = "last-recording.json.tmp";
const AUDIO_TMP_FILENAME: &str = "last-recording.wav.tmp";
const AUDIO_GENERATION_PREFIX: &str = "last-recording-";
const MANIFEST_VERSION: u32 = 2;
const LEGACY_MANIFEST_VERSION: u32 = 1;
const SUPPORTED_CHANNELS: u16 = 1;
const SUPPORTED_BITS_PER_SAMPLE: u16 = 16;
const WAV_HEADER_SIZE: usize = 44;
static AUDIO_GENERATION_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Saved recording plus the metadata needed to replay it.
#[derive(Debug, Clone)]
pub struct SavedRecording {
    /// Manifest describing the stored recording and replay settings.
    pub manifest: SavedRecordingManifest,
    /// Full normalized 16 kHz mono audio samples.
    pub samples: Vec<f32>,
}

/// JSON manifest describing a saved recording.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SavedRecordingManifest {
    /// Schema version for forward compatibility.
    pub version: u32,
    /// Saved sample rate in Hz.
    pub sample_rate_hz: u32,
    /// Number of channels in the stored audio.
    pub channels: u16,
    /// Number of normalized samples stored in the WAV file.
    pub sample_count: usize,
    /// Relative WAV file name paired with this manifest.
    pub audio_filename: String,
    /// Chunk target duration to use when replaying the audio.
    pub chunk_target_duration_secs: u64,
    /// Effective output format used by the original command.
    pub output_format: Option<String>,
    /// Effective pipeline configuration used by the original command.
    pub pipeline: SavedPipelineConfig,
}

impl SavedRecordingManifest {
    /// Build a manifest for a newly captured recording.
    #[must_use]
    pub fn new(
        sample_count: usize,
        chunk_target_duration_secs: u64,
        output_format: Option<ResponseFormat>,
        pipeline_config: &PipelineConfig,
    ) -> Self {
        Self {
            version: MANIFEST_VERSION,
            sample_rate_hz: TRANSCRIPTION_SAMPLE_RATE,
            channels: SUPPORTED_CHANNELS,
            sample_count,
            audio_filename: next_audio_filename(),
            chunk_target_duration_secs,
            output_format: output_format.map(|format| format.as_str().to_string()),
            pipeline: SavedPipelineConfig::from_pipeline_config(pipeline_config),
        }
    }

    /// Parse the optional saved output format back into the runtime enum.
    ///
    /// # Errors
    ///
    /// Returns [`SavedRecordingError::InvalidManifest`] when the manifest
    /// contains an unknown output format string.
    pub fn output_format(&self) -> Result<Option<ResponseFormat>, SavedRecordingError> {
        self.output_format
            .as_deref()
            .map(parse_response_format)
            .transpose()
    }

    fn validate(&self) -> Result<(), SavedRecordingError> {
        if self.version != MANIFEST_VERSION && self.version != LEGACY_MANIFEST_VERSION {
            return Err(SavedRecordingError::InvalidManifest(format!(
                "unsupported manifest version {}",
                self.version
            )));
        }
        if self.sample_rate_hz != TRANSCRIPTION_SAMPLE_RATE {
            return Err(SavedRecordingError::InvalidManifest(format!(
                "unsupported sample rate {} Hz",
                self.sample_rate_hz
            )));
        }
        if self.channels != SUPPORTED_CHANNELS {
            return Err(SavedRecordingError::InvalidManifest(format!(
                "unsupported channel count {}",
                self.channels
            )));
        }
        validate_audio_filename(&self.audio_filename)?;
        let _ = self.output_format()?;
        let _ = self.pipeline.to_pipeline_config()?;
        Ok(())
    }
}

/// Serializable pipeline settings stored alongside the saved recording.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SavedPipelineConfig {
    /// Selected transcription provider.
    #[serde(default)]
    pub transcription_provider: Option<String>,
    /// Optional API endpoint override for transcription.
    pub base_url: Option<String>,
    /// Optional ISO-639-1 language code.
    pub language: Option<String>,
    /// Effective prompt sent to Whisper.
    pub prompt: Option<String>,
    /// Provider response format string.
    pub response_format: String,
    /// Optional Whisper model string.
    pub transcription_model: Option<String>,
    /// Optional raw wire model identifier for exact retry replay.
    #[serde(default)]
    pub transcription_model_id: Option<String>,
    /// Optional sampling temperature.
    pub temperature: Option<f32>,
    /// Optional timestamp granularities.
    pub timestamp_granularities: Vec<String>,
    /// Whether post-processing was enabled.
    pub post_process: bool,
    /// Selected post-process provider.
    #[serde(default)]
    pub post_process_provider: Option<String>,
    /// Optional saved post-processing model identifier.
    pub post_process_model: Option<String>,
    /// Optional base URL for post-processing.
    pub post_process_base_url: Option<String>,
}

impl SavedPipelineConfig {
    /// Convert the runtime pipeline config into a manifest-safe shape.
    #[must_use]
    pub fn from_pipeline_config(config: &PipelineConfig) -> Self {
        Self {
            transcription_provider: Some(config.transcription_provider.to_string()),
            base_url: config.base_url.clone(),
            language: config.language.clone(),
            prompt: config.prompt.clone(),
            response_format: config.response_format.as_str().to_string(),
            transcription_model: config
                .transcription_model
                .map(|model| model.preset().to_string()),
            transcription_model_id: config.transcription_model_id.clone(),
            temperature: config.temperature,
            timestamp_granularities: config
                .timestamp_granularities
                .iter()
                .map(|granularity| granularity.as_str().to_string())
                .collect(),
            post_process: config.post_process,
            post_process_provider: Some(config.post_process_provider.to_string()),
            post_process_model: config
                .post_process_model
                .as_ref()
                .map(|model| model.as_str().to_string()),
            post_process_base_url: config.post_process_base_url.clone(),
        }
    }

    /// Convert the saved manifest settings back into a runtime pipeline config.
    ///
    /// # Errors
    ///
    /// Returns [`SavedRecordingError::InvalidManifest`] when any stored enum or
    /// model identifier cannot be parsed.
    pub fn to_pipeline_config(&self) -> Result<PipelineConfig, SavedRecordingError> {
        let transcription_provider = self
            .transcription_provider
            .as_deref()
            .unwrap_or("groq")
            .parse::<TranscriptionProviderKind>()
            .map_err(|err| {
                SavedRecordingError::InvalidManifest(format!(
                    "invalid transcription provider {:?}: {err}",
                    self.transcription_provider
                ))
            })?;
        let response_format = parse_response_format(&self.response_format)?;
        let transcription_model = self
            .transcription_model
            .as_deref()
            .map(parse_whisper_model)
            .transpose()?;
        let transcription_model_id = self.transcription_model_id.clone().or_else(|| {
            transcription_model.and_then(|model| {
                default_transcription_model_id(transcription_provider, model).map(str::to_string)
            })
        });
        let timestamp_granularities = self
            .timestamp_granularities
            .iter()
            .map(|granularity| parse_timestamp_granularity(granularity))
            .collect::<Result<Vec<_>, _>>()?;
        let post_process_provider = self
            .post_process_provider
            .as_deref()
            .unwrap_or("groq")
            .parse::<PostProcessProviderKind>()
            .map_err(|err| {
                SavedRecordingError::InvalidManifest(format!(
                    "invalid post-process provider {:?}: {err}",
                    self.post_process_provider
                ))
            })?;
        let post_process_model = self
            .post_process_model
            .as_deref()
            .map(|model| {
                ModelId::new(model).map_err(|err| {
                    SavedRecordingError::InvalidManifest(format!(
                        "invalid post-process model {model:?}: {err}"
                    ))
                })
            })
            .transpose()?;

        Ok(PipelineConfig {
            transcription_provider,
            base_url: self.base_url.clone(),
            language: self.language.clone(),
            prompt: self.prompt.clone(),
            response_format,
            transcription_model,
            transcription_model_id,
            temperature: self.temperature,
            timestamp_granularities,
            post_process: self.post_process,
            post_process_provider,
            post_process_model,
            post_process_base_url: self.post_process_base_url.clone(),
            ..PipelineConfig::default()
        })
    }
}

/// Persistent storage for the reusable last recording.
pub struct SavedRecordingStore {
    dir: PathBuf,
    manifest_path: PathBuf,
}

impl SavedRecordingStore {
    /// Create a store using the platform-standard local app data directory.
    ///
    /// # Errors
    ///
    /// Returns [`SavedRecordingError::NoDataDir`] if the platform data
    /// directory cannot be determined.
    pub fn open() -> Result<Self, SavedRecordingError> {
        let dirs =
            ProjectDirs::from("dev", "tin", "dictate").ok_or(SavedRecordingError::NoDataDir)?;
        let dir = dirs.data_local_dir().to_path_buf();
        Ok(Self::open_at(dir))
    }

    /// Create a store rooted at a specific directory (for testing).
    #[must_use]
    pub fn open_at(dir: PathBuf) -> Self {
        let manifest_path = dir.join(MANIFEST_FILENAME);
        Self { dir, manifest_path }
    }

    /// Save the given recording by atomically replacing the manifest.
    ///
    /// # Errors
    ///
    /// Returns [`SavedRecordingError`] on serialization, encoding, or file IO
    /// failures.
    pub fn save(&self, recording: &SavedRecording) -> Result<(), SavedRecordingError> {
        self.save_with_cancellation(recording, &CancellationContext::new())
            .map_err(|err| match err {
                CancellationError::Cancelled => {
                    unreachable!("fresh cancellation context cannot be cancelled")
                }
                CancellationError::Error(err) => err,
            })
    }

    /// Save the given recording while allowing cancellation before activation.
    ///
    /// The new audio is staged on disk first. Cancellation is checked before
    /// the active manifest is replaced so a cancelled session does not become
    /// the new retry target. Any staged files are cleaned up on cancellation.
    ///
    /// # Errors
    ///
    /// Returns [`CancellationError`] with [`SavedRecordingError`] if
    /// serialization, encoding, or file IO fails, or `Cancelled` if
    /// cancellation is requested before the new manifest is activated.
    pub fn save_with_cancellation(
        &self,
        recording: &SavedRecording,
        cancellation: &CancellationContext,
    ) -> CancellationResult<(), SavedRecordingError> {
        let manifest = recording.manifest.clone();
        manifest.validate().map_err(CancellationError::Error)?;
        let audio = WavEncoder
            .encode(&recording.samples, TRANSCRIPTION_SAMPLE_RATE)
            .map_err(|err| {
                CancellationError::Error(SavedRecordingError::InvalidAudio(err.to_string()))
            })?;

        if let Some(parent) = self.dir.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(SavedRecordingError::from)
                .map_err(CancellationError::Error)?;
        }
        std::fs::create_dir_all(&self.dir)
            .map_err(SavedRecordingError::from)
            .map_err(CancellationError::Error)?;

        let manifest_json = serde_json::to_string_pretty(&manifest)
            .map_err(SavedRecordingError::from)
            .map_err(CancellationError::Error)?;
        let manifest_tmp_path = self.dir.join(MANIFEST_TMP_FILENAME);
        let audio_tmp_path = self.dir.join(AUDIO_TMP_FILENAME);
        let audio_path = self.audio_path_for_filename(&manifest.audio_filename);

        std::fs::write(&audio_tmp_path, audio.data())
            .map_err(SavedRecordingError::from)
            .map_err(CancellationError::Error)?;
        std::fs::write(&manifest_tmp_path, manifest_json)
            .map_err(SavedRecordingError::from)
            .map_err(CancellationError::Error)?;
        if cancellation.check().is_err() {
            cleanup_staged_save_paths([
                audio_tmp_path.as_path(),
                manifest_tmp_path.as_path(),
                audio_path.as_path(),
            ]);
            return Err(CancellationError::Cancelled);
        }
        std::fs::rename(&audio_tmp_path, &audio_path)
            .map_err(SavedRecordingError::from)
            .map_err(CancellationError::Error)?;
        if cancellation.check().is_err() {
            cleanup_staged_save_paths([
                audio_tmp_path.as_path(),
                manifest_tmp_path.as_path(),
                audio_path.as_path(),
            ]);
            return Err(CancellationError::Cancelled);
        }
        std::fs::rename(&manifest_tmp_path, &self.manifest_path)
            .map_err(SavedRecordingError::from)
            .map_err(CancellationError::Error)?;
        self.cleanup_stale_audio_files(&manifest.audio_filename)
            .map_err(CancellationError::Error)?;

        Ok(())
    }

    /// Load the saved recording.
    ///
    /// # Errors
    ///
    /// Returns [`SavedRecordingError::NoSavedRecording`] when either the audio
    /// or manifest file is missing, or another [`SavedRecordingError`] when the
    /// saved files are malformed.
    pub fn load(&self) -> Result<SavedRecording, SavedRecordingError> {
        if !self.manifest_path.exists() {
            return Err(SavedRecordingError::NoSavedRecording);
        }

        let manifest_json = std::fs::read_to_string(&self.manifest_path)?;
        let manifest: SavedRecordingManifest = serde_json::from_str(&manifest_json)?;
        manifest.validate()?;
        let audio_path = self.audio_path_for_filename(&manifest.audio_filename);
        if !audio_path.exists() {
            return Err(SavedRecordingError::NoSavedRecording);
        }

        let audio_bytes = std::fs::read(audio_path)?;
        let samples = decode_wav_pcm16_mono_16khz(&audio_bytes)?;

        if samples.len() != manifest.sample_count {
            return Err(SavedRecordingError::InvalidAudio(format!(
                "manifest expected {} samples, found {}",
                manifest.sample_count,
                samples.len()
            )));
        }

        Ok(SavedRecording { manifest, samples })
    }

    /// The resolved manifest file path.
    #[must_use]
    pub fn manifest_path(&self) -> &Path {
        &self.manifest_path
    }

    fn audio_path_for_filename(&self, filename: &str) -> PathBuf {
        self.dir.join(filename)
    }

    fn cleanup_stale_audio_files(
        &self,
        active_audio_filename: &str,
    ) -> Result<(), SavedRecordingError> {
        let entries = std::fs::read_dir(&self.dir)?;

        for entry in entries {
            let path = entry?.path();
            if !path.is_file() {
                continue;
            }

            let Some(filename) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };

            let has_wav_extension = path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"));
            let is_saved_audio = filename.starts_with(AUDIO_GENERATION_PREFIX) && has_wav_extension;
            if is_saved_audio && filename != active_audio_filename {
                std::fs::remove_file(path)?;
            }
        }

        Ok(())
    }
}

fn next_audio_filename() -> String {
    let timestamp_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let counter = AUDIO_GENERATION_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(
        "{AUDIO_GENERATION_PREFIX}{}-{timestamp_nanos}-{counter}.wav",
        std::process::id()
    )
}

fn cleanup_staged_save_paths<'a>(paths: impl IntoIterator<Item = &'a Path>) {
    for path in paths {
        match std::fs::remove_file(path) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Ok(()) | Err(_) => {}
        }
    }
}

fn validate_audio_filename(value: &str) -> Result<(), SavedRecordingError> {
    if value.is_empty() {
        return Err(SavedRecordingError::InvalidManifest(
            "audio filename must not be empty".to_string(),
        ));
    }

    let mut components = Path::new(value).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(()),
        _ => Err(SavedRecordingError::InvalidManifest(format!(
            "audio filename {value:?} must be a plain file name"
        ))),
    }
}

fn parse_response_format(value: &str) -> Result<ResponseFormat, SavedRecordingError> {
    value.parse::<ResponseFormat>().map_err(|err| {
        SavedRecordingError::InvalidManifest(format!("invalid response format {value:?}: {err}"))
    })
}

fn parse_whisper_model(value: &str) -> Result<WhisperModel, SavedRecordingError> {
    value.parse::<WhisperModel>().map_err(|err| {
        SavedRecordingError::InvalidManifest(format!("invalid whisper model {value:?}: {err}"))
    })
}

const fn default_transcription_model_id(
    provider: TranscriptionProviderKind,
    model: WhisperModel,
) -> Option<&'static str> {
    match provider {
        TranscriptionProviderKind::Groq => match model {
            WhisperModel::LargeV3Turbo => Some("whisper-large-v3-turbo"),
            WhisperModel::LargeV3 => Some("whisper-large-v3"),
        },
        TranscriptionProviderKind::Fireworks => match model {
            WhisperModel::LargeV3Turbo => Some("whisper-v3-turbo"),
            WhisperModel::LargeV3 => Some("whisper-v3"),
        },
        TranscriptionProviderKind::OpenAiCompatible => None,
    }
}

fn parse_timestamp_granularity(value: &str) -> Result<TimestampGranularity, SavedRecordingError> {
    value.parse::<TimestampGranularity>().map_err(|err| {
        SavedRecordingError::InvalidManifest(format!(
            "invalid timestamp granularity {value:?}: {err}"
        ))
    })
}

fn decode_wav_pcm16_mono_16khz(bytes: &[u8]) -> Result<Vec<f32>, SavedRecordingError> {
    if bytes.len() < WAV_HEADER_SIZE {
        return Err(SavedRecordingError::InvalidAudio(
            "WAV file is too small".to_string(),
        ));
    }
    if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(SavedRecordingError::InvalidAudio(
            "missing RIFF/WAVE header".to_string(),
        ));
    }
    if &bytes[12..16] != b"fmt " || &bytes[36..40] != b"data" {
        return Err(SavedRecordingError::InvalidAudio(
            "unsupported WAV chunk layout".to_string(),
        ));
    }

    let fmt_size = u32::from_le_bytes(bytes[16..20].try_into().expect("slice length"));
    let audio_format = u16::from_le_bytes(bytes[20..22].try_into().expect("slice length"));
    let channels = u16::from_le_bytes(bytes[22..24].try_into().expect("slice length"));
    let sample_rate = u32::from_le_bytes(bytes[24..28].try_into().expect("slice length"));
    let bits_per_sample = u16::from_le_bytes(bytes[34..36].try_into().expect("slice length"));
    let data_size = usize::try_from(u32::from_le_bytes(
        bytes[40..44].try_into().expect("slice length"),
    ))
    .expect("u32 fits into usize");

    if fmt_size != 16 || audio_format != 1 {
        return Err(SavedRecordingError::InvalidAudio(
            "only PCM WAV files are supported".to_string(),
        ));
    }
    if channels != SUPPORTED_CHANNELS {
        return Err(SavedRecordingError::InvalidAudio(format!(
            "expected mono audio, found {channels} channels"
        )));
    }
    if sample_rate != TRANSCRIPTION_SAMPLE_RATE {
        return Err(SavedRecordingError::InvalidAudio(format!(
            "expected {TRANSCRIPTION_SAMPLE_RATE} Hz audio, found {sample_rate} Hz"
        )));
    }
    if bits_per_sample != SUPPORTED_BITS_PER_SAMPLE {
        return Err(SavedRecordingError::InvalidAudio(format!(
            "expected 16-bit PCM audio, found {bits_per_sample}-bit"
        )));
    }
    if bytes.len() != WAV_HEADER_SIZE + data_size {
        return Err(SavedRecordingError::InvalidAudio(
            "WAV file length does not match data header".to_string(),
        ));
    }
    if data_size % 2 != 0 {
        return Err(SavedRecordingError::InvalidAudio(
            "PCM16 payload must contain an even number of bytes".to_string(),
        ));
    }

    let mut samples = Vec::with_capacity(data_size / 2);
    for chunk in bytes[WAV_HEADER_SIZE..].chunks_exact(2) {
        let pcm = i16::from_le_bytes([chunk[0], chunk[1]]);
        samples.push(f32::from(pcm) / f32::from(i16::MAX));
    }

    Ok(samples)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_recording() -> SavedRecording {
        let samples = vec![0.0, 0.25, -0.5, 1.0, -1.0];
        let pipeline = PipelineConfig {
            transcription_provider: TranscriptionProviderKind::Fireworks,
            base_url: Some("https://whisper.example.com/v1/audio/transcriptions".to_string()),
            language: Some("en".to_string()),
            prompt: Some("Use correct punctuation.".to_string()),
            response_format: ResponseFormat::VerboseJson,
            transcription_model: Some(WhisperModel::LargeV3),
            transcription_model_id: Some("whisper-v3".to_string()),
            temperature: Some(0.2),
            timestamp_granularities: vec![
                TimestampGranularity::Word,
                TimestampGranularity::Segment,
            ],
            post_process: true,
            post_process_provider: PostProcessProviderKind::Fireworks,
            post_process_model: Some(ModelId::new("openai/gpt-oss-20b").unwrap()),
            post_process_base_url: Some(
                "https://chat.example.com/openai/v1/chat/completions".to_string(),
            ),
            request_policies: crate::request_policy::RequestPolicies::default(),
        };

        SavedRecording {
            manifest: SavedRecordingManifest::new(
                samples.len(),
                90,
                Some(ResponseFormat::VerboseJson),
                &pipeline,
            ),
            samples,
        }
    }

    fn temp_store() -> (TempDir, SavedRecordingStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = SavedRecordingStore::open_at(dir.path().to_path_buf());
        (dir, store)
    }

    #[test]
    fn save_and_load_round_trip() {
        let (_dir, store) = temp_store();
        let recording = sample_recording();

        store.save(&recording).unwrap();
        let loaded = store.load().unwrap();

        assert_eq!(loaded.manifest, recording.manifest);
        assert_eq!(loaded.samples.len(), recording.samples.len());
        for (loaded_sample, saved_sample) in loaded.samples.iter().zip(&recording.samples) {
            assert!((loaded_sample - saved_sample).abs() < 0.000_1);
        }
    }

    #[test]
    fn load_missing_returns_no_saved_recording() {
        let (_dir, store) = temp_store();
        let result = store.load();
        assert!(matches!(result, Err(SavedRecordingError::NoSavedRecording)));
    }

    #[test]
    fn load_rejects_malformed_manifest_json() {
        let (_dir, store) = temp_store();
        std::fs::create_dir_all(store.manifest_path().parent().unwrap()).unwrap();
        std::fs::write(store.manifest_path(), "{not json").unwrap();

        let result = store.load();
        assert!(matches!(result, Err(SavedRecordingError::ManifestJson(_))));
    }

    #[test]
    fn load_rejects_invalid_audio() {
        let (_dir, store) = temp_store();
        let recording = sample_recording();
        std::fs::create_dir_all(store.manifest_path().parent().unwrap()).unwrap();
        std::fs::write(
            store.manifest_path(),
            serde_json::to_string_pretty(&recording.manifest).unwrap(),
        )
        .unwrap();
        std::fs::write(
            store.audio_path_for_filename(&recording.manifest.audio_filename),
            b"not a wav",
        )
        .unwrap();

        let result = store.load();
        assert!(matches!(result, Err(SavedRecordingError::InvalidAudio(_))));
    }

    #[test]
    fn load_rejects_manifest_audio_mismatch() {
        let (_dir, store) = temp_store();
        let mut recording = sample_recording();
        recording.manifest.sample_count += 1;
        store.save(&recording).unwrap();

        let result = store.load();
        assert!(matches!(result, Err(SavedRecordingError::InvalidAudio(_))));
    }

    #[test]
    fn save_cleans_up_tmp_files() {
        let (_dir, store) = temp_store();
        store.save(&sample_recording()).unwrap();

        let manifest_tmp = store.manifest_path().with_file_name(MANIFEST_TMP_FILENAME);
        let audio_tmp = store.dir.join(AUDIO_TMP_FILENAME);
        assert!(!manifest_tmp.exists());
        assert!(!audio_tmp.exists());
    }

    #[test]
    fn cancelled_save_preserves_previous_recording_and_cleans_up_staged_files() {
        let (_dir, store) = temp_store();
        let existing = sample_recording();
        store.save(&existing).unwrap();

        let replacement = SavedRecording {
            manifest: SavedRecordingManifest::new(
                12,
                45,
                Some(ResponseFormat::Json),
                &PipelineConfig::default(),
            ),
            samples: vec![0.5; 12],
        };
        let replacement_audio_path =
            store.audio_path_for_filename(&replacement.manifest.audio_filename);
        let cancellation = CancellationContext::new();
        cancellation.cancel();

        let result = store.save_with_cancellation(&replacement, &cancellation);

        assert!(matches!(result, Err(CancellationError::Cancelled)));
        let loaded = store.load().unwrap();
        assert_eq!(loaded.manifest, existing.manifest);
        assert_eq!(loaded.samples.len(), existing.samples.len());
        assert!(!replacement_audio_path.exists());
        assert!(!store.dir.join(AUDIO_TMP_FILENAME).exists());
        assert!(!store.dir.join(MANIFEST_TMP_FILENAME).exists());
    }

    #[test]
    fn load_rejects_manifest_without_audio_filename() {
        let (_dir, store) = temp_store();
        let recording = sample_recording();
        let mut manifest_json = serde_json::to_value(&recording.manifest).unwrap();
        manifest_json
            .as_object_mut()
            .unwrap()
            .remove("audio_filename")
            .unwrap();

        std::fs::create_dir_all(store.manifest_path().parent().unwrap()).unwrap();
        std::fs::write(
            store.manifest_path(),
            serde_json::to_string_pretty(&manifest_json).unwrap(),
        )
        .unwrap();

        let result = store.load();
        assert!(matches!(result, Err(SavedRecordingError::ManifestJson(_))));
    }

    #[test]
    fn load_uses_manifest_referenced_audio_when_newer_audio_is_staged() {
        let (_dir, store) = temp_store();
        let recording = sample_recording();
        store.save(&recording).unwrap();

        let staged = SavedRecording {
            manifest: SavedRecordingManifest {
                audio_filename: format!("{AUDIO_GENERATION_PREFIX}staged.wav"),
                ..sample_recording().manifest
            },
            samples: vec![0.5; 12],
        };
        let staged_audio = WavEncoder
            .encode(&staged.samples, TRANSCRIPTION_SAMPLE_RATE)
            .unwrap();
        std::fs::write(
            store.audio_path_for_filename(&staged.manifest.audio_filename),
            staged_audio.data(),
        )
        .unwrap();

        let loaded = store.load().unwrap();
        assert_eq!(
            loaded.manifest.audio_filename,
            recording.manifest.audio_filename
        );
        assert_eq!(loaded.samples.len(), recording.samples.len());
    }

    #[test]
    fn save_removes_stale_audio_generations() {
        let (dir, store) = temp_store();
        let first = sample_recording();
        let second = sample_recording();

        store.save(&first).unwrap();
        store.save(&second).unwrap();

        let wav_files = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"))
            })
            .collect::<Vec<_>>();

        assert_eq!(wav_files.len(), 1);
        assert_eq!(
            wav_files[0],
            dir.path().join(&second.manifest.audio_filename)
        );
    }

    #[test]
    fn pipeline_manifest_round_trip() {
        let recording = sample_recording();
        let config = recording.manifest.pipeline.to_pipeline_config().unwrap();

        assert_eq!(
            config.base_url.as_deref(),
            Some("https://whisper.example.com/v1/audio/transcriptions")
        );
        assert_eq!(
            config.transcription_provider,
            TranscriptionProviderKind::Fireworks
        );
        assert_eq!(config.language.as_deref(), Some("en"));
        assert_eq!(config.prompt.as_deref(), Some("Use correct punctuation."));
        assert_eq!(config.response_format, ResponseFormat::VerboseJson);
        assert_eq!(config.transcription_model, Some(WhisperModel::LargeV3));
        assert_eq!(config.transcription_model_id.as_deref(), Some("whisper-v3"));
        assert_eq!(config.temperature, Some(0.2));
        assert_eq!(
            config.timestamp_granularities,
            vec![TimestampGranularity::Word, TimestampGranularity::Segment]
        );
        assert!(config.post_process);
        assert_eq!(
            config.post_process_provider,
            PostProcessProviderKind::Fireworks
        );
        assert_eq!(
            config.post_process_model.as_ref().map(ModelId::as_str),
            Some("openai/gpt-oss-20b")
        );
        assert_eq!(
            config.post_process_base_url.as_deref(),
            Some("https://chat.example.com/openai/v1/chat/completions")
        );
    }

    #[test]
    fn output_format_round_trip() {
        let manifest = sample_recording().manifest;
        assert_eq!(
            manifest.output_format().unwrap(),
            Some(ResponseFormat::VerboseJson)
        );
    }

    #[test]
    fn version_one_manifest_defaults_missing_providers_to_groq() {
        let manifest_json = serde_json::json!({
            "version": 1,
            "sample_rate_hz": TRANSCRIPTION_SAMPLE_RATE,
            "channels": SUPPORTED_CHANNELS,
            "sample_count": 5,
            "audio_filename": "last-recording-test.wav",
            "chunk_target_duration_secs": 30,
            "output_format": "json",
            "pipeline": {
                "base_url": "https://api.groq.com/openai/v1/audio/transcriptions",
                "language": "en",
                "prompt": "hello",
                "response_format": "json",
                "transcription_model": "whisper-large-v3",
                "temperature": 0.0,
                "timestamp_granularities": [],
                "post_process": false,
                "post_process_model": null,
                "post_process_base_url": null
            }
        });

        let manifest: SavedRecordingManifest = serde_json::from_value(manifest_json).unwrap();
        manifest.validate().unwrap();
        let config = manifest.pipeline.to_pipeline_config().unwrap();

        assert_eq!(
            config.transcription_provider,
            TranscriptionProviderKind::Groq
        );
        assert_eq!(config.post_process_provider, PostProcessProviderKind::Groq);
        assert_eq!(
            config.transcription_model_id.as_deref(),
            Some("whisper-large-v3")
        );
    }

    #[test]
    fn open_at_reports_expected_paths() {
        let dir = PathBuf::from("/tmp/dictate-tests");
        let store = SavedRecordingStore::open_at(dir.clone());
        assert_eq!(store.manifest_path(), dir.join(MANIFEST_FILENAME));
        assert_eq!(store.dir, dir);
    }
}
