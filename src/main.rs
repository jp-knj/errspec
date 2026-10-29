mod app;
mod input;
mod onboard;
mod output;
mod runner;
mod scan;
mod tool_commands;
mod tsrs_adapter;

use clap::{Parser, Subcommand, ValueEnum};
use std::{path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(
    version,
    about = "Connect TypeScript error variants to explicit failure contracts and tests"
)]
struct Cli {
    #[command(subcommand)]
    command: Action,
    #[arg(long, global = true, default_value = "errspec.json")]
    config: PathBuf,
    #[arg(long, global = true, value_enum, default_value = "text")]
    format: Format,
    #[arg(long, global = true)]
    baseline: Option<PathBuf>,
    /// Fail when an existing v2 scenario changes from the baseline (check/verify).
    #[arg(long, global = true, requires = "baseline")]
    strict_baseline: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Json,
    Text,
}

#[derive(Clone, Copy, ValueEnum)]
enum Runner {
    Vitest,
    Bun,
}

#[derive(Subcommand)]
enum Action {
    /// Generate configuration and intentionally failing tests for one exported type.
    Init {
        #[arg(long, default_value = "tsconfig.json")]
        project: PathBuf,
        #[arg(long)]
        source: String,
        #[arg(long)]
        export: String,
        #[arg(long, default_value = "_tag")]
        discriminant: String,
        #[arg(long, value_enum, default_value = "vitest")]
        runner: Runner,
        #[arg(long, default_value = "errspec.generated.test.ts")]
        test: PathBuf,
        #[arg(long)]
        contract: Option<String>,
    },
    /// Discover error candidates and conservative interprocedural propagation.
    Scan {
        #[arg(long, default_value = "tsconfig.json")]
        project: PathBuf,
        #[arg(long, value_delimiter = ',', default_value = "_tag,kind,type,code")]
        discriminant: Vec<String>,
        #[arg(long)]
        fail_on_unknown: bool,
    },
    /// Resolve error tags and check registrations. Does not execute tests.
    Check,
    /// Run Vitest or Bun test and match exact test file + full name.
    Verify {
        #[arg(long, value_enum, default_value = "vitest")]
        runner: Runner,
        /// Vitest executable; defaults to <config directory>/node_modules/.bin/vitest.
        #[arg(long, conflicts_with = "bun")]
        vitest: Option<PathBuf>,
        /// Bun executable; defaults to bun on PATH.
        #[arg(long, conflicts_with = "vitest")]
        bun: Option<PathBuf>,
        /// Run only files referenced by registered contracts (useful during adoption).
        #[arg(long)]
        only_contract_tests: bool,
    },
    /// Write an explicit baseline. Refuses to overwrite an existing file.
    Snapshot {
        #[arg(long)]
        out: PathBuf,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Some(code) = tool_commands::execute(&cli) {
        return code;
    }
    let format = cli.format;
    let report = app::execute(cli);
    output::write(&report, format)
}
