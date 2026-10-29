use errspec::{
    config::ValidatedConfig,
    contract,
    model::*,
    observation::{self, RunContext},
};
use serde_json::{Value, json};
use std::fs;

fn config() -> Value {
    json!({"schemaVersion":2,"project":"tsconfig.json","contracts":[{
        "id":"checkout", "source":{"file":"e.ts","export":"E","discriminant":"tag"},
        "cases":[{"id":"first","tag":"A","expect":"reject without writing",
          "test":{"file":"case.test.ts"},"observe":{"faults":["gateway"],"assertions":["status","writes"]}},
          {"id":"retry","tag":"A","expect":"stop after retry",
          "test":{"file":"case.test.ts"},"observe":{"faults":["gateway"],"assertions":["attempts"]}}]
    }]})
}
fn record() -> Value {
    json!({"schemaVersion":1,"kind":"errspec/observation","runId":"run-1",
      "contractId":"checkout","scenarioId":"first","tag":"A","file":"case.test.ts",
      "testName":scenario_test_name("checkout","first"),"state":"passed",
      "faults":[{"id":"gateway","calls":1,"completed":1}],
      "assertions":[{"id":"status","status":"passed"},{"id":"writes","status":"passed"}]})
}
fn check(mut values: Vec<Value>) -> (Report, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("case.test.ts"), "").unwrap();
    let mut cfg: Config = serde_json::from_value(config()).unwrap();
    cfg.contracts[0].cases.truncate(1);
    let cfg = ValidatedConfig::try_from(cfg).unwrap();
    let (mut result, findings) =
        contract::compare(dir.path(), &cfg.contracts()[0], vec!["A".into()]);
    assert!(findings.is_empty());
    result.cases[0].test_status = "passed".into();
    let mut report = Report::new("verify");
    report.contracts.push(result);
    let records: Vec<_> = values
        .drain(..)
        .map(|v| serde_json::from_value(v).unwrap())
        .collect();
    let run = RunContext::new("run-1");
    let records = observation::validate(records, &run).unwrap();
    observation::apply(&mut report, dir.path(), &run, &records).unwrap();
    (report, dir)
}

#[test]
fn multiple_scenarios_share_a_tag_but_ids_are_unique() {
    let cfg: Config = serde_json::from_value(config()).unwrap();
    let cfg = ValidatedConfig::try_from(cfg).unwrap();
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("case.test.ts"), "").unwrap();
    let (result, gaps) = contract::compare(
        dir.path(),
        &cfg.contracts()[0],
        vec!["A".into(), "B".into()],
    );
    assert_eq!(result.cases.len(), 3);
    assert_eq!(gaps.len(), 1);
    assert_eq!(gaps[0].tag.as_deref(), Some("B"));
    assert_eq!(
        result.cases[0].test.as_ref().unwrap().name,
        scenario_test_name("checkout", "first")
    );
    assert_eq!(
        result.cases[0].observation_status,
        ObservationStatus::NotRun
    );
    let mut bad = config();
    bad["contracts"][0]["cases"][1]["id"] = json!("first");
    assert!(ValidatedConfig::try_from(serde_json::from_value::<Config>(bad).unwrap()).is_err());
}

#[test]
fn version_two_cannot_omit_observation_requirements_or_use_old_names() {
    for (key, value) in [
        ("id", json!(null)),
        ("observe", json!(null)),
        ("observe", json!({"faults":[],"assertions":["a"]})),
        ("observe", json!({"faults":["a","a"],"assertions":["a"]})),
        ("test", json!({"file":"case.test.ts","name":"old name"})),
    ] {
        let mut bad = config();
        bad["contracts"][0]["cases"][0][key] = value;
        assert!(
            ValidatedConfig::try_from(serde_json::from_value::<Config>(bad).unwrap()).is_err(),
            "{key}"
        );
    }
    let mut bad = config();
    bad["schemaVersion"] = json!(1);
    assert!(ValidatedConfig::try_from(serde_json::from_value::<Config>(bad).unwrap()).is_err());
}

#[test]
fn passing_test_requires_actual_complete_observations() {
    let (report, _dir) = check(vec![record()]);
    assert!(report.diagnostics.is_empty());
    assert_eq!(
        report.contracts[0].cases[0].observation_status,
        ObservationStatus::Passed
    );
    let (report, _dir) = check(vec![]);
    assert_eq!(report.diagnostics[0].code, "EC008");
    for (field, value, code) in [
        ("faults", json!([]), "EC009"),
        (
            "faults",
            json!([{"id":"gateway","calls":0,"completed":0}]),
            "EC009",
        ),
        (
            "faults",
            json!([{"id":"gateway","calls":2,"completed":1}]),
            "EC009",
        ),
        (
            "assertions",
            json!([{"id":"status","status":"passed"}]),
            "EC010",
        ),
        (
            "assertions",
            json!([{"id":"status","status":"failed"},{"id":"writes","status":"passed"}]),
            "EC010",
        ),
        ("state", json!("running"), "EC012"),
        ("state", json!("failed"), "EC012"),
    ] {
        let mut changed = record();
        changed[field] = value;
        let (report, _dir) = check(vec![changed]);
        assert!(
            report.diagnostics.iter().any(|f| f.code == code),
            "{report:?}"
        );
        assert_ne!(
            report.contracts[0].cases[0].observation_status,
            ObservationStatus::Passed
        );
    }
}

#[test]
fn wrong_identity_extra_ids_and_repeated_execution_are_rejected() {
    for (field, value) in [
        ("tag", json!("B")),
        ("file", json!("missing.ts")),
        ("testName", json!("another test")),
        ("scenarioId", json!("other")),
        (
            "assertions",
            json!([{"id":"status","status":"passed"},{"id":"writes","status":"passed"},{"id":"typo","status":"passed"}]),
        ),
    ] {
        let mut changed = record();
        changed[field] = value;
        let (report, _dir) = check(vec![changed]);
        assert!(report.diagnostics.iter().any(|f| f.code == "EC011"));
        assert_ne!(
            report.contracts[0].cases[0].observation_status,
            ObservationStatus::Passed
        );
    }
    let (report, _dir) = check(vec![record(), record()]);
    assert_eq!(
        report.contracts[0].cases[0].observation_status,
        ObservationStatus::Invalid
    );
}

#[test]
fn observation_reader_rejects_stale_corrupt_partial_and_invalid_records() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("one.json");
    fs::write(&path, record().to_string()).unwrap();
    assert_eq!(
        observation::read(dir.path(), &RunContext::new("run-1"))
            .unwrap()
            .records()
            .len(),
        1
    );
    assert!(observation::read(dir.path(), &RunContext::new("old-run")).is_err());
    fs::write(&path, "{").unwrap();
    assert!(observation::read(dir.path(), &RunContext::new("run-1")).is_err());
    fs::remove_file(&path).unwrap();
    fs::write(dir.path().join("one.json.tmp"), "{}").unwrap();
    assert!(observation::read(dir.path(), &RunContext::new("run-1")).is_err());
    for change in [
        json!([{"id":"gateway","calls":1,"completed":2}]),
        json!([{"id":"gateway","calls":1,"completed":1},{"id":"gateway","calls":1,"completed":1}]),
    ] {
        let mut value = record();
        value["faults"] = change;
        assert!(
            observation::validate(
                vec![serde_json::from_value(value).unwrap()],
                &RunContext::new("run-1")
            )
            .is_err()
        );
    }
}

#[test]
fn baseline_tracks_scenario_removal_and_requirement_changes_with_unchanged_tags() {
    let cfg =
        ValidatedConfig::try_from(serde_json::from_value::<Config>(config()).unwrap()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("case.test.ts"), "").unwrap();
    let (result, _) = contract::compare(dir.path(), &cfg.contracts()[0], vec!["A".into()]);
    let mut report = Report::new("check");
    report.contracts.push(result);
    let before = contract::snapshot(&report);
    report.contracts[0].cases.pop();
    report.contracts[0].cases[0]
        .observe
        .as_mut()
        .unwrap()
        .assertions
        .pop();
    let delta = contract::diff(&contract::snapshot(&report), &before).unwrap();
    assert!(delta.added.is_empty() && delta.removed.is_empty());
    assert_eq!(delta.removed_scenarios["checkout"], ["retry"]);
    assert_eq!(delta.changed_scenarios["checkout"], ["first"]);
}
