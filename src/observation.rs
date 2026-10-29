//! Trusted SDK observations, scoped to one CLI invocation. These are execution
//! evidence, not proof that the chosen fault or expected value models the domain.
use crate::model::*;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioState {
    Running,
    Passed,
    Failed,
    Incomplete,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Observation {
    pub schema_version: u32,
    pub kind: String,
    pub run_id: String,
    pub contract_id: String,
    pub scenario_id: String,
    pub tag: String,
    pub file: String,
    pub test_name: String,
    pub state: ScenarioState,
    pub faults: Vec<FaultObservation>,
    pub assertions: Vec<AssertionObservation>,
}

fn invalid(message: impl Into<String>) -> Finding {
    Finding::new("EC_EVIDENCE", message)
}

pub fn read(directory: &Path, run_id: &str) -> Result<Vec<Observation>, Finding> {
    let mut files = fs::read_dir(directory)
        .map_err(|e| invalid(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| invalid(e.to_string()))?;
    files.sort_by_key(|e| e.file_name());
    if files.len() > 10_000 {
        return Err(invalid("Too many observation files (limit 10000)"));
    }
    let mut total = 0;
    let mut records = Vec::new();
    for entry in files {
        let path = entry.path();
        if !entry
            .file_type()
            .map_err(|e| invalid(e.to_string()))?
            .is_file()
            || path.extension().is_none_or(|e| e != "json")
        {
            return Err(invalid("Incomplete or unexpected observation file"));
        }
        let size = entry.metadata().map_err(|e| invalid(e.to_string()))?.len();
        total += size;
        if size > 1_048_576 || total > 33_554_432 {
            return Err(invalid("Observation size limit exceeded"));
        }
        let bytes = fs::read(&path).map_err(|e| invalid(e.to_string()))?;
        let record: Observation = serde_json::from_slice(&bytes)
            .map_err(|e| invalid(format!("Invalid observation JSON: {e}")))?;
        validate(&record, run_id)?;
        records.push(record);
    }
    Ok(records)
}

pub fn validate(record: &Observation, run_id: &str) -> Result<(), Finding> {
    if record.schema_version != 1 || record.kind != "errspec/observation" || record.run_id != run_id
    {
        return Err(invalid(
            "Observation protocol or current run id does not match",
        ));
    }
    if [
        &record.contract_id,
        &record.scenario_id,
        &record.file,
        &record.test_name,
    ]
    .iter()
    .any(|s| s.trim().is_empty())
    {
        return Err(invalid("Observation identity is incomplete"));
    }
    let mut ids = BTreeSet::new();
    for f in &record.faults {
        if f.id.trim().is_empty() || !ids.insert(&f.id) || f.completed > f.calls {
            return Err(invalid("Invalid fault counters or duplicate fault id"));
        }
    }
    ids.clear();
    for a in &record.assertions {
        if a.id.trim().is_empty() || !ids.insert(&a.id) {
            return Err(invalid("Invalid or duplicate assertion id"));
        }
    }
    Ok(())
}

/// Match observations already accepted by `read` (or `validate`) for this run.
pub fn apply(report: &mut Report, root: &Path, records: &[Observation]) {
    let mut index: BTreeMap<(&str, &str), Vec<&Observation>> = BTreeMap::new();
    for record in records {
        index
            .entry((&record.contract_id, &record.scenario_id))
            .or_default()
            .push(record);
    }
    for contract in &mut report.contracts {
        for case in &mut contract.cases {
            let (Some(id), Some(required), Some(test)) = (&case.id, &case.observe, &case.test)
            else {
                continue;
            };
            let finding = |code: &str, message: String| {
                Finding::new(code, message)
                    .for_case(&contract.id, &case.tag)
                    .for_scenario(Some(id))
            };
            let Some(matches) = index.remove(&(contract.id.as_str(), id.as_str())) else {
                case.observation_status = ObservationStatus::Missing;
                report.diagnostics.push(finding(
                    "EC008",
                    "No SDK observation from this execution".into(),
                ));
                continue;
            };
            if matches.len() != 1 {
                case.observation_status = ObservationStatus::Invalid;
                report.diagnostics.push(finding(
                    "EC011",
                    "Scenario executed more than once; retries/repeats are ambiguous".into(),
                ));
                continue;
            }
            let record = matches[0];
            let expected_file = root.join(&test.file).canonicalize().ok();
            let observed_file = root.join(&record.file).canonicalize().ok();
            if expected_file.is_none()
                || expected_file != observed_file
                || record.test_name != test.name
                || record.tag != case.tag
            {
                case.observation_status = ObservationStatus::Invalid;
                report.diagnostics.push(finding(
                    "EC011",
                    "Observation file, test name or error tag does not match registration".into(),
                ));
                continue;
            }
            case.observed_faults = record.faults.clone();
            case.observed_assertions = record.assertions.clone();
            let mut complete = true;
            for id in &required.faults {
                let observed = record.faults.iter().find(|f| f.id == *id);
                if observed.is_none_or(|f| f.calls == 0 || f.calls != f.completed) {
                    complete = false;
                    report.diagnostics.push(finding(
                        "EC009",
                        format!("Required fault was not observed to complete: {id}"),
                    ));
                }
            }
            for id in &required.assertions {
                if !record
                    .assertions
                    .iter()
                    .any(|a| a.id == *id && a.status == AssertionStatus::Passed)
                {
                    complete = false;
                    report.diagnostics.push(finding(
                        "EC010",
                        format!("Required assertion did not pass: {id}"),
                    ));
                }
            }
            let unexpected = record
                .faults
                .iter()
                .any(|f| !required.faults.contains(&f.id))
                || record
                    .assertions
                    .iter()
                    .any(|a| !required.assertions.contains(&a.id));
            if unexpected {
                complete = false;
                report.diagnostics.push(finding(
                    "EC011",
                    "Observation contains undeclared fault/assertion ids".into(),
                ));
            }
            if record.state != ScenarioState::Passed {
                report.diagnostics.push(finding(
                    "EC012",
                    format!("Scenario did not finish successfully: {:?}", record.state),
                ));
            }
            case.observation_status = match record.state {
                ScenarioState::Failed => ObservationStatus::Failed,
                ScenarioState::Running | ScenarioState::Incomplete => ObservationStatus::Incomplete,
                ScenarioState::Passed if unexpected => ObservationStatus::Invalid,
                ScenarioState::Passed if !complete => ObservationStatus::Incomplete,
                ScenarioState::Passed if case.test_status != "passed" => ObservationStatus::Failed,
                ScenarioState::Passed => ObservationStatus::Passed,
            };
        }
    }
    for ((contract, scenario), _) in index {
        let mut f = Finding::new("EC011", "Observation has no registered scenario")
            .for_scenario(Some(scenario));
        f.contract_id = Some(contract.into());
        report.diagnostics.push(f);
    }
}
