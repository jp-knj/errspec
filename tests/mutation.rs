use errspec::mutation::{self, Execution, Status};
use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn generate(source: &str) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_errspec"))
        .arg("__mutation-source")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            json!({"file":"src/rules.ts","source":source})
                .to_string()
                .as_bytes(),
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn all_builtin_families_have_owned_syntax_candidates() {
    let source = r#"import { x } from "package";
type Label = "not runtime";
declare const ambient: { readonly x: true };
export function sample(a: number, b: number): number {
  let n = a + b;
  n += 1;
  const arr = [n];
  const arrow = (x: number) => x * 2;
  if (n >= 18 && true) { console.log("reached"); }
  const obj = { value: n };
  const value = obj?.value;
  const regex = /^\d{2,4}$/i;
  const method = arr.filter(x => !x);
  const negative = -n;
  n++;
  return n;
}
"#;
    let result = generate(source);
    let candidates = result["candidates"].as_array().unwrap();
    for family in mutation::MUTATORS {
        assert!(
            candidates.iter().any(|c| c["mutator"] == family),
            "Missing {family}"
        );
    }
    assert!(
        !candidates
            .iter()
            .any(|c| c["original"] == "\"not runtime\"" || c["original"] == "\"package\"")
    );
    assert!(candidates.iter().all(|c| c["ignored"].is_null()));
    insta::assert_json_snapshot!(result);
}

#[test]
fn identity_is_independent_of_trivia_and_unrelated_named_declarations() {
    let source = "export function f(a:number){return a + 1;}";
    let first = generate(source);
    let second = generate(&format!(
        "// 日本語と emoji 🧪\nconst unrelated=1;\n{source}"
    ));
    let keys = |v: &Value| {
        v["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["mutator"] == "ArithmeticOperator")
            .map(|c| c["key"].clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(keys(&first), keys(&second));
    let candidate = second["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["mutator"] == "ArithmeticOperator")
        .unwrap();
    let text = format!("// 日本語と emoji 🧪\nconst unrelated=1;\n{source}");
    assert_eq!(
        &text[candidate["byteOffset"].as_u64().unwrap() as usize
            ..candidate["end"].as_u64().unwrap() as usize],
        "a + 1"
    );
    assert_eq!(candidate["line"], 3);
}

#[test]
fn explicit_return_types_and_unsafe_blocks_are_distinguished() {
    let result = generate(
        r#"
export async function a(): Promise<boolean> { return true; }
export function b(): Promise<number> { return Promise.resolve(2); }
export function c(): readonly string[] { return ["x"]; }
export function d(): (boolean | null) { return null; }
export function predicate(x: unknown): x is string { return true; }
export function* generator(): number { yield 1; }
class Base {} class Derived extends Base { constructor(){super();} get x(){return 1;} }
"#,
    );
    let candidates = result["candidates"].as_array().unwrap();
    assert!(candidates.iter().any(
        |c| c["mutator"] == "FnValue" && c["replacement"] == "{ return Promise.resolve(-1); }"
    ));
    assert!(
        candidates
            .iter()
            .any(|c| c["mutator"] == "FnValue" && c["replacement"] == "{ return []; }")
    );
    assert!(candidates.iter().any(|c| c["ignored"].is_string()));
    insta::assert_json_snapshot!(result);
}

#[test]
fn return_type_syntax_is_read_from_the_ast_including_trivia_and_nested_unions() {
    let result = generate(
        r#"
export async function promised(): (Promise /* comment */ < boolean >) { return true; }
export function union(): (number) | (null) { return 2; }
export function quoted(): 'a | b' { return 'different'; }
export function array(): ReadonlyArray /* comment */ <string> { return ['x']; }
export function same(): 'xyzzy' { return 'xyzzy'; }
export function assertion(value: unknown): asserts value is string { throw 1; }
"#,
    );
    let candidates = result["candidates"].as_array().unwrap();
    assert!(candidates.iter().any(|c| c["mutator"] == "FnValue"
        && c["original"] == "{ return true; }"
        && c["replacement"] == "{ return false; }"));
    assert!(
        candidates
            .iter()
            .any(|c| c["mutator"] == "FnValue" && c["replacement"] == "{ return null; }")
    );
    assert!(
        candidates
            .iter()
            .any(|c| c["mutator"] == "FnValue" && c["replacement"] == "{ return \"a | b\"; }")
    );
    assert!(
        candidates
            .iter()
            .any(|c| c["mutator"] == "FnValue" && c["replacement"] == "{ return []; }")
    );
    assert!(!candidates.iter().any(|c| c["mutator"] == "FnValue"
        && (c["original"] == "{ return 'xyzzy'; }" || c["original"] == "{ throw 1; }")));
}

fn execution(status: &str, errors: Vec<&str>) -> Execution {
    serde_json::from_value(json!({"selected":["test"],"tests":[{"id":"test","taskId":"task","file":"x.test.ts","name":"test","status":status,"started":true,"duration":1.0,"errors":errors,"hits":{}}],"failedModules":[],"errors":[],"staticHits":{},"hits":{},"wallTimeout":false})).unwrap()
}

#[test]
fn retry_judgments_never_count_an_incomplete_execution_as_survival() {
    let selected = vec!["test".into()];
    let pass = mutation::judge(&execution("passed", vec![]), &selected, 1);
    let killed = mutation::judge(
        &execution("failed", vec!["AssertionError: mismatch"]),
        &selected,
        1,
    );
    let timeout = mutation::judge(
        &execution("failed", vec!["Test timed out in 5000ms"]),
        &selected,
        1,
    );
    let mut absent = execution("skipped", vec![]);
    absent.tests.clear();
    let runtime = mutation::judge(&absent, &selected, 2);
    assert_eq!(pass.status, Status::Survived);
    assert_eq!(
        mutation::confirmed(&[timeout.clone(), runtime.clone()]),
        Status::Pending
    );
    assert_eq!(
        mutation::confirmed(&[runtime.clone(), runtime.clone()]),
        Status::RuntimeError
    );
    assert_eq!(
        mutation::confirmed(&[timeout.clone(), pass.clone()]),
        Status::Survived
    );
    assert_eq!(
        mutation::confirmed(&[timeout.clone(), killed.clone()]),
        Status::Killed
    );
    assert_eq!(
        mutation::confirmed(std::slice::from_ref(&timeout)),
        Status::Pending
    );
    let limit = mutation::judge(
        &execution("failed", vec!["ERRSPEC_HIT_LIMIT"]),
        &selected,
        1,
    );
    assert_eq!(
        mutation::confirmed(std::slice::from_ref(&limit)),
        Status::Timeout
    );
    insta::assert_json_snapshot!(json!([pass, killed, timeout, runtime, limit]));
}

#[test]
fn protocol_identity_is_stricter_than_missing_test_results() {
    let selected = vec!["test".into()];
    let mut result = execution("passed", vec![]);
    assert!(mutation::validate_execution(&result, &selected).is_ok());
    result.tests.clear();
    assert!(mutation::validate_execution(&result, &selected).is_ok());
    result.selected.clear();
    assert!(mutation::validate_execution(&result, &selected).is_err());
    let mut duplicate = execution("passed", vec![]);
    duplicate.tests.push(duplicate.tests[0].clone());
    assert!(mutation::validate_execution(&duplicate, &selected).is_err());
}

#[test]
fn score_and_gate_depend_on_completion() {
    let mut report = mutation::Report::new(true);
    let candidate: mutation::Candidate =
        serde_json::from_value(generate("export const f=()=>true;")["candidates"][0].clone())
            .unwrap();
    report.mutants.push(mutation::Mutant {
        candidate,
        status: Status::NoCoverage,
        covered_by: vec![],
        killed_by: vec![],
        failed_modules: vec![],
        attempts: vec![],
    });
    report.finish();
    assert!(report.summary.score.is_none());
    assert!(report.gate.passed.is_none());
    assert_eq!(report.exit_code(), 2);
    report.run_complete = true;
    report.finish();
    assert_eq!(report.gate.passed, Some(false));
    assert_eq!(report.summary.score, Some(0.0));
    assert_eq!(report.exit_code(), 1);
}
