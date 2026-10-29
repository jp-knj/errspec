//! Verify resource lifetime and process boundaries through both runner adapters.
#![cfg(unix)]
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Output},
};
use tempfile::TempDir;

const TEST_FILE: &str = "case 日本語.test.ts";

struct Fixture {
    dir: TempDir,
    project: PathBuf,
    workspaces: PathBuf,
}

impl Fixture {
    fn new(runner: &str, mode: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        let workspaces = dir.path().join("workspaces");
        fs::create_dir(&project).unwrap();
        fs::create_dir(&workspaces).unwrap();
        fs::write(project.join("errors.ts"), "export type E = {tag:'A'};").unwrap();
        fs::write(
            project.join("tsconfig.json"),
            r#"{"compilerOptions":{"strict":true,"skipLibCheck":true},"files":["errors.ts"]}"#,
        )
        .unwrap();
        fs::write(project.join(TEST_FILE), "export {};").unwrap();
        fs::write(
            project.join("errspec.json"),
            json!({"schemaVersion":2,"project":"tsconfig.json","contracts":[{
                "id":"checkout","source":{"file":"errors.ts","export":"E","discriminant":"tag"},
                "cases":[{"id":"first","tag":"A","expect":"reject","test":{"file":TEST_FILE},
                    "observe":{"faults":["gateway"],"assertions":["status"]}}]
            }]})
            .to_string(),
        )
        .unwrap();
        let report = if mode == "bad-report" {
            "{".into()
        } else if runner == "vitest" {
            json!({"success":true,"testResults":[{"name":TEST_FILE,"assertionResults":[{
                "fullName":"errspec [\"checkout\",\"first\"]","status":"passed"
            }]}]})
            .to_string()
        } else {
            format!(
                r#"<testsuites tests="1" failures="0"><testsuite file="{TEST_FILE}"><testcase name="errspec [&quot;checkout&quot;,&quot;first&quot;]"/></testsuite></testsuites>"#
            )
        };
        fs::write(project.join("runner-report"), report).unwrap();
        let record = json!({"schemaVersion":1,"kind":"errspec/observation","runId":"%s",
            "contractId":"checkout","scenarioId":"first","tag":"A","file":TEST_FILE,
            "testName":"errspec [\"checkout\",\"first\"]","state":"passed",
            "faults":[{"id":"gateway","calls":1,"completed":1}],"assertions":[{"id":"status","status":"passed"}]
        }).to_string();
        let record = if mode == "bad-observations" || mode == "bad-report" {
            "{"
        } else {
            &record
        };
        let exit = if mode == "bad-observations" { 1 } else { 0 };
        let script = if mode == "spawn-failure" {
            "#!/errspec/nonexistent/interpreter\n".to_owned()
        } else {
            format!(
                r#"#!/bin/sh
set -eu
pwd -P > cwd
printf '%s\n' "$@" > args
printf '%s\n' "$ERRSPEC_RUN_ID" "$ERRSPEC_OBSERVATIONS" > session
cat runner-report > "$4"
printf '{record}' "$ERRSPEC_RUN_ID" > "$ERRSPEC_OBSERVATIONS/one.json"
printf 'runner-out\n'
printf 'runner-err\n' >&2
exit {exit}
"#
            )
        };
        let executable = project.join("runner.sh");
        fs::write(&executable, script).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            dir,
            project,
            workspaces,
        }
    }

    fn verify(&self, runner: &str) -> (Output, Value) {
        let output = Command::new(env!("CARGO_BIN_EXE_errspec"))
            .current_dir(self.dir.path())
            .env("TMPDIR", &self.workspaces)
            .args([
                "verify",
                "--runner",
                runner,
                &format!("--{runner}"),
                "./project/runner.sh",
                "--config",
                "project/errspec.json",
                "--format",
                "json",
                "--only-contract-tests",
            ])
            .output()
            .unwrap();
        let report =
            serde_json::from_slice(&output.stdout).unwrap_or_else(|_| panic!("{output:?}"));
        assert_eq!(
            fs::read_dir(&self.workspaces).unwrap().count(),
            0,
            "workspace leaked"
        );
        (output, report)
    }
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
fn both_runners_keep_observations_through_judgment_and_forward_arguments_and_logs() {
    for runner in ["vitest", "bun"] {
        let fixture = Fixture::new(runner, "success");
        let (output, report) = fixture.verify(runner);
        assert_eq!(output.status.code(), Some(0), "{report}");
        assert_eq!(output.stderr, b"runner-out\nrunner-err\n");
        assert_eq!(
            report["contracts"][0]["cases"][0]["observationStatus"],
            "passed"
        );
        assert_eq!(report["testRunner"], runner);
        let project = fixture.project.canonicalize().unwrap();
        assert_eq!(
            fs::read_to_string(project.join("cwd")).unwrap().trim(),
            project.to_str().unwrap()
        );
        let args = fs::read_to_string(project.join("args")).unwrap();
        let args: Vec<_> = args.lines().collect();
        let expected = if runner == "vitest" {
            ["run", "--reporter=json", "--outputFile"]
        } else {
            ["test", "--reporter=junit", "--reporter-outfile"]
        };
        assert_eq!(&args[..3], &expected);
        assert_eq!(&args[4..], &[project.join(TEST_FILE).to_str().unwrap()]);
        let session = fs::read_to_string(project.join("session")).unwrap();
        let session: Vec<_> = session.lines().collect();
        let workspace = PathBuf::from(session[0]);
        assert_eq!(PathBuf::from(args[3]).parent(), Some(workspace.as_path()));
        assert_eq!(PathBuf::from(session[1]), workspace.join("observations"));
        assert!(!workspace.exists());
    }
}

#[test]
fn both_runners_preserve_failure_precedence_partial_results_and_cleanup() {
    for runner in ["vitest", "bun"] {
        for mode in ["bad-report", "bad-observations"] {
            let fixture = Fixture::new(runner, mode);
            let (output, report) = fixture.verify(runner);
            assert_eq!(output.status.code(), Some(2), "{report}");
            assert_eq!(report["analysisComplete"], false);
            if mode == "bad-report" {
                assert_eq!(codes(&report), ["EC_RUNNER"]);
                assert_eq!(report["testsExecuted"], false);
                assert!(report.get("testRunner").is_none());
            } else {
                assert_eq!(codes(&report), ["EC006", "EC_EVIDENCE"]);
                assert_eq!(report["testsExecuted"], true);
                assert_eq!(report["testRunner"], runner);
                assert_eq!(report["contracts"][0]["cases"][0]["testStatus"], "passed");
            }
            assert_eq!(
                report["contracts"][0]["cases"][0]["observationStatus"],
                "not_run"
            );
        }
    }
}

#[test]
fn spawn_failure_cleans_the_workspace_for_both_runners() {
    for runner in ["vitest", "bun"] {
        let fixture = Fixture::new(runner, "spawn-failure");
        let (output, report) = fixture.verify(runner);
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(codes(&report), ["EC_RUNNER"]);
        assert_eq!(report["testsExecuted"], false);
        assert!(!fixture.project.join("session").exists());
    }
}
