use crate::{
    config::{CaseKind, ValidatedContract},
    model::*,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

/// Compare only contracts obtained through configuration validation.
///
/// ```compile_fail
/// use errspec::{contract, model::Contract};
/// fn unchecked(raw: &Contract) {
///     contract::compare(std::path::Path::new("."), raw, vec![]);
/// }
/// ```
pub fn compare(
    root: &Path,
    contract: &ValidatedContract,
    tags: Vec<String>,
) -> (ContractResult, Vec<Finding>) {
    let mut findings = Vec::new();
    let mut cases = Vec::new();
    for tag in &tags {
        let registered: Vec<_> = contract
            .cases()
            .iter()
            .filter(|case| case.tag() == tag)
            .collect();
        if registered.is_empty() {
            findings.push(
                Finding::new(
                    "EC001",
                    "Error variant has no registered failure contract and test",
                )
                .for_case(contract.id(), tag),
            );
            cases.push(CaseResult {
                id: None,
                tag: tag.clone(),
                registration: "missing",
                test_status: "not_run".into(),
                test: None,
                expect: None,
                observe: None,
                observation_status: ObservationStatus::NotRequired,
                observed_faults: Vec::new(),
                observed_assertions: Vec::new(),
            });
        }
        for case in registered {
            let (id, observe) = match case.kind() {
                CaseKind::Legacy => (None, None),
                CaseKind::Scenario { id, observe } => (Some(id.as_str()), Some(observe)),
            };
            if !root.join(&case.test().file).is_file() {
                findings.push(
                    Finding::new(
                        "EC003",
                        format!("Referenced test file does not exist: {}", case.test().file),
                    )
                    .for_case(contract.id(), tag)
                    .for_scenario(id),
                );
            }
            cases.push(CaseResult {
                id: id.map(str::to_owned),
                tag: tag.clone(),
                registration: "registered",
                test_status: "not_run".into(),
                test: Some(case.test().clone()),
                expect: Some(case.expect().to_owned()),
                observe: observe.cloned(),
                observation_status: if observe.is_some() {
                    ObservationStatus::NotRun
                } else {
                    ObservationStatus::NotRequired
                },
                observed_faults: Vec::new(),
                observed_assertions: Vec::new(),
            });
        }
    }
    for case in contract.cases() {
        let id = match case.kind() {
            CaseKind::Legacy => None,
            CaseKind::Scenario { id, .. } => Some(id.as_str()),
        };
        if !tags.iter().any(|tag| tag == case.tag()) {
            findings.push(
                Finding::new("EC004", "Registered tag no longer exists in the error type")
                    .for_case(contract.id(), case.tag())
                    .for_scenario(id),
            );
        }
    }
    (
        ContractResult {
            id: contract.id().to_owned(),
            source: contract.source().clone(),
            declared_boundaries: contract.boundaries().to_vec(),
            tags,
            cases,
        },
        findings,
    )
}

pub fn snapshot(report: &Report) -> Snapshot {
    Snapshot {
        schema_version: 2,
        kind: "errspec/snapshot".into(),
        contracts: report
            .contracts
            .iter()
            .map(|c| {
                (
                    c.id.clone(),
                    SnapshotContract {
                        source: c.source.clone(),
                        tags: c.tags.clone(),
                        scenarios: c.cases.iter().any(|case| case.id.is_some()).then(|| {
                            c.cases
                                .iter()
                                .filter_map(|case| {
                                    Some((
                                        case.id.clone()?,
                                        SnapshotScenario {
                                            tag: case.tag.clone(),
                                            expect: case.expect.clone()?,
                                            test: case.test.clone()?,
                                            observe: case.observe.clone()?,
                                        },
                                    ))
                                })
                                .collect()
                        }),
                    },
                )
            })
            .collect(),
    }
}

pub fn diff(current: &Snapshot, previous: &Snapshot) -> Result<Delta, Finding> {
    if !matches!(previous.schema_version, 1 | 2)
        || !matches!(
            previous.kind.as_str(),
            "errspec/snapshot" | "error-contract/snapshot"
        )
    {
        return Err(Finding::new(
            "EC_BASELINE",
            "Unsupported snapshot kind or version",
        ));
    }
    let mut delta = Delta::default();
    for (id, new) in &current.contracts {
        let old = previous.contracts.get(id);
        if old.is_some_and(|old| old.source != new.source) {
            return Err(Finding::new(
                "EC_BASELINE",
                format!("Source changed for {id}; review and create a new baseline"),
            ));
        }
        let before: BTreeSet<_> = old
            .map(|v| v.tags.iter().cloned().collect())
            .unwrap_or_default();
        let after: BTreeSet<_> = new.tags.iter().cloned().collect();
        let added: Vec<_> = after.difference(&before).cloned().collect();
        let removed: Vec<_> = before.difference(&after).cloned().collect();
        if !added.is_empty() {
            delta.added.insert(id.clone(), added);
        }
        if !removed.is_empty() {
            delta.removed.insert(id.clone(), removed);
        }
        let empty = BTreeMap::new();
        let before = old.and_then(|old| old.scenarios.as_ref()).unwrap_or(&empty);
        let after = new.scenarios.as_ref().unwrap_or(&empty);
        let added: Vec<_> = after
            .keys()
            .filter(|key| !before.contains_key(*key))
            .cloned()
            .collect();
        let removed: Vec<_> = before
            .keys()
            .filter(|key| !after.contains_key(*key))
            .cloned()
            .collect();
        let changed: Vec<_> = after
            .iter()
            .filter(|(key, value)| before.get(*key).is_some_and(|old| old != *value))
            .map(|(key, _)| key.clone())
            .collect();
        if !added.is_empty() {
            delta.added_scenarios.insert(id.clone(), added);
        }
        if !removed.is_empty() {
            delta.removed_scenarios.insert(id.clone(), removed);
        }
        if !changed.is_empty() {
            delta.changed_scenarios.insert(id.clone(), changed);
        }
    }
    delta.removed_contracts = previous
        .contracts
        .keys()
        .filter(|id| !current.contracts.contains_key(*id))
        .cloned()
        .collect();
    Ok(delta)
}
