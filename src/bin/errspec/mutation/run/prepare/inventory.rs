//! Acquire source inventory while preserving partial baseline and candidate evidence.
use super::super::{
    Configuration, Failure,
    driver::{Driver, Response, execution, only},
};
use errspec::mutation::{self, Candidate, Mutant, Report, Status};
use serde_json::json;
use std::collections::BTreeMap;
use std::{collections::BTreeSet, fs, time::Duration};

pub(in crate::mutation::run) struct Inventory {
    pub(in crate::mutation::run) files: BTreeSet<String>,
    pub(in crate::mutation::run) keys: BTreeSet<String>,
    pub(in crate::mutation::run) syntax: BTreeMap<String, mutation::syntax::Metadata>,
    pub(in crate::mutation::run) sources: BTreeMap<String, String>,
}

pub(in crate::mutation::run) fn acquire(
    configuration: &Configuration,
    driver: &mut Driver,
    custom_mutators: &[String],
    report: &mut Report,
) -> Result<(Inventory, Response), Failure> {
    let request = &configuration.request;
    let changes = &configuration.changes;
    // Baselines use the project's runner timeouts, not a whole-run time budget.
    let prepared = driver.request(
        json!({"op":if request.cache {"discover"} else {"prepare"}}),
        Duration::MAX,
        Duration::MAX,
    )?;
    report.tests_executed |= prepared.messages.iter().any(|m| m["type"] == "progress");
    if let Some(value) = prepared.messages.iter().find(|m| m["type"] == "baseline") {
        report.baseline = execution(value)?.tests;
    }
    // Preserve successfully inventoried files even when prepare fails later.
    let mut files = BTreeSet::new();
    let mut keys = BTreeSet::new();
    let mut syntax = std::collections::BTreeMap::new();
    let mut sources = std::collections::BTreeMap::new();
    for file in prepared.messages.iter().filter(|m| m["type"] == "file") {
        let name = file["file"]
            .as_str()
            .ok_or_else(|| Failure::protocol("Missing inventory filename"))?;
        if !files.insert(name.to_owned()) {
            return Err(Failure::protocol("Duplicate inventory file"));
        }
        let candidates: Vec<Candidate> = serde_json::from_value(file["candidates"].clone())
            .map_err(|e| Failure::protocol(e.to_string()))?;
        if changes.is_some() || request.cache && request.cache_scope {
            let metadata: mutation::syntax::Metadata =
                serde_json::from_value(file["metadata"].clone())
                    .map_err(|e| Failure::protocol(e.to_string()))?;
            let source = fs::read_to_string(request.root.join(name))?;
            if !mutation::syntax::source_matches(&metadata, &source) {
                return Err(Failure::new(
                    "EC_MUTATION_INPUT",
                    "Source changed since syntax acquisition",
                ));
            }
            mutation::selection::validate(&metadata, &source, &candidates)
                .map_err(Failure::protocol)?;
            syntax.insert(name.to_owned(), metadata);
            sources.insert(name.to_owned(), source);
        }
        let critical: BTreeSet<String> =
            serde_json::from_value(file.get("critical").cloned().unwrap_or(json!([])))
                .map_err(|e| Failure::protocol(e.to_string()))?;
        if critical
            .iter()
            .any(|key| !candidates.iter().any(|c| &c.key == key))
        {
            return Err(Failure::protocol("Unknown critical mutant key"));
        }
        if !critical.is_empty() && report.policy.is_none() {
            report.policy = Some(mutation::policy::PolicyReport::new(request.policy.clone()));
        }
        for candidate in candidates {
            if candidate.location.file != name
                || !keys.insert(candidate.key.clone())
                || !(mutation::MUTATORS.contains(&candidate.mutator.as_str())
                    || custom_mutators.contains(&candidate.mutator))
                || candidate.location.byte_offset > candidate.end
                || candidate.key.len() != 64
            {
                return Err(Failure::protocol("Invalid candidate identity or location"));
            }
            let status = if candidate.ignored.is_some() {
                Status::Ignored
            } else {
                Status::Pending
            };
            report.mutants.push(Mutant {
                critical: critical.contains(&candidate.key),
                accepted: None,
                reused: false,
                infected_by: None,
                matrix: None,
                candidate,
                status,
                covered_by: vec![],
                killed_by: vec![],
                failed_modules: vec![],
                attempts: vec![],
                reason: None,
            });
        }
    }
    if prepared.timeout {
        return Err(Failure::new(
            "EC_MUTATION_BASELINE",
            "Baseline or generation timed out",
        ));
    }
    if let Err(mut failure) = prepared.check_error() {
        if failure.code == "EC_MUTATION_BASELINE"
            && let Ok(value) = only(&prepared.messages, "baseline")
            && let Ok(plain) = execution(value)
            && let Err(message) = mutation::validate_baseline(&plain, "Unmodified baseline")
        {
            failure.message = message;
        }
        return Err(failure);
    }
    Ok((
        Inventory {
            files,
            keys,
            syntax,
            sources,
        },
        prepared,
    ))
}
