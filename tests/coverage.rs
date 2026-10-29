use errspec::{
    coverage::{AssertionRecord, TagStatus, judge},
    discovery::Candidate,
    model::Source,
};
use serde_json::{Value, json};
use std::{fs, os::unix::fs::PermissionsExt, process::Command};
use tempfile::TempDir;

fn candidate(file: &str, export: &str, tags: &[&str]) -> Candidate {
    Candidate {
        source: Source {
            file: file.into(),
            export: export.into(),
            discriminant: "_tag".into(),
        },
        tags: tags.iter().map(|t| (*t).into()).collect(),
    }
}

fn record(values: &[(&str, &str)]) -> AssertionRecord {
    AssertionRecord {
        file: "a.test.ts".into(),
        test: Some("t".into()),
        values: values
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect(),
    }
}

#[test]
fn tags_are_asserted_mentioned_or_missing() {
    let candidates = [
        candidate("errors.ts", "Failure", &["A", "B", "C", "D"]),
        // The same declaration re-exported elsewhere is judged once.
        candidate("index.ts", "Failure", &["A", "B", "C", "D"]),
        candidate("store.ts", "StoreFailure", &["A"]),
    ];
    let records = [
        record(&[("_tag", "A")]),
        record(&[("", "D")]),
        record(&[("kind", "B")]),
    ];
    let sources = ["mock(() => ({ _tag: 'B' }))".to_owned()];
    let (types, summary) = judge(&candidates, &records, &sources);
    assert_eq!(types.len(), 2);
    let status: Vec<_> = types[0]
        .tags
        .iter()
        .map(|t| (t.tag.as_str(), t.status))
        .collect();
    assert_eq!(
        status,
        [
            ("A", TagStatus::Asserted),
            // Another discriminant carrying "B" is not evidence for _tag.
            ("B", TagStatus::Mentioned),
            ("C", TagStatus::Missing),
            ("D", TagStatus::Asserted),
        ]
    );
    assert_eq!(types[0].tags[0].tests, ["a.test.ts > t"]);
    // "A" appears in two types but counts once.
    assert_eq!(
        (
            summary.types,
            summary.tags,
            summary.asserted,
            summary.mentioned,
            summary.missing
        ),
        (2, 4, 2, 1, 1)
    );
}

fn fixture() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("errors.ts"),
        "export type Failure = {_tag:'A'} | {_tag:'B'} | {_tag:'C'};",
    )
    .unwrap();
    fs::write(
        dir.path().join("tsconfig.json"),
        r#"{"compilerOptions":{"strict":true,"skipLibCheck":true},"files":["errors.ts"]}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("case.test.ts"),
        "const injected = { _tag: 'B' };",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("node_modules/vitest")).unwrap();
    fs::write(dir.path().join("node_modules/vitest/package.json"), "{}").unwrap();
    // Arguments: run --config <wrapper> --reporter=json --outputFile <report> [extra...]
    let report = json!({"success": true, "testResults": [{"name": "case.test.ts", "assertionResults": [{"fullName": "t", "status": "passed"}]}]});
    let runner = dir.path().join("runner.sh");
    fs::write(
        &runner,
        format!(
            "#!/bin/sh\nset -eu\ngrep -q setupFiles \"$3\"\necho \"$@\" > args.txt\nprintf '%s' '{report}' > \"$6\"\nprintf '%s\\n' '{{\"file\":\"case.test.ts\",\"test\":\"t\",\"values\":[[\"_tag\",\"A\"]]}}' > \"$ERRSPEC_COVERAGE/one.jsonl\"\n"
        ),
    )
    .unwrap();
    fs::set_permissions(&runner, fs::Permissions::from_mode(0o755)).unwrap();
    dir
}

fn coverage(dir: &TempDir, extra: &[&str]) -> (i32, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_errspec"))
        .current_dir(dir.path())
        .args(["coverage", "--vitest", "./runner.sh", "--format", "json"])
        .args(extra)
        .output()
        .unwrap();
    let report = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&output.stderr)));
    (output.status.code().unwrap(), report)
}

#[test]
fn coverage_runs_vitest_with_the_recorder_and_judges_scanned_tags() {
    let dir = fixture();
    let (status, report) = coverage(&dir, &["--", "--project", "backend"]);
    assert_eq!(status, 0, "{report}");
    assert_eq!(report["kind"], "errspec/coverage");
    assert_eq!(report["runnerOk"], true);
    assert_eq!(
        report["summary"],
        json!({"types": 1, "tags": 3, "asserted": 1, "mentioned": 1, "missing": 1})
    );
    let args = fs::read_to_string(dir.path().join("args.txt")).unwrap();
    assert!(args.trim_end().ends_with("--project backend"), "{args}");
    let (status, _) = coverage(&dir, &["--fail-on-missing"]);
    assert_eq!(status, 1);
}

#[test]
fn coverage_without_vitest_reports_where_it_looked() {
    let dir = fixture();
    fs::remove_dir_all(dir.path().join("node_modules")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_errspec"))
        .current_dir(dir.path())
        .args(["coverage", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["testsExecuted"], false);
    let message = report["diagnostics"][0]["message"].as_str().unwrap();
    assert!(message.contains("pass --vitest PATH"), "{message}");
}

#[test]
fn markdown_lists_unasserted_tags_per_type() {
    let dir = fixture();
    let output = Command::new(env!("CARGO_BIN_EXE_errspec"))
        .current_dir(dir.path())
        .args([
            "coverage",
            "--vitest",
            "./runner.sh",
            "--format",
            "markdown",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        concat!(
            "**errspec coverage**: 1 error types, 3 tags: 1 asserted, 1 mentioned only, 1 missing\n",
            "\n",
            "### `errors.ts` Failure (`_tag`): 1/3 asserted\n",
            "\n",
            "| Tag | Status | Note |\n",
            "|---|---|---|\n",
            "| `B` | mentioned | Written in a test, but no assertion received it |\n",
            "| `C` | missing | No test refers to it |\n",
        )
    );
}

#[test]
fn markdown_is_rejected_outside_coverage() {
    let dir = fixture();
    let output = Command::new(env!("CARGO_BIN_EXE_errspec"))
        .current_dir(dir.path())
        .args(["scan", "--format", "markdown"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("supported only by coverage"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
