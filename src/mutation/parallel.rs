//! Compatibility judgments for isolated workers' fresh probe baselines.
use super::{Execution, validate_baseline, validate_execution};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Serialize)]
pub struct Info {
    pub requested: usize,
    pub workers: usize,
}

pub fn compatible_probe(expected: &Execution, actual: &Execution) -> Result<(), String> {
    validate_execution(expected, &[])?;
    validate_baseline(expected, "Expected probe")?;
    validate_execution(actual, &[])?;
    validate_baseline(actual, "Worker probe")?;
    if expected.wall_timeout || actual.wall_timeout {
        return Err("Worker probe was incomplete".into());
    }
    let keys = |values: &BTreeMap<String, u64>| values.keys().cloned().collect::<BTreeSet<_>>();
    let tests = |execution: &Execution| {
        execution
            .tests
            .iter()
            .map(|test| {
                (
                    test.id.clone(),
                    (
                        test.file.clone(),
                        test.name.clone(),
                        test.status.clone(),
                        test.started,
                        keys(&test.hits),
                        test.infected.clone(),
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>()
    };
    let static_hits = |execution: &Execution| {
        execution
            .static_hits
            .iter()
            .map(|(key, files)| (key.clone(), files.iter().cloned().collect::<BTreeSet<_>>()))
            .collect::<BTreeMap<_, _>>()
    };
    if keys(&expected.hits) != keys(&actual.hits)
        || tests(expected) != tests(actual)
        || static_hits(expected) != static_hits(actual)
    {
        return Err(
            "Worker probe changed test registration, results, reachability or infection".into(),
        );
    }
    Ok(())
}
