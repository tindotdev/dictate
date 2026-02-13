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
    if let Some(format) = args.format {
        options = options.response_format(format);
    }
    if let Some(model_str) = args.model {
        // clap's value_parser already validated this is a valid model name
        let model = model_str
            .parse::<dictate_core::WhisperModel>()
            .expect("clap-validated model should parse");
        options = options.model(model);
    }
    if let Some(temperature) = args.temperature {
        options = options.temperature(temperature);
    }
    if let Some(timestamp_granularities) = args.timestamp_granularities {
        options = options.timestamp_granularities(timestamp_granularities);
    }
    if args.stdout {
        options = options.stdout(true);
    }
    if args.no_clipboard {
        options = options.no_clipboard(true);
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
