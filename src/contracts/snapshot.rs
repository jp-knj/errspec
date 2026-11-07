//! Baseline capture and comparison, independent of execution and report assembly.
use crate::{contract::Registration, model::*};
use std::collections::{BTreeMap, BTreeSet};

pub fn capture(registrations: &[Registration<'_>]) -> Snapshot {
    Snapshot {
        schema_version: 2,
        kind: "errspec/snapshot".into(),
        contracts: registrations
            .iter()
            .map(|c| {
                (
                    c.contract.id().to_owned(),
                    SnapshotContract {
                        source: c.contract.source().clone(),
                        tags: c.tags.clone(),
                        scenarios: c.cases.iter().any(|case| case.scenario().is_some()).then(
                            || {
                                c.cases
                                    .iter()
                                    .filter_map(|case| {
                                        Some((
                                            case.scenario()?.0.to_owned(),
                                            SnapshotScenario {
                                                tag: case.tag().to_owned(),
                                                expect: case.registered()?.expect().to_owned(),
                                                test: case.registered()?.test().clone(),
                                                observe: case.scenario()?.1.clone(),
                                            },
                                        ))
                                    })
                                    .collect()
                            },
                        ),
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
