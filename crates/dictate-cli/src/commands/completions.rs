use crate::args::CompletionsArgs;

/// Generate shell completions and write them to stdout.
pub fn run(args: &CompletionsArgs) {
    let mut cmd = <crate::args::Cli as clap::CommandFactory>::command();
    clap_complete::generate(args.shell, &mut cmd, "dictate", &mut std::io::stdout());
}
