//! Result reuse judgments over owned cache records and current execution facts.
use super::{Attempt, Mutant, Status};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub const ENGINE_FINGERPRINT: &str = env!("ERRSPEC_ENGINE_FINGERPRINT");

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct File {
    kind: String,
    schema_version: u8,
    fingerprint: String,
    acquisition_mode: String,
    records: BTreeMap<String, Record>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    covered_by: Vec<String>,
    selected: Vec<String>,
    attempt: Attempt,
}
impl Record {
    fn valid(&self, mutant: &Mutant, selected: &[String]) -> bool {
        if selected.is_empty()
            || self.selected != selected
            || self.covered_by != mutant.covered_by
            || self.covered_by.iter().collect::<BTreeSet<_>>().len() != self.covered_by.len()
            || selected.iter().any(|id| !self.covered_by.contains(id))
            || self.selected.iter().collect::<BTreeSet<_>>().len() != self.selected.len()
            || self.attempt.number != 1
            || !self.attempt.failed_modules.is_empty()
            || self.attempt.killed_by.iter().collect::<BTreeSet<_>>().len()
                != self.attempt.killed_by.len()
            || self
                .attempt
                .killed_by
                .iter()
                .any(|id| !selected.contains(id))
        {
            return false;
        }
        match self.attempt.status {
            Status::Killed => {
                !self.attempt.killed_by.is_empty() && self.attempt.reason == "Selected test failed"
            }
            Status::Survived => {
                self.attempt.killed_by.is_empty()
                    && self.attempt.reason == "All selected tests passed"
            }
            _ => false,
        }
    }
}
impl File {
    pub fn new(fingerprint: String) -> Self {
        Self {
            kind: "errspec/mutation-results".into(),
            schema_version: 1,
            fingerprint,
            acquisition_mode: "first_detection".into(),
            records: BTreeMap::new(),
        }
    }
    pub fn compatible(&self, fingerprint: &str) -> bool {
        self.kind == "errspec/mutation-results"
            && self.schema_version == 1
            && self.acquisition_mode == "first_detection"
            && self.fingerprint == fingerprint
            && fingerprint.len() == 64
            && fingerprint.bytes().all(|c| c.is_ascii_hexdigit())
    }
    /// Recheck identity and test evidence before applying any deserialized status.
    pub fn reuse(&self, fingerprint: &str, mutant: &mut Mutant, selected: &[String]) -> bool {
        if !self.compatible(fingerprint) || mutant.status != Status::Pending {
            return false;
        }
        let Some(record) = self
            .records
            .get(&mutant.candidate.key)
            .filter(|record| record.valid(mutant, selected))
        else {
            return false;
        };
        mutant.status = record.attempt.status;
        mutant.killed_by = mutant
            .covered_by
            .iter()
            .filter(|id| record.attempt.killed_by.contains(id))
            .cloned()
            .collect();
        mutant.failed_modules.clear();
        mutant.attempts = vec![record.attempt.clone()];
        mutant.reused = true;
        true
    }
    /// Only stable, first-attempt assertion outcomes are eligible. Timeout,
    /// runtime-error, compilation and reachability decisions are revalidated.
    pub fn record(&mut self, mutant: &Mutant, selected: &[String]) -> bool {
        if mutant.attempts.len() != 1 || mutant.attempts[0].status != mutant.status {
            return false;
        }
        let record = Record {
            covered_by: mutant.covered_by.clone(),
            selected: selected.into(),
            attempt: mutant.attempts[0].clone(),
        };
        if !record.valid(mutant, selected) {
            return false;
        }
        self.records.insert(mutant.candidate.key.clone(), record);
        true
    }
    pub fn retain(&mut self, keys: &BTreeSet<String>) {
        self.records.retain(|key, _| keys.contains(key));
    }
    pub fn len(&self) -> usize {
        self.records.len()
    }
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub relocation: bool,
    pub directory: String,
    pub reused: usize,
    pub executed: usize,
    pub saved: usize,
    pub invalidated: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub coverage_reused: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled_reason: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}
