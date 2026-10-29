//! Observable behavior at the CLI request and output boundaries.
use serde_json::json;
use std::{
    fs,
    process::{Command, Output},
};
use tempfile::TempDir;

fn run(dir: &TempDir, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_errspec"))
        .current_dir(dir.path())
        .args(args)
        .output()
        .unwrap()
}

fn fixture() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("tsconfig.json"),
        r#"{"compilerOptions":{"strict":true,"skipLibCheck":true},"files":["errors.ts"]}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("errors.ts"),
        "export type Failure = {_tag:'Offline'};",
    )
    .unwrap();
    dir
}

#[test]
fn unsupported_baseline_options_fail_before_project_access_in_both_formats() {
    for command in ["scan", "init"] {
        for format in ["text", "json"] {
            for strict in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                let mut args = vec![
                    command,
                    "--project",
                    "missing.json",
                    "--baseline",
                    "missing-baseline.json",
                    "--format",
                    format,
                ];
                if command == "init" {
                    args.extend(["--source", "missing.ts", "--export", "Failure"]);
                }
                if strict {
                    args.push("--strict-baseline");
                }
                let output = run(&dir, &args);
                assert_eq!(output.status.code(), Some(2));
                assert!(output.stdout.is_empty());
                assert_eq!(
                    String::from_utf8(output.stderr).unwrap(),
                    format!("{command} does not accept baseline options\n")
                );
                assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
            }
        }
    }
}

#[test]
fn init_text_preserves_escaped_file_names_and_next_step_message() {
    let dir = fixture();
    let config = "config \"日本語\".json";
    let test = "cases 日本語.test.ts";
    let output = run(
        &dir,
        &[
            "init",
            "--source",
            "errors.ts",
            "--export",
            "Failure",
            "--config",
            config,
            "--test",
            test,
            "--format",
            "text",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let root = dir.path().canonicalize().unwrap();
    let expected = format!(
        "Created configuration and tests: {}\nTests intentionally fail until you specify the expected behavior. Install @errspec/testkit and your test runner, then run errspec verify.\n",
        json!([root.join(config), root.join(test)])
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
}

#[test]
fn init_failure_keeps_compact_json_or_stderr_without_partial_files() {
    for format in ["json", "text"] {
        let dir = fixture();
        let path = dir.path().join("errspec.json");
        fs::write(&path, "keep").unwrap();
        let output = run(
            &dir,
            &[
                "init",
                "--source",
                "errors.ts",
                "--export",
                "Failure",
                "--format",
                format,
            ],
        );
        assert_eq!(output.status.code(), Some(2));
        let error = format!(
            "Refusing to overwrite {}",
            path.canonicalize().unwrap().display()
        );
        if format == "json" {
            assert_eq!(
                String::from_utf8(output.stdout).unwrap(),
                format!(
                    "{}\n",
                    json!({"kind":"errspec/init", "schemaVersion":1, "ready":false, "error":error})
                )
            );
            assert!(output.stderr.is_empty());
        } else {
            assert!(output.stdout.is_empty());
            assert_eq!(
                String::from_utf8(output.stderr).unwrap(),
                format!("EC_INIT {error}\n")
            );
        }
        assert_eq!(fs::read_to_string(path).unwrap(), "keep");
        assert!(!dir.path().join("errspec.generated.test.ts").exists());
    }
}

#[test]
fn verification_reports_configuration_failure_before_runner_option_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("errspec.json"),
        r#"{"schemaVersion":0,"project":"x","contracts":[]}"#,
    )
    .unwrap();
    let output = run(
        &dir,
        &[
            "verify",
            "--runner",
            "bun",
            "--vitest",
            "missing-vitest",
            "--format",
            "json",
        ],
    );
    assert_eq!(output.status.code(), Some(2));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["diagnostics"].as_array().unwrap().len(), 1);
    assert_eq!(report["diagnostics"][0]["code"], "EC_CONFIG");
    assert_eq!(
        report["diagnostics"][0]["message"],
        "Require schemaVersion: 1 or 2, a project, and at least one contract"
    );
    assert_eq!(report["testsExecuted"], false);
}
