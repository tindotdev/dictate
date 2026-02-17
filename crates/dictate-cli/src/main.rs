mod args;
mod commands;
mod ui;

use clap::Parser;
use std::process::ExitCode;
use thiserror::Error;

#[derive(Debug, Error)]
enum CliError {
    #[error("audio operation failed")]
    Audio(#[from] dictate_core::AudioError),

    #[error("recording failed")]
    Record(#[from] commands::record::RecordError),

    #[error("dictionary error")]
    Dictionary(#[from] dictate_core::DictionaryError),

    #[error("remember failed")]
    Remember(#[from] commands::remember::RememberError),

    #[error("vocab command failed")]
    Vocab(#[from] commands::vocab::VocabError),
}

type Result<T> = std::result::Result<T, CliError>;

fn run() -> Result<()> {
    let cli = args::Cli::parse();

    let args = match cli.command {
        Some(args::Commands::Devices) => {
            commands::devices::run()?;
            return Ok(());
        }
        Some(args::Commands::Remember) => {
            commands::remember::run()?;
            return Ok(());
        }
        Some(args::Commands::Dictionary) => {
            commands::dictionary::run()?;
            return Ok(());
        }
        Some(args::Commands::Vocab(args)) => {
            commands::vocab::run(&args)?;
            return Ok(());
        }
        Some(args::Commands::Completions(args)) => {
            commands::completions::run(&args);
            return Ok(());
        }
        Some(args::Commands::Record(args)) => args,
        None => cli.record_args,
    };

    let mut options = commands::record::RecordOptions::new();

    if let Some(device) = args.device {
        options = options.device(device);
    }
    if let Some(base_url) = args.base_url {
        options = options.base_url(base_url);
    }
    if let Some(language) = args.language {
        options = options.language(language);
    }
    if let Some(prompt) = args.prompt {
        options = options.prompt(prompt);
    }
    if let Some(format_str) = args.format {
        let format = format_str
            .parse::<dictate_core::ResponseFormat>()
            .expect("clap-validated format should parse");
        options = options.response_format(format);
    }
    if let Some(model_str) = args.transcription_model {
        // clap's value_parser already validated this is a valid model name
        let model = model_str
            .parse::<dictate_core::WhisperModel>()
            .expect("clap-validated model should parse");
        options = options.transcription_model(model);
    }
    if let Some(temperature) = args.temperature {
        options = options.temperature(temperature);
    }
    if let Some(granularity_strs) = args.timestamp_granularities {
        let granularities = granularity_strs
            .iter()
            .map(|s| {
                s.parse::<dictate_core::TimestampGranularity>()
                    .expect("clap-validated granularity should parse")
            })
            .collect();
        options = options.timestamp_granularities(granularities);
    }
    if args.stdout {
        options = options.stdout(true);
    }
    if args.no_clipboard {
        options = options.no_clipboard(true);
    }
    if args.post_process {
        options = options.post_process(true);
    }
    if let Some(model) = args.post_process_model {
        options = options.post_process_model(model);
    }
    if let Some(url) = args.post_process_base_url {
        options = options.post_process_base_url(url);
    }

    commands::record::run(&options)?;

    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("[dictate] error: {err}");
            // Show the full error chain for debugging
            let mut source = std::error::Error::source(&err);
            while let Some(cause) = source {
                eprintln!("  caused by: {cause}");
                source = cause.source();
            }
            ExitCode::FAILURE
        }
    }
}
