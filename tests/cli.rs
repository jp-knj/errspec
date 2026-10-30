use serde_json::{Value, json};
use std::{fs, process::Command};
use tempfile::TempDir;

#[test]
fn sdk_types_preserve_fault_arguments_and_assertion_value_types() {
    let dir = fixture(
        r#"
import type { ScenarioScope } from './sdk/index.mjs';
declare const s: ScenarioScope;
const fail = s.fault('gateway', async (code: number) => ({ code }));
const result: Promise<{code:number}> = fail(429);
// @ts-expect-error argument type must survive the wrapper
fail('429');
s.check('status', 429 as number, 429);
// @ts-expect-error mismatched comparison type must be rejected
s.check('status', 429, '429');
export type ErrorType = {_tag:'A'};
"#,
        &["A"],
    );
    fs::create_dir(dir.path().join("sdk")).unwrap();
    fs::write(
        dir.path().join("sdk/index.d.mts"),
        include_str!("../packages/testkit/index.d.mts"),
    )
    .unwrap();
    let (status, report) = run(&dir, &["check"]);
    assert_eq!(status, 0, "{report}");
}

/// Minimal declarations for the Node APIs the TypeScript testkit imports; the repository has no @types/node.
const NODE_STUB: &str = r#"
declare module "node:assert/strict" { export function deepStrictEqual(actual: unknown, expected: unknown): void; }
declare module "node:crypto" { export function randomUUID(): string; }
declare module "node:fs" {
  export function writeFileSync(path: string, data: string, options: { flag: string }): void;
  export function renameSync(from: string, to: string): void;
}
declare module "node:path" { export function join(...parts: string[]): string; }
declare module "node:url" { export function fileURLToPath(url: string | URL): string; }
declare var process: { env: Record<string, string | undefined> };
"#;

#[test]
fn typescript_testkit_type_checks_strictly_with_the_public_types() {
    for (extra, expected) in [("", 0), ("\nconst broken: number = 'x';\n", 2)] {
        let dir = fixture(
            r#"
/// <reference path="./sdk/node.d.ts" />
import { scenarioTest, type ScenarioScope } from './sdk/index';
declare const s: ScenarioScope;
const fail = s.fault('gateway', async (code: number) => ({ code }));
const result: Promise<{code:number}> = fail(429);
// @ts-expect-error argument type must survive the wrapper
fail('429');
// @ts-expect-error mismatched comparison type must be rejected
s.check('status', 429, '429');
scenarioTest((_name, body) => body(), 'file:///a.test.ts', { contract: 'c', id: 'i', tag: 'A', withoutFaults: true, description: 'd' }, (scope) => scope.check('n', 1, 1));
export type ErrorType = {_tag:'A'};
"#,
            &["A"],
        );
        // The flags of @tsconfig/strictest that apply to vendored code, without the unused-name checks for this fixture.
        fs::write(
            dir.path().join("tsconfig.json"),
            r#"{"compilerOptions":{"strict":true,"skipLibCheck":true,"target":"ES2022","module":"ESNext","moduleResolution":"Bundler","exactOptionalPropertyTypes":true,"noFallthroughCasesInSwitch":true,"noImplicitOverride":true,"noImplicitReturns":true,"noPropertyAccessFromIndexSignature":true,"noUncheckedIndexedAccess":true,"isolatedModules":true,"checkJs":true},"files":["errors.ts"]}"#,
        )
        .unwrap();
        fs::create_dir(dir.path().join("sdk")).unwrap();
        fs::write(dir.path().join("sdk/node.d.ts"), NODE_STUB).unwrap();
        fs::write(
            dir.path().join("sdk/index.ts"),
            format!("{}{extra}", include_str!("../packages/testkit/index.ts")),
        )
        .unwrap();
        let (status, report) = run(&dir, &["check"]);
        assert_eq!(status, expected, "{report}");
    }
}

fn fixture(source: &str, cases: &[&str]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("errors.ts"), source).unwrap();
    fs::write(dir.path().join("test.ts"), "export {};").unwrap();
    fs::write(dir.path().join("tsconfig.json"), r#"{"compilerOptions":{"strict":true,"skipLibCheck":true,"target":"ES2022","module":"ESNext","moduleResolution":"Bundler"},"files":["errors.ts"]}"#).unwrap();
    let config = json!({"schemaVersion":1,"project":"tsconfig.json","contracts":[{
        "id":"checkout","source":{"file":"errors.ts","export":"ErrorType","discriminant":"_tag"},
        "cases":cases.iter().map(|tag| json!({"tag":tag,"expect":"must reject", "test":{"file":"test.ts","name":tag}})).collect::<Vec<_>>()
    }]});
    fs::write(dir.path().join("errspec.json"), config.to_string()).unwrap();
    dir
}

fn run(dir: &TempDir, command: &[&str]) -> (i32, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_errspec"))
        .current_dir(dir.path())
        .args(command)
        .args(["--format", "json"])
        .output()
        .unwrap();
    let report = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.code().unwrap(), report)
}

fn has(report: &Value, code: &str) -> bool {
    report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["code"] == code)
}

#[test]
fn added_variant_requires_a_case_and_is_in_delta() {
    let dir = fixture("export type ErrorType = {_tag:'A'} | {_tag:'B'};", &["A"]);
    fs::write(dir.path().join("baseline.json"), json!({"schemaVersion":1,"kind":"errspec/snapshot","contracts":{"checkout":{"source":{"file":"errors.ts","export":"ErrorType","discriminant":"_tag"},"tags":["A"]}}}).to_string()).unwrap();
    let (status, report) = run(&dir, &["check", "--baseline", "baseline.json"]);
    assert_eq!(status, 1);
    assert!(has(&report, "EC001"));
    assert_eq!(report["delta"]["added"]["checkout"], json!(["B"]));
    assert_eq!(report["analysisComplete"], true);
}

#[test]
fn resolves_reexports_generics_intersections_and_path_aliases() {
    let dir = fixture(
        "export type { Failures as ErrorType } from '@domain/errors';",
        &["A", "B"],
    );
    fs::create_dir(dir.path().join("domain")).unwrap();
    fs::write(dir.path().join("domain/errors.ts"), "interface Base { message: string }; type Tagged<T extends string> = Base & {_tag:T}; export type Failures = Tagged<'A'> | Tagged<'B'>;").unwrap();
    fs::write(dir.path().join("tsconfig.json"), r#"{"compilerOptions":{"strict":true,"skipLibCheck":true,"module":"ESNext","moduleResolution":"Bundler","paths":{"@domain/*":["./domain/*"]}},"files":["errors.ts"]}"#).unwrap();
    let (status, report) = run(&dir, &["check"]);
    assert_eq!(status, 0, "{report}");
    assert_eq!(report["contracts"][0]["tags"], json!(["A", "B"]));
    assert_eq!(report["testsExecuted"], false);
    assert_eq!(report["contracts"][0]["cases"][0]["testStatus"], "not_run");
}

#[test]
fn wide_unknown_any_and_optional_discriminants_are_incomplete() {
    for ty in [
        "any",
        "unknown",
        "{_tag: string}",
        "{_tag?: 'A'}",
        "{_tag:'A'} | {code:1}",
    ] {
        let dir = fixture(&format!("export type ErrorType = {ty};"), &[]);
        let (status, report) = run(&dir, &["check"]);
        assert_eq!(status, 2, "{ty}: {report}");
        assert!(has(&report, "EC002"));
        assert_eq!(report["analysisComplete"], false);
    }
}

#[test]
fn finite_literals_and_never_are_supported() {
    let dir = fixture(
        "export type ErrorType = {_tag:'日本語' | 'A'};",
        &["A", "日本語"],
    );
    assert_eq!(run(&dir, &["check"]).0, 0);
    let dir = fixture("export type ErrorType = never;", &[]);
    let (status, report) = run(&dir, &["check"]);
    assert_eq!(status, 0, "{report}");
    assert_eq!(report["contracts"][0]["tags"], json!([]));
}

#[test]
fn compiler_error_is_not_a_clean_scan() {
    let dir = fixture("export type ErrorType = MissingType;", &[]);
    let (status, report) = run(&dir, &["check"]);
    assert_eq!(status, 2, "{report}");
    assert!(has(&report, "TS2304"));
    assert_eq!(report["diagnostics"][0]["line"], 1);
}

#[test]
fn text_output_locates_diagnostics_without_a_case() {
    let dir = fixture("\nexport type ErrorType = MissingType;", &[]);
    let output = Command::new(env!("CARGO_BIN_EXE_errspec"))
        .current_dir(dir.path())
        .arg("check")
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let last = stdout.lines().last().unwrap();
    assert!(last.starts_with("TS2304 "), "{stdout}");
    assert!(last.contains("errors.ts:2: Cannot find name"), "{stdout}");
}

#[test]
fn files_outside_root_dir_do_not_block_analysis() {
    let dir = fixture(
        "import type { Shared } from './shared/types';\nexport type ErrorType = Shared;",
        &["A"],
    );
    fs::create_dir(dir.path().join("shared")).unwrap();
    fs::write(
        dir.path().join("shared/types.ts"),
        "export type Shared = {_tag:'A'};",
    )
    .unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("tsconfig.json"), r#"{"compilerOptions":{"strict":true,"skipLibCheck":true,"target":"ES2022","module":"ESNext","moduleResolution":"Bundler","rootDir":"./src","outDir":"./dist"},"files":["errors.ts"]}"#).unwrap();
    let (status, report) = run(&dir, &["check"]);
    assert_eq!(status, 0, "{report}");
    let output = Command::new(env!("CARGO_BIN_EXE_errspec"))
        .current_dir(dir.path())
        .args(["scan", "--format", "json"])
        .output()
        .unwrap();
    let scan: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(scan["scanComplete"], true, "{scan}");
}

#[test]
fn missing_test_file_and_removed_case_fail() {
    let dir = fixture("export type ErrorType = {_tag:'A'};", &["A", "B"]);
    fs::remove_file(dir.path().join("test.ts")).unwrap();
    let (status, report) = run(&dir, &["check"]);
    assert_eq!(status, 1);
    assert!(has(&report, "EC003"));
    assert!(has(&report, "EC004"));
}

#[test]
fn duplicate_cases_and_unknown_config_fields_are_rejected() {
    let dir = fixture("export type ErrorType = {_tag:'A'};", &["A", "A"]);
    let (status, report) = run(&dir, &["check"]);
    assert_eq!(status, 2);
    assert!(has(&report, "EC_CONFIG"));
    fs::write(
        dir.path().join("errspec.json"),
        r#"{"schemaVersion":1,"project":"tsconfig.json","contracts":[],"constracts":[]}"#,
    )
    .unwrap();
    assert_eq!(run(&dir, &["check"]).0, 2);
}

#[test]
fn snapshot_is_roundtrippable_and_does_not_overwrite() {
    let dir = fixture("export type ErrorType = {_tag:'A'};", &["A"]);
    let (status, report) = run(&dir, &["snapshot", "--out", "baseline.json"]);
    assert_eq!(status, 0, "{report}");
    assert_eq!(run(&dir, &["check", "--baseline", "baseline.json"]).0, 0);
    assert_eq!(run(&dir, &["snapshot", "--out", "baseline.json"]).0, 2);
}

#[test]
fn text_output_distinguishes_registration_from_execution() {
    let dir = fixture("export type ErrorType = {_tag:'A'};", &["A"]);
    let output = Command::new(env!("CARGO_BIN_EXE_errspec"))
        .current_dir(dir.path())
        .arg("check")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        concat!(
            "check: pass (analysis complete: true, tests executed: false)\n",
            "checkout: A\n",
            "  A : registered, test=not_run, observation=not_required\n",
        )
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn snapshot_inventories_tags_even_when_registrations_are_missing() {
    let dir = fixture("export type ErrorType = {_tag:'A'} | {_tag:'B'};", &["A"]);
    let (status, report) = run(&dir, &["snapshot", "--out", "baseline.json"]);
    assert_eq!(status, 1);
    assert!(has(&report, "EC001"));
    let saved: Value =
        serde_json::from_slice(&fs::read(dir.path().join("baseline.json")).unwrap()).unwrap();
    assert_eq!(saved["contracts"]["checkout"]["tags"], json!(["A", "B"]));
}

#[cfg(unix)]
#[test]
fn runner_uses_config_directory_and_keeps_logs_out_of_json_despite_registration_gaps() {
    use std::os::unix::fs::PermissionsExt;
    let dir = fixture("export type ErrorType = {_tag:'A'} | {_tag:'B'};", &["A"]);
    let caller = tempfile::tempdir().unwrap();
    let executable = caller.path().join("fake-bun");
    fs::write(&executable, r#"#!/bin/sh
set -eu
test -f test.ts
test -d "$ERRSPEC_OBSERVATIONS"
test -n "$ERRSPEC_RUN_ID"
echo runner-stdout
echo runner-stderr >&2
printf '%s' '<testsuites tests="1" failures="0"><testsuite file="test.ts"><testcase name="A"/></testsuite></testsuites>' > "$4"
"#).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_errspec"))
        .current_dir(caller.path())
        .args([
            "verify",
            "--runner",
            "bun",
            "--bun",
            "./fake-bun",
            "--format",
            "json",
            "--config",
        ])
        .arg(dir.path().join("errspec.json"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(has(&report, "EC001"));
    assert_eq!(report["testsExecuted"], true);
    assert_eq!(report["analysisComplete"], true);
    assert_eq!(report["contracts"][0]["cases"][0]["testStatus"], "passed");
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "runner-stdout\nrunner-stderr\n"
    );
}

#[test]
fn missing_vitest_cannot_be_reported_as_passed() {
    let dir = fixture("export type ErrorType = {_tag:'A'};", &["A"]);
    let (status, report) = run(&dir, &["verify"]);
    assert_eq!(status, 2);
    assert!(has(&report, "EC_RUNNER"));
    assert_eq!(report["testsExecuted"], false);
    let message = report["diagnostics"][0]["message"].as_str().unwrap();
    assert!(message.contains("node_modules/.bin/vitest"), "{message}");
    assert!(message.ends_with("--vitest PATH when vitest is installed elsewhere"));
}

#[test]
fn missing_bun_and_mismatched_runner_options_are_rejected() {
    let dir = fixture("export type ErrorType = {_tag:'A'};", &["A"]);
    let (status, report) = run(&dir, &["verify", "--runner", "bun", "--bun", "missing-bun"]);
    assert_eq!(status, 2);
    assert!(has(&report, "EC_RUNNER"));
    let message = report["diagnostics"][0]["message"].as_str().unwrap();
    assert!(message.contains("missing-bun: "), "{message}");
    assert!(!message.contains("--vitest"), "{message}");
    assert_eq!(report["testsExecuted"], false);
    let (status, report) = run(
        &dir,
        &["verify", "--runner", "bun", "--vitest", "missing-vitest"],
    );
    assert_eq!(status, 2);
    assert!(has(&report, "EC_CONFIG"));
}

#[cfg(unix)]
#[test]
fn bun_without_junit_file_paths_is_rejected_before_tests_run() {
    use std::os::unix::fs::PermissionsExt;
    let dir = fixture("export type ErrorType = {_tag:'A'};", &["A"]);
    let executable = dir.path().join("fake-bun");
    let ran = dir.path().join("ran");
    for (version, expected) in [("1.2.18", 2), ("1.2.19", 0), ("1.4.4-canary.1", 0)] {
        let _ = fs::remove_file(&ran);
        fs::write(&executable, format!(r#"#!/bin/sh
if [ "$1" = --version ]; then echo {version}; exit 0; fi
touch ran
printf '%s' '<testsuites tests="1" failures="0"><testsuite file="test.ts"><testcase name="A"/></testsuite></testsuites>' > "$4"
"#)).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        let (status, report) = run(
            &dir,
            &[
                "verify",
                "--runner",
                "bun",
                "--bun",
                executable.to_str().unwrap(),
            ],
        );
        assert_eq!(status, expected, "{version}: {report}");
        assert_eq!(ran.exists(), expected == 0, "{version}");
        if expected == 2 {
            let message = report["diagnostics"][0]["message"].as_str().unwrap();
            assert_eq!(report["diagnostics"][0]["code"], "EC_RUNNER");
            assert!(message.contains("Bun 1.2.18") && message.contains("1.2.19 or later"));
            assert_eq!(report["testsExecuted"], false);
        }
    }
}

#[cfg(unix)]
#[test]
fn zero_exit_bun_without_valid_report_cannot_pass() {
    use std::os::unix::fs::PermissionsExt;
    let dir = fixture("export type ErrorType = {_tag:'A'};", &["A"]);
    let executable = dir.path().join("fake-bun");
    for body in ["exit 0", "printf '%s' '<testsuites>' > \"$4\""] {
        fs::write(&executable, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        let (status, report) = run(
            &dir,
            &[
                "verify",
                "--runner",
                "bun",
                "--bun",
                executable.to_str().unwrap(),
            ],
        );
        assert_eq!(status, 2, "{report}");
        assert!(has(&report, "EC_RUNNER"));
        assert_eq!(report["testsExecuted"], false);
        assert_eq!(report["contracts"][0]["cases"][0]["testStatus"], "not_run");
    }
}
