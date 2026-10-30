use errspec::{
    coverage::{
        AssertionRecord, CoverageDelta, ErrorType, TagChange, TagCoverage, TagRef, TagStatus,
        TypeCoverage, capture, compare, judge,
    },
    model::Source,
};
use serde_json::{Value, json};
use std::{fs, os::unix::fs::PermissionsExt, process::Command};
use tempfile::TempDir;

fn candidate(file: &str, export: &str, tags: &[&str]) -> ErrorType {
    ErrorType {
        source: Source {
            file: file.into(),
            export: export.into(),
            discriminant: "_tag".into(),
        },
        tags: tags.iter().map(|t| (*t).into()).collect(),
        flows: vec![],
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

#[test]
fn shared_tag_values_are_counted_per_discriminant_in_any_order() {
    let mut code = candidate("code.ts", "CodeFailure", &["Shared"]);
    code.source.discriminant = "code".into();
    let mut kind = candidate("kind.ts", "KindFailure", &["Shared"]);
    kind.source.discriminant = "kind".into();
    for candidates in [[code.clone(), kind.clone()], [kind, code]] {
        let (types, summary) = judge(&candidates, &[record(&[("code", "Shared")])], &[]);
        for t in &types {
            let wanted = if t.source.discriminant == "code" {
                TagStatus::Asserted
            } else {
                TagStatus::Missing
            };
            assert_eq!(t.tags[0].status, wanted, "{t:?}");
        }
        assert_eq!(
            (
                summary.types,
                summary.tags,
                summary.asserted,
                summary.mentioned,
                summary.missing
            ),
            (2, 2, 1, 0, 1)
        );
    }
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

fn types(tags: &[(&str, TagStatus)]) -> Vec<TypeCoverage> {
    vec![TypeCoverage {
        source: candidate("errors.ts", "Failure", &[]).source,
        flows: vec![],
        tags: tags
            .iter()
            .map(|(tag, status)| TagCoverage {
                tag: (*tag).into(),
                status: *status,
                tests: vec![],
            })
            .collect(),
    }]
}

#[test]
fn baseline_fails_only_on_regressions_and_new_missing_tags() {
    use TagStatus::*;
    let before = capture(&types(&[
        ("Kept", Missing),
        ("Weaker", Asserted),
        ("Dropped", Mentioned),
        ("Better", Missing),
    ]));
    let after = types(&[
        ("Kept", Missing),
        ("Weaker", Mentioned),
        ("Dropped", Missing),
        ("Better", Asserted),
        ("NewGap", Missing),
        ("NewMention", Mentioned),
    ]);
    let key = "errors.ts#Failure#_tag".to_owned();
    let change = |tag: &str, from, to| TagChange {
        type_key: key.clone(),
        tag: tag.into(),
        from,
        to,
    };
    assert_eq!(
        compare(&before, &after).unwrap(),
        CoverageDelta {
            regressed: vec![
                change("Weaker", Asserted, Mentioned),
                change("Dropped", Mentioned, Missing)
            ],
            new_missing: vec![TagRef {
                type_key: key.clone(),
                tag: "NewGap".into(),
            }],
            improved: vec![change("Better", Missing, Asserted)],
        }
    );
    // An unchanged gap passes: adoption can start from today's state.
    assert_eq!(
        compare(&before, &types(&[("Kept", Missing)])).unwrap(),
        CoverageDelta::default()
    );
}

#[test]
fn baseline_of_another_kind_is_rejected() {
    let mut other = capture(&types(&[]));
    other.kind = "errspec/snapshot".into();
    assert_eq!(
        compare(&other, &types(&[])).unwrap_err().code,
        "EC_BASELINE"
    );
}

#[test]
fn saved_baseline_ratchets_later_runs() {
    let dir = fixture();
    let (status, _) = coverage(&dir, &["--save-baseline", "coverage-baseline.json"]);
    assert_eq!(status, 0);
    let saved: Value =
        serde_json::from_slice(&fs::read(dir.path().join("coverage-baseline.json")).unwrap())
            .unwrap();
    assert_eq!(
        saved["types"]["errors.ts#Failure#_tag"],
        json!({"A": "asserted", "B": "mentioned", "C": "missing"})
    );
    // Existing files are never overwritten.
    let (status, report) = coverage(&dir, &["--save-baseline", "coverage-baseline.json"]);
    assert_eq!(status, 2, "{report}");
    let (status, report) = coverage(&dir, &["--baseline", "coverage-baseline.json"]);
    assert_eq!(status, 0, "{report}");
    assert_eq!(report["delta"]["regressed"], json!([]));
    // The test no longer mentions B: mentioned -> missing is a regression.
    fs::write(dir.path().join("case.test.ts"), "export {};").unwrap();
    let (status, report) = coverage(&dir, &["--baseline", "coverage-baseline.json"]);
    assert_eq!(status, 1, "{report}");
    assert_eq!(
        report["delta"]["regressed"],
        json!([{"type": "errors.ts#Failure#_tag", "tag": "B", "from": "mentioned", "to": "missing"}])
    );
}

fn append_runner(dir: &TempDir, script: &str) {
    let path = dir.path().join("runner.sh");
    let original = fs::read_to_string(&path).unwrap();
    fs::write(path, format!("{original}{script}\n")).unwrap();
}

#[test]
fn invalid_assertion_records_cannot_pass_after_a_successful_test_run() {
    for corruption in [
        "printf '%s' '{' >> \"$ERRSPEC_COVERAGE/one.jsonl\"",
        "mkdir \"$ERRSPEC_COVERAGE/unreadable.jsonl\"",
    ] {
        for extra in [&[][..], &["--fail-on-missing"][..]] {
            let dir = fixture();
            append_runner(&dir, corruption);
            let (status, report) = coverage(&dir, extra);
            assert_eq!(status, 2, "{corruption}: {report}");
            assert_eq!(report["testsExecuted"], true, "{report}");
            assert_eq!(report["runnerOk"], true, "{report}");
            assert_eq!(report["diagnostics"][0]["code"], "EC_RUNNER");
            assert_eq!(report["types"], json!([]), "{report}");
        }
    }
}

#[test]
fn a_failed_runner_with_valid_records_keeps_its_results_and_exit_one() {
    let dir = fixture();
    append_runner(&dir, "exit 1");
    let (status, report) = coverage(&dir, &[]);
    assert_eq!(status, 1, "{report}");
    assert_eq!(report["testsExecuted"], true);
    assert_eq!(report["runnerOk"], false);
    assert_eq!(report["types"][0]["tags"][0]["status"], "asserted");
    assert_eq!(report["diagnostics"][0]["code"], "EC_RUNNER");
}

#[test]
fn fail_on_missing_checks_each_discriminant_even_when_a_value_is_shared() {
    let dir = fixture();
    fs::write(
        dir.path().join("errors.ts"),
        "export type CodeFailure = {code:'A'}; export type KindFailure = {kind:'A'};",
    )
    .unwrap();
    fs::write(dir.path().join("case.test.ts"), "export {};").unwrap();
    let runner = dir.path().join("runner.sh");
    let script = fs::read_to_string(&runner).unwrap();
    fs::write(runner, script.replace("\"_tag\"", "\"code\"")).unwrap();
    let (status, report) = coverage(&dir, &["--fail-on-missing"]);
    assert_eq!(status, 1, "{report}");
    assert_eq!(report["runnerOk"], true);
    assert_eq!(
        report["summary"],
        json!({"types": 2, "tags": 2, "asserted": 1, "mentioned": 0, "missing": 1})
    );
}

#[test]
fn invalid_records_fail_baseline_comparison_and_prevent_saving_partial_results() {
    let dir = fixture();
    let (status, _) = coverage(&dir, &["--save-baseline", "baseline.json"]);
    assert_eq!(status, 0);
    let original = fs::read(dir.path().join("baseline.json")).unwrap();
    append_runner(&dir, "printf '%s' '{' >> \"$ERRSPEC_COVERAGE/one.jsonl\"");
    for extra in [
        ["--baseline", "baseline.json"],
        ["--save-baseline", "partial.json"],
    ] {
        let (status, report) = coverage(&dir, &extra);
        assert_eq!(status, 2, "{extra:?}: {report}");
        assert_eq!(report["testsExecuted"], true);
        assert_eq!(report["runnerOk"], true);
        assert_eq!(report["diagnostics"][0]["code"], "EC_RUNNER");
        assert!(report.get("delta").is_none(), "{report}");
    }
    assert_eq!(
        fs::read(dir.path().join("baseline.json")).unwrap(),
        original
    );
    assert!(!dir.path().join("partial.json").exists());
}
