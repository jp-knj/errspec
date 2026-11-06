//! Complete per-test observations and conservative relationships over acquired rows.
use super::{Execution, Mutant, Status};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Observation {
    pub complete: bool,
    pub selected: Vec<String>,
    pub executed: Vec<String>,
    pub killed_by: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub incomplete_reason: Option<String>,
}
impl Observation {
    pub fn acquire(result: &Execution, selected: &[String]) -> Self {
        let mut executed = vec![];
        let mut killed_by = vec![];
        let timeout = result.wall_timeout
            || result
                .tests
                .iter()
                .any(|test| test.errors.iter().any(|error| super::is_timeout(error)));
        let mut invalid = false;
        for id in selected {
            match result.tests.iter().find(|test| &test.id == id) {
                Some(test)
                    if test.started && matches!(test.status.as_str(), "passed" | "failed") =>
                {
                    executed.push(id.clone());
                    if test.status == "failed" {
                        if test.errors.is_empty()
                            || test.errors.iter().any(|error| super::is_timeout(error))
                        {
                            invalid = true;
                        } else {
                            killed_by.push(id.clone());
                        }
                    }
                }
                _ => invalid = true,
            }
        }
        let reason = if let Some(reason) = result.acquisition_incomplete.as_deref() {
            Some(reason)
        } else if timeout {
            Some("Timeout or hit limit leaves unknown test outcomes")
        } else if !result.failed_modules.is_empty() || !result.errors.is_empty() {
            Some("Runner or module errors prevent complete test attribution")
        } else if invalid
            || selected.is_empty()
            || super::validate_execution(result, selected).is_err()
            || selected.iter().collect::<BTreeSet<_>>().len() != selected.len()
        {
            Some("Not every selected test has an ordinary completed result")
        } else {
            None
        };
        Self {
            complete: reason.is_none(),
            selected: selected.into(),
            executed,
            killed_by,
            incomplete_reason: reason.map(String::from),
        }
    }
    pub fn invalidate(&mut self, reason: &str) {
        self.complete = false;
        self.incomplete_reason = Some(reason.into());
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub key: String,
    pub status: Status,
    #[serde(flatten)]
    pub observation: Observation,
}
#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub full: usize,
    pub timeout: usize,
    pub incomplete: usize,
}
#[derive(Debug, Serialize)]
pub struct Subsumed {
    pub test: String,
    pub kills: usize,
    pub by: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct Duplicates {
    pub tests: Vec<String>,
    pub kills: usize,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub acquisition: &'static str,
    pub rows: Vec<Row>,
    pub mutants: Counts,
    pub tests: usize,
    pub no_kills: Vec<String>,
    pub subsumed: Vec<Subsumed>,
    pub duplicates: Vec<Duplicates>,
}
pub fn analyze(mutants: &[Mutant]) -> Report {
    let rows: Vec<_> = mutants
        .iter()
        .map(|mutant| {
            let mut observation =
                mutant.matrix.clone().unwrap_or_else(|| Observation {
                    complete: false,
                    selected: mutant.covered_by.clone(),
                    executed: vec![],
                    killed_by: vec![],
                    incomplete_reason: Some(mutant.reason.clone().unwrap_or_else(|| {
                        format!("No complete acquisition for {:?}", mutant.status)
                    })),
                });
            if observation.complete
                && (observation.selected.is_empty()
                    || observation.selected.iter().collect::<BTreeSet<_>>().len()
                        != observation.selected.len()
                    || observation.killed_by.iter().collect::<BTreeSet<_>>().len()
                        != observation.killed_by.len()
                    || observation
                        .killed_by
                        .iter()
                        .any(|id| !observation.selected.contains(id))
                    || (mutant.status == Status::Killed && observation.killed_by.is_empty())
                    || (mutant.status == Status::Survived && !observation.killed_by.is_empty())
                    || observation.selected != mutant.covered_by
                    || observation.executed != observation.selected
                    || observation.killed_by != mutant.killed_by
                    || !matches!(mutant.status, Status::Killed | Status::Survived))
            {
                observation.invalidate("Row evidence does not match the current candidate result");
            }
            Row {
                key: mutant.candidate.key.clone(),
                status: mutant.status,
                observation,
            }
        })
        .collect();
    let mut counts = Counts::default();
    let mut covering = BTreeSet::new();
    let mut kills = BTreeMap::<String, BTreeSet<String>>::new();
    for row in &rows {
        if row.observation.complete {
            counts.full += 1;
            covering.extend(row.observation.selected.iter().cloned());
            for test in &row.observation.killed_by {
                kills
                    .entry(test.clone())
                    .or_default()
                    .insert(row.key.clone());
            }
        } else if row.status == Status::Timeout {
            counts.timeout += 1;
        } else if !row.observation.selected.is_empty()
            && !matches!(
                row.status,
                Status::Ignored | Status::CompileError | Status::NoCoverage
            )
        {
            counts.incomplete += 1;
        }
    }
    let no_kills = covering
        .iter()
        .filter(|id| !kills.contains_key(*id))
        .cloned()
        .collect();
    let mut subsumed = vec![];
    let mut signatures = BTreeMap::<Vec<String>, Vec<String>>::new();
    for (test, own) in &kills {
        let mut by: Vec<_> = kills
            .iter()
            .filter(|(other, set)| *other != test && own.len() < set.len() && own.is_subset(set))
            .map(|(id, _)| id.clone())
            .collect();
        by.sort_by(|a, b| kills[b].len().cmp(&kills[a].len()).then(a.cmp(b)));
        if !by.is_empty() {
            subsumed.push(Subsumed {
                test: test.clone(),
                kills: own.len(),
                by,
            });
        }
        signatures
            .entry(own.iter().cloned().collect())
            .or_default()
            .push(test.clone());
    }
    let mut duplicates: Vec<_> = signatures
        .into_iter()
        .filter(|(_, tests)| tests.len() > 1)
        .map(|(keys, tests)| Duplicates {
            tests,
            kills: keys.len(),
        })
        .collect();
    duplicates.sort_by(|a, b| a.tests[0].cmp(&b.tests[0]));
    Report {
        acquisition: "all_covering_tests",
        rows,
        mutants: counts,
        tests: covering.len(),
        no_kills,
        subsumed,
        duplicates,
    }
}
