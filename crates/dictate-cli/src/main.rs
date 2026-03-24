mod args;
mod commands;
mod ui;

use clap::Parser;
use serde_json::json;
use std::process::ExitCode;
use thiserror::Error;

#[derive(Debug, Error)]
enum CliError {
    #[error("audio operation failed")]
    Audio(#[from] dictate_core::AudioError),

    #[error("recording failed")]
    Record(#[from] commands::record::RecordError),

    #[error("vocab command failed")]
    Vocab(#[from] commands::vocab::VocabError),
}

type Result<T> = std::result::Result<T, CliError>;

fn run(cli: args::Cli) -> Result<commands::record::RunOutcome> {
    match cli.command {
        Some(args::Commands::Devices) => {
            commands::devices::run()?;
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

const fn cli_requests_json_events(cli: &args::Cli) -> bool {
    match &cli.command {
        Some(args::Commands::Record(args)) => args.transcription.json_events,
        Some(args::Commands::Retry(args)) => args.transcription.json_events,
        Some(
            args::Commands::Devices | args::Commands::Vocab(_) | args::Commands::Completions(_),
        ) => false,
        None => cli.record_args.transcription.json_events,
    }
}

fn build_record_options(args: args::RecordArgs) -> commands::record::RecordOptions {
    let mut options = apply_record_post_process(
        apply_transcription_options(commands::record::RecordOptions::new(), args.transcription),
        args.post_process,
    );

    if let Some(device) = args.device {
        options = options.device(device);
    }
    if let Some(stop_after) = args.stop_after {
        options = options.stop_after(stop_after);
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

    if let Some(provider) = args.provider {
        let provider = provider
            .parse::<dictate_core::TranscriptionProviderKind>()
            .expect("clap-validated provider should parse");
        options = options.transcription_provider(provider);
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
        let model = model_str
            .parse::<dictate_core::WhisperModel>()
            .expect("clap-validated model should parse");
        options = options.transcription_model(model);
    }
    if let Some(model_id) = args.transcription_model_id {
        options = options.transcription_model_id(model_id);
    }
    if let Some(temperature) = args.temperature {
        options = options.temperature(temperature);
    }
    if args.json_events {
        options = options.json_events(true);
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
    if let Some(provider) = args.provider {
        let provider = provider
            .parse::<dictate_core::PostProcessProviderKind>()
            .expect("clap-validated provider should parse");
        post_process = post_process.provider(provider);
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
    if let Some(provider) = args.provider {
        let provider = provider
            .parse::<dictate_core::PostProcessProviderKind>()
            .expect("clap-validated provider should parse");
        post_process = post_process.provider(provider);
    }
    if let Some(model) = args.model {
        post_process = post_process.model(model);
    }
    if let Some(url) = args.base_url {
        post_process = post_process.base_url(url);
    }

    options.post_process_options(post_process)
}

fn emit_json_failure(err: &CliError) {
    let mut causes = Vec::new();
    let mut source = std::error::Error::source(err);
    while let Some(cause) = source {
        causes.push(cause.to_string());
        source = cause.source();
    }

    eprintln!(
        "{}",
        serde_json::to_string(&json!({
            "event": "result",
            "status": "failed",
            "message": err.to_string(),
            "causes": causes,
        }))
        .expect("JSON failure event should serialize")
    );
}

fn exit_code_for_run_result(
    result: Result<commands::record::RunOutcome>,
    json_events: bool,
) -> ExitCode {
    match result {
        Ok(commands::record::RunOutcome::Completed) => ExitCode::SUCCESS,
        Ok(commands::record::RunOutcome::Cancelled) => ExitCode::from(130),
        Err(err) => {
            if json_events {
                emit_json_failure(&err);
            } else {
                eprintln!("[dictate] error: {err}");
                let mut source = std::error::Error::source(&err);
                while let Some(cause) = source {
                    eprintln!("  caused by: {cause}");
                    source = cause.source();
                }
            }
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let cli = args::Cli::parse();
    let json_events = cli_requests_json_events(&cli);
    exit_code_for_run_result(run(cli), json_events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn cancelled_run_maps_to_exit_code_130() {
        assert_eq!(
            exit_code_for_run_result(Ok(commands::record::RunOutcome::Cancelled), false),
            ExitCode::from(130)
        );
    }

    #[test]
    fn completed_run_maps_to_success() {
        assert_eq!(
            exit_code_for_run_result(Ok(commands::record::RunOutcome::Completed), false),
            ExitCode::SUCCESS
        );
    }

    #[test]
    fn build_record_options_maps_stop_after() {
        let options = build_record_options(args::RecordArgs {
            device: None,
            stop_after: Some(Duration::from_secs(30)),
            transcription: args::TranscriptionArgs {
                provider: None,
                base_url: None,
                json_events: false,
                language: None,
                prompt: None,
                format: None,
                transcription_model: None,
                transcription_model_id: None,
                temperature: None,
                timestamp_granularities: None,
                output: args::OutputArgs {
                    stdout: false,
                    no_clipboard: true,
                },
            },
            post_process: args::RecordPostProcessArgs {
                enabled: false,
                provider: None,
                model: None,
                base_url: None,
            },
            save_last_audio: false,
        });

        assert_eq!(options.stop_after, Some(Duration::from_secs(30)));
    }
}
