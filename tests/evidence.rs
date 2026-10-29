use errspec::config::ValidatedConfig;
use errspec::{contract, model::*, vitest};
use serde_json::json;
use std::fs;

#[test]
fn skipped_missing_ambiguous_or_failed_tests_never_count_as_passed() {
    for (states, wanted) in [
        (vec!["passed"], "passed"),
        (vec!["pending"], "pending"),
        (vec!["failed"], "failed"),
        (vec![], "not_found"),
        (vec!["passed", "passed"], "ambiguous"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("case.test.ts"), "").unwrap();
        let c: Contract=serde_json::from_value(json!({"id":"x","source":{"file":"e.ts","export":"E","discriminant":"tag"},"cases":[{"tag":"A","expect":"rejects","test":{"file":"case.test.ts","name":"rejects A"}}]})).unwrap();
        let config = ValidatedConfig::try_from(Config {
            schema_version: 1,
            project: "tsconfig.json".into(),
            contracts: vec![c],
        })
        .unwrap();
        let (result, _) = contract::compare(dir.path(), &config.contracts()[0], vec!["A".into()]);
        let mut report = Report::new("verify");
        report.contracts.push(result);
        let evidence=serde_json::from_value(json!({"success":true,"testResults":[{"name":dir.path().join("case.test.ts"),"assertionResults":states.iter().map(|s|json!({"fullName":"rejects A","status":s})).collect::<Vec<_>>()}]})).unwrap();
        vitest::apply(&mut report, dir.path(), evidence, true);
        assert_eq!(report.contracts[0].cases[0].test_status, wanted);
        assert_eq!(report.diagnostics.is_empty(), wanted == "passed");
    }
}

#[test]
fn failed_runner_invalidates_individually_passing_tests() {
    let mut report = Report::new("verify");
    let evidence = serde_json::from_value(json!({"success":true,"testResults":[]})).unwrap();
    vitest::apply(&mut report, std::path::Path::new("."), evidence, false);
    assert_eq!(report.diagnostics[0].code, "EC006");
}

#[test]
fn removed_contract_is_visible_and_source_changes_reject_baseline() {
    let previous: Snapshot=serde_json::from_value(json!({"schemaVersion":1,"kind":"error-contract/snapshot","contracts":{"x":{"source":{"file":"e.ts","export":"E","discriminant":"tag"},"tags":["A"]}}})).unwrap();
    let mut current = contract::snapshot(&Report::new("check"));
    assert_eq!(
        contract::diff(&current, &previous)
            .unwrap()
            .removed_contracts,
        vec!["x"]
    );
    current.contracts.insert(
        "x".into(),
        SnapshotContract {
            source: Source {
                file: "other.ts".into(),
                export: "E".into(),
                discriminant: "tag".into(),
            },
            tags: vec!["A".into()],
            scenarios: None,
        },
    );
    assert!(contract::diff(&current, &previous).is_err());
}
