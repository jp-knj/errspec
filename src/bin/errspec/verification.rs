//! Coordinate test and observation judgments after runner execution.
use crate::{input::resolve_paths, request::Runner, runner};
use errspec::{contract::Registration, evidence, model::Finding, observation};
use std::path::{Path, PathBuf};

pub(crate) struct Verification {
    pub tests: evidence::TestRun,
    pub diagnostics: Vec<Finding>,
    // Observation read failure must retain the successfully parsed runner results.
    pub observations: Result<(observation::ObservationJudgments, Vec<Finding>), Finding>,
}

pub(crate) fn verify(
    registrations: &[Registration<'_>],
    root: &Path,
    runner: Runner,
    vitest: Option<&Path>,
    bun: Option<&Path>,
    test_files: &[PathBuf],
) -> Result<Verification, Finding> {
    let execution = runner::run(root, runner, vitest, bun, test_files)?;
    let registered_files = || {
        registrations
            .iter()
            .flat_map(|r| &r.cases)
            .filter_map(|case| case.test().map(|test| test.file.as_str()))
    };
    let mut paths = resolve_paths(root, registered_files());
    paths.extend(resolve_paths(
        root,
        execution
            .evidence
            .tests
            .iter()
            .map(|test| test.file.as_str()),
    ));
    let (tests, diagnostics) = evidence::judge(
        registrations,
        &paths,
        execution.evidence,
        execution.process_ok,
    );
    let run = execution.observations.context();
    let observations = (|| {
        let records = execution.observations.read()?;
        // Resolve again after reading observations, preserving the existing phase boundary.
        let mut paths = resolve_paths(root, registered_files());
        paths.extend(resolve_paths(
            root,
            records.records().iter().map(|record| record.file.as_str()),
        ));
        observation::judge(registrations, &tests, &paths, run, &records)
    })();
    Ok(Verification {
        tests,
        diagnostics,
        observations,
    })
}
