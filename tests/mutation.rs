use errspec::mutation::{self, Execution, Status};
use serde_json::{Value, json};

#[test]
fn passing_cases_do_not_hide_baseline_module_failures() {
    let mut baseline:errspec::mutation::Execution=serde_json::from_value(json!({"selected":[],"tests":[{"id":"pass","taskId":"1","file":"passing.test.ts","name":"passes","status":"passed","started":true,"duration":0,"errors":[],"hits":{}}],"failedModules":["missing.test.ts"],"errors":["Cannot resolve missing-dependency"],"staticHits":{},"hits":{},"wallTimeout":false})).unwrap();
    let message =
        errspec::mutation::validate_baseline(&baseline, "Unmodified baseline").unwrap_err();
    assert!(message.contains("missing.test.ts"));
    assert!(message.contains("Cannot resolve missing-dependency"));
    baseline.failed_modules.clear();
    baseline.errors.clear();
    assert!(errspec::mutation::validate_baseline(&baseline, "Unmodified baseline").is_ok());
    baseline.tests.clear();
    assert!(
        errspec::mutation::validate_baseline(&baseline, "Unmodified baseline")
            .unwrap_err()
            .contains("No passing test")
    );
}
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn generate(source: &str) -> Value {
    generate_options(source, json!({}))
}

fn generate_options(source: &str, options: Value) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_errspec"))
        .arg("__mutation-source")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut request = json!({"file":"src/rules.ts","source":source});
    request
        .as_object_mut()
        .unwrap()
        .extend(options.as_object().unwrap().clone());
    child
        .stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
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
fn comment_directives_select_families_and_preserve_position_independent_ids() {
    let plain =
        generate("export const before=true;\nexport const hidden=true;\nexport const after=true;");
    for prefix in ["errspec", "mutator"] {
        let marked = generate(&format!(
            "export const before=true;\n// {prefix}-disable-next-line BooleanLiteral: specified behavior\nexport const hidden=true;\nexport const after=true;"
        ));
        let candidates = marked["candidates"].as_array().unwrap();
        assert_eq!(candidates.len(), 3);
        assert_eq!(candidates[0]["ignored"], Value::Null);
        assert_eq!(candidates[1]["ignored"], "disabled: specified behavior");
        assert_eq!(candidates[2]["ignored"], Value::Null);
        for (before, after) in plain["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .zip(candidates)
        {
            assert_eq!(before["key"], after["key"]);
        }
    }
    let families = generate(
        "// errspec-disable-next-line ArithmeticOperator, BooleanLiteral: intentional\nexport const value=true ? 1+2 : 3;",
    );
    for candidate in families["candidates"].as_array().unwrap() {
        assert_eq!(
            candidate["ignored"].is_string(),
            ["ArithmeticOperator", "BooleanLiteral"]
                .contains(&candidate["mutator"].as_str().unwrap())
        );
    }
}

#[test]
fn stryker_regions_and_restores_are_family_specific_and_line_overrides_remain_explicit() {
    let result = generate(
        "/* Stryker disable all: fixed */\nconst a=true;\n// Stryker restore BooleanLiteral\nconst b=true;\nconst s='text';\n// errspec-enable all\nconst c=true; // mutator-disable-line BooleanLiteral: line only\nconst d=true;",
    );
    let candidates = result["candidates"].as_array().unwrap();
    let reasons = candidates
        .iter()
        .map(|candidate| candidate["ignored"].clone())
        .collect::<Vec<_>>();
    assert_eq!(
        reasons,
        vec![
            json!("disabled: fixed"),
            Value::Null,
            json!("disabled: fixed"),
            json!("disabled: line only"),
            Value::Null
        ]
    );
    let next = generate(
        "// Stryker disable next-line BooleanLiteral: intended\nconst a=true;\nconst b=true;",
    );
    assert_eq!(next["candidates"][0]["ignored"], "disabled: intended");
    assert_eq!(next["candidates"][1]["ignored"], Value::Null);
}

#[test]
fn block_trailing_and_embedded_comments_follow_javascript_line_boundaries() {
    let result = generate(
        "/* 日本🧪\r\n * errspec-disable-next-line BooleanLiteral: multiline\r\n */\r\nconst a=true;\r\nconst b=true;",
    );
    // A description before the directive is not itself a directive.
    assert!(
        result["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .all(|candidate| candidate["ignored"].is_null())
    );
    for newline in ["\n", "\r", "\r\n", "\u{2028}", "\u{2029}"] {
        let source = format!(
            "/* errspec-disable-next-line BooleanLiteral:{newline} Unicode reason */{newline}const a=true;{newline}const b=true; // errspec-disable-line BooleanLiteral: trailing"
        );
        let result = generate(&source);
        assert_eq!(
            result["candidates"][0]["ignored"],
            "disabled: Unicode reason"
        );
        assert_eq!(result["candidates"][1]["ignored"], "disabled: trailing");
    }
    let inline = generate(
        "const a=true && /* errspec-disable BooleanLiteral: inline */ false; /* errspec-enable all */ const b=true;",
    );
    let booleans = inline["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|candidate| candidate["mutator"] == "BooleanLiteral")
        .collect::<Vec<_>>();
    assert_eq!(booleans[0]["ignored"], Value::Null);
    assert_eq!(booleans[1]["ignored"], "disabled: inline");
    assert_eq!(booleans[2]["ignored"], Value::Null);
}

#[test]
fn directive_text_inside_literals_regex_templates_and_jsx_cannot_exclude_code() {
    let source = "const a='// errspec-disable all';const b=`\n// mutator-disable all\n${true}`;const re=/[/* Stryker disable all */]/;const jsx=<div>\n// errspec-disable all\n</div>;export const flag=true;";
    let result = generate_options(source, json!({"file":"src/rules.tsx"}));
    assert!(
        result["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|candidate| candidate["mutator"] == "BooleanLiteral")
            .all(|candidate| candidate["ignored"].is_null())
    );
    let malformed = generate(
        "// errspec-disable-next-line BooleanLiteral-nope\nconst a=true;\n// Stryker disable\nconst b=true;",
    );
    assert!(
        malformed["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .all(|candidate| candidate["ignored"].is_null())
    );
    let expression = generate(
        "const text=`raw ${ /* errspec-disable-line BooleanLiteral: embedded expression */ true } tail`;\nconst enabled=true;",
    );
    let booleans = expression["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|candidate| candidate["mutator"] == "BooleanLiteral")
        .collect::<Vec<_>>();
    assert_eq!(booleans[0]["ignored"], "disabled: embedded expression");
    assert_eq!(booleans[1]["ignored"], Value::Null);
}

#[test]
fn owned_syntax_facts_keep_scope_hashes_stable_across_position_changes() {
    let source = "export function a(x:number){return x+1;}\nexport function b(){return 1;}";
    let original = generate_options(source, json!({"facts":true}));
    let moved = generate_options(
        "// moved\n\nexport function a(x: number) { return x + 1; }\nexport function b(){return 1;}",
        json!({"facts":true}),
    );
    let changed = generate_options(
        &source.replace("b(){return 1", "b(){return 2"),
        json!({"facts":true}),
    );
    let a = "module/FunctionDeclaration:a#0";
    let b = "module/FunctionDeclaration:b#0";
    assert_eq!(original["metadata"]["version"], 1);
    assert_eq!(
        original["metadata"]["scopes"][a]["hash"],
        moved["metadata"]["scopes"][a]["hash"]
    );
    assert_eq!(
        original["metadata"]["scopes"][a]["hash"],
        changed["metadata"]["scopes"][a]["hash"]
    );
    assert_ne!(
        original["metadata"]["scopes"][b]["hash"],
        changed["metadata"]["scopes"][b]["hash"]
    );
    assert_eq!(
        original["metadata"]["scopes"]["module"]["effectHash"],
        changed["metadata"]["scopes"]["module"]["effectHash"]
    );
    assert_ne!(
        original["metadata"]["scopes"][a]["start"],
        moved["metadata"]["scopes"][a]["start"]
    );
    for node in original["metadata"]["nodes"].as_array().unwrap() {
        assert!(node["start"].as_u64().unwrap() <= node["end"].as_u64().unwrap());
        assert!(node["end"].as_u64().unwrap() <= source.len() as u64);
    }
    let jsx = generate_options(
        "export function view(){return <div>Hello</div>;}",
        json!({"file":"src/view.tsx","facts":true}),
    );
    let changed_jsx = generate_options(
        "export function view(){return <div>World</div>;}",
        json!({"file":"src/view.tsx","facts":true}),
    );
    assert_ne!(
        jsx["metadata"]["scopes"]["module/FunctionDeclaration:view#0"]["hash"],
        changed_jsx["metadata"]["scopes"]["module/FunctionDeclaration:view#0"]["hash"]
    );
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
        reason: None,
        accepted: None,
        reused: false,
        critical: false,
        infected_by: None,
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
