use errspec::{
    config::ValidatedConfig,
    evidence::{self, RunEvidence, TestEvidence},
    model::*,
    observation::{self, RunContext},
    observation_io, snapshot,
};
use serde_json::{Value, json};
use std::fs;
mod support;

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
fn check(values: Vec<Value>) -> Report {
    check_with(config(), values)
}
fn check_with(config: Value, values: Vec<Value>) -> Report {
    check_runner(config, &[&scenario_test_name("checkout", "first")], values)
}
fn check_names(runner_names: &[&str], values: Vec<Value>) -> Report {
    check_runner(config(), runner_names, values)
}
fn check_runner(config: Value, runner_names: &[&str], values: Vec<Value>) -> Report {
    let mut cfg: Config = serde_json::from_value(config).unwrap();
    cfg.contracts[0].cases.truncate(1);
    let cfg = ValidatedConfig::try_from(cfg).unwrap();
    let mut data = support::register(&cfg, &["A"]);
    let paths = support::paths(&["case.test.ts"]);
    let (tests, findings) = evidence::judge(
        &data.registrations,
        &paths,
        RunEvidence {
            runner: "vitest",
            success: true,
            tests: runner_names
                .iter()
                .map(|name| TestEvidence {
                    file: "case.test.ts".into(),
                    name: (*name).into(),
                    status: "passed".into(),
                })
                .collect(),
        },
        true,
    );
    data.diagnostics.extend(findings);
    let run = RunContext::new("run-1");
    let records = observation::validate(
        values
            .into_iter()
            .map(|v| serde_json::from_value(v).unwrap())
            .collect(),
        &run,
    )
    .unwrap();
    let (observations, findings) =
        observation::judge(&data.registrations, &tests, &paths, &run, &records).unwrap();
    data.tests = Some(tests);
    data.observations = Some(observations);
    data.diagnostics.extend(findings);
    data.finish("verify")
}

#[test]
fn multiple_scenarios_share_a_tag_but_ids_are_unique() {
    let cfg: Config = serde_json::from_value(config()).unwrap();
    let cfg = ValidatedConfig::try_from(cfg).unwrap();
    let data = support::register(&cfg, &["A", "B"]);
    insta::assert_debug_snapshot!("scenario_registrations", data.registrations);
    let report = data.finish("check");
    insta::assert_json_snapshot!("registered_and_missing_scenarios", report);
    let result = &report.contracts[0];
    let gaps = &report.diagnostics;
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
        ("observe", json!({"faults":["a"],"assertions":[]})),
        ("observe", json!({"faults":[""],"assertions":["a"]})),
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
    let mut reports = Vec::new();
    let report = check(vec![record()]);
    assert!(report.diagnostics.is_empty());
    assert_eq!(
        report.contracts[0].cases[0].observation_status,
        ObservationStatus::Passed
    );
    reports.push(json!({"case": "complete", "report": report}));
    let report = check(vec![]);
    assert_eq!(report.diagnostics[0].code, "EC008");
    reports.push(json!({"case": "missing", "report": report}));
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
        changed[field] = value.clone();
        let report = check(vec![changed]);
        assert!(
            report.diagnostics.iter().any(|f| f.code == code),
            "{report:?}"
        );
        if let Some(state) = value.as_str().filter(|_| field == "state") {
            assert!(
                report
                    .diagnostics
                    .iter()
                    .any(|f| f.message.ends_with(&format!(": {state}"))),
                "{report:?}"
            );
        }
        assert_ne!(
            report.contracts[0].cases[0].observation_status,
            ObservationStatus::Passed
        );
        reports.push(json!({"field": field, "value": value, "report": report}));
    }
    insta::assert_json_snapshot!("scenario_observation_judgments", reports);
}

#[test]
fn explicitly_fault_free_scenarios_pass_only_without_fault_observations() {
    let mut cfg = config();
    cfg["contracts"][0]["cases"][0]["observe"]["faults"] = json!([]);
    let mut input_only = record();
    input_only["faults"] = json!([]);
    let report = check_with(cfg.clone(), vec![input_only.clone()]);
    assert!(report.diagnostics.is_empty(), "{report:?}");
    assert_eq!(
        report.contracts[0].cases[0].observation_status,
        ObservationStatus::Passed
    );
    // A fault that the configuration did not require is still undeclared evidence.
    let report = check_with(cfg.clone(), vec![record()]);
    assert!(report.diagnostics.iter().any(|f| f.code == "EC011"));
    // Required comparisons are not relaxed with the faults.
    input_only["assertions"] = json!([{"id":"status","status":"passed"}]);
    let report = check_with(cfg, vec![input_only]);
    assert!(report.diagnostics.iter().any(|f| f.code == "EC010"));
}

#[test]
fn a_description_after_the_stable_name_matches_only_its_own_scenario() {
    let base = scenario_test_name("checkout", "first");
    let described = format!("{base} [異常系] 書き込まない");
    let mut observed = record();
    observed["testName"] = json!(described);
    let report = check_names(&[&described], vec![observed.clone()]);
    assert!(report.diagnostics.is_empty(), "{report:?}");
    assert_eq!(
        report.contracts[0].cases[0].observation_status,
        ObservationStatus::Passed
    );
    // The observation must name the same runner test, not just the stable prefix.
    let report = check_names(&[&described], vec![record()]);
    assert!(report.diagnostics.iter().any(|f| f.code == "EC011"));
    let report = check_names(&[&base, &described], vec![observed]);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|f| f.code == "EC005" && f.message.contains("ambiguous"))
    );
    for unrelated in [format!("{base}x"), format!("{base}\u{3000}説明")] {
        let report = check_names(&[&unrelated], vec![record()]);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|f| f.code == "EC005" && f.message.contains("not_found")),
            "{unrelated}"
        );
    }
}

fn passes(report: &Report) -> bool {
    report.diagnostics.is_empty()
        && report.contracts[0].cases[0].observation_status == ObservationStatus::Passed
}

fn has(report: &Report, code: &str, message: &str) -> bool {
    report
        .diagnostics
        .iter()
        .any(|f| f.code == code && f.message.contains(message))
}

#[test]
fn describe_titles_before_the_stable_name_match_the_scenario() {
    let base = scenario_test_name("checkout", "first");
    let described = format!("{base} [異常系] 書き込まない");
    let mut plain = record();
    plain["testName"] = json!(base);
    let mut with_description = record();
    with_description["testName"] = json!(described);
    for (runner_name, observed) in [
        (format!("submit {base}"), &plain),
        (format!("outer inner group {base}"), &plain),
        (format!("submitFormEbook {described}"), &with_description),
        (format!("日本語 の グループ {described}"), &with_description),
        // A describe title may itself contain brackets and quotes.
        (format!("[\"x\"] {base}"), &plain),
    ] {
        let report = check_names(&[&runner_name], vec![observed.clone()]);
        assert!(passes(&report), "{runner_name}: {report:?}");
    }
}

#[test]
fn the_observation_names_the_sdk_part_without_describe_titles() {
    let base = scenario_test_name("checkout", "first");
    let runner_name = format!("group {base}");
    // The SDK cannot know describe titles, so recording them would be a mismatch.
    let mut prefixed = record();
    prefixed["testName"] = json!(runner_name);
    let report = check_names(&[&runner_name], vec![prefixed]);
    assert!(has(&report, "EC011", "test name"), "{report:?}");
    // A description in the runner name must also appear in the observation.
    let report = check_names(&[&format!("group {base} 説明")], vec![record()]);
    assert!(has(&report, "EC011", "test name"), "{report:?}");
}

#[test]
fn the_stable_name_must_be_a_whole_space_delimited_part() {
    let base = scenario_test_name("checkout", "first");
    for runner_name in [
        format!("group{base}"),
        format!("group {base}x"),
        format!("group\u{3000}{base}"),
        format!("group {}", scenario_test_name("checkout", "first-retry")),
        format!("group {}", scenario_test_name("checkout-v2", "first")),
    ] {
        let report = check_names(&[&runner_name], vec![record()]);
        assert!(
            has(&report, "EC005", "not_found"),
            "{runner_name}: {report:?}"
        );
    }
}

#[test]
fn the_same_scenario_in_two_describes_is_ambiguous() {
    let base = scenario_test_name("checkout", "first");
    let (a, b) = (format!("a {base}"), format!("b {base}"));
    let report = check_names(&[&a, &b], vec![record()]);
    assert!(has(&report, "EC005", "ambiguous"), "{report:?}");
    let report = check_names(&[&base, &a], vec![record()]);
    assert!(has(&report, "EC005", "ambiguous"), "{report:?}");
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
        changed[field] = value.clone();
        let report = check(vec![changed]);
        assert!(report.diagnostics.iter().any(|f| f.code == "EC011"));
        assert_ne!(
            report.contracts[0].cases[0].observation_status,
            ObservationStatus::Passed
        );
    }
    let report = check(vec![record(), record()]);
    assert_eq!(
        report.contracts[0].cases[0].observation_status,
        ObservationStatus::Invalid
    );
    insta::assert_json_snapshot!("duplicate_scenario_execution", report);
}

#[test]
fn observation_reader_rejects_stale_corrupt_partial_and_invalid_records() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("one.json");
    fs::write(&path, record().to_string()).unwrap();
    assert_eq!(
        observation_io::read(dir.path(), &RunContext::new("run-1"))
            .unwrap()
            .records()
            .len(),
        1
    );
    assert!(observation_io::read(dir.path(), &RunContext::new("old-run")).is_err());
    fs::write(&path, "{").unwrap();
    assert!(observation_io::read(dir.path(), &RunContext::new("run-1")).is_err());
    fs::remove_file(&path).unwrap();
    fs::write(dir.path().join("one.json.tmp"), "{}").unwrap();
    assert!(observation_io::read(dir.path(), &RunContext::new("run-1")).is_err());
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
    let before = snapshot::capture(&support::register(&cfg, &["A"]).registrations);
    let mut changed = config();
    changed["contracts"][0]["cases"]
        .as_array_mut()
        .unwrap()
        .pop();
    changed["contracts"][0]["cases"][0]["observe"]["assertions"]
        .as_array_mut()
        .unwrap()
        .pop();
    let cfg =
        ValidatedConfig::try_from(serde_json::from_value::<Config>(changed).unwrap()).unwrap();
    let after = snapshot::capture(&support::register(&cfg, &["A"]).registrations);
    let delta = snapshot::diff(&after, &before).unwrap();
    assert!(delta.added.is_empty() && delta.removed.is_empty());
    assert_eq!(delta.removed_scenarios["checkout"], ["retry"]);
    assert_eq!(delta.changed_scenarios["checkout"], ["first"]);
    insta::assert_json_snapshot!(
        "scenario_history_delta",
        json!({"before": before, "after": after, "delta": delta})
    );
}
