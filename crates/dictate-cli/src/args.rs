use clap::{Args, Parser, Subcommand};
use dictate_core::{ResponseFormat, TimestampGranularity};

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

    /// Model selection: "whisper-large-v3-turbo" (default, faster) or "whisper-large-v3" (more accurate)
    #[arg(long, value_parser = ["whisper-large-v3-turbo", "whisper-large-v3"])]
    pub model: Option<String>,

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

    #[test]
    fn parse_dict_alias() {
        let cli = Cli::parse_from(["dictate", "dict"]);
        assert!(matches!(cli.command, Some(Commands::Dictionary)));
    }
}
