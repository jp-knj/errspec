//! Ranked recommendations from current reachability; old kills are hints, never verdicts.
use super::{Diagnostic, Execution, Mutant, selection::Changes};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// Changed scopes without an observed candidate cannot justify omitting tests.
pub fn fallback(
    changes: &Changes,
    mutants: &[Mutant],
    metadata: &BTreeMap<String, super::syntax::Metadata>,
) -> Option<String> {
    for (file, ranges) in &changes.files {
        if super::selection::test_file(file) {
            continue;
        }
        if changes.removals.contains(file) {
            return Some(
                "Removed source lines may erase the former scope; select every collected test file"
                    .into(),
            );
        }
        let Some(facts) = metadata.get(file) else {
            continue;
        };
        for range in ranges {
            let scopes: Vec<_> = facts
                .scopes
                .iter()
                .filter(|(_, scope)| {
                    scope.function && scope.line <= range.end && range.start <= scope.end_line
                })
                .collect();
            if scopes.is_empty() {
                return Some("Changed module-level source has no bounded function cohort; select every collected test file".into());
            }
            for (name, _) in scopes {
                let observed = mutants
                    .iter()
                    .filter(|mutant| {
                        mutant.candidate.location.file == *file && !mutant.covered_by.is_empty()
                    })
                    .any(|mutant| {
                        let mut owner = facts.owners.get(&mutant.candidate.key);
                        while let Some(current) = owner {
                            if current == name {
                                return true;
                            }
                            owner = facts
                                .scopes
                                .get(current)
                                .and_then(|scope| scope.parent.as_ref());
                        }
                        false
                    });
                if !observed {
                    return Some("Changed function has no attributable current mutation coverage; select every collected test file".into());
                }
            }
        }
    }
    None
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pick {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test: Option<String>,
    pub file: String,
    pub kills: usize,
    pub covers: usize,
    pub evidence: &'static str,
    pub reason: String,
}
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    pub warnings: Vec<super::selection::Warning>,
    pub since: String,
    pub base: String,
    pub changed_files: Vec<String>,
    pub current_tests: usize,
    pub observed_mutants: usize,
    pub files: Vec<String>,
    pub killers: Vec<Pick>,
    pub covering: Vec<Pick>,
    pub changed_tests: Vec<Pick>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
}

pub fn rank(
    since: &str,
    changes: &Changes,
    mutants: &[Mutant],
    baseline: &Execution,
    hints: &BTreeMap<String, Vec<String>>,
    full_reason: Option<&str>,
) -> Selection {
    let present: BTreeMap<_, _> = baseline
        .tests
        .iter()
        .map(|test| (test.id.clone(), test))
        .collect();
    let mut coverage = BTreeMap::<String, usize>::new();
    let mut kills = BTreeMap::<String, usize>::new();
    for mutant in mutants {
        for id in &mutant.covered_by {
            if present.contains_key(id) {
                *coverage.entry(id.clone()).or_default() += 1;
                if hints
                    .get(&mutant.candidate.key)
                    .is_some_and(|ids| ids.contains(id))
                {
                    *kills.entry(id.clone()).or_default() += 1;
                }
            }
        }
    }
    let changed_source = changes
        .files
        .keys()
        .any(|file| !super::selection::test_file(file));
    let fallback_reason=full_reason.map(String::from).or_else(||(changed_source&&coverage.is_empty()).then(||"Changed or removed source has no attributable current mutation coverage; select every collected test file".into()));
    let mut ids: Vec<_> = coverage.keys().cloned().collect();
    ids.sort_by(|a, b| {
        kills
            .get(b)
            .copied()
            .unwrap_or(0)
            .cmp(&kills.get(a).copied().unwrap_or(0))
            .then(coverage[b].cmp(&coverage[a]))
            .then(a.cmp(b))
    });
    let mut killers = vec![];
    let mut covering = vec![];
    for id in ids {
        let count = kills.get(&id).copied().unwrap_or(0);
        let covers = coverage[&id];
        let pick = Pick {
            file: present[&id].file.clone(),
            test: Some(id),
            kills: count,
            covers,
            evidence: if count > 0 {
                "historical_kill_hint"
            } else {
                "current_coverage"
            },
            reason: if count > 0 {
                format!(
                    "Previously killed {count} changed-scope mutants; historical ranking hint, currently covers {covers}"
                )
            } else {
                format!("Currently covers {covers} changed-scope mutants")
            },
        };
        if count > 0 {
            killers.push(pick);
        } else {
            covering.push(pick);
        }
    }
    let current_files: BTreeSet<_> = baseline
        .tests
        .iter()
        .map(|test| test.file.clone())
        .collect();
    let mut changed_tests: Vec<_> = changes
        .files
        .keys()
        .filter(|file| current_files.contains(*file))
        .map(|file| Pick {
            test: None,
            file: file.clone(),
            kills: 0,
            covers: 0,
            evidence: "changed_test",
            reason: "Changed test file collected by the current baseline".into(),
        })
        .collect();
    if let Some(reason) = &fallback_reason {
        for file in &current_files {
            if !killers
                .iter()
                .chain(&covering)
                .chain(&changed_tests)
                .any(|pick| &pick.file == file)
            {
                changed_tests.push(Pick {
                    test: None,
                    file: file.clone(),
                    kills: 0,
                    covers: 0,
                    evidence: "conservative",
                    reason: reason.clone(),
                });
            }
        }
    }
    let mut files = vec![];
    let mut seen = BTreeSet::new();
    for pick in killers.iter().chain(&covering).chain(&changed_tests) {
        if seen.insert(pick.file.clone()) {
            files.push(pick.file.clone());
        }
    }
    Selection {
        warnings: vec![],
        since: since.into(),
        base: changes.base.clone(),
        changed_files: changes.files.keys().cloned().collect(),
        current_tests: baseline.tests.len(),
        observed_mutants: mutants.len(),
        files,
        killers,
        covering,
        changed_tests,
        fallback_reason,
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub kind: &'static str,
    pub schema_version: u8,
    pub run_complete: bool,
    pub ok: bool,
    pub tests_executed: bool,
    pub coverage: &'static str,
    #[serde(flatten)]
    pub selection: Selection,
    pub diagnostics: Vec<Diagnostic>,
    #[serde(skip)]
    pub interrupted: Option<u8>,
}
impl Report {
    pub fn from_mutation(mut report: super::Report) -> Self {
        if !report.run_complete
            && let Some(selection) = &mut report.test_selection
        {
            selection.files.clear();
        }
        Self {
            kind: "errspec/test-selection",
            schema_version: 1,
            run_complete: report.run_complete,
            ok: report.run_complete && report.diagnostics.is_empty(),
            tests_executed: report.tests_executed,
            coverage: if report
                .cache
                .as_ref()
                .is_some_and(|info| info.coverage_reused)
            {
                "validated_probe_cache"
            } else {
                "current_probe"
            },
            selection: report.test_selection.take().unwrap_or_default(),
            diagnostics: report.diagnostics,
            interrupted: report.interrupted,
        }
    }
    pub fn exit_code(&self) -> u8 {
        self.interrupted.unwrap_or(if self.ok { 0 } else { 2 })
    }
}
