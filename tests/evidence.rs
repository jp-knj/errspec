use errspec::config::ValidatedConfig;
use errspec::{evidence, model::*, snapshot, vitest};
mod support;
use serde_json::json;

#[test]
fn skipped_missing_ambiguous_or_failed_tests_never_count_as_passed() {
    let mut reports = Vec::new();
    for (states, wanted) in [
        (vec!["passed"], "passed"),
        (vec!["pending"], "pending"),
        (vec!["failed"], "failed"),
        (vec!["future-status"], "future-status"),
        (vec!["Passed"], "Passed"),
        (vec![], "not_found"),
        (vec!["passed", "passed"], "ambiguous"),
    ] {
        let c: Contract=serde_json::from_value(json!({"id":"x","source":{"file":"e.ts","export":"E","discriminant":"tag"},"cases":[{"tag":"A","expect":"rejects","test":{"file":"case.test.ts","name":"rejects A"}}]})).unwrap();
        let config = ValidatedConfig::try_from(Config {
            schema_version: 1,
            project: "tsconfig.json".into(),
            contracts: vec![c],
        })
        .unwrap();
        let mut data = support::register(&config, &["A"]);
        let raw = serde_json::from_value(json!({"success":true,"testResults":[{"name":"case.test.ts","assertionResults":states.iter().map(|s|json!({"fullName":"rejects A","status":s})).collect::<Vec<_>>()}]})).unwrap();
        let (tests, findings) = evidence::judge(
            &data.registrations,
            &support::paths(&["case.test.ts"]),
            vitest::normalize(raw),
            true,
        );
        if wanted == "ambiguous" {
            insta::assert_debug_snapshot!("ambiguous_test_judgment", tests);
        }
        data.tests = Some(tests);
        data.diagnostics.extend(findings);
        let report = data.finish("verify");
        assert_eq!(report.contracts[0].cases[0].test_status, wanted);
        assert_eq!(report.diagnostics.is_empty(), wanted == "passed");
        reports.push(report);
    }
    insta::assert_json_snapshot!("test_matching_statuses", reports);
}

#[test]
fn failed_runner_invalidates_individually_passing_tests() {
    let raw = serde_json::from_value(json!({"success":true,"testResults":[]})).unwrap();
    let (_, findings) = evidence::judge(&[], &Default::default(), vitest::normalize(raw), false);
    assert_eq!(findings[0].code, "EC006");
}

#[test]
fn removed_contract_is_visible_and_source_changes_reject_baseline() {
    let previous: Snapshot=serde_json::from_value(json!({"schemaVersion":1,"kind":"error-contract/snapshot","contracts":{"x":{"source":{"file":"e.ts","export":"E","discriminant":"tag"},"tags":["A"]}}})).unwrap();
    let mut current = snapshot::capture(&[]);
    assert_eq!(
        snapshot::diff(&current, &previous)
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
    assert!(snapshot::diff(&current, &previous).is_err());
}
