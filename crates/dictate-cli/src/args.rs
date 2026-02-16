use clap::{Args, Parser, Subcommand};
use dictate_core::{ModelId, ResponseFormat, TimestampGranularity};

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

    /// Record audio until Ctrl+C
    Record(RecordArgs),

    /// Add custom terms to dictionary (improves accuracy for technical terms, names, jargon)
    Remember,

    /// List dictionary entries
    #[command(alias = "dict")]
    Dictionary,

    /// Manage vocabulary words for transcription biasing
    Vocab(VocabArgs),
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
Words that already exist in vocabulary are skipped with a warning.\n\
Words that already exist as dictionary correction values are also skipped,\n\
since they are already included in prompt hints."
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
}

#[derive(Args)]
pub struct RecordArgs {
    /// Select a specific audio input device (index from `devices`, or case-insensitive partial name match)
    #[arg(long)]
    pub device: Option<String>,

    /// Override Groq transcription API URL (falls back to `GROQ_BASE_URL` when omitted)
    #[arg(long)]
    pub base_url: Option<String>,

    /// ISO-639-1 language code (e.g., "en", "es", "fr") to improve accuracy and latency
    #[arg(long)]
    pub language: Option<String>,

    /// Text to guide transcription style or spelling (max 224 tokens)
    #[arg(long)]
    pub prompt: Option<String>,

    /// Response format: "json" (default), "`verbose_json`", or "text"
    #[arg(long)]
    pub format: Option<ResponseFormat>,

    /// Transcription model: "whisper-large-v3-turbo" (default, faster) or "whisper-large-v3" (more accurate)
    #[arg(long, value_parser = ["whisper-large-v3-turbo", "whisper-large-v3"])]
    pub transcription_model: Option<String>,

    /// Sampling temperature (0.0-1.0). Default 0.0 is recommended for transcription
    #[arg(long, value_parser = parse_temperature)]
    pub temperature: Option<f32>,

    /// Timestamp granularities: "segment", "word", or both (comma-separated).
    /// Requires --format `verbose_json`. Example: --timestamps word,segment
    #[arg(long = "timestamps", value_delimiter = ',')]
    pub timestamp_granularities: Option<Vec<TimestampGranularity>>,

    /// Print transcript to stdout instead of copying to clipboard
    #[arg(long, conflicts_with = "no_clipboard")]
    pub stdout: bool,

    /// Skip clipboard entirely (headless/scripted use). Prints to stdout.
    #[arg(long, conflicts_with = "stdout")]
    pub no_clipboard: bool,

    /// Post-process transcription with LLM for better punctuation and formatting
    #[arg(long, short = 'p')]
    pub post_process: bool,

    /// Model for post-processing (default: openai/gpt-oss-20b)
    #[arg(long, requires = "post_process")]
    pub post_process_model: Option<ModelId>,

    /// Override post-processing chat API URL (falls back to `GROQ_CHAT_BASE_URL` when omitted)
    #[arg(long, requires = "post_process")]
    pub post_process_base_url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn record_accepts_base_url_flag() {
        let cli = Cli::parse_from([
            "dictate",
            "record",
            "--base-url",
            "http://127.0.0.1:8080/openai/v1/audio/transcriptions",
        ]);

        let Some(Commands::Record(args)) = cli.command else {
            panic!("expected record subcommand");
        };

        assert_eq!(
            args.base_url.as_deref(),
            Some("http://127.0.0.1:8080/openai/v1/audio/transcriptions")
        );
    }

    #[test]
    fn record_defaults_base_url_to_none() {
        let cli = Cli::parse_from(["dictate", "record"]);

        let Some(Commands::Record(args)) = cli.command else {
            panic!("expected record subcommand");
        };

        assert_eq!(args.base_url, None);
    }

    #[test]
    fn record_accepts_transcription_model_flag() {
        let cli = Cli::parse_from([
            "dictate",
            "record",
            "--transcription-model",
            "whisper-large-v3",
        ]);

        let Some(Commands::Record(args)) = cli.command else {
            panic!("expected record subcommand");
        };

        assert_eq!(
            args.transcription_model.as_deref(),
            Some("whisper-large-v3")
        );
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
        let cli = Cli::try_parse_from(["dictate", "record", "--temperature", "0.5"]).unwrap();

        let Some(Commands::Record(args)) = cli.command else {
            panic!("expected record subcommand");
        };

        assert_eq!(args.temperature, Some(0.5));
    }

    #[test]
    fn stdout_flag_defaults_to_false() {
        let cli = Cli::parse_from(["dictate", "record"]);

        let Some(Commands::Record(args)) = cli.command else {
            panic!("expected record subcommand");
        };

        assert!(!args.stdout);
    }

    #[test]
    fn stdout_flag_can_be_enabled() {
        let cli = Cli::parse_from(["dictate", "record", "--stdout"]);

        let Some(Commands::Record(args)) = cli.command else {
            panic!("expected record subcommand");
        };

        assert!(args.stdout);
    }

    #[test]
    fn no_clipboard_flag_defaults_to_false() {
        let cli = Cli::parse_from(["dictate", "record"]);

        let Some(Commands::Record(args)) = cli.command else {
            panic!("expected record subcommand");
        };

        assert!(!args.no_clipboard);
    }

    #[test]
    fn no_clipboard_flag_can_be_enabled() {
        let cli = Cli::parse_from(["dictate", "record", "--no-clipboard"]);

        let Some(Commands::Record(args)) = cli.command else {
            panic!("expected record subcommand");
        };

        assert!(args.no_clipboard);
    }

    #[test]
    fn stdout_and_no_clipboard_conflict() {
        let result = Cli::try_parse_from(["dictate", "record", "--stdout", "--no-clipboard"]);
        assert!(result.is_err());
    }

    // --- Top-level flag tests (no `record` subcommand) ---

    #[test]
    fn top_level_stdout_flag() {
        let cli = Cli::parse_from(["dictate", "--stdout"]);
        assert!(cli.command.is_none());
        assert!(cli.record_args.stdout);
    }

    #[test]
    fn top_level_no_clipboard_flag() {
        let cli = Cli::parse_from(["dictate", "--no-clipboard"]);
        assert!(cli.command.is_none());
        assert!(cli.record_args.no_clipboard);
    }

    #[test]
    fn top_level_device_flag() {
        let cli = Cli::parse_from(["dictate", "--device", "USB"]);
        assert!(cli.command.is_none());
        assert_eq!(cli.record_args.device.as_deref(), Some("USB"));
    }

    #[test]
    fn top_level_language_flag() {
        let cli = Cli::parse_from(["dictate", "--language", "en"]);
        assert!(cli.command.is_none());
        assert_eq!(cli.record_args.language.as_deref(), Some("en"));
    }

    #[test]
    fn top_level_multiple_flags() {
        let cli = Cli::parse_from(["dictate", "--stdout", "--language", "es", "--device", "mic"]);
        assert!(cli.command.is_none());
        assert!(cli.record_args.stdout);
        assert_eq!(cli.record_args.language.as_deref(), Some("es"));
        assert_eq!(cli.record_args.device.as_deref(), Some("mic"));
    }

    #[test]
    fn top_level_no_args_defaults_to_record() {
        let cli = Cli::parse_from(["dictate"]);
        assert!(cli.command.is_none());
        assert!(!cli.record_args.stdout);
        assert!(!cli.record_args.no_clipboard);
        assert!(cli.record_args.device.is_none());
    }

    #[test]
    fn top_level_stdout_and_no_clipboard_conflict() {
        let result = Cli::try_parse_from(["dictate", "--stdout", "--no-clipboard"]);
        assert!(result.is_err());
    }

    // --- Dictionary subcommand tests ---

    #[test]
    fn parse_remember_subcommand() {
        let cli = Cli::parse_from(["dictate", "remember"]);
        assert!(matches!(cli.command, Some(Commands::Remember)));
    }

    #[test]
    fn parse_dictionary_subcommand() {
        let cli = Cli::parse_from(["dictate", "dictionary"]);
        assert!(matches!(cli.command, Some(Commands::Dictionary)));
    }

    // --- Vocab subcommand tests ---

    #[test]
    fn parse_vocab_add_subcommand() {
        let cli = Cli::parse_from(["dictate", "vocab", "add", "AWS", "OpenAI"]);

        let Some(Commands::Vocab(vocab)) = cli.command else {
            panic!("expected vocab subcommand");
        };

        let VocabCommand::Add { words } = vocab.command else {
            panic!("expected vocab add");
        };

        assert_eq!(words, vec!["AWS", "OpenAI"]);
    }

    #[test]
    fn parse_vocab_remove_subcommand() {
        let cli = Cli::parse_from(["dictate", "vocab", "remove", "AWS"]);

        let Some(Commands::Vocab(vocab)) = cli.command else {
            panic!("expected vocab subcommand");
        };

        let VocabCommand::Remove { words } = vocab.command else {
            panic!("expected vocab remove");
        };

        assert_eq!(words, vec!["AWS"]);
    }

    #[test]
    fn parse_vocab_list_subcommand() {
        let cli = Cli::parse_from(["dictate", "vocab", "list"]);

        let Some(Commands::Vocab(vocab)) = cli.command else {
            panic!("expected vocab subcommand");
        };

        assert!(matches!(vocab.command, VocabCommand::List));
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
    fn parse_dict_alias() {
        let cli = Cli::parse_from(["dictate", "dict"]);
        assert!(matches!(cli.command, Some(Commands::Dictionary)));
    }

    // --- Post-processing flag tests ---

    #[test]
    fn post_process_flag_defaults_to_false() {
        let cli = Cli::parse_from(["dictate", "record"]);

        let Some(Commands::Record(args)) = cli.command else {
            panic!("expected record subcommand");
        };

        assert!(!args.post_process);
        assert!(args.post_process_model.is_none());
    }

    #[test]
    fn post_process_long_flag() {
        let cli = Cli::parse_from(["dictate", "record", "--post-process"]);

        let Some(Commands::Record(args)) = cli.command else {
            panic!("expected record subcommand");
        };

        assert!(args.post_process);
    }

    #[test]
    fn post_process_short_flag() {
        let cli = Cli::parse_from(["dictate", "record", "-p"]);

        let Some(Commands::Record(args)) = cli.command else {
            panic!("expected record subcommand");
        };

        assert!(args.post_process);
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
        let cli = Cli::parse_from([
            "dictate",
            "record",
            "--post-process",
            "--post-process-model",
            "llama-3.1-8b-instant",
        ]);

        let Some(Commands::Record(args)) = cli.command else {
            panic!("expected record subcommand");
        };

        assert!(args.post_process);
        assert_eq!(
            args.post_process_model
                .as_ref()
                .map(dictate_core::ModelId::as_str),
            Some("llama-3.1-8b-instant")
        );
    }

    #[test]
    fn top_level_post_process_flag() {
        let cli = Cli::parse_from(["dictate", "-p"]);
        assert!(cli.command.is_none());
        assert!(cli.record_args.post_process);
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
        let cli = Cli::parse_from([
            "dictate",
            "record",
            "--post-process",
            "--post-process-base-url",
            "https://chat.example.com/v1/chat/completions",
        ]);

        let Some(Commands::Record(args)) = cli.command else {
            panic!("expected record subcommand");
        };

        assert!(args.post_process);
        assert_eq!(
            args.post_process_base_url.as_deref(),
            Some("https://chat.example.com/v1/chat/completions")
        );
    }
}
