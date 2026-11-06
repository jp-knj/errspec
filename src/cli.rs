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
    /// Analysis backend: use the project's TypeScript 6/7 API when installed.
    #[arg(long, global = true, value_enum)]
    analysis_engine: Option<CliAnalysisEngine>,
    /// Installed TypeScript package directory (relative to the invoking directory).
    #[arg(long, global = true)]
    typescript_package: Option<PathBuf>,
    /// Node executable used only by the TypeScript analysis backend.
    #[arg(long, global = true)]
    analysis_node: Option<PathBuf>,
}

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum Format {
    Json,
    Text,
    /// GitHub-flavored Markdown; coverage only.
    Markdown,
}

#[derive(Clone, Copy, ValueEnum)]
enum CliRunner {
    Vitest,
    Bun,
}

#[derive(Clone, Copy, ValueEnum)]
enum CliAnalysisEngine {
    Auto,
    Tsrs,
    Typescript,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum MutationDiffUnit {
    Node,
    Scope,
}
impl From<MutationDiffUnit> for errspec::mutation::selection::Unit {
    fn from(unit: MutationDiffUnit) -> Self {
        match unit {
            MutationDiffUnit::Node => Self::Node,
            MutationDiffUnit::Scope => Self::Scope,
        }
    }
}

#[derive(clap::Args)]
struct MutationPolicyArgs {
    #[arg(long)]
    only: Vec<String>,
    #[arg(long)]
    mutants_per_line: Option<usize>,
    #[arg(long)]
    time_budget: Option<f64>,
    #[arg(long)]
    fail_on_survived: bool,
    #[arg(long, conflicts_with = "fail_on_survived")]
    no_fail_on_survived: bool,
    #[arg(long)]
    threshold_high: Option<f64>,
    #[arg(long)]
    threshold_low: Option<f64>,
    #[arg(long)]
    threshold_break: Option<f64>,
    #[arg(long)]
    critical: Vec<String>,
    #[arg(long)]
    accepted_file: Option<String>,
}

impl From<MutationPolicyArgs> for errspec::mutation::policy::Policy {
    fn from(args: MutationPolicyArgs) -> Self {
        Self {
            only: args.only,
            mutants_per_line: args.mutants_per_line,
            time_budget: args.time_budget,
            fail_on_survived: args.fail_on_survived && !args.no_fail_on_survived,
            threshold_high: args.threshold_high,
            threshold_low: args.threshold_low,
            threshold_break: args.threshold_break,
            critical: args.critical,
            accepted_file: args.accepted_file,
        }
    }
}

impl From<CliAnalysisEngine> for crate::analysis_backend::Engine {
    fn from(engine: CliAnalysisEngine) -> Self {
        match engine {
            CliAnalysisEngine::Auto => Self::Auto,
            CliAnalysisEngine::Tsrs => Self::Tsrs,
            CliAnalysisEngine::Typescript => Self::Typescript,
        }
    }
}

#[derive(clap::Args)]
struct MutationArgs {
    /// Git revision used to select changed mutation nodes or scopes.
    #[arg(long)]
    since: Option<String>,
    #[arg(long, value_enum)]
    diff_unit: Option<MutationDiffUnit>,
    /// Execute one deterministic partition, e.g. --shard 1/4.
    #[arg(long)]
    shard: Option<errspec::mutation::sharding::Shard>,
    /// Independent mutation workers; defaults to one.
    #[arg(short = 'j', long, default_value_t = 1)]
    concurrency: usize,
    /// Exclude logging-only syntax; opt-in to preserve existing mutation runs.
    #[arg(long, conflicts_with = "no_arid")]
    arid: bool,
    #[arg(long)]
    no_arid: bool,
    /// Replace logging callee patterns; requires --arid to enable the heuristic.
    #[arg(long)]
    arid_callee: Vec<String>,
    /// Mutation settings; separate from the error-contract configuration.
    #[arg(long)]
    config_file: Option<PathBuf>,
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
    #[arg(long, conflicts_with = "fail_on_undetected")]
    no_fail_on_undetected: bool,
    /// Check each mutant with the project's TypeScript before running it.
    #[arg(long, conflicts_with = "no_typecheck")]
    typecheck: bool,
    /// Disable mutant typechecking explicitly.
    #[arg(long)]
    no_typecheck: bool,
    #[arg(long, conflicts_with = "no_weak_mutation")]
    weak_mutation: bool,
    /// Reuse an instrumented Vitest session with isolated workers.
    #[arg(long, conflicts_with = "no_reuse_session")]
    reuse_session: bool,
    /// Reuse stable assertion outcomes with exactly matching inputs.
    #[arg(long, conflicts_with = "no_cache")]
    cache: bool,
    /// Allow project-root relocation for path-independent deterministic tests.
    #[arg(long, conflicts_with = "no_cache_relocation")]
    cache_relocation: bool,
    #[arg(long)]
    no_cache_relocation: bool,
    /// Reuse unchanged function outcomes with complete, disjoint test-file cohorts.
    #[arg(long, conflicts_with = "no_cache_scope")]
    cache_scope: bool,
    #[arg(long)]
    no_cache_scope: bool,
    /// Experimental survivor reuse through a static graph; may miss value flow through tests.
    #[arg(long, conflicts_with = "no_experimental_callgraph")]
    experimental_callgraph: bool,
    #[arg(long)]
    no_experimental_callgraph: bool,
    #[arg(long)]
    no_cache: bool,
    #[arg(long)]
    cache_dir: Option<PathBuf>,
    /// Refresh observations from all tests even when coverage is cached.
    #[arg(long, conflicts_with = "no_full_dry_run")]
    full_dry_run: bool,
    #[arg(long)]
    no_full_dry_run: bool,
    #[arg(long)]
    no_reuse_session: bool,
    #[arg(long)]
    no_weak_mutation: bool,
    #[arg(long)]
    typecheck_project: Option<PathBuf>,
    #[arg(long)]
    typecheck_package: Option<PathBuf>,
    #[command(flatten)]
    policy: Box<MutationPolicyArgs>,
}

#[derive(Subcommand)]
enum Action {
    /// Record reasoned approval of equivalent or intentionally untested mutants.
    Accept {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long, default_value = "errspec.accepted.json")]
        accepted_file: PathBuf,
        #[arg(required = true)]
        keys: Vec<String>,
        #[arg(long)]
        reason: String,
    },
    /// Generate mutations, run Vitest, and report detected and undetected changes.
    Mutate(Box<MutationArgs>),
    #[command(name = "__mutation-source", hide = true)]
    MutationSource,
    #[command(name = "__mutation-api", hide = true)]
    MutationApi,
    #[command(name = "__mutation-supervisor", hide = true)]
    MutationSupervisor { configuration: PathBuf },
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
        /// Write the current per-tag statuses for --baseline. Refuses to overwrite.
        #[arg(long, conflicts_with = "baseline")]
        save_baseline: Option<PathBuf>,
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
    pub(crate) fn into_request(
        self,
        explicit_config: bool,
        overrides: std::collections::BTreeSet<String>,
    ) -> Result<(Request, Format), String> {
        if matches!(self.format, Format::Markdown)
            && !matches!(self.command, Action::Coverage { .. })
        {
            return Err("--format markdown is supported only by coverage".into());
        }
        let explicit_analysis = self.analysis_engine.is_some()
            || self.typescript_package.is_some()
            || self.analysis_node.is_some();
        let analysis = crate::analysis_backend::Options {
            engine: self.analysis_engine.map(Into::into).unwrap_or_default(),
            package: self.typescript_package,
            node: self.analysis_node.unwrap_or_else(|| "node".into()),
        };
        if analysis.engine == crate::analysis_backend::Engine::Tsrs && analysis.package.is_some() {
            return Err("--typescript-package conflicts with --analysis-engine tsrs".into());
        }
        let command = match self.command {
            Action::Accept {
                root,
                accepted_file,
                keys,
                reason,
            } => {
                if explicit_config
                    || self.baseline.is_some()
                    || self.strict_baseline
                    || explicit_analysis
                {
                    return Err("accept does not accept contract or analysis options".into());
                }
                return Ok((
                    Request::Accept {
                        root,
                        file: accepted_file,
                        keys,
                        reason,
                    },
                    self.format,
                ));
            }
            Action::MutationSource => return Ok((Request::MutationSource, self.format)),
            Action::MutationApi => return Ok((Request::MutationApi, self.format)),
            Action::MutationSupervisor { configuration } => {
                return Ok((Request::MutationSupervisor(configuration), self.format));
            }
            Action::Mutate(args) => {
                let MutationArgs {
                    since,
                    diff_unit,
                    shard,
                    concurrency,
                    arid,
                    no_arid,
                    arid_callee,
                    config_file,
                    root,
                    include,
                    exclude,
                    exclude_mutator,
                    vitest_config,
                    vitest_project,
                    vitest_package,
                    node,
                    fail_on_undetected,
                    no_fail_on_undetected: _,
                    typecheck,
                    no_typecheck: _,
                    weak_mutation,
                    reuse_session,
                    cache,
                    cache_relocation,
                    no_cache_relocation: _,
                    cache_scope,
                    no_cache_scope: _,
                    experimental_callgraph,
                    no_experimental_callgraph: _,
                    no_cache: _,
                    cache_dir,
                    full_dry_run,
                    no_full_dry_run: _,
                    no_reuse_session: _,
                    no_weak_mutation: _,
                    typecheck_project,
                    typecheck_package,
                    policy,
                } = *args;
                if explicit_config
                    || self.baseline.is_some()
                    || self.strict_baseline
                    || explicit_analysis
                {
                    return Err(
                        "mutate does not accept --config, baseline, or analysis options".into(),
                    );
                }
                return Ok((
                    Request::Mutation(Box::new(MutationRequest {
                        config_file,
                        overrides,
                        options: errspec::mutation::options::Options {
                            since,
                            diff_unit: diff_unit.map(Into::into),
                            shard,
                            concurrency,
                            arid: arid && !no_arid,
                            arid_callees: (!arid_callee.is_empty()).then_some(arid_callee),
                            root,
                            include,
                            exclude,
                            exclude_mutator,
                            vitest_config,
                            vitest_project,
                            vitest_package,
                            node,
                            fail_on_undetected,
                            typecheck,
                            weak_mutation,
                            reuse_session,
                            cache,
                            cache_relocation,
                            cache_scope,
                            experimental_callgraph,
                            cache_dir,
                            full_dry_run,
                            typecheck_project,
                            typecheck_package,
                            policy: (*policy).into(),
                        },
                    })),
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
                        analysis,
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
                        analysis,
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
                save_baseline,
                runner_args,
            } => {
                if self.strict_baseline {
                    return Err("coverage does not accept --strict-baseline".into());
                }
                return Ok((
                    Request::Coverage(CoverageRequest {
                        analysis,
                        project,
                        discriminants: discriminant,
                        vitest,
                        vitest_config,
                        fail_on_missing,
                        runner_args,
                        baseline: self.baseline,
                        save_baseline,
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
                analysis,
                command,
                config: self.config,
                baseline: self.baseline,
                strict_baseline: self.strict_baseline,
            }),
            self.format,
        ))
    }
}
