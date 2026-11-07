mod analysis;
mod app;
mod cli;
mod contracts;
mod coverage;
mod discovery;
mod init;
mod mutation;
mod output;
mod request;
mod support;

// Preserve crate-root names while feature directories own the implementations.
pub(crate) use analysis::{backend as analysis_backend, tsrs as tsrs_adapter};
pub(crate) use contracts::{failure_paths, runner, source as scenario_source, verification};
pub(crate) use coverage::run as coverage_runner;
pub(crate) use discovery::scan;
pub(crate) use init::{run as onboard, scaffold, skills as skill_actions};
pub(crate) use mutation::{
    actions as mutation_actions, api as mutation_api, cache as mutation_cache,
    config as mutation_config, git as mutation_git, ownership as mutation_ownership,
    reporting as mutation_reporting, run as mutate, source as mutation_source,
};
pub(crate) use support::{command_output, input, owned_process};

use clap::{CommandFactory, FromArgMatches};
use std::process::ExitCode;

fn main() -> ExitCode {
    if let Err(error) = owned_process::retain_children() {
        eprintln!("Cannot retain child-process ownership: {error}");
        return ExitCode::from(2);
    }
    let matches = cli::Cli::command().get_matches();
    let explicit_config =
        matches.value_source("config") == Some(clap::parser::ValueSource::CommandLine);
    let cli = cli::Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    let (request, format) = match cli.into_request(
        explicit_config,
        matches
            .subcommand()
            .filter(|(name, _)| matches!(*name, "mutate" | "select-tests"))
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
                report.presentation = errspec::mutation::reporting::Presentation {
                    root: command
                        .get_one::<std::path::PathBuf>("root")
                        .cloned()
                        .unwrap_or_else(|| ".".into()),
                    reporters: command
                        .get_many::<errspec::mutation::reporting::Reporter>("reporter")
                        .map(|values| values.copied().collect())
                        .unwrap_or_default(),
                    directory: command.get_one::<std::path::PathBuf>("report_dir").cloned(),
                };
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
    if matches!(request, request::Request::FormatReport) {
        return mutation_reporting::entry();
    }
    if matches!(request, request::Request::MutationApi) {
        return mutation_api::entry();
    }
    if let request::Request::MutationSupervisor(path) = &request {
        return mutation_ownership::entry(path);
    }
    output::write(app::execute(request), format)
}
