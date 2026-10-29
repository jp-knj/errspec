use errspec::{
    config::{CaseKind, ValidatedConfig},
    contract,
    model::{Config, ObservationStatus, Report, scenario_test_name},
    observation::{self, RunContext},
};
use serde_json::{Value, json};
use std::fs;

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/observation-v1.json")).unwrap()
}

fn config(version: u32) -> Config {
    let record = fixture();
    let mut case =
        json!({"tag":"Offline", "expect":"reject after retry", "test":{"file":"scenario.test.ts"}});
    if version == 1 {
        case["test"]["name"] = json!("legacy test");
    } else {
        case["id"] = record["scenarioId"].clone();
        case["observe"] = json!({"faults":["gateway"],"assertions":["status"]});
    }
    serde_json::from_value(json!({
        "schemaVersion": version, "project":"tsconfig.json", "contracts":[{
            "id":record["contractId"],
            "source":{"file":"errors.ts", "export":"Failure", "discriminant":"_tag"},
            "cases":[case]
        }]
    }))
    .unwrap()
}

#[test]
fn validated_cases_preserve_legacy_names_and_require_scenario_fields() {
    let legacy = ValidatedConfig::try_from(config(1)).unwrap();
    let case = &legacy.contracts()[0].cases()[0];
    assert!(matches!(case.kind(), CaseKind::Legacy));
    assert_eq!(case.test().name, "legacy test");

    let scenario = ValidatedConfig::try_from(config(2)).unwrap();
    let case = &scenario.contracts()[0].cases()[0];
    let CaseKind::Scenario { id, observe } = case.kind() else {
        panic!("scenario expected")
    };
    assert_eq!(id, "retry/一");
    assert_eq!(observe.faults, ["gateway"]);
    assert_eq!(observe.assertions, ["status"]);
    assert_eq!(case.test().name, fixture()["testName"]);

    // An explicitly supplied SDK name and an omitted name normalize identically.
    let mut explicit = config(2);
    explicit.contracts[0].cases[0].test.name = case.test().name.clone();
    let explicit = ValidatedConfig::try_from(explicit).unwrap();
    assert_eq!(explicit.contracts()[0].cases()[0].test(), case.test());
}

#[test]
fn shared_sdk_fixture_passes_only_in_the_execution_that_validated_it() {
    let run = RunContext::new("fixture-run");
    let records =
        observation::validate(vec![serde_json::from_value(fixture()).unwrap()], &run).unwrap();
    let record = &records.records()[0];
    assert_eq!(
        record.test_name,
        scenario_test_name(&record.contract_id, &record.scenario_id)
    );
    assert_eq!((record.faults[0].calls, record.faults[0].completed), (2, 2));
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("scenario.test.ts"), "").unwrap();
    let cfg = ValidatedConfig::try_from(config(2)).unwrap();
    let (mut result, findings) =
        contract::compare(dir.path(), &cfg.contracts()[0], vec!["Offline".into()]);
    assert!(findings.is_empty());
    result.cases[0].test_status = "passed".into();
    let mut report = Report::new("verify");
    report.contracts.push(result);
    let before = serde_json::to_value(&report).unwrap();
    let other_run = RunContext::new("another-run");
    let error = observation::apply(&mut report, dir.path(), &other_run, &records).unwrap_err();
    assert_eq!(error.code, "EC_EVIDENCE");
    assert_eq!(serde_json::to_value(&report).unwrap(), before);
    observation::apply(&mut report, dir.path(), &run, &records).unwrap();
    assert!(report.diagnostics.is_empty());
    assert_eq!(
        report.contracts[0].cases[0].observation_status,
        ObservationStatus::Passed
    );
}

#[test]
fn in_memory_boundary_rejects_stale_and_malformed_observations() {
    let run = RunContext::new("fixture-run");
    for (field, value) in [
        ("runId", json!("old-run")),
        ("schemaVersion", json!(2)),
        ("kind", json!("other")),
        ("scenarioId", json!(" ")),
        (
            "faults",
            json!([{"id":"gateway", "calls":1, "completed":2}]),
        ),
        (
            "assertions",
            json!([{"id":"status", "status":"passed"},{"id":"status", "status":"passed"}]),
        ),
    ] {
        let mut raw = fixture();
        raw[field] = value;
        let error =
            observation::validate(vec![serde_json::from_value(raw).unwrap()], &run).unwrap_err();
        assert_eq!(error.code, "EC_EVIDENCE", "{field}");
    }
    // Even an empty, previously validated batch cannot cross execution boundaries.
    let empty = observation::validate(vec![], &run).unwrap();
    assert!(
        observation::apply(
            &mut Report::new("verify"),
            std::path::Path::new("."),
            &RunContext::new("other"),
            &empty
        )
        .is_err()
    );
}
