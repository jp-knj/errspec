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
    sdk_names: BTreeMap<TestRef, String>,
}

impl TestRun {
    pub fn status(&self, test: &TestRef) -> &TestStatus {
        self.statuses.get(test).unwrap_or(&TestStatus::NotFound)
    }
    /// The name the SDK registered for the single matching test: the runner's full name
    /// without describe prefixes, including any description.
    pub fn sdk_name<'a>(&'a self, test: &'a TestRef) -> &'a str {
        self.sdk_names.get(test).unwrap_or(&test.name)
    }
}

/// Runners prefix describe titles with spaces, and the SDK may append a description after
/// one space. The stable name must therefore appear as a whole space-delimited part.
/// The JSON tuple ends with `]`, so one scenario's name cannot extend into another's.
fn scenario_part<'a>(runner_name: &'a str, scenario_name: &str) -> Option<&'a str> {
    runner_name
        .match_indices(scenario_name)
        .map(|(start, _)| start)
        .find(|&start| {
            let before = &runner_name[..start];
            let after = &runner_name[start + scenario_name.len()..];
            (before.is_empty() || before.ends_with(' '))
                && (after.is_empty() || after.starts_with(' '))
        })
        .map(|start| &runner_name[start..])
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
    let mut sdk_names = BTreeMap::new();
    let mut findings = Vec::new();
    for registration in registrations {
        for case in &registration.cases {
            let Some(test) = case.test() else { continue };
            let matches: Vec<_> = match paths.get(&test.file) {
                // Legacy names are user-specified and must match exactly.
                Some(path) if case.scenario().is_none() => results
                    .get(&(path, test.name.clone()))
                    .into_iter()
                    .flat_map(|values| values.iter().map(|status| (test.name.as_str(), status)))
                    .collect(),
                Some(path) => results
                    .iter()
                    .filter(|((file, _), _)| *file == path)
                    .filter_map(|((_, name), values)| {
                        scenario_part(name, &test.name).map(|part| (part, values))
                    })
                    .flat_map(|(part, values)| values.iter().map(move |status| (part, status)))
                    .collect(),
                None => Vec::new(),
            };
            let status = match matches.as_slice() {
                [] => TestStatus::NotFound,
                [(name, status)] => {
                    sdk_names.insert(test.clone(), (*name).to_owned());
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
            sdk_names,
        },
        findings,
    )
}
