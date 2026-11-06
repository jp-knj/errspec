use errspec::mutation::{self, Execution, Status};
use serde_json::{Value, json};

#[test]
fn test_recommendations_keep_shared_line_module_syntax_outside_function_cohorts() {
    use errspec::mutation::{
        Mutant,
        selection::{Changes, Lines},
        test_selection,
    };
    use std::collections::{BTreeMap, BTreeSet};
    let file = "src/rules.ts";
    let cases = [
        (
            "import {f} from './one'; export function a(){return f(1)}\nexport function b(){return f(2)}\n",
            "import {f} from './other'; export function a(){return f(1)}\nexport function b(){return f(2)}\n",
            Lines { start: 1, end: 1 },
            true,
        ),
        (
            "globalThis.convert=String; export function a(){return true}\nexport function b(){return globalThis.convert(1)}\n",
            "globalThis.convert=Boolean; export function a(){return true}\nexport function b(){return globalThis.convert(1)}\n",
            Lines { start: 1, end: 1 },
            true,
        ),
        (
            "export const a=()=>true,convert=String;\nexport function b(){return convert(1)}\n",
            "export const a=()=>true,convert=Boolean;\nexport function b(){return convert(1)}\n",
            Lines { start: 1, end: 1 },
            true,
        ),
        (
            "import {f} from './other';\nexport function a(){\nreturn f(1)\n}\nexport function b(){return f(2)}\n",
            "import {f} from './other';\nexport function a(){\nreturn f(2)\n}\nexport function b(){return f(2)}\n",
            Lines { start: 3, end: 3 },
            false,
        ),
        (
            "export const a=()=>true;\nexport const b=()=>true;\n",
            "export const a=()=>!false;\nexport const b=()=>true;\n",
            Lines { start: 1, end: 1 },
            false,
        ),
        (
            "export function a(){return true}\nconst convert=String;\nexport function b(){return convert(1)}\n",
            "export function a(){return true}\nconst convert=Boolean;\nexport function b(){return convert(1)}\n",
            Lines { start: 1, end: 2 },
            true,
        ),
        (
            "import './side'; export function a(){return true}\nexport function b(){return String(globalThis.flag)}\n",
            "export function a(){return true}\nexport function b(){return String(globalThis.flag)}\n",
            Lines { start: 1, end: 1 },
            true,
        ),
    ];
    for (before, source, range, module_changed) in cases {
        let generated = generate_options(source, json!({"facts":true}));
        let metadata: mutation::syntax::Metadata =
            serde_json::from_value(generated["metadata"].clone()).unwrap();
        let candidates: Vec<mutation::Candidate> =
            serde_json::from_value(generated["candidates"].clone()).unwrap();
        let mutants: Vec<_> = candidates
            .into_iter()
            .filter(|candidate| {
                metadata.scopes[&metadata.owners[&candidate.key]]
                    .name
                    .as_deref()
                    == Some("a")
            })
            .map(|candidate| Mutant {
                candidate,
                status: Status::Pending,
                covered_by: vec!["a.test.ts#a".into()],
                killed_by: vec![],
                failed_modules: vec![],
                attempts: vec![],
                reason: None,
                accepted: None,
                critical: false,
                reused: false,
                infected_by: None,
                matrix: None,
            })
            .collect();
        assert!(!mutants.is_empty());
        let changes = Changes {
            base: "base".into(),
            fingerprint: "fingerprint".into(),
            files: BTreeMap::from([(file.into(), vec![range])]),
            removals: BTreeSet::new(),
        };
        let metadata = BTreeMap::from([(file.into(), metadata)]);
        let sources = BTreeMap::from([(file.into(), source.into())]);
        let previous = BTreeMap::from([(
            file.into(),
            test_selection::PreviousSource {
                text: before.into(),
                metadata: serde_json::from_value(
                    generate_options(before, json!({"facts":true}))["metadata"].clone(),
                )
                .unwrap(),
            },
        )]);
        let fallback = test_selection::fallback(&changes, &mutants, &metadata, &sources, &previous);
        assert_eq!(
            fallback,
            module_changed.then(|| "Changed module-level source has no bounded function cohort; select every collected test file".into()),
            "{source}",
        );
        assert!(
            test_selection::fallback(&changes, &mutants, &metadata, &sources, &BTreeMap::new())
                .unwrap()
                .contains("unavailable"),
        );
    }
}

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
fn arid_excludes_logging_calls_and_compound_logging_only_statements_when_enabled() {
    for source in [
        "function logValue(){console.log('hello');}",
        "const report=()=>this.logger?.trace('hello');",
        "function report(n:number){if(n>1){logger.info('yes')}else{log.warn('no')}}",
        "function report(){debug('hello');service.trace('other');}",
        "function report(){data.logger.info('hello');}",
        "async function report(){await logger.info('hello');}",
    ] {
        let normal = generate(source);
        let arid = generate_options(source, json!({"arid":true}));
        let original = normal["candidates"].as_array().unwrap();
        let excluded = arid["candidates"].as_array().unwrap();
        assert!(!excluded.is_empty());
        assert!(
            excluded
                .iter()
                .all(|candidate| candidate["ignored"] == "arid: logging-only code"),
            "{source}: {arid}"
        );
        assert!(
            original
                .iter()
                .all(|candidate| candidate["ignored"].is_null())
        );
        assert_eq!(
            original
                .iter()
                .map(|candidate| &candidate["key"])
                .collect::<Vec<_>>(),
            excluded
                .iter()
                .map(|candidate| &candidate["key"])
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn arid_does_not_cross_mixed_statements_functions_computed_paths_or_casts() {
    let mixed = generate_options(
        "function run(flag:boolean){if(flag){logger.info('hello');updateState();}else{return 'value';}return flag;}console.log(()=>{const state=true;return state;});",
        json!({"arid":true}),
    );
    let candidates = mixed["candidates"].as_array().unwrap();
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate["mutator"] == "ConditionalExpression"
                && candidate["ignored"].is_null())
    );
    assert!(candidates.iter().any(
        |candidate| candidate["mutator"] == "BooleanLiteral" && candidate["ignored"].is_null()
    ));
    let update = candidates
        .iter()
        .find(|candidate| candidate["original"] == "updateState();")
        .unwrap();
    assert!(update["ignored"].is_null());
    for source in [
        "logger['info']('hello');",
        "(logger as any).debug('hello');",
        "catalog.log('hello');",
        "function empty(){}",
        "async function resolved(){(await logger).debug('hello');}",
    ] {
        let result = generate_options(source, json!({"arid":true}));
        assert!(
            result["candidates"]
                .as_array()
                .unwrap()
                .iter()
                .all(|candidate| candidate["ignored"].is_null()),
            "{source}: {result}"
        );
    }
}

#[test]
fn custom_arid_patterns_replace_defaults_and_explicit_comments_keep_precedence() {
    let source = "console.log('ordinary');metrics.record('suppressed');";
    let result = generate_options(source, json!({"arid":true,"aridCallees":["metrics.*"]}));
    for candidate in result["candidates"].as_array().unwrap() {
        let position = candidate["byteOffset"].as_u64().unwrap() as usize;
        assert_eq!(
            candidate["ignored"].is_string(),
            position >= source.find("metrics").unwrap()
        );
    }
    let empty = generate_options(source, json!({"arid":true,"aridCallees":[]}));
    assert!(
        empty["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .all(|candidate| candidate["ignored"].is_null())
    );
    let commented = generate_options(
        "// errspec-disable-next-line: intentional trace\nconsole.log('hi');",
        json!({"arid":true}),
    );
    assert!(
        commented["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .all(|candidate| candidate["ignored"] == "disabled: intentional trace")
    );
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
    assert_eq!(original["metadata"]["version"], 2);
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
fn unowned_anonymous_default_function_bodies_remain_module_inputs() {
    for declaration in [
        "export default function",
        "export default async function",
        "export default function*",
    ] {
        let source = format!("{declaration}(){{const value=1;return value-value;}}");
        let before = generate_options(&source, json!({"facts":true}));
        let after = generate_options(&source.replace("value=1", "value=0"), json!({"facts":true}));
        assert_ne!(
            before["metadata"]["scopes"]["module"]["effectHash"],
            after["metadata"]["scopes"]["module"]["effectHash"],
            "Anonymous bodies have no independently tracked function owner: {declaration}"
        );
    }
}

#[test]
fn runtime_calls_and_import_bindings_are_acquired_without_inventing_dynamic_targets() {
    let source = "import defaultFn, {foo as local, type OnlyType, 'literal-name' as literal} from './de\\u0070';\nimport * as namespace from './other';import type TypeOnly from './types';import './effect';\nexport function outer(parameter:()=>number){const nested=()=>local();return defaultFn()+namespace.method()+this.method()+parameter()+handlers[index]()+literal()+(local as ()=>number)();}";
    let result = generate_options(source, json!({"facts":true}));
    let facts: errspec::mutation::syntax::Metadata =
        serde_json::from_value(result["metadata"].clone()).unwrap();
    let candidates: Vec<mutation::Candidate> =
        serde_json::from_value(result["candidates"].clone()).unwrap();
    errspec::mutation::selection::validate(&facts, source, &candidates).unwrap();
    let imports = facts
        .imports
        .iter()
        .map(|value| {
            (
                value.source.as_str(),
                value.imported.as_str(),
                value.local.as_str(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        imports,
        vec![
            ("./dep", "default", "defaultFn"),
            ("./dep", "foo", "local"),
            ("./dep", "literal-name", "literal"),
            ("./other", "*", "namespace")
        ]
    );
    let calls = facts
        .calls
        .iter()
        .map(|call| call.callee.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(
        calls,
        vec![
            Some("local"),
            Some("defaultFn"),
            Some("namespace.method"),
            Some("this.method"),
            Some("parameter"),
            None,
            Some("literal"),
            Some("local")
        ]
    );
    assert!(facts.calls[0].scope.contains("VariableDeclaration:nested"));
    assert!(
        facts.calls[1..]
            .iter()
            .all(|call| call.scope.ends_with("FunctionDeclaration:outer#0"))
    );
    let mut invalid = facts.clone();
    invalid.calls[0].scope = "missing".into();
    assert!(errspec::mutation::selection::validate(&invalid, source, &candidates).is_err());
    let mut duplicate = facts.clone();
    duplicate.calls.push(facts.calls[0].clone());
    assert!(errspec::mutation::selection::validate(&duplicate, source, &candidates).is_err());
}

#[test]
fn static_call_relationships_follow_imports_methods_and_direction_without_joining_sibling_callers()
{
    use errspec::mutation::{
        callgraph::Graph,
        syntax::{Metadata, scope_key},
    };
    use std::collections::BTreeMap;
    fn facts(source: &str) -> Metadata {
        serde_json::from_value(generate_options(source, json!({"facts":true}))["metadata"].clone())
            .unwrap()
    }
    fn owner(file: &str, metadata: &Metadata, name: &str) -> String {
        let key = metadata
            .scopes
            .iter()
            .find(|(_, scope)| scope.function && scope.name.as_deref() == Some(name))
            .unwrap()
            .0;
        scope_key(file, key)
    }
    let mut a = facts(
        "import defaultFn,{f as alias} from './b';import * as ns from './b';export function left(){return alias();}export function sibling(){return alias();}export function namespaced(){return ns.f();}export function fromDefault(){return defaultFn();}export function shadow(alias:()=>number){return alias();}export function separate(){return 1;}export const obj={a(){return 1},b(){return this.a();}};export function member(){return obj.b();}",
    );
    let b = facts(
        "export default function main(){return 1;}export function f(){return 1;}export function separateB(){return 1;}",
    );
    for binding in &mut a.imports {
        binding.resolved = Some("src/b.ts".into());
    }
    let graph = Graph::build(&BTreeMap::from([
        ("src/a.ts".into(), &a),
        ("src/b.ts".into(), &b),
    ]))
    .unwrap();
    let left = owner("src/a.ts", &a, "left");
    let sibling = owner("src/a.ts", &a, "sibling");
    let f = owner("src/b.ts", &b, "f");
    assert!(graph.related(&left, &f));
    assert!(graph.related(&f, &left));
    assert!(
        !graph.related(&left, &sibling),
        "A common callee is not a directed path between its callers"
    );
    assert!(graph.related(&owner("src/a.ts", &a, "namespaced"), &f));
    let default = owner("src/a.ts", &a, "fromDefault");
    assert!(graph.related(&default, &owner("src/b.ts", &b, "main")));
    assert!(!graph.related(&default, &owner("src/b.ts", &b, "separateB")));
    assert!(graph.related(
        &owner("src/a.ts", &a, "member"),
        &owner("src/a.ts", &a, "a")
    ));
    assert!(!graph.related(&left, &owner("src/a.ts", &a, "separate")));
    assert!(
        graph.related(
            &owner("src/a.ts", &a, "shadow"),
            &owner("src/b.ts", &b, "separateB")
        ),
        "Unknown parameter calls cannot establish independence"
    );
    let restored: Graph = serde_json::from_value(serde_json::to_value(&graph).unwrap()).unwrap();
    assert!(restored.valid());
    assert!(!restored.related(&left, &sibling));
    a.imports[1].resolved = None;
    let unavailable = Graph::build(&BTreeMap::from([
        ("src/a.ts".into(), &a),
        ("src/b.ts".into(), &b),
    ]))
    .unwrap();
    assert!(unavailable.related(&left, &owner("src/b.ts", &b, "separateB")));
}

#[test]
fn scope_entries_identify_the_direct_body_and_exclude_ignored_or_removed_guards() {
    let source = "export function outer(n:number){const callback=()=>true;return callback();}\nexport const arrow=(n:number)=>n+1;";
    let generated = generate_options(source, json!({"facts":true}));
    let metadata: mutation::syntax::Metadata =
        serde_json::from_value(generated["metadata"].clone()).unwrap();
    let candidates: Vec<mutation::Candidate> =
        serde_json::from_value(generated["candidates"].clone()).unwrap();
    mutation::selection::validate(&metadata, source, &candidates).unwrap();
    for (owner, scope) in &metadata.scopes {
        if !scope.function {
            continue;
        }
        let entry = scope.entry_key.as_ref().unwrap();
        let candidate = candidates
            .iter()
            .find(|candidate| &candidate.key == entry)
            .unwrap();
        let body = mutation::syntax::function_body(&metadata, scope).unwrap();
        assert_eq!(candidate.location.byte_offset as usize, body.start);
        assert_eq!(candidate.end as usize, body.end);
        assert_eq!(metadata.owners[entry], *owner);
    }
    let outer = &metadata.scopes["module/FunctionDeclaration:outer#0"];
    let entry = candidates
        .iter()
        .find(|candidate| Some(&candidate.key) == outer.entry_key.as_ref())
        .unwrap();
    assert!(entry.original.contains("return callback()"));
    let removed = generate_options(
        source,
        json!({"facts":true,"excludeMutators":["BlockStatement","ArrowFunction"]}),
    );
    assert!(
        removed["metadata"]["scopes"]
            .as_object()
            .unwrap()
            .values()
            .all(|scope| scope.get("entryKey").is_none())
    );
    let ignored = generate_options(
        "// errspec-disable-next-line\nexport function ignored(){return true;}",
        json!({"facts":true}),
    );
    assert!(
        ignored["metadata"]["scopes"]
            .as_object()
            .unwrap()
            .values()
            .all(|scope| scope.get("entryKey").is_none())
    );
}

#[test]
fn scope_entry_validation_rejects_cross_owner_keys_and_broken_syntax_graphs() {
    let source = "export const a=()=>true;export const b=()=>false;";
    let generated = generate_options(source, json!({"facts":true}));
    let original: mutation::syntax::Metadata =
        serde_json::from_value(generated["metadata"].clone()).unwrap();
    let candidates: Vec<mutation::Candidate> =
        serde_json::from_value(generated["candidates"].clone()).unwrap();
    let changed = source.replacen("const a", "const c", 1);
    assert!(!mutation::syntax::source_matches(&original, &changed));
    assert!(mutation::selection::validate(&original, &changed, &candidates).is_err());
    let mut wrong = original.clone();
    wrong
        .scopes
        .get_mut("module/VariableDeclaration:a#0")
        .unwrap()
        .entry_key = original.scopes["module/VariableDeclaration:b#0"]
        .entry_key
        .clone();
    assert!(mutation::selection::validate(&wrong, source, &candidates).is_err());
    let mut cycle = original.clone();
    cycle.nodes[0].children.push(0);
    assert!(mutation::selection::validate(&cycle, source, &candidates).is_err());
    let mut detached = original.clone();
    detached.nodes[0].children.clear();
    assert!(mutation::selection::validate(&detached, source, &candidates).is_err());
    let mut hash = original;
    hash.scopes.get_mut("module").unwrap().hash = "unknown".into();
    assert!(mutation::selection::validate(&hash, source, &candidates).is_err());
}

#[test]
fn full_scope_probe_binds_static_entries_and_promotes_local_candidates_to_their_function() {
    let source = "export function outer(n:number){const local=true;return n+1;}\nexport const arrow=()=>false;";
    let generated = generate_options(source, json!({"facts":true}));
    let metadata: mutation::syntax::Metadata =
        serde_json::from_value(generated["metadata"].clone()).unwrap();
    let candidates: Vec<mutation::Candidate> =
        serde_json::from_value(generated["candidates"].clone()).unwrap();
    let outer = metadata.scopes["module/FunctionDeclaration:outer#0"]
        .entry_key
        .as_ref()
        .unwrap();
    let arrow = metadata.scopes["module/VariableDeclaration:arrow#0"]
        .entry_key
        .as_ref()
        .unwrap();
    let probe:Execution=serde_json::from_value(json!({"selected":[],"tests":[{"id":"test-a","taskId":"current","file":"a.test.ts","name":"static","status":"passed","started":true,"duration":1,"errors":[],"hits":{}},{"id":"test-b","taskId":"other","file":"b.test.ts","name":"arrow","status":"passed","started":true,"duration":1,"errors":[],"hits":{arrow:1}}],"hits":{outer:1,arrow:1},"staticHits":{outer:["a.test.ts"]},"failedModules":[],"errors":[],"wallTimeout":false})).unwrap();
    let sources = std::collections::BTreeMap::from([(
        "src/rules.ts".into(),
        mutation::scope_cache::Source {
            text: source,
            metadata: &metadata,
            candidates: &candidates,
        },
    )]);
    let context =
        mutation::scope_cache::from_probe("f".repeat(64), &sources, &probe, true).unwrap();
    let outer_owner = &context.candidates[outer];
    let local = candidates
        .iter()
        .find(|candidate| candidate.original == "true")
        .unwrap();
    assert_eq!(&context.candidates[&local.key], outer_owner);
    assert_eq!(
        context.scopes[outer_owner]
            .observation
            .as_ref()
            .unwrap()
            .files,
        std::collections::BTreeSet::from(["a.test.ts".into()])
    );
    assert_eq!(context.scopes.len(), 3, "module and two functions only");
    let mut unknown = probe.clone();
    unknown.tests[0].infected.insert("unknown".into());
    assert!(mutation::scope_cache::from_probe("f".repeat(64), &sources, &unknown, true).is_err());
    let mut unattributed = probe;
    unattributed.static_hits.clear();
    let context =
        mutation::scope_cache::from_probe("f".repeat(64), &sources, &unattributed, true).unwrap();
    assert!(
        context.scopes[outer_owner].observation.is_none(),
        "a hit without attribution is unknown, not unused"
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
fn runner_deadlines_stay_incomplete_while_assertion_and_locator_failures_remain_kills() {
    let selected = vec!["test".into()];
    let inputs = [
        ("Error: Exceeded timeout of 40 ms for a test.", true),
        ("Error: Exceeded timeout of 40 ms for a hook.", true),
        ("Test timeout of 1500ms exceeded.", true),
        (
            "Error: Timed out 5000ms waiting for expect(locator).toHaveText",
            false,
        ),
        ("TimeoutError: locator.click: Timeout 30ms exceeded.", false),
        ("AssertionError: timeout value was incorrect", false),
    ];
    let results: Vec<_> = inputs
        .into_iter()
        .map(|(error, deadline)| {
            let result = execution("failed", vec![error]);
            let attempt = mutation::judge(&result, &selected, 1);
            let observation = mutation::kill_matrix::Observation::acquire(&result, &selected);
            assert_eq!(
                attempt.status,
                if deadline {
                    Status::Timeout
                } else {
                    Status::Killed
                }
            );
            assert_eq!(observation.complete, !deadline);
            assert_eq!(observation.killed_by.is_empty(), deadline);
            json!({"error":error,"attempt":attempt,"matrix":observation})
        })
        .collect();
    insta::assert_json_snapshot!(results);
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
        matrix: None,
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

#[test]
fn runtime_dependency_facts_distinguish_syntax_from_type_imports_and_literal_text() {
    let source = r#"
import type { Type } from './type-only.ts';
import { value } from './value.ts';
import './effect.ts';
export { value as forwarded } from './reexport.ts';
export type { Type as ForwardedType } from './export-type.ts';
const decoy = "import './literal.ts'";
type Derived = import('./type-query.ts').Type;
const load = () => import('./dynamic.ts');
const common = require('./common.cjs');
const unknown = import(decoy);
"#;
    let generated = generate_options(source, json!({"validateOnly":true,"facts":true}));
    let facts = json!({"dependencies":generated["metadata"]["dependencies"],"candidates":generated["candidates"]});
    assert_eq!(
        facts["dependencies"],
        json!([
            "./common.cjs",
            "./dynamic.ts",
            "./effect.ts",
            "./reexport.ts",
            "./value.ts"
        ])
    );
    assert_eq!(facts["candidates"], json!([]));
    insta::assert_json_snapshot!("runtime_dependency_facts", facts);
    assert!(generate(source).get("metadata").is_none());
}

#[test]
fn child_process_facts_require_an_actual_module_and_started_test() {
    let mut execution:Execution=serde_json::from_value(json!({"selected":[],"tests":[{"id":"t","taskId":"1","file":"a.test.ts","name":"runs","status":"passed","started":true,"duration":0,"errors":[],"hits":{}}],"failedModules":[],"errors":[],"staticHits":{},"hits":{},"wallTimeout":false,"childProcesses":[{"test":"t","file":"a.test.ts","executable":"node","entries":["src/child.ts"],"method":"spawnSync"}]})).unwrap();
    assert!(mutation::validate_execution(&execution, &[]).is_ok());
    execution.child_processes[0].test = Some("unknown".into());
    assert!(mutation::validate_execution(&execution, &[]).is_err());
    execution.child_processes[0].test = None;
    execution.child_processes[0].entries = vec!["../outside.ts".into()];
    assert!(mutation::validate_execution(&execution, &[]).is_err());
}
