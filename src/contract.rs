use crate::model::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub fn validate(config: &Config) -> Result<(), Finding> {
    if !matches!(config.schema_version, 1 | 2)
        || config.project.trim().is_empty()
        || config.contracts.is_empty()
    {
        return Err(Finding::new(
            "EC_CONFIG",
            "Require schemaVersion: 1 or 2, a project, and at least one contract",
        ));
    }
    let mut ids = BTreeSet::new();
    for c in &config.contracts {
        if c.id.trim().is_empty() || !ids.insert(&c.id) {
            return Err(Finding::new(
                "EC_CONFIG",
                format!("Empty or duplicate contract id: {:?}", c.id),
            ));
        }
        if [&c.source.file, &c.source.export, &c.source.discriminant]
            .iter()
            .any(|s| s.trim().is_empty())
        {
            return Err(Finding::new(
                "EC_CONFIG",
                format!("Incomplete source in {}", c.id),
            ));
        }
        let mut identities = BTreeSet::new();
        for case in &c.cases {
            let invalid = |message: &str| {
                Finding::new("EC_CONFIG", message)
                    .for_case(&c.id, &case.tag)
                    .for_scenario(case.id.as_deref())
            };
            if [&case.expect, &case.test.file]
                .iter()
                .any(|s| s.trim().is_empty())
            {
                return Err(invalid("Case requires expect and test.file"));
            }
            if config.schema_version == 1 {
                if case.id.is_some() || case.observe.is_some() || case.test.name.trim().is_empty() {
                    return Err(invalid(
                        "Version 1 requires test.name; scenario ids and observe require schemaVersion: 2",
                    ));
                }
                if !identities.insert(case.tag.as_str()) {
                    return Err(invalid("Duplicate case tag"));
                }
            } else {
                let Some(id) = case.id.as_deref().filter(|id| !id.trim().is_empty()) else {
                    return Err(invalid("Version 2 requires a nonempty scenario id"));
                };
                if !identities.insert(id) {
                    return Err(invalid("Duplicate scenario id within contract"));
                }
                if !case.test.name.is_empty() && case.test.name != scenario_test_name(&c.id, id) {
                    return Err(invalid(
                        "Version 2 uses SDK-generated test names; omit test.name",
                    ));
                }
                let Some(requirements) = &case.observe else {
                    return Err(invalid(
                        "Version 2 requires observe.faults and observe.assertions",
                    ));
                };
                for list in [&requirements.faults, &requirements.assertions] {
                    let unique: BTreeSet<_> = list.iter().collect();
                    if list.is_empty()
                        || unique.len() != list.len()
                        || list.iter().any(|id| id.trim().is_empty())
                    {
                        return Err(invalid(
                            "Observation ids must be nonempty, unique, nonempty lists",
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn compare(
    root: &Path,
    contract: &Contract,
    tags: Vec<String>,
) -> (ContractResult, Vec<Finding>) {
    let mut findings = Vec::new();
    let mut cases = Vec::new();
    for tag in &tags {
        let registered: Vec<_> = contract
            .cases
            .iter()
            .filter(|case| case.tag == *tag)
            .collect();
        if registered.is_empty() {
            findings.push(
                Finding::new(
                    "EC001",
                    "Error variant has no registered failure contract and test",
                )
                .for_case(&contract.id, tag),
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
            if !root.join(&case.test.file).is_file() {
                findings.push(
                    Finding::new(
                        "EC003",
                        format!("Referenced test file does not exist: {}", case.test.file),
                    )
                    .for_case(&contract.id, tag)
                    .for_scenario(case.id.as_deref()),
                );
            }
            let mut test = case.test.clone();
            if let Some(id) = &case.id {
                test.name = scenario_test_name(&contract.id, id);
            }
            cases.push(CaseResult {
                id: case.id.clone(),
                tag: tag.clone(),
                registration: "registered",
                test_status: "not_run".into(),
                test: Some(test),
                expect: Some(case.expect.clone()),
                observe: case.observe.clone(),
                observation_status: if case.observe.is_some() {
                    ObservationStatus::NotRun
                } else {
                    ObservationStatus::NotRequired
                },
                observed_faults: Vec::new(),
                observed_assertions: Vec::new(),
            });
        }
    }
    for case in &contract.cases {
        if !tags.contains(&case.tag) {
            findings.push(
                Finding::new("EC004", "Registered tag no longer exists in the error type")
                    .for_case(&contract.id, &case.tag)
                    .for_scenario(case.id.as_deref()),
            );
        }
    }
    (
        ContractResult {
            id: contract.id.clone(),
            source: contract.source.clone(),
            declared_boundaries: contract.boundaries.clone(),
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
