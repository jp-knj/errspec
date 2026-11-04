mod analysis_backend;
mod app;
mod cli;
mod coverage_runner;
mod failure_paths;
mod input;
mod mutate;
mod mutation_actions;
mod mutation_api;
mod mutation_cache;
mod mutation_config;
mod mutation_git;
mod mutation_source;
mod onboard;
mod output;
mod owned_process;
mod request;
mod runner;
mod scaffold;
mod scan;
mod scenario_source;
mod tsrs_adapter;
mod verification;

use clap::{CommandFactory, FromArgMatches};
use std::process::ExitCode;

fn main() -> ExitCode {
    let matches = cli::Cli::command().get_matches();
    let explicit_config =
        matches.value_source("config") == Some(clap::parser::ValueSource::CommandLine);
    let cli = cli::Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    let (request, format) = match cli.into_request(
        explicit_config,
        matches
            .subcommand()
            .filter(|(name, _)| *name == "mutate")
            .map(|(_, cmd)| {
                cmd.ids()
                    .filter(|id| {
                        cmd.value_source(id.as_str())
                            == Some(clap::parser::ValueSource::CommandLine)
                    })
                    .map(|id| id.to_string())
                    .collect()
            })
            .unwrap_or_default(),
    ) {
        Ok(request) => request,
        Err(error) => {
            if let Some(("mutate", command)) = matches.subcommand() {
                let mut report =
                    errspec::mutation::Report::new(command.get_flag("fail_on_undetected"));
                report.error("EC_MUTATION_CONFIG", error);
                report.finish();
                let format = command
                    .get_one::<cli::Format>("format")
                    .copied()
                    .unwrap_or(cli::Format::Text);
                return output::write(app::Outcome::Mutation(Box::new(report)), format);
            }
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    if matches!(request, request::Request::MutationSource) {
        return mutate::source_entry();
    }
    if matches!(request, request::Request::MutationApi) {
        return mutation_api::entry();
    }
    output::write(app::execute(request), format)
}
