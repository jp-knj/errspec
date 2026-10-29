//! Exercise the filesystem/process boundary through the real CLI.
#![cfg(unix)]
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    process::Command,
};
use tempfile::TempDir;

fn fixture(test_file: &str, exists: bool) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("errors.ts"), "export type E = {tag:'A'};").unwrap();
    fs::write(
        dir.path().join("tsconfig.json"),
        r#"{"compilerOptions":{"strict":true,"skipLibCheck":true},"files":["errors.ts"]}"#,
    )
    .unwrap();
    if exists {
        fs::write(dir.path().join("case.test.ts"), "export {};").unwrap();
    }
    fs::write(
        dir.path().join("errspec.json"),
        json!({"schemaVersion":2,"project":"tsconfig.json","contracts":[{
            "id":"checkout","source":{"file":"errors.ts","export":"E","discriminant":"tag"},
            "cases":[{"id":"first","tag":"A","expect":"reject", "test":{"file":test_file},
            "observe":{"faults":["gateway"],"assertions":["status"]}}]
        }]})
        .to_string(),
    )
    .unwrap();
    dir
}

fn verify(
    dir: &TempDir,
    status: &str,
    mutation: &str,
    corrupt: bool,
    process_code: u8,
) -> (i32, Value) {
    fs::write(dir.path().join("runner-report.json"), json!({"success":true,"testResults":[{
        "name":"case.test.ts","assertionResults":[{"fullName":"errspec [\"checkout\",\"first\"]","status":status}]
    }]}).to_string()).unwrap();
    let observation = if corrupt {
        "printf '{' > \"$ERRSPEC_OBSERVATIONS/one.json\"".to_owned()
    } else {
        r#"printf '{"schemaVersion":1,"kind":"errspec/observation","runId":"%s","contractId":"checkout","scenarioId":"first","tag":"A","file":"case.test.ts","testName":"errspec [\"checkout\",\"first\"]","state":"passed","faults":[{"id":"gateway","calls":1,"completed":1}],"assertions":[{"id":"status","status":"passed"}]}' "$ERRSPEC_RUN_ID" > "$ERRSPEC_OBSERVATIONS/one.json""#.to_owned()
    };
    let runner = dir.path().join("runner.sh");
    fs::write(&runner, format!("#!/bin/sh\nset -eu\ncat runner-report.json > \"$4\"\n{observation}\n{mutation}\nexit {process_code}\n")).unwrap();
    fs::set_permissions(&runner, fs::Permissions::from_mode(0o755)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_errspec"))
        .current_dir(dir.path())
        .args(["verify", "--vitest", "./runner.sh", "--format", "json"])
        .output()
        .unwrap();
    let report = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&output.stderr)));
    (output.status.code().unwrap(), report)
}

fn codes(report: &Value) -> Vec<&str> {
    report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap())
        .collect()
}

#[test]
fn test_file_created_during_execution_does_not_erase_registration_gap() {
    let dir = fixture("case.test.ts", false);
    let (exit, report) = verify(&dir, "passed", ": > case.test.ts", false, 0);
    assert_eq!(exit, 1, "{report}");
    assert_eq!(codes(&report), ["EC003"]);
    assert_eq!(report["contracts"][0]["cases"][0]["testStatus"], "passed");
    assert_eq!(
        report["contracts"][0]["cases"][0]["observationStatus"],
        "passed"
    );
}

#[test]
fn deleted_file_cannot_match_even_when_both_evidence_paths_are_identical() {
    let dir = fixture("case.test.ts", true);
    let (exit, report) = verify(&dir, "passed", "rm case.test.ts", false, 0);
    assert_eq!(exit, 1, "{report}");
    assert_eq!(codes(&report), ["EC005", "EC011"]);
    assert_eq!(report["analysisComplete"], true);
    assert_eq!(
        report["contracts"][0]["cases"][0]["testStatus"],
        "not_found"
    );
    assert_eq!(
        report["contracts"][0]["cases"][0]["observationStatus"],
        "invalid"
    );
}

#[test]
fn symlink_registration_matches_runner_and_observation_real_paths() {
    let dir = fixture("alias.test.ts", true);
    symlink("case.test.ts", dir.path().join("alias.test.ts")).unwrap();
    let (exit, report) = verify(&dir, "passed", ":", false, 0);
    assert_eq!(exit, 0, "{report}");
    assert!(codes(&report).is_empty());
    assert_eq!(
        report["contracts"][0]["cases"][0]["test"]["file"],
        "alias.test.ts"
    );
    assert_eq!(
        report["contracts"][0]["cases"][0]["observationStatus"],
        "passed"
    );
}

#[test]
fn observation_read_failure_keeps_runner_results_and_diagnostic_order() {
    let dir = fixture("case.test.ts", false);
    let (exit, report) = verify(&dir, "future-status", ": > case.test.ts", true, 1);
    assert_eq!(exit, 2, "{report}");
    assert_eq!(codes(&report), ["EC003", "EC005", "EC006", "EC_EVIDENCE"]);
    assert_eq!(report["testsExecuted"], true);
    assert_eq!(report["testRunner"], "vitest");
    assert_eq!(report["analysisComplete"], false);
    let case = &report["contracts"][0]["cases"][0];
    assert_eq!(case["testStatus"], "future-status");
    assert_eq!(case["observationStatus"], "not_run");
    assert!(case.get("observedFaults").is_none());
    assert!(case.get("observedAssertions").is_none());
}

#[test]
fn malformed_runner_report_stops_before_observation_judgment() {
    let dir = fixture("case.test.ts", true);
    let (exit, report) = verify(&dir, "passed", "printf '{' > \"$4\"", false, 0);
    assert_eq!(exit, 2, "{report}");
    assert_eq!(codes(&report), ["EC_RUNNER"]);
    assert_eq!(report["testsExecuted"], false);
    assert!(report.get("testRunner").is_none());
    assert_eq!(report["contracts"][0]["cases"][0]["testStatus"], "not_run");
    assert_eq!(
        report["contracts"][0]["cases"][0]["observationStatus"],
        "not_run"
    );
}
