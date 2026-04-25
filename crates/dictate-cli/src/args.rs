use std::time::Duration;

use clap::{Args, Parser, Subcommand};
use dictate_core::ModelId;

/// Parse and validate temperature in the 0.0–1.0 range.
fn parse_temperature(s: &str) -> Result<f32, String> {
    let temp: f32 = s
        .parse()
        .map_err(|_| format!("'{s}' is not a valid number"))?;
    if !(0.0..=1.0).contains(&temp) {
        return Err(format!("temperature {temp} is out of range (0.0–1.0)"));
    }
    Ok(temp)
}

fn invalid_duration(trimmed: &str) -> String {
    format!(
        "invalid duration '{trimmed}'; use a positive number optionally suffixed with ms, s, m, or h"
    )
}

/// Parse a positive duration with optional unit suffix.
///
/// Supported suffixes:
/// - `ms` for milliseconds
/// - `s` for seconds
/// - `m` for minutes
/// - `h` for hours
///
/// Bare numbers default to seconds.
fn parse_duration(s: &str) -> Result<Duration, String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err("duration cannot be empty".to_string());
    }

    let (value, multiplier) = [("ms", 0.001), ("s", 1.0), ("m", 60.0), ("h", 3600.0)]
        .into_iter()
        .find_map(|(suffix, multiplier)| {
            trimmed
                .strip_suffix(suffix)
                .map(|value| (value, multiplier))
        })
        .unwrap_or((trimmed, 1.0));

    let amount = value
        .parse::<f64>()
        .map_err(|_| invalid_duration(trimmed))?;

    if !amount.is_finite() || amount <= 0.0 {
        return Err(invalid_duration(trimmed));
    }

    let seconds = amount * multiplier;
    if !seconds.is_finite() || seconds <= 0.0 {
        return Err(invalid_duration(trimmed));
    }

    Duration::try_from_secs_f64(seconds).map_err(|_| invalid_duration(trimmed))
}

#[derive(Parser)]
#[command(
    name = "dictate",
    version,
    about = "Record audio, transcribe, and copy to clipboard",
    after_help = "Run `dictate` without arguments to record from the default input device.",
    args_conflicts_with_subcommands = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    #[command(flatten)]
    pub record_args: RecordArgs,
}

#[derive(Subcommand)]
pub enum Commands {
    /// List available audio input devices
    Devices,

    /// Record audio until Enter or `--stop-after`; Ctrl+C cancels the session
    Record(RecordArgs),

    /// Reuse the last saved recording and rerun transcription; Ctrl+C cancels the session
    Retry(RetryArgs),

    /// Manage vocabulary words for transcription biasing
    Vocab(VocabArgs),

    /// Generate shell completions
    #[command(hide = true)]
    Completions(CompletionsArgs),
}

#[derive(Args)]
pub struct VocabArgs {
    #[command(subcommand)]
    pub command: VocabCommand,
}

#[derive(Subcommand)]
pub enum VocabCommand {
    /// Add one or more vocabulary words
    #[command(
        long_about = "Add one or more vocabulary words used to bias transcription.\n\
Words that already exist in vocabulary are skipped with a warning."
    )]
    Add {
        /// Words to add (e.g., `AWS` `OpenAI` `Kubernetes`)
        #[arg(required = true)]
        words: Vec<String>,
    },

    /// Remove one or more vocabulary words
    Remove {
        /// Words to remove
        #[arg(required = true)]
        words: Vec<String>,
    },

    /// List all vocabulary words
    List,

    /// Edit vocabulary words in `$VISUAL` or `$EDITOR`
    Edit,
}

#[derive(Args)]
pub struct CompletionsArgs {
    /// Shell to generate completions for
    pub shell: clap_complete::Shell,
}

#[derive(Args)]
pub struct OutputArgs {
    /// Print transcript to stdout while still copying to clipboard
    #[arg(long, conflicts_with = "no_clipboard")]
    pub stdout: bool,

    /// Skip clipboard entirely (headless/scripted use). Prints to stdout.
    #[arg(long, conflicts_with = "stdout")]
    pub no_clipboard: bool,
}

#[derive(Args)]
pub struct TranscriptionArgs {
    /// Transcription provider (`groq`, `fireworks`, or `openai-compatible`)
    #[arg(long = "transcription-provider", value_parser = ["groq", "fireworks", "openai-compatible"])]
    pub provider: Option<String>,

    /// Override transcription API URL (provider env/defaults are used when omitted)
    #[arg(long)]
    pub base_url: Option<String>,

    /// Emit machine-readable JSONL progress events on stderr
    #[arg(long)]
    pub json_events: bool,

    /// ISO-639-1 language code (e.g., "en", "es", "fr") to improve accuracy and latency
    #[arg(long)]
    pub language: Option<String>,

    /// Text to guide transcription style or spelling (max 224 tokens)
    #[arg(long)]
    pub prompt: Option<String>,

    /// Response format: "json" (default), "`verbose_json`", or "text"
    #[arg(long, value_parser = ["json", "verbose_json", "text"])]
    pub format: Option<String>,

    /// Semantic transcription preset: `large-v3-turbo` (default, faster) or `large-v3` (more accurate)
    #[arg(long, value_parser = ["large-v3-turbo", "large-v3"], conflicts_with = "transcription_model_id")]
    pub transcription_model: Option<String>,

    /// Raw provider model id for transcription
    #[arg(long, conflicts_with = "transcription_model")]
    pub transcription_model_id: Option<String>,

    /// Sampling temperature (0.0-1.0). Default 0.0 is recommended for transcription
    #[arg(long, value_parser = parse_temperature)]
    pub temperature: Option<f32>,

    /// Timestamp granularities: "segment", "word", or both (comma-separated).
    /// Requires --format `verbose_json`. Example: --timestamps word,segment
    #[arg(long = "timestamps", value_delimiter = ',', value_parser = ["word", "segment"])]
    pub timestamp_granularities: Option<Vec<String>>,

    #[command(flatten)]
    pub output: OutputArgs,
}

#[derive(Args)]
pub struct RecordPostProcessArgs {
    /// Post-process transcription with LLM for better punctuation and formatting
    #[arg(long = "post-process", short = 'p', id = "post_process")]
    pub enabled: bool,

    /// Post-process provider (`groq`, `fireworks`, or `openai-compatible`)
    #[arg(
        long = "post-process-provider",
        requires = "post_process",
        value_parser = ["groq", "fireworks", "openai-compatible"],
        id = "post_process_provider"
    )]
    pub provider: Option<String>,

    /// Model for post-processing (default: openai/gpt-oss-20b)
    #[arg(
        long = "post-process-model",
        requires = "post_process",
        id = "post_process_model"
    )]
    pub model: Option<ModelId>,

    /// Override post-processing chat API URL
    #[arg(
        long = "post-process-base-url",
        requires = "post_process",
        id = "post_process_base_url"
    )]
    pub base_url: Option<String>,

    /// Supplemental context sent only to the post-processing request
    #[arg(
        long = "post-process-context",
        requires = "post_process",
        id = "post_process_context"
    )]
    pub context: Option<String>,
}

#[derive(Args)]
pub struct RetryPostProcessArgs {
    /// Post-process transcription with LLM for better punctuation and formatting
    #[arg(
        long = "post-process",
        short = 'p',
        conflicts_with = "no_post_process",
        id = "post_process"
    )]
    pub enabled: bool,

    /// Skip post-processing even if the saved recording used it
    #[arg(
        long = "no-post-process",
        conflicts_with = "post_process",
        id = "no_post_process"
    )]
    pub disabled: bool,

    /// Post-process provider (`groq`, `fireworks`, or `openai-compatible`)
    #[arg(
        long = "post-process-provider",
        conflicts_with = "no_post_process",
        value_parser = ["groq", "fireworks", "openai-compatible"],
        id = "post_process_provider"
    )]
    pub provider: Option<String>,

    /// Model for post-processing (default: openai/gpt-oss-20b)
    #[arg(
        long = "post-process-model",
        conflicts_with = "no_post_process",
        id = "post_process_model"
    )]
    pub model: Option<ModelId>,

    /// Override post-processing chat API URL (falls back to saved settings, then `GROQ_CHAT_BASE_URL`)
    #[arg(
        long = "post-process-base-url",
        conflicts_with = "no_post_process",
        id = "post_process_base_url"
    )]
    pub base_url: Option<String>,

    /// Supplemental context sent only to this retry post-processing request
    #[arg(
        long = "post-process-context",
        conflicts_with = "no_post_process",
        id = "post_process_context"
    )]
    pub context: Option<String>,
}

#[derive(Args)]
pub struct RecordArgs {
    /// Select a specific audio input device (index from `devices`, or case-insensitive partial name match)
    #[arg(long)]
    pub device: Option<String>,

    /// Automatically stop recording after this duration (e.g. `30s`, `2m`, `500ms`)
    #[arg(long, value_name = "DURATION", value_parser = parse_duration)]
    pub stop_after: Option<Duration>,

    #[command(flatten)]
    pub transcription: TranscriptionArgs,

    #[command(flatten)]
    pub post_process: RecordPostProcessArgs,

    /// Save the captured audio locally so `dictate retry` can reuse it later
    #[arg(long)]
    pub save_last_audio: bool,
}

#[derive(Args)]
pub struct RetryArgs {
    #[command(flatten)]
    pub transcription: TranscriptionArgs,

    #[command(flatten)]
    pub post_process: RetryPostProcessArgs,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn parse_cli<'a>(args: impl IntoIterator<Item = &'a str>) -> Cli {
        Cli::parse_from(std::iter::once("dictate").chain(args))
    }

    fn parse_record<'a>(args: impl IntoIterator<Item = &'a str>) -> RecordArgs {
        let cli = parse_cli(std::iter::once("record").chain(args));

        let Some(Commands::Record(args)) = cli.command else {
            panic!("expected record subcommand");
        };

        args
    }

    fn parse_retry<'a>(args: impl IntoIterator<Item = &'a str>) -> RetryArgs {
        let cli = parse_cli(std::iter::once("retry").chain(args));

        let Some(Commands::Retry(args)) = cli.command else {
            panic!("expected retry subcommand");
        };

        args
    }

    fn parse_vocab<'a>(args: impl IntoIterator<Item = &'a str>) -> VocabArgs {
        let cli = parse_cli(std::iter::once("vocab").chain(args));

        let Some(Commands::Vocab(args)) = cli.command else {
            panic!("expected vocab subcommand");
        };

        args
    }

    fn parse_completions<'a>(args: impl IntoIterator<Item = &'a str>) -> CompletionsArgs {
        let cli = parse_cli(std::iter::once("completions").chain(args));

        let Some(Commands::Completions(args)) = cli.command else {
            panic!("expected completions subcommand");
        };

        args
    }

    fn parse_top_level<'a>(args: impl IntoIterator<Item = &'a str>) -> RecordArgs {
        let cli = parse_cli(args);

        assert!(cli.command.is_none());

        cli.record_args
    }

    #[test]
    fn record_accepts_base_url_flag() {
        let args = parse_record([
            "--base-url",
            "http://127.0.0.1:8080/openai/v1/audio/transcriptions",
        ]);

        assert_eq!(
            args.transcription.base_url.as_deref(),
            Some("http://127.0.0.1:8080/openai/v1/audio/transcriptions")
        );
    }

    #[test]
    fn record_defaults_base_url_to_none() {
        let args = parse_record([]);

        assert_eq!(args.transcription.base_url, None);
    }

    #[test]
    fn record_save_last_audio_flag_can_be_enabled() {
        let args = parse_record(["--save-last-audio"]);

        assert!(args.save_last_audio);
    }

    #[test]
    fn record_accepts_stop_after_flag() {
        let args = parse_record(["--stop-after", "2.5s"]);

        assert_eq!(args.stop_after, Some(Duration::from_secs_f64(2.5)));
    }

    #[test]
    fn record_rejects_non_positive_stop_after() {
        let result = Cli::try_parse_from(["dictate", "record", "--stop-after", "0"]);
        assert!(result.is_err());
    }

    #[test]
    fn record_rejects_oversized_stop_after() {
        let result = Cli::try_parse_from(["dictate", "record", "--stop-after", "1e308h"]);
        assert!(result.is_err());
    }

    #[test]
    fn retry_rejects_stop_after_flag() {
        let result = Cli::try_parse_from(["dictate", "retry", "--stop-after", "30s"]);
        assert!(result.is_err());
    }

    #[test]
    fn retry_accepts_base_url_flag() {
        let args = parse_retry([
            "--json-events",
            "--base-url",
            "http://127.0.0.1:8080/openai/v1/audio/transcriptions",
        ]);

        assert_eq!(
            args.transcription.base_url.as_deref(),
            Some("http://127.0.0.1:8080/openai/v1/audio/transcriptions")
        );
        assert!(args.transcription.json_events);
    }

    #[test]
    fn record_accepts_json_events_flag() {
        let args = parse_record(["--json-events"]);

        assert!(args.transcription.json_events);
    }

    #[test]
    fn retry_rejects_device_flag() {
        let result = Cli::try_parse_from(["dictate", "retry", "--device", "USB"]);
        assert!(result.is_err());
    }

    #[test]
    fn retry_no_post_process_flag() {
        let args = parse_retry(["--no-post-process"]);

        assert!(args.post_process.disabled);
        assert!(!args.post_process.enabled);
    }

    #[test]
    fn retry_post_process_and_no_post_process_conflict() {
        let result =
            Cli::try_parse_from(["dictate", "retry", "--post-process", "--no-post-process"]);
        assert!(result.is_err());
    }

    #[test]
    fn retry_accepts_post_process_model_without_post_process_flag() {
        let args = parse_retry(["--post-process-model", "openai/gpt-oss-20b"]);

        assert_eq!(
            args.post_process.model.as_ref().map(ModelId::as_str),
            Some("openai/gpt-oss-20b")
        );
    }

    #[test]
    fn retry_accepts_post_process_base_url_without_post_process_flag() {
        let args = parse_retry([
            "--post-process-base-url",
            "http://127.0.0.1:8080/openai/v1/chat/completions",
        ]);

        assert_eq!(
            args.post_process.base_url.as_deref(),
            Some("http://127.0.0.1:8080/openai/v1/chat/completions")
        );
    }

    #[test]
    fn retry_rejects_post_process_model_when_no_post_process_is_set() {
        let result = Cli::try_parse_from([
            "dictate",
            "retry",
            "--no-post-process",
            "--post-process-model",
            "openai/gpt-oss-20b",
        ]);

        assert!(result.is_err());
    }

    #[test]
    fn retry_rejects_post_process_base_url_when_no_post_process_is_set() {
        let result = Cli::try_parse_from([
            "dictate",
            "retry",
            "--no-post-process",
            "--post-process-base-url",
            "http://127.0.0.1:8080/openai/v1/chat/completions",
        ]);

        assert!(result.is_err());
    }

    #[test]
    fn record_accepts_transcription_model_flag() {
        let args = parse_record(["--transcription-model", "large-v3"]);

        assert_eq!(
            args.transcription.transcription_model.as_deref(),
            Some("large-v3")
        );
    }

    #[test]
    fn record_accepts_transcription_provider_and_model_id_flags() {
        let args = parse_record([
            "--transcription-provider",
            "fireworks",
            "--transcription-model-id",
            "whisper-v3",
        ]);

        assert_eq!(args.transcription.provider.as_deref(), Some("fireworks"));
        assert_eq!(
            args.transcription.transcription_model_id.as_deref(),
            Some("whisper-v3")
        );
    }

    #[test]
    fn record_rejects_transcription_model_and_model_id_together() {
        let result = Cli::try_parse_from([
            "dictate",
            "record",
            "--transcription-model",
            "large-v3",
            "--transcription-model-id",
            "whisper-v3",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn record_rejects_legacy_model_flag() {
        let result = Cli::try_parse_from(["dictate", "record", "--model", "whisper-large-v3"]);
        assert!(result.is_err());
    }

    #[test]
    fn temperature_rejects_out_of_range() {
        let result = Cli::try_parse_from(["dictate", "record", "--temperature", "1.5"]);
        assert!(result.is_err());
    }

    #[test]
    fn temperature_accepts_valid_value() {
        let args = parse_record(["--temperature", "0.5"]);

        assert_eq!(args.transcription.temperature, Some(0.5));
    }

    #[test]
    fn stdout_flag_defaults_to_false() {
        let args = parse_record([]);

        assert!(!args.transcription.output.stdout);
    }

    #[test]
    fn stdout_flag_can_be_enabled() {
        let args = parse_record(["--stdout"]);

        assert!(args.transcription.output.stdout);
    }

    #[test]
    fn no_clipboard_flag_defaults_to_false() {
        let args = parse_record([]);

        assert!(!args.transcription.output.no_clipboard);
    }

    #[test]
    fn no_clipboard_flag_can_be_enabled() {
        let args = parse_record(["--no-clipboard"]);

        assert!(args.transcription.output.no_clipboard);
    }

    #[test]
    fn stdout_and_no_clipboard_conflict() {
        let result = Cli::try_parse_from(["dictate", "record", "--stdout", "--no-clipboard"]);
        assert!(result.is_err());
    }

    // --- Top-level flag tests (no `record` subcommand) ---

    #[test]
    fn top_level_stdout_flag() {
        let args = parse_top_level(["--stdout"]);
        assert!(args.transcription.output.stdout);
    }

    #[test]
    fn top_level_no_clipboard_flag() {
        let args = parse_top_level(["--no-clipboard"]);
        assert!(args.transcription.output.no_clipboard);
    }

    #[test]
    fn top_level_device_flag() {
        let args = parse_top_level(["--device", "USB"]);
        assert_eq!(args.device.as_deref(), Some("USB"));
    }

    #[test]
    fn top_level_language_flag() {
        let args = parse_top_level(["--language", "en"]);
        assert_eq!(args.transcription.language.as_deref(), Some("en"));
    }

    #[test]
    fn top_level_multiple_flags() {
        let args = parse_top_level([
            "--stdout",
            "--language",
            "es",
            "--device",
            "mic",
            "--stop-after",
            "45s",
        ]);
        assert!(args.transcription.output.stdout);
        assert_eq!(args.transcription.language.as_deref(), Some("es"));
        assert_eq!(args.device.as_deref(), Some("mic"));
        assert_eq!(args.stop_after, Some(Duration::from_secs(45)));
    }

    #[test]
    fn top_level_no_args_defaults_to_record() {
        let args = parse_top_level([]);
        assert!(!args.transcription.output.stdout);
        assert!(!args.transcription.output.no_clipboard);
        assert!(args.device.is_none());
    }

    #[test]
    fn top_level_stdout_and_no_clipboard_conflict() {
        let result = Cli::try_parse_from(["dictate", "--stdout", "--no-clipboard"]);
        assert!(result.is_err());
    }

    // --- Vocab subcommand tests ---

    #[test]
    fn parse_vocab_add_subcommand() {
        let vocab = parse_vocab(["add", "AWS", "OpenAI"]);

        let VocabCommand::Add { words } = vocab.command else {
            panic!("expected vocab add");
        };

        assert_eq!(words, vec!["AWS", "OpenAI"]);
    }

    #[test]
    fn parse_vocab_remove_subcommand() {
        let vocab = parse_vocab(["remove", "AWS"]);

        let VocabCommand::Remove { words } = vocab.command else {
            panic!("expected vocab remove");
        };

        assert_eq!(words, vec!["AWS"]);
    }

    #[test]
    fn parse_vocab_list_subcommand() {
        let vocab = parse_vocab(["list"]);

        assert!(matches!(vocab.command, VocabCommand::List));
    }

    #[test]
    fn parse_vocab_edit_subcommand() {
        let vocab = parse_vocab(["edit"]);

        assert!(matches!(vocab.command, VocabCommand::Edit));
    }

    #[test]
    fn vocab_add_requires_words() {
        let result = Cli::try_parse_from(["dictate", "vocab", "add"]);
        assert!(result.is_err());
    }

    #[test]
    fn vocab_remove_requires_words() {
        let result = Cli::try_parse_from(["dictate", "vocab", "remove"]);
        assert!(result.is_err());
    }

    #[test]
    fn remember_is_rejected() {
        let result = Cli::try_parse_from(["dictate", "remember"]);
        assert!(result.is_err());
    }

    #[test]
    fn dictionary_is_rejected() {
        let result = Cli::try_parse_from(["dictate", "dictionary"]);
        assert!(result.is_err());
    }

    // --- Post-processing flag tests ---

    #[test]
    fn post_process_flag_defaults_to_false() {
        let args = parse_record([]);

        assert!(!args.post_process.enabled);
        assert!(args.post_process.model.is_none());
    }

    #[test]
    fn post_process_long_flag() {
        let args = parse_record(["--post-process"]);

        assert!(args.post_process.enabled);
    }

    #[test]
    fn post_process_short_flag() {
        let args = parse_record(["-p"]);

        assert!(args.post_process.enabled);
    }

    #[test]
    fn post_process_model_requires_post_process() {
        let result = Cli::try_parse_from([
            "dictate",
            "record",
            "--post-process-model",
            "llama-3.1-8b-instant",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn post_process_model_accepted_with_post_process() {
        let args = parse_record([
            "--post-process",
            "--post-process-model",
            "llama-3.1-8b-instant",
        ]);

        assert!(args.post_process.enabled);
        assert_eq!(
            args.post_process
                .model
                .as_ref()
                .map(dictate_core::ModelId::as_str),
            Some("llama-3.1-8b-instant")
        );
    }

    #[test]
    fn top_level_post_process_flag() {
        let args = parse_top_level(["-p"]);
        assert!(args.post_process.enabled);
    }

    #[test]
    fn post_process_base_url_requires_post_process() {
        let result = Cli::try_parse_from([
            "dictate",
            "record",
            "--post-process-base-url",
            "https://chat.example.com/v1/chat/completions",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn post_process_base_url_accepted_with_post_process() {
        let args = parse_record([
            "--post-process",
            "--post-process-base-url",
            "https://chat.example.com/v1/chat/completions",
        ]);

        assert!(args.post_process.enabled);
        assert_eq!(
            args.post_process.base_url.as_deref(),
            Some("https://chat.example.com/v1/chat/completions")
        );
    }

    #[test]
    fn record_post_process_context_requires_post_process() {
        let result =
            Cli::try_parse_from(["dictate", "record", "--post-process-context", "SNAKE_CASE"]);
        assert!(result.is_err());
    }

    #[test]
    fn record_accepts_post_process_context_with_post_process() {
        let args = parse_record(["--post-process", "--post-process-context", "SNAKE_CASE"]);

        assert_eq!(args.post_process.context.as_deref(), Some("SNAKE_CASE"));
    }

    #[test]
    fn retry_accepts_post_process_context_without_post_process_flag() {
        let args = parse_retry(["--post-process-context", "FRESH_CONTEXT"]);

        assert_eq!(args.post_process.context.as_deref(), Some("FRESH_CONTEXT"));
    }

    #[test]
    fn retry_rejects_post_process_context_when_no_post_process_is_set() {
        let result = Cli::try_parse_from([
            "dictate",
            "retry",
            "--no-post-process",
            "--post-process-context",
            "SNAKE_CASE",
        ]);
        assert!(result.is_err());
    }

    // --- Completions subcommand tests ---

    #[test]
    fn parse_completions_fish() {
        let args = parse_completions(["fish"]);
        assert_eq!(args.shell, clap_complete::Shell::Fish);
    }

    #[test]
    fn completions_requires_shell_argument() {
        let result = Cli::try_parse_from(["dictate", "completions"]);
        assert!(result.is_err());
    }

    #[test]
    fn completions_rejects_invalid_shell() {
        let result = Cli::try_parse_from(["dictate", "completions", "nushell"]);
        assert!(result.is_err());
    }

    // --- Format / timestamps value_parser tests ---

    #[test]
    fn format_rejects_invalid_value() {
        let result = Cli::try_parse_from(["dictate", "record", "--format", "xml"]);
        assert!(result.is_err());
    }

    #[test]
    fn format_accepts_verbose_json() {
        let args = parse_record(["--format", "verbose_json"]);
        assert_eq!(args.transcription.format.as_deref(), Some("verbose_json"));
    }
}
