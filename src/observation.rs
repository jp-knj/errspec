//! Trusted SDK observations, scoped to one CLI invocation. These are execution
//! evidence, not proof that the chosen fault or expected value models the domain.
use crate::{
    contract::Registration,
    evidence::{ResolvedPaths, TestRun, TestStatus},
    model::*,
};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

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

/// Identity of the current CLI execution, supplied by the runner boundary.
pub struct RunContext {
    id: String,
}

impl RunContext {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

/// Format-checked records belonging to one execution. They are not verdicts.
#[derive(Debug)]
pub struct ValidatedObservations {
    run_id: String,
    records: Vec<Observation>,
}

impl ValidatedObservations {
    pub(crate) fn new(run: &RunContext) -> Self {
        Self {
            run_id: run.id.clone(),
            records: Vec::new(),
        }
    }
    pub(crate) fn push(&mut self, record: Observation) -> Result<(), Finding> {
        validate_record(&record, &self.run_id)?;
        self.records.push(record);
        Ok(())
    }
    pub fn records(&self) -> &[Observation] {
        &self.records
    }
}

fn invalid(message: impl Into<String>) -> Finding {
    Finding::new("EC_EVIDENCE", message)
}

/// Validate in-memory records using the same boundary as file-based input.
pub fn validate(
    records: Vec<Observation>,
    run: &RunContext,
) -> Result<ValidatedObservations, Finding> {
    let mut validated = ValidatedObservations::new(run);
    for record in records {
        validated.push(record)?;
    }
    Ok(validated)
}

fn validate_record(record: &Observation, run_id: &str) -> Result<(), Finding> {
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

#[derive(Debug)]
pub struct ScenarioJudgment {
    pub status: ObservationStatus,
    pub faults: Vec<FaultObservation>,
    pub assertions: Vec<AssertionObservation>,
}

impl ScenarioJudgment {
    fn new(status: ObservationStatus) -> Self {
        Self {
            status,
            faults: Vec::new(),
            assertions: Vec::new(),
        }
    }
}

#[derive(Debug, Default)]
pub struct ObservationJudgments {
    cases: BTreeMap<String, BTreeMap<String, ScenarioJudgment>>,
}

impl ObservationJudgments {
    pub fn get(&self, contract: &str, scenario: &str) -> Option<&ScenarioJudgment> {
        self.cases.get(contract)?.get(scenario)
    }
}

/// Match validated records against registration and test judgments without mutating either.
///
/// ```compile_fail
/// use errspec::{contract::Registration, evidence::{ResolvedPaths, TestRun}, observation::{self, Observation, RunContext}};
/// fn unchecked(contracts: &[Registration<'_>], tests: &TestRun, paths: &ResolvedPaths, raw: &[Observation], run: &RunContext) {
///     observation::judge(contracts, tests, paths, run, raw);
/// }
/// ```
pub fn judge(
    registrations: &[Registration<'_>],
    tests: &TestRun,
    paths: &ResolvedPaths,
    run: &RunContext,
    records: &ValidatedObservations,
) -> Result<(ObservationJudgments, Vec<Finding>), Finding> {
    if records.run_id != run.id {
        return Err(invalid(
            "Observation protocol or current run id does not match",
        ));
    }
    let mut index: BTreeMap<(&str, &str), Vec<&Observation>> = BTreeMap::new();
    for record in &records.records {
        index
            .entry((&record.contract_id, &record.scenario_id))
            .or_default()
            .push(record);
    }
    let mut judgments = ObservationJudgments::default();
    let mut findings = Vec::new();
    for registration in registrations {
        let contract = registration.contract;
        for case in &registration.cases {
            let (Some((id, required)), Some(test)) = (case.scenario(), case.test()) else {
                continue;
            };
            let finding = |code: &str, message: String| {
                Finding::new(code, message)
                    .for_case(contract.id(), case.tag())
                    .for_scenario(Some(id))
            };
            let judgment = judgments
                .cases
                .entry(contract.id().to_owned())
                .or_default()
                .entry(id.to_owned())
                .or_insert_with(|| ScenarioJudgment::new(ObservationStatus::Missing));
            let Some(matches) = index.remove(&(contract.id(), id)) else {
                judgment.status = ObservationStatus::Missing;
                findings.push(finding(
                    "EC008",
                    "No SDK observation from this execution".into(),
                ));
                continue;
            };
            if matches.len() != 1 {
                judgment.status = ObservationStatus::Invalid;
                findings.push(finding(
                    "EC011",
                    "Scenario executed more than once; retries/repeats are ambiguous".into(),
                ));
                continue;
            }
            let record = matches[0];
            let expected_file = paths.get(&test.file);
            let observed_file = paths.get(&record.file);
            if expected_file.is_none()
                || expected_file != observed_file
                || record.test_name != test.name
                || record.tag != case.tag()
            {
                judgment.status = ObservationStatus::Invalid;
                findings.push(finding(
                    "EC011",
                    "Observation file, test name or error tag does not match registration".into(),
                ));
                continue;
            }
            judgment.faults = record.faults.clone();
            judgment.assertions = record.assertions.clone();
            let mut complete = true;
            for id in &required.faults {
                let observed = record.faults.iter().find(|f| f.id == *id);
                if observed.is_none_or(|f| f.calls == 0 || f.calls != f.completed) {
                    complete = false;
                    findings.push(finding(
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
                    findings.push(finding(
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
                findings.push(finding(
                    "EC011",
                    "Observation contains undeclared fault/assertion ids".into(),
                ));
            }
            if record.state != ScenarioState::Passed {
                findings.push(finding(
                    "EC012",
                    format!("Scenario did not finish successfully: {:?}", record.state),
                ));
            }
            judgment.status = match record.state {
                ScenarioState::Failed => ObservationStatus::Failed,
                ScenarioState::Running | ScenarioState::Incomplete => ObservationStatus::Incomplete,
                ScenarioState::Passed if unexpected => ObservationStatus::Invalid,
                ScenarioState::Passed if !complete => ObservationStatus::Incomplete,
                ScenarioState::Passed if tests.status(test) != &TestStatus::Passed => {
                    ObservationStatus::Failed
                }
                ScenarioState::Passed => ObservationStatus::Passed,
            };
        }
    }
    for ((contract, scenario), _) in index {
        let mut f = Finding::new("EC011", "Observation has no registered scenario")
            .for_scenario(Some(scenario));
        f.contract_id = Some(contract.into());
        findings.push(f);
    }
    Ok((judgments, findings))
}
