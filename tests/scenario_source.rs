//! `check` matches registered test files against the configuration through tsrs, without running them.
//! Every rejection here is a case that would certainly fail at runtime; every accepted case that is
//! not statically decidable is left to the runtime observation checks.
use serde_json::{Value, json};
use std::{fs, process::Command};
use tempfile::TempDir;

const RUNNER: &str = "type Body = () => Promise<void>;
interface Describe { (name: string, body: () => void): void; only: Describe; each(rows: unknown[]): Describe; }
declare const test: (name: string, body: Body) => void;
declare const describe: Describe;
export { test, describe };
";

const GOOD: &str = r#"
import { test } from './runner';
import { scenarioTest } from './sdk/index.mjs';
const CONTRACT = 'checkout';
scenarioTest(test, 'file:///x', { contract: CONTRACT, id: 'declined', tag: 'Declined', description: 'd' }, async (s) => {
  await s.fault('gateway', async () => 1)();
  s.check('status', 402, 402);
});
for (const { id } of [{ id: 'limit-first' }, { id: 'limit-retries' }] as const) {
  scenarioTest(test, 'file:///x', { contract: CONTRACT, id, tag: 'RateLimited' }, async (s) => {
    await s.fault('gateway', async () => 1)();
    s.check('status', 429, 429);
  });
}
scenarioTest(test, 'file:///x', { contract: CONTRACT, id: 'invalid', tag: 'Invalid', withoutFaults: true }, async (s) => {
  s.check('status', 400, 400);
});
"#;

struct Project {
    files: Vec<(String, String)>,
    project_files: Vec<String>,
    config: Value,
}

fn case(id: &str, tag: &str, faults: Value, file: &str) -> Value {
    json!({"id":id,"tag":tag,"expect":"reject","test":{"file":file},
        "observe":{"faults":faults,"assertions":["status"]}})
}

fn cases() -> Vec<Value> {
    vec![
        case("declined", "Declined", json!(["gateway"]), "case.test.ts"),
        case(
            "limit-first",
            "RateLimited",
            json!(["gateway"]),
            "case.test.ts",
        ),
        case(
            "limit-retries",
            "RateLimited",
            json!(["gateway"]),
            "case.test.ts",
        ),
        case("invalid", "Invalid", json!([]), "case.test.ts"),
    ]
}

impl Project {
    fn new(test_source: &str) -> Self {
        Self {
            files: vec![("case.test.ts".into(), test_source.into())],
            project_files: vec!["errors.ts".into(), "case.test.ts".into()],
            config: json!({"schemaVersion":2,"project":"tsconfig.json","contracts":[{
                "id":"checkout","source":{"file":"errors.ts","export":"E","discriminant":"_tag"},
                "cases":cases()
            }]}),
        }
    }
    fn file(mut self, path: &str, text: &str) -> Self {
        self.files.push((path.into(), text.into()));
        self.project_files.push(path.into());
        self
    }
    fn outside_project(mut self) -> Self {
        self.project_files.retain(|f| f != "case.test.ts");
        self
    }
    fn write(&self) -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        let write = |path: &str, text: &str| fs::write(dir.path().join(path), text).unwrap();
        fs::create_dir(dir.path().join("sdk")).unwrap();
        write(
            "sdk/index.d.mts",
            include_str!("../packages/testkit/index.d.mts"),
        );
        write(
            "errors.ts",
            "export type E = {_tag:'Declined'} | {_tag:'RateLimited'} | {_tag:'Invalid'};\n\
             export type F = {kind:'Lost'};",
        );
        write("runner.ts", RUNNER);
        for (path, text) in &self.files {
            write(path, text);
        }
        write(
            "tsconfig.json",
            &json!({"compilerOptions":{"strict":true,"skipLibCheck":true,"target":"ES2022","module":"ESNext","moduleResolution":"Bundler"},"files":self.project_files}).to_string(),
        );
        write("errspec.json", &self.config.to_string());
        dir
    }
}

struct Outcome {
    status: i32,
    findings: Vec<Value>,
}

impl Outcome {
    fn codes(&self) -> Vec<&str> {
        self.findings
            .iter()
            .map(|f| f["code"].as_str().unwrap())
            .collect()
    }
    fn has(&self, code: &str, message: &str) -> bool {
        self.findings.iter().any(|f| {
            f["code"] == code && f["message"].as_str().is_some_and(|m| m.contains(message))
        })
    }
}

fn run(dir: &TempDir, args: &[&str]) -> Outcome {
    let output = Command::new(env!("CARGO_BIN_EXE_errspec"))
        .current_dir(dir.path())
        .args(args)
        .args(["--format", "json"])
        .output()
        .unwrap();
    let report: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!("{}", String::from_utf8_lossy(&output.stderr));
    });
    assert_eq!(report["analysisComplete"], true, "{report}");
    Outcome {
        status: output.status.code().unwrap(),
        findings: report["diagnostics"].as_array().unwrap().clone(),
    }
}

fn check_source(source: &str) -> Outcome {
    run(&Project::new(source).write(), &["check"])
}

fn replaced(from: &str, to: &str) -> String {
    assert!(GOOD.contains(from), "fixture does not contain {from:?}");
    GOOD.replacen(from, to, 1)
}

fn assert_clean(outcome: &Outcome, label: &str) {
    assert_eq!(outcome.status, 0, "{label}: {:?}", outcome.findings);
    assert!(
        outcome.findings.is_empty(),
        "{label}: {:?}",
        outcome.findings
    );
}

fn assert_reports(outcome: &Outcome, code: &str, message: &str, label: &str) {
    assert_eq!(outcome.status, 1, "{label}: {:?}", outcome.findings);
    assert!(
        outcome.has(code, message),
        "{label}: {:?}",
        outcome.findings
    );
}

// [正常系]

#[test]
fn matching_tests_pass_including_literal_ids_from_const_arrays() {
    assert_clean(&check_source(GOOD), "good");
}

#[test]
fn aliased_and_reexported_sdk_functions_are_still_inspected() {
    let aliased = replaced("import { scenarioTest } from './sdk/index.mjs';", "import { scenarioTest as register } from './sdk/index.mjs';\nconst scenarioTest = register;")
        .replacen("s.check('status', 402, 402)", "s.check('code', 402, 402)", 1);
    assert_reports(
        &check_source(&aliased),
        "EC016",
        "check id \"code\"",
        "alias",
    );

    let reexported = replaced(
        "import { scenarioTest } from './sdk/index.mjs';",
        "import { scenarioTest } from './helpers';",
    )
    .replacen(
        "s.check('status', 402, 402)",
        "s.check('code', 402, 402)",
        1,
    );
    let dir = Project::new(&reexported)
        .file(
            "helpers.ts",
            "export { scenarioTest } from './sdk/index.mjs';\n",
        )
        .write();
    assert_reports(
        &run(&dir, &["check"]),
        "EC016",
        "check id \"code\"",
        "re-export",
    );
}

#[test]
fn tag_unions_from_loops_pass_when_they_contain_the_registered_tag() {
    let source = replaced(
        "for (const { id } of [{ id: 'limit-first' }, { id: 'limit-retries' }] as const) {\n  scenarioTest(test, 'file:///x', { contract: CONTRACT, id, tag: 'RateLimited' }",
        "for (const { id, tag } of [{ id: 'limit-first', tag: 'RateLimited' }, { id: 'limit-retries', tag: 'RateLimited' }] as const) {\n  scenarioTest(test, 'file:///x', { contract: CONTRACT, id, tag }",
    );
    assert_clean(&check_source(&source), "tag from loop");
}

#[test]
fn ids_inside_callbacks_within_the_body_are_inspected() {
    let source = replaced(
        "s.check('status', 402, 402);",
        "[402].forEach((code) => s.check('statuz', code, 402));",
    );
    assert_reports(
        &check_source(&source),
        "EC016",
        "check id \"statuz\"",
        "callback",
    );
}

#[test]
fn findings_carry_the_contract_scenario_tag_and_source_location() {
    let outcome = check_source(&replaced(
        "s.check('status', 402, 402)",
        "s.check('code', 402, 402)",
    ));
    let finding = outcome
        .findings
        .iter()
        .find(|f| f["code"] == "EC016")
        .unwrap();
    assert_eq!(finding["contractId"], "checkout");
    assert_eq!(finding["scenarioId"], "declined");
    assert_eq!(finding["tag"], "Declined");
    assert_eq!(finding["file"], "case.test.ts");
    let offset = finding["byteOffset"].as_u64().unwrap() as usize;
    assert!(
        GOOD[..].len() > offset
            && replaced("s.check('status', 402, 402)", "s.check('code', 402, 402)")[offset..]
                .starts_with(
                    "scenarioTest(test, 'file:///x', { contract: CONTRACT, id: 'declined'"
                ),
        "{finding}"
    );
}

#[test]
fn text_output_names_the_static_finding() {
    let dir = Project::new(&replaced(
        "s.check('status', 402, 402)",
        "s.check('code', 402, 402)",
    ))
    .write();
    let output = Command::new(env!("CARGO_BIN_EXE_errspec"))
        .current_dir(dir.path())
        .arg("check")
        .output()
        .unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(text.contains("EC016 checkout Declined declined"), "{text}");
}

// [異常系]

#[test]
fn definite_contradictions_are_reported_before_tests_run() {
    for (from, to, code, message) in [
        (
            "{ id: 'limit-retries' }",
            "{ id: 'limit-retries' }, { id: 'limit-later' }",
            "EC015",
            "not in the configuration: checkout / limit-later",
        ),
        (
            "id: 'declined', tag: 'Declined'",
            "id: 'declined', tag: 'RateLimited'",
            "EC015",
            "differs from the registered tag Declined",
        ),
        (
            "tag: 'Invalid', withoutFaults: true",
            "tag: 'Invalid'",
            "EC015",
            "does not declare withoutFaults",
        ),
        (
            "tag: 'Invalid', withoutFaults: true",
            "tag: 'Invalid', withoutFaults: false",
            "EC015",
            "does not declare withoutFaults",
        ),
        (
            "tag: 'Declined', description",
            "tag: 'Declined', withoutFaults: true, description",
            "EC015",
            "requires faults",
        ),
        (
            "const CONTRACT = 'checkout';",
            "const CONTRACT = 'checkout-v2';",
            "EC015",
            "checkout-v2 / declined",
        ),
        (
            "s.check('status', 402, 402)",
            "s.check('code', 402, 402)",
            "EC016",
            "check id \"code\"",
        ),
        (
            "await s.fault('gateway', async () => 1)();\n  s.check('status', 402",
            "await s.fault('gate', async () => 1)();\n  s.check('status', 402",
            "EC016",
            "fault id \"gate\"",
        ),
    ] {
        assert_reports(&check_source(&replaced(from, to)), code, message, to);
    }
}

#[test]
fn every_member_of_a_literal_union_is_matched() {
    // One registered and one unregistered contract id: only the unregistered pair is reported.
    let source = replaced(
        "const CONTRACT = 'checkout';",
        "const CONTRACT = 'checkout';\nconst EITHER: 'checkout' | 'billing' = Math.random() > 1 ? 'checkout' : 'billing';",
    )
    .replacen("{ contract: CONTRACT, id: 'declined'", "{ contract: EITHER, id: 'declined'", 1);
    let outcome = check_source(&source);
    assert_reports(&outcome, "EC015", "billing / declined", "union");
    assert_eq!(outcome.codes(), vec!["EC015"], "{:?}", outcome.findings);
}

#[test]
fn a_scenario_registered_for_another_file_is_reported() {
    let other = r#"
import { test } from './runner';
import { scenarioTest } from './sdk/index.mjs';
scenarioTest(test, 'file:///y', { contract: 'checkout', id: 'declined', tag: 'Declined' }, async (s) => {
  await s.fault('gateway', async () => 1)();
  s.check('status', 402, 402);
});
"#;
    let mut project = Project::new(GOOD).file("other.test.ts", other);
    let mut contract_cases = cases();
    contract_cases.push(case(
        "lost",
        "Declined",
        json!(["gateway"]),
        "other.test.ts",
    ));
    project.config["contracts"][0]["cases"] = json!(contract_cases);
    let outcome = run(&project.write(), &["check"]);
    assert_reports(&outcome, "EC015", "different test file", "other file");
}

#[test]
fn every_scenario_of_a_shared_body_reports_an_undeclared_id() {
    let outcome = check_source(&replaced(
        "await s.fault('gateway', async () => 1)();\n    s.check('status', 429",
        "await s.fault('gate', async () => 1)();\n    s.check('status', 429",
    ));
    assert_eq!(outcome.status, 1);
    let scenarios: Vec<_> = outcome
        .findings
        .iter()
        .filter(|f| f["code"] == "EC016")
        .map(|f| f["scenarioId"].as_str().unwrap())
        .collect();
    assert_eq!(scenarios, vec!["limit-first", "limit-retries"]);
}

#[test]
fn describe_nesting_is_reported_for_each_describe_form() {
    for wrapper in [
        "describe('group', ",
        "describe.only('group', ",
        "describe.each([1])('group', ",
    ] {
        let nested = GOOD
            .replacen("import { test }", "import { describe, test }", 1)
            .replacen(
                "scenarioTest(test, 'file:///x', { contract: CONTRACT, id: 'invalid'",
                &format!("{wrapper}() => scenarioTest(test, 'file:///x', {{ contract: CONTRACT, id: 'invalid'"),
                1,
            )
            .replacen("s.check('status', 400, 400);\n});", "s.check('status', 400, 400);\n}));", 1);
        let outcome = check_source(&nested);
        assert_reports(&outcome, "EC017", "inside describe", wrapper);
    }
}

#[test]
fn verify_reports_static_findings_alongside_runtime_results() {
    use std::os::unix::fs::PermissionsExt;
    let dir = Project::new(&replaced(
        "s.check('status', 402, 402)",
        "s.check('code', 402, 402)",
    ))
    .write();
    let runner = dir.path().join("fake-bun");
    fs::write(
        &runner,
        "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 1.4.4; exit 0; fi\nprintf '%s' '<testsuites tests=\"0\" failures=\"0\"/>' > \"$4\"\n",
    )
    .unwrap();
    fs::set_permissions(&runner, fs::Permissions::from_mode(0o755)).unwrap();
    let outcome = run(
        &dir,
        &[
            "verify",
            "--runner",
            "bun",
            "--bun",
            runner.to_str().unwrap(),
        ],
    );
    assert_eq!(outcome.status, 1, "{:?}", outcome.findings);
    assert!(outcome.has("EC016", "check id \"code\""));
    // The run still happened: runtime diagnostics are reported next to the static one.
    assert!(outcome.codes().contains(&"EC005"), "{:?}", outcome.findings);
}

// [エッジケース]

#[test]
fn values_that_are_not_statically_decidable_are_left_to_runtime() {
    for (from, to) in [
        (
            "const CONTRACT = 'checkout';",
            "const CONTRACT: string = 'nowhere';",
        ),
        ("id: 'declined', tag", "id: String('nowhere'), tag"),
        (
            "{ contract: CONTRACT, id: 'declined', tag: 'Declined', description: 'd' }",
            "{ ...{ contract: CONTRACT }, id: 'declined', tag: 'Declined', description: 'd' }",
        ),
        (
            "tag: 'Invalid', withoutFaults: true",
            "tag: 'Invalid', withoutFaults: Math.random() < 2",
        ),
        (
            "s.check('status', 402, 402)",
            "s.check(String('code'), 402, 402)",
        ),
    ] {
        assert_clean(&check_source(&replaced(from, to)), to);
    }
}

#[test]
fn a_spread_may_supply_without_faults_so_it_is_not_reported() {
    let source = replaced(
        "{ contract: CONTRACT, id: 'invalid', tag: 'Invalid', withoutFaults: true }",
        "{ contract: CONTRACT, id: 'invalid', tag: 'Invalid', ...{ withoutFaults: true as const } }",
    );
    assert_clean(&check_source(&source), "spread");
}

#[test]
fn definitions_passed_by_variable_are_left_to_runtime() {
    let source = replaced(
        "scenarioTest(test, 'file:///x', { contract: CONTRACT, id: 'declined', tag: 'Declined', description: 'd' }",
        "const definition = { contract: 'elsewhere', id: 'declined', tag: 'Declined' } as const;\nscenarioTest(test, 'file:///x', definition",
    );
    assert_clean(&check_source(&source), "variable");
}

#[test]
fn scope_passed_to_helpers_outside_the_body_is_not_inspected() {
    let source = replaced(
        "const CONTRACT = 'checkout';",
        "const CONTRACT = 'checkout';\nimport type { ScenarioScope } from './sdk/index.mjs';\nconst helper = (s: ScenarioScope) => s.check('anything', 1, 1);",
    );
    assert_clean(&check_source(&source), "helper");
}

#[test]
fn same_named_functions_and_methods_outside_the_testkit_are_ignored() {
    let source = r#"
import { test } from './runner';
function scenarioTest(_t: unknown, _f: string, _d: { contract: string; id: string; tag: string }, _b: (s: { check(id: string): void }) => void) {}
scenarioTest(test, 'file:///x', { contract: 'nowhere', id: 'nothing', tag: 'None' }, (s) => s.check('anything'));
const fake = { fault: (_id: string) => 0, check: (_id: string) => 0 };
fake.fault('x'); fake.check('y');
"#;
    assert_clean(&check_source(source), "lookalike");
}

#[test]
fn test_files_outside_the_project_are_matched_only_at_runtime() {
    let broken = replaced("s.check('status', 402, 402)", "s.check('code', 402, 402)");
    let outcome = run(&Project::new(&broken).outside_project().write(), &["check"]);
    assert_clean(&outcome, "outside");
}

#[test]
fn version_one_configurations_are_not_statically_matched() {
    let mut project = Project::new(&replaced(
        "s.check('status', 402, 402)",
        "s.check('code', 402, 402)",
    ));
    let legacy: Vec<_> = ["Declined", "RateLimited", "Invalid"]
        .iter()
        .map(|tag| json!({"tag":tag,"expect":"reject","test":{"file":"case.test.ts","name":tag}}))
        .collect();
    project.config = json!({"schemaVersion":1,"project":"tsconfig.json","contracts":[{
        "id":"checkout","source":{"file":"errors.ts","export":"E","discriminant":"_tag"},
        "cases":legacy
    }]});
    assert_clean(&run(&project.write(), &["check"]), "v1");
}

#[test]
fn other_contracts_in_the_same_file_are_matched_independently() {
    let source = format!(
        "{GOOD}\nscenarioTest(test, 'file:///x', {{ contract: 'shipping', id: 'lost', tag: 'Lost' }}, async (s) => {{\n  await s.fault('carrier', async () => 1)();\n  s.check('status', 404, 404);\n}});\n"
    );
    let mut project = Project::new(&source);
    project.config["contracts"].as_array_mut().unwrap().push(
        json!({"id":"shipping","source":{"file":"errors.ts","export":"F","discriminant":"kind"},
            "cases":[case("lost", "Lost", json!(["carrier"]), "case.test.ts")]}),
    );
    assert_clean(&run(&project.write(), &["check"]), "two contracts");
    let dir = {
        let mut broken =
            Project::new(&source.replacen("s.fault('carrier'", "s.fault('courier'", 1));
        broken.config = project.config.clone();
        broken.write()
    };
    let outcome = run(&dir, &["check"]);
    assert_reports(&outcome, "EC016", "fault id \"courier\"", "shipping");
    assert!(
        outcome
            .findings
            .iter()
            .all(|f| f["contractId"] == "shipping")
    );
}
