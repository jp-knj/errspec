mod app;
mod cli;
mod input;
mod onboard;
mod output;
mod request;
mod runner;
mod scan;
mod tsrs_adapter;

use clap::Parser;
use std::process::ExitCode;

fn main() -> ExitCode {
    let (request, format) = match cli::Cli::parse().into_request() {
        Ok(request) => request,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    output::write(app::execute(request), format)
}
