//! Runner-independent test judgments over resolved file identities. No filesystem access.
use crate::{
    contract::Registration,
    model::{Finding, TestRef},
};
use std::{collections::BTreeMap, path::PathBuf};

/// Paths resolved by the I/O boundary at the current execution stage.
/// Missing entries are unresolved; two unresolved paths never identify the same file.
pub type ResolvedPaths = BTreeMap<String, PathBuf>;

pub struct RunEvidence {
    pub runner: &'static str,
    pub success: bool,
    pub tests: Vec<TestEvidence>,
}

pub struct TestEvidence {
    pub file: String,
    pub name: String,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TestStatus {
    NotRun,
    NotFound,
    Ambiguous,
    Passed,
    Failed,
    Pending,
    Skipped,
    Todo,
    Unknown(String),
}

impl TestStatus {
    fn from_runner(value: String) -> Self {
        match value.as_str() {
            "passed" => Self::Passed,
            "failed" => Self::Failed,
            "pending" => Self::Pending,
            "skipped" => Self::Skipped,
            "todo" => Self::Todo,
            _ => Self::Unknown(value),
        }
    }
    pub fn as_str(&self) -> &str {
        match self {
            Self::NotRun => "not_run",
            Self::NotFound => "not_found",
            Self::Ambiguous => "ambiguous",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Pending => "pending",
            Self::Skipped => "skipped",
            Self::Todo => "todo",
            Self::Unknown(value) => value,
        }
    }
}

#[derive(Debug)]
pub struct TestRun {
    pub runner: &'static str,
    statuses: BTreeMap<TestRef, TestStatus>,
    full_names: BTreeMap<TestRef, String>,
}

impl TestRun {
    pub fn status(&self, test: &TestRef) -> &TestStatus {
        self.statuses.get(test).unwrap_or(&TestStatus::NotFound)
    }
    /// The runner's name for the single matching test, including any SDK description.
    pub fn full_name<'a>(&'a self, test: &'a TestRef) -> &'a str {
        self.full_names.get(test).unwrap_or(&test.name)
    }
}

/// A scenario name may carry a human description after one space. The JSON tuple
/// ends with `]`, so a description cannot make one scenario's name match another.
fn names_scenario(runner_name: &str, scenario_name: &str) -> bool {
    runner_name
        .strip_prefix(scenario_name)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
}

pub fn judge(
    registrations: &[Registration<'_>],
    paths: &ResolvedPaths,
    evidence: RunEvidence,
    process_ok: bool,
) -> (TestRun, Vec<Finding>) {
    let mut results: BTreeMap<(&PathBuf, String), Vec<TestStatus>> = BTreeMap::new();
    for test in evidence.tests {
        if let Some(path) = paths.get(&test.file) {
            results
                .entry((path, test.name))
                .or_default()
                .push(TestStatus::from_runner(test.status));
        }
    }
    let mut statuses = BTreeMap::new();
    let mut full_names = BTreeMap::new();
    let mut findings = Vec::new();
    for registration in registrations {
        for case in &registration.cases {
            let Some(test) = case.test() else { continue };
            let matches: Vec<_> = match paths.get(&test.file) {
                // Legacy names are user-specified and must match exactly.
                Some(path) if case.scenario().is_none() => results
                    .get(&(path, test.name.clone()))
                    .into_iter()
                    .flat_map(|values| values.iter().map(|status| (&test.name, status)))
                    .collect(),
                Some(path) => results
                    .iter()
                    .filter(|((file, name), _)| *file == path && names_scenario(name, &test.name))
                    .flat_map(|((_, name), values)| values.iter().map(move |status| (name, status)))
                    .collect(),
                None => Vec::new(),
            };
            let status = match matches.as_slice() {
                [] => TestStatus::NotFound,
                [(name, status)] => {
                    full_names.insert(test.clone(), (*name).clone());
                    (*status).clone()
                }
                _ => TestStatus::Ambiguous,
            };
            if status != TestStatus::Passed {
                findings.push(case.finding(
                    registration.contract.id(),
                    "EC005",
                    format!("Referenced test is {}: {}", status.as_str(), test.name),
                ));
            }
            statuses.insert(test.clone(), status);
        }
    }
    if !process_ok || !evidence.success {
        findings.push(Finding::new(
            "EC006",
            format!(
                "{} run failed; passing individual tests do not make the run successful",
                evidence.runner
            ),
        ));
    }
    (
        TestRun {
            runner: evidence.runner,
            statuses,
            full_names,
        },
        findings,
    )
}
