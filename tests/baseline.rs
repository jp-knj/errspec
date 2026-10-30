use serde_json::{Value, json};
use std::{fs, process::Command};
use tempfile::TempDir;

fn run(dir: &TempDir, args: &[&str]) -> (i32, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_errspec"))
        .current_dir(dir.path())
        .args(args)
        .args(["--format", "json"])
        .output()
        .unwrap();
    let report = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&output.stderr)));
    (output.status.code().unwrap(), report)
}

fn save(dir: &TempDir, config: &Value) {
    fs::write(dir.path().join("errspec.json"), config.to_string()).unwrap();
}

fn fixture() -> (TempDir, Value) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("errors.ts"), "export type E = {tag: 'A'};").unwrap();
    fs::write(dir.path().join("case.test.ts"), "export {};").unwrap();
    fs::write(dir.path().join("other.test.ts"), "export {};").unwrap();
    fs::write(
        dir.path().join("tsconfig.json"),
        r#"{"compilerOptions":{"strict":true,"skipLibCheck":true,"target":"ES2022"},"files":["errors.ts"]}"#,
    )
    .unwrap();
    let config = json!({"schemaVersion":2,"project":"tsconfig.json","contracts":[{
        "id":"checkout","source":{"file":"errors.ts","export":"E","discriminant":"tag"},
        "cases":[{"id":"first","tag":"A","expect":"reject without writing",
            "test":{"file":"case.test.ts"},"observe":{"faults":["gateway"],"assertions":["status","orders"]}}]
    }]});
    save(&dir, &config);
    assert_eq!(run(&dir, &["snapshot", "--out", "baseline.json"]).0, 0);
    (dir, config)
}

const STRICT: &[&str] = &["check", "--baseline", "baseline.json", "--strict-baseline"];

#[test]
fn changed_scenarios_fail_only_when_strict_baseline_is_enabled() {
    let mut reports = Vec::new();
    let (dir, original) = fixture();
    for (field, replacement) in [
        ("expect", json!("return success")),
        (
            "observe",
            json!({"faults":["gateway"],"assertions":["status"]}),
        ),
        (
            "observe",
            json!({"faults":["different"],"assertions":["status","orders"]}),
        ),
        (
            "observe",
            json!({"faults":["gateway"],"assertions":["orders","status"]}),
        ),
        ("test", json!({"file":"other.test.ts"})),
    ] {
        let mut config = original.clone();
        config["contracts"][0]["cases"][0][field] = replacement;
        save(&dir, &config);
        let (status, normal) = run(&dir, &["check", "--baseline", "baseline.json"]);
        assert_eq!(status, 0, "{normal}");
        let (status, strict) = run(&dir, STRICT);
        assert_eq!(status, 1, "{strict}");
        assert_eq!(strict["analysisComplete"], true);
        assert_eq!(strict["testsExecuted"], false);
        assert_eq!(strict["delta"], normal["delta"]);
        assert_eq!(
            strict["delta"]["changedScenarios"]["checkout"],
            json!(["first"])
        );
        assert_eq!(strict["diagnostics"][0]["code"], "EC014");
        assert_eq!(strict["diagnostics"][0]["contractId"], "checkout");
        assert_eq!(strict["diagnostics"][0]["scenarioId"], "first");
        reports.push(json!({"field": field, "normal": normal, "strict": strict}));
    }
    insta::assert_json_snapshot!("strict_scenario_changes", reports);
    let original_baseline = fs::read(dir.path().join("baseline.json")).unwrap();
    assert_eq!(run(&dir, &["snapshot", "--out", "reviewed.json"]).0, 0);
    assert_eq!(
        run(
            &dir,
            &["check", "--baseline", "reviewed.json", "--strict-baseline"]
        )
        .0,
        0
    );
    assert_eq!(
        fs::read(dir.path().join("baseline.json")).unwrap(),
        original_baseline
    );
}

#[test]
fn unchanged_and_added_scenarios_pass_but_removals_still_fail() {
    let (dir, mut config) = fixture();
    assert_eq!(run(&dir, STRICT).0, 0);
    let mut added = config["contracts"][0]["cases"][0].clone();
    added["id"] = json!("retry");
    config["contracts"][0]["cases"]
        .as_array_mut()
        .unwrap()
        .push(added);
    save(&dir, &config);
    let (status, report) = run(&dir, STRICT);
    assert_eq!(status, 0, "{report}");
    assert_eq!(
        report["delta"]["addedScenarios"]["checkout"],
        json!(["retry"])
    );
    config["contracts"][0]["cases"]
        .as_array_mut()
        .unwrap()
        .remove(0);
    save(&dir, &config);
    let (status, report) = run(&dir, STRICT);
    assert_eq!(status, 1);
    assert_eq!(report["diagnostics"][0]["code"], "EC013");
    insta::assert_json_snapshot!("scenario_added_and_removed", report);
}

#[test]
fn strict_baseline_requires_version_two_history_for_every_contract() {
    let (dir, _) = fixture();
    let path = dir.path().join("baseline.json");
    let original: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for version in [1, 2] {
        let mut old = original.clone();
        old["schemaVersion"] = json!(version);
        old["contracts"]["checkout"]
            .as_object_mut()
            .unwrap()
            .remove("scenarios");
        fs::write(&path, old.to_string()).unwrap();
        assert_eq!(run(&dir, &["check", "--baseline", "baseline.json"]).0, 0);
        let (status, report) = run(&dir, STRICT);
        assert_eq!(status, 2, "{report}");
        assert_eq!(report["diagnostics"][0]["code"], "EC_BASELINE");
    }
    let mut mixed = original.clone();
    mixed["contracts"]["other"] = original["contracts"]["checkout"].clone();
    mixed["contracts"]["other"]
        .as_object_mut()
        .unwrap()
        .remove("scenarios");
    fs::write(&path, mixed.to_string()).unwrap();
    let (status, report) = run(&dir, STRICT);
    assert_eq!(status, 2, "{report}");
    assert_eq!(report["diagnostics"][0]["code"], "EC_BASELINE");
}

#[test]
fn strict_baseline_rejects_legacy_config_and_snapshot_writes() {
    let (dir, mut config) = fixture();
    let (status, report) = run(
        &dir,
        &[
            "snapshot",
            "--out",
            "new.json",
            "--baseline",
            "baseline.json",
            "--strict-baseline",
        ],
    );
    assert_eq!(status, 2, "{report}");
    assert_eq!(report["diagnostics"][0]["code"], "EC_CONFIG");
    assert!(!dir.path().join("new.json").exists());
    config["schemaVersion"] = json!(1);
    let case = config["contracts"][0]["cases"][0].as_object_mut().unwrap();
    case.remove("id");
    case.remove("observe");
    case["test"]["name"] = json!("legacy");
    save(&dir, &config);
    let (status, report) = run(&dir, STRICT);
    assert_eq!(status, 2, "{report}");
    assert_eq!(report["diagnostics"][0]["code"], "EC_CONFIG");
}

#[test]
fn strict_baseline_requires_an_explicit_comparison_file() {
    let (dir, _) = fixture();
    let output = Command::new(env!("CARGO_BIN_EXE_errspec"))
        .current_dir(dir.path())
        .args(["check", "--strict-baseline"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--baseline"));
}
