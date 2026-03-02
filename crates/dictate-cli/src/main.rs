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

    match cli.command {
        Some(args::Commands::Devices) => {
            commands::devices::run()?;
        }
        Some(args::Commands::Remember) => {
            commands::remember::run()?;
        }
        Some(args::Commands::Dictionary) => {
            commands::dictionary::run()?;
        }
        Some(args::Commands::Vocab(args)) => {
            commands::vocab::run(&args)?;
        }
        Some(args::Commands::Completions(args)) => {
            commands::completions::run(&args);
        }
        Some(args::Commands::Retry(args)) => {
            let options = build_retry_options(args);
            commands::record::run_retry(&options)?;
        }
        Some(args::Commands::Record(args)) => {
            let options = build_record_options(args);
            commands::record::run(&options)?;
        }
        None => {
            let options = build_record_options(cli.record_args);
            commands::record::run(&options)?;
        }
    }

    Ok(())
}

fn build_record_options(args: args::RecordArgs) -> commands::record::RecordOptions {
    let mut options = build_transcription_options(
        commands::record::RecordOptions::new(),
        args.base_url,
        args.language,
        args.prompt,
        args.format,
        args.transcription_model,
        args.temperature,
        args.timestamp_granularities,
        args.stdout,
        args.no_clipboard,
        if args.post_process { Some(true) } else { None },
        args.post_process_model,
        args.post_process_base_url,
    );

    if let Some(device) = args.device {
        options = options.device(device);
    }
    if args.save_last_audio {
        options = options.save_last_audio(true);
    }

    options
}

fn build_retry_options(args: args::RetryArgs) -> commands::record::RecordOptions {
    build_transcription_options(
        commands::record::RecordOptions::new(),
        args.base_url,
        args.language,
        args.prompt,
        args.format,
        args.transcription_model,
        args.temperature,
        args.timestamp_granularities,
        args.stdout,
        args.no_clipboard,
        if args.no_post_process {
            Some(false)
        } else if args.post_process {
            Some(true)
        } else {
            None
        },
        args.post_process_model,
        args.post_process_base_url,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_transcription_options(
    mut options: commands::record::RecordOptions,
    base_url: Option<String>,
    language: Option<String>,
    prompt: Option<String>,
    format: Option<String>,
    transcription_model: Option<String>,
    temperature: Option<f32>,
    timestamp_granularities: Option<Vec<String>>,
    stdout: bool,
    no_clipboard: bool,
    post_process_override: Option<bool>,
    post_process_model: Option<dictate_core::ModelId>,
    post_process_base_url: Option<String>,
) -> commands::record::RecordOptions {
    if let Some(base_url) = base_url {
        options = options.base_url(base_url);
    }
    if let Some(language) = language {
        options = options.language(language);
    }
    if let Some(prompt) = prompt {
        options = options.prompt(prompt);
    }
    if let Some(format_str) = format {
        let format = format_str
            .parse::<dictate_core::ResponseFormat>()
            .expect("clap-validated format should parse");
        options = options.response_format(format);
    }
    if let Some(model_str) = transcription_model {
        let model = model_str
            .parse::<dictate_core::WhisperModel>()
            .expect("clap-validated model should parse");
        options = options.transcription_model(model);
    }
    if let Some(temperature) = temperature {
        options = options.temperature(temperature);
    }
    if let Some(granularity_strs) = timestamp_granularities {
        let granularities = granularity_strs
            .iter()
            .map(|s| {
                s.parse::<dictate_core::TimestampGranularity>()
                    .expect("clap-validated granularity should parse")
            })
            .collect();
        options = options.timestamp_granularities(granularities);
    }
    if stdout {
        options = options.stdout(true);
    }
    if no_clipboard {
        options = options.no_clipboard(true);
    }
    if let Some(post_process) = post_process_override {
        options = options.post_process(post_process);
    }
    if let Some(model) = post_process_model {
        options = options.post_process_model(model);
    }
    if let Some(url) = post_process_base_url {
        options = options.post_process_base_url(url);
    }

    options
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
