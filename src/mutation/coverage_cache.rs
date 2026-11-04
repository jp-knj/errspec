//! Reuse of probe observations, independently of mutant-trial outcomes.
use super::{Execution, validate_baseline, validate_execution};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct File {
    kind: String,
    schema_version: u8,
    fingerprint: String,
    acquisition_mode: String,
    execution: Execution,
}
impl File {
    pub fn new(fingerprint: String, execution: Execution) -> Self {
        Self {
            kind: "errspec/mutation-coverage".into(),
            schema_version: 1,
            fingerprint,
            acquisition_mode: "probe_all_tests".into(),
            execution,
        }
    }

    /// The ordinary baseline is always current. Registration, statuses and
    /// candidate references must agree before old observations can be applied.
    pub fn reuse(
        &self,
        fingerprint: &str,
        plain: &Execution,
        keys: &BTreeSet<String>,
    ) -> Option<Execution> {
        if self.kind != "errspec/mutation-coverage"
            || self.schema_version != 1
            || self.acquisition_mode != "probe_all_tests"
            || self.fingerprint != fingerprint
            || fingerprint.len() != 64
            || !fingerprint.bytes().all(|c| c.is_ascii_hexdigit())
            || !self.execution.selected.is_empty()
            || self.execution.wall_timeout
            || plain.wall_timeout
            || validate_execution(plain, &[]).is_err()
            || validate_execution(&self.execution, &[]).is_err()
            || validate_baseline(plain, "Current baseline").is_err()
            || validate_baseline(&self.execution, "Cached probe").is_err()
            || self.execution.tests.len() != plain.tests.len()
        {
            return None;
        }
        let cached: BTreeMap<_, _> = self.execution.tests.iter().map(|t| (&t.id, t)).collect();
        let files: BTreeSet<_> = plain.tests.iter().map(|t| &t.file).collect();
        let execution = &self.execution;
        if execution.hits.iter().any(|(key, count)| {
            let observed = execution.tests.iter().try_fold(0u64, |sum, test| {
                sum.checked_add(test.hits.get(key).copied().unwrap_or(0))
            });
            !keys.contains(key)
                || *count == 0
                || observed.is_none_or(|sum| {
                    sum > *count || sum == 0 && !execution.static_hits.contains_key(key)
                })
        }) || execution.static_hits.iter().any(|(key, hit_files)| {
            !execution.hits.contains_key(key)
                || hit_files.is_empty()
                || hit_files.iter().collect::<BTreeSet<_>>().len() != hit_files.len()
                || hit_files.iter().any(|file| !files.contains(file))
        }) || execution.module_durations.iter().any(|(file, duration)| {
            !files.contains(file) || !duration.is_finite() || *duration < 0.0
        }) {
            return None;
        }
        let mut result = execution.clone();
        result.tests.clear();
        for current in &plain.tests {
            let old = *cached.get(&current.id)?;
            if old.file != current.file
                || old.name != current.name
                || old.status != current.status
                || old.started != current.started
                || !old.errors.is_empty()
                || !old.started && !old.hits.is_empty()
                || old.hits.iter().any(|(key, count)| {
                    *count == 0 || execution.hits.get(key).is_none_or(|total| count > total)
                })
            {
                return None;
            }
            let mut observed = old.clone();
            observed.task_id = current.task_id.clone();
            observed.duration = old.duration.max(current.duration);
            result.tests.push(observed);
        }
        for (file, duration) in &plain.module_durations {
            result
                .module_durations
                .entry(file.clone())
                .and_modify(|old| *old = old.max(*duration))
                .or_insert(*duration);
        }
        Some(result)
    }
}
