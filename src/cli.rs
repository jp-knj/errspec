//! Command-line syntax and its conversion to internal requests.
use crate::request::{
    ContractAction, ContractRequest, CoverageRequest, InitRequest, MutationRequest, Request,
    Runner, ScanRequest,
};
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    version,
    about = "Connect TypeScript error variants to explicit failure contracts and tests"
)]
pub(crate) struct Cli {
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
pub(crate) enum Format {
    Json,
    Text,
}

#[derive(Clone, Copy, ValueEnum)]
enum CliRunner {
    Vitest,
    Bun,
}

#[derive(Subcommand)]
enum Action {
    /// Generate mutations, run Vitest, and report detected and undetected changes.
    Mutate {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        include: Vec<String>,
        #[arg(long)]
        exclude: Vec<String>,
        #[arg(long)]
        exclude_mutator: Vec<String>,
        #[arg(long)]
        vitest_config: Option<PathBuf>,
        #[arg(long)]
        vitest_project: Option<String>,
        #[arg(long)]
        vitest_package: Option<PathBuf>,
        #[arg(long, default_value = "node")]
        node: PathBuf,
        #[arg(long)]
        fail_on_undetected: bool,
    },
    #[command(name = "__mutation-source", hide = true)]
    MutationSource,
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
        runner: CliRunner,
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
    /// Run existing Vitest tests and report which scanned error tags they assert.
    Coverage {
        #[arg(long, default_value = "tsconfig.json")]
        project: PathBuf,
        #[arg(long, value_delimiter = ',', default_value = "_tag,kind,type,code")]
        discriminant: Vec<String>,
        /// Vitest executable; defaults to the nearest node_modules/.bin/vitest.
        #[arg(long)]
        vitest: Option<PathBuf>,
        /// Vitest config; defaults to the nearest vitest.config.* or vite.config.*.
        #[arg(long)]
        vitest_config: Option<PathBuf>,
        /// Exit 1 when a tag is neither asserted nor mentioned by any test.
        #[arg(long)]
        fail_on_missing: bool,
        /// Extra arguments for `vitest run`, after `--` (e.g. `-- --project backend`).
        #[arg(last = true)]
        runner_args: Vec<String>,
    },
    /// Resolve error tags and check registrations. Does not execute tests.
    Check,
    /// Run Vitest or Bun test and match exact test file + full name.
    Verify {
        #[arg(long, value_enum, default_value = "vitest")]
        runner: CliRunner,
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

impl From<CliRunner> for Runner {
    fn from(runner: CliRunner) -> Self {
        match runner {
            CliRunner::Vitest => Self::Vitest,
            CliRunner::Bun => Self::Bun,
        }
    }
}

impl Cli {
    /// Translate CLI syntax once. Execution code never receives clap argument types.
    pub(crate) fn into_request(self, explicit_config: bool) -> Result<(Request, Format), String> {
        let command = match self.command {
            Action::MutationSource => return Ok((Request::MutationSource, self.format)),
            Action::Mutate {
                root,
                include,
                exclude,
                exclude_mutator,
                vitest_config,
                vitest_project,
                vitest_package,
                node,
                fail_on_undetected,
            } => {
                if explicit_config || self.baseline.is_some() || self.strict_baseline {
                    return Err("mutate does not accept --config or baseline options".into());
                }
                return Ok((
                    Request::Mutation(MutationRequest {
                        root,
                        include,
                        exclude,
                        exclude_mutator,
                        vitest_config,
                        vitest_project,
                        vitest_package,
                        node,
                        fail_on_undetected,
                    }),
                    self.format,
                ));
            }
            Action::Init {
                project,
                source,
                export,
                discriminant,
                runner,
                test,
                contract,
            } => {
                if self.baseline.is_some() || self.strict_baseline {
                    return Err("init does not accept baseline options".into());
                }
                return Ok((
                    Request::Init(InitRequest {
                        project,
                        source,
                        export,
                        discriminant,
                        runner: runner.into(),
                        test,
                        contract,
                        config: self.config,
                    }),
                    self.format,
                ));
            }
            Action::Scan {
                project,
                discriminant,
                fail_on_unknown,
            } => {
                if self.baseline.is_some() || self.strict_baseline {
                    return Err("scan does not accept baseline options".into());
                }
                return Ok((
                    Request::Scan(ScanRequest {
                        project,
                        discriminants: discriminant,
                        fail_on_unknown,
                    }),
                    self.format,
                ));
            }
            Action::Coverage {
                project,
                discriminant,
                vitest,
                vitest_config,
                fail_on_missing,
                runner_args,
            } => {
                if self.baseline.is_some() || self.strict_baseline {
                    return Err("coverage does not accept baseline options".into());
                }
                return Ok((
                    Request::Coverage(CoverageRequest {
                        project,
                        discriminants: discriminant,
                        vitest,
                        vitest_config,
                        fail_on_missing,
                        runner_args,
                    }),
                    self.format,
                ));
            }
            Action::Check => ContractAction::Check,
            Action::Verify {
                runner,
                vitest,
                bun,
                only_contract_tests,
            } => ContractAction::Verify {
                runner: runner.into(),
                vitest,
                bun,
                only_contract_tests,
            },
            Action::Snapshot { out } => ContractAction::Snapshot { out },
        };
        Ok((
            Request::Contract(ContractRequest {
                command,
                config: self.config,
                baseline: self.baseline,
                strict_baseline: self.strict_baseline,
            }),
            self.format,
        ))
    }
}
