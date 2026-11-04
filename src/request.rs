//! Internal execution requests. No CLI parsing, serialization, or process I/O.
use std::path::PathBuf;

pub(crate) enum Request {
    Contract(ContractRequest),
    Scan(ScanRequest),
    Init(InitRequest),
    Coverage(CoverageRequest),
    Mutation(MutationRequest),
    MutationSource,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MutationRequest {
    pub root: PathBuf,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub exclude_mutator: Vec<String>,
    pub vitest_config: Option<PathBuf>,
    pub vitest_project: Option<String>,
    pub vitest_package: Option<PathBuf>,
    pub node: PathBuf,
    pub fail_on_undetected: bool,
}

pub(crate) struct ContractRequest {
    pub analysis: crate::analysis_backend::Options,
    pub command: ContractAction,
    pub config: PathBuf,
    pub baseline: Option<PathBuf>,
    pub strict_baseline: bool,
}

pub(crate) enum ContractAction {
    Check,
    Verify {
        runner: Runner,
        // Validate executable/runner consistency at the execution boundary, after
        // configuration and analysis, preserving existing diagnostic precedence.
        vitest: Option<PathBuf>,
        bun: Option<PathBuf>,
        only_contract_tests: bool,
    },
    Snapshot {
        out: PathBuf,
    },
}

impl ContractAction {
    pub fn mode(&self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Verify { .. } => "verify",
            Self::Snapshot { .. } => "snapshot",
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Runner {
    Vitest,
    Bun,
}

pub(crate) struct ScanRequest {
    pub analysis: crate::analysis_backend::Options,
    pub project: PathBuf,
    pub discriminants: Vec<String>,
    pub fail_on_unknown: bool,
}

pub(crate) struct CoverageRequest {
    pub analysis: crate::analysis_backend::Options,
    pub project: PathBuf,
    pub discriminants: Vec<String>,
    pub vitest: Option<PathBuf>,
    pub vitest_config: Option<PathBuf>,
    pub fail_on_missing: bool,
    pub runner_args: Vec<String>,
    pub baseline: Option<PathBuf>,
    pub save_baseline: Option<PathBuf>,
}

pub(crate) struct InitRequest {
    pub analysis: crate::analysis_backend::Options,
    pub project: PathBuf,
    pub source: String,
    pub export: String,
    pub discriminant: String,
    pub runner: Runner,
    pub test: PathBuf,
    pub config: PathBuf,
    pub contract: Option<String>,
}
