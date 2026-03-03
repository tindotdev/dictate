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

fn run() -> Result<commands::record::RunOutcome> {
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
            return commands::record::run_retry(&options).map_err(CliError::from);
        }
        Some(args::Commands::Record(args)) => {
            let options = build_record_options(args);
            return commands::record::run(&options).map_err(CliError::from);
        }
        None => {
            let options = build_record_options(cli.record_args);
            return commands::record::run(&options).map_err(CliError::from);
        }
    }

    Ok(commands::record::RunOutcome::Completed)
}

fn build_record_options(args: args::RecordArgs) -> commands::record::RecordOptions {
    let mut options = apply_record_post_process(
        apply_transcription_options(commands::record::RecordOptions::new(), args.transcription),
        args.post_process,
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
    apply_retry_post_process(
        apply_transcription_options(commands::record::RecordOptions::new(), args.transcription),
        args.post_process,
    )
}

fn apply_transcription_options(
    mut options: commands::record::RecordOptions,
    args: args::TranscriptionArgs,
) -> commands::record::RecordOptions {
    let output = commands::record::OutputOptions::new()
        .stdout(args.output.stdout)
        .no_clipboard(args.output.no_clipboard);

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
    options.output(output)
}

fn apply_record_post_process(
    options: commands::record::RecordOptions,
    args: args::RecordPostProcessArgs,
) -> commands::record::RecordOptions {
    let mut post_process = commands::record::PostProcessOptions::new();

    if args.enabled {
        post_process = post_process.enabled(true);
    }
    if let Some(model) = args.model {
        post_process = post_process.model(model);
    }
    if let Some(url) = args.base_url {
        post_process = post_process.base_url(url);
    }

    options.post_process_options(post_process)
}

fn apply_retry_post_process(
    options: commands::record::RecordOptions,
    args: args::RetryPostProcessArgs,
) -> commands::record::RecordOptions {
    let mut post_process = commands::record::PostProcessOptions::new();

    if args.disabled {
        post_process = post_process.enabled(false);
    } else if args.enabled {
        post_process = post_process.enabled(true);
    }
    if let Some(model) = args.model {
        post_process = post_process.model(model);
    }
    if let Some(url) = args.base_url {
        post_process = post_process.base_url(url);
    }

    options.post_process_options(post_process)
}

fn exit_code_for_run_result(result: Result<commands::record::RunOutcome>) -> ExitCode {
    match result {
        Ok(commands::record::RunOutcome::Completed) => ExitCode::SUCCESS,
        Ok(commands::record::RunOutcome::Cancelled) => ExitCode::from(130),
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

fn main() -> ExitCode {
    exit_code_for_run_result(run())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_run_maps_to_exit_code_130() {
        assert_eq!(
            exit_code_for_run_result(Ok(commands::record::RunOutcome::Cancelled)),
            ExitCode::from(130)
        );
    }

    #[test]
    fn completed_run_maps_to_success() {
        assert_eq!(
            exit_code_for_run_result(Ok(commands::record::RunOutcome::Completed)),
            ExitCode::SUCCESS
        );
    }
}
