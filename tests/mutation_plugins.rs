use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn generate(source: &str, options: Value) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_errspec"))
        .arg("__mutation-source")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut request = json!({"file":"src/plugin.ts","source":source});
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
    child.wait_with_output().unwrap()
}

#[test]
fn template_plugin_nodes_preserve_literal_text_and_expression_comments() {
    let source = r"export const text = tag`https://\${literal}/${host /* } */}//${value}/*tail*/`;";
    let output = generate(source, json!({"facts":true}));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    insta::assert_json_snapshot!("template_plugin_nodes", result["metadata"]["nodes"]);
}

#[test]
fn template_plugin_tokens_preserve_escaped_delimiters_and_nested_templates() {
    for (template, expected) in [
        (r"`https://${host}/path`", vec!["`https://${", "}/path`"]),
        (r"`/* ${value} */`", vec!["`/* ${", "} */`"]),
        (
            r"`\${literal} ${value}\`tail`",
            vec![r"`\${literal} ${", r"}\`tail`"],
        ),
        (r"`${first}${second}`", vec!["`${", "}${", "}`"]),
        (
            r"`あ${first}\${literal} /* ${second}終`",
            vec!["`あ${", r"}\${literal} /* ${", "}終`"],
        ),
        (
            r"`outer${`https://${host}`}tail`",
            vec!["`outer${", "`https://${", "}`", "}tail`"],
        ),
        (r"tag`\unicode${value}`", vec![r"`\unicode${", "}`"]),
    ] {
        let source = format!("export const text = {template};");
        let output = generate(&source, json!({"facts":true}));
        assert!(
            output.status.success(),
            "{template}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        let tokens: Vec<_> = result["metadata"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|node| {
                matches!(
                    node["kind"].as_str(),
                    Some("TemplateHead" | "TemplateMiddle" | "TemplateTail")
                )
            })
            .map(|node| {
                &source[node["start"].as_u64().unwrap() as usize
                    ..node["end"].as_u64().unwrap() as usize]
            })
            .collect();
        assert_eq!(tokens, expected, "{template}");
    }
}

#[test]
fn line_comments_end_at_every_ecmascript_line_terminator() {
    for terminator in ["\n", "\r", "\r\n", "\u{2028}", "\u{2029}"] {
        let expression = format!("1 // ignored +{terminator} /* ignored +{terminator} */ + 2");
        let source = format!("// lead{terminator}export const n = {expression};");
        for facts in [false, true] {
            let output = generate(&source, json!({"facts":facts}));
            assert!(
                output.status.success(),
                "{terminator:?}, facts={facts}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let result: Value = serde_json::from_slice(&output.stdout).unwrap();
            let candidates = result["candidates"].as_array().unwrap();
            assert_eq!(candidates.len(), 1);
            assert_eq!(candidates[0]["mutator"], "ArithmeticOperator");
            assert_eq!(candidates[0]["original"], expression);
            assert_eq!(
                candidates[0]["replacement"],
                expression.replace("*/ +", "*/ -")
            );
            assert_eq!(candidates[0]["byteOffset"], source.find('1').unwrap());
            assert_eq!(candidates[0]["end"], source.len() - 1);
            if facts {
                let nodes = result["metadata"]["nodes"].as_array().unwrap();
                assert_eq!(nodes[0]["start"], source.find("export").unwrap());
                let operator = nodes
                    .iter()
                    .find(|node| node["kind"] == "PlusToken")
                    .unwrap();
                assert_eq!(operator["start"], source.rfind('+').unwrap());
                assert_eq!(operator["end"], source.rfind('+').unwrap() + 1);
            }
        }
    }
}

#[test]
fn native_plugin_candidates_keep_owned_ids_locations_and_exclusion_reasons() {
    let source =
        "// errspec-disable-next-line Double: protocol constant\nexport const value=()=>2;";
    let start = source.find('2').unwrap();
    let options = json!({"facts":true,"customMutators":["Double"],"pluginEdits":[{"mutator":"Double","anchorStart":start,"anchorEnd":start+1,"anchorKind":"NumericLiteral","start":start,"end":start+1,"replacement":"4"}]});
    let output = generate(source, options.clone());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    insta::assert_json_snapshot!(result["candidates"]);
    let custom = result["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|candidate| candidate["mutator"] == "Double")
        .unwrap();
    assert_eq!(custom["ignored"], "disabled: protocol constant");
    assert!(
        result["metadata"]["owners"]
            .get(custom["key"].as_str().unwrap())
            .is_some()
    );
    let mut changed = options;
    changed["excludeMutators"] = json!(["Double"]);
    let excluded: Value = serde_json::from_slice(&generate(source, changed).stdout).unwrap();
    assert_eq!(
        excluded["candidates"].as_array().unwrap().len() + 1,
        result["candidates"].as_array().unwrap().len()
    );
}

#[test]
fn plugin_replacements_cannot_change_function_strictness_or_target_erased_types() {
    let source = "function value(){'use strict';return this;}";
    let start = source.find('{').unwrap();
    let result = generate(
        source,
        json!({"customMutators":["Body"],"pluginEdits":[{"mutator":"Body","anchorStart":start,"anchorEnd":source.len(),"anchorKind":"Block","start":start,"end":source.len(),"replacement":"{return this;}"}]}),
    );
    assert!(result.status.success());
    let result: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(
        result["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|candidate| candidate["mutator"] == "Body")
            .unwrap()["ignored"],
        "Plugin body replacement would change a directive prologue"
    );
    let source = "type Only = 'literal';";
    let start = source.find('\'').unwrap();
    let output = generate(
        source,
        json!({"customMutators":["Type"],"pluginEdits":[{"mutator":"Type","anchorStart":start,"anchorEnd":start+9,"anchorKind":"StringLiteral","start":start,"end":start+9,"replacement":"'other'"}]}),
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Type"));
}

#[test]
fn dynamic_import_plugin_anchors_and_runtime_dependency_facts_are_preserved() {
    let source = "import type {T} from './types'; import main, {type U, value as local} from './mod'; export * from './other'; const lazy = import('./lazy'); require('./cjs');";
    let start = source.find("import('./lazy')").unwrap();
    let end = start + "import('./lazy')".len();
    let output = generate(
        source,
        json!({"facts":true,"customMutators":["Load"],"pluginEdits":[{"mutator":"Load","anchorKind":"CallExpression","anchorStart":start,"anchorEnd":end,"start":start,"end":end,"replacement":"Promise.resolve({})"}]}),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        result["metadata"]["dependencies"],
        json!(["./cjs", "./lazy", "./mod", "./other"])
    );
    let imports = result["metadata"]["imports"].as_array().unwrap();
    assert_eq!(imports.len(), 2);
    assert_eq!(
        (&imports[0]["imported"], &imports[0]["local"]),
        (&json!("default"), &json!("main"))
    );
    assert_eq!(
        (&imports[1]["imported"], &imports[1]["local"]),
        (&json!("value"), &json!("local"))
    );
    let nodes = result["metadata"]["nodes"].as_array().unwrap();
    let call = nodes
        .iter()
        .find(|node| node["kind"] == "CallExpression" && node["start"] == start)
        .unwrap();
    assert_eq!(call["children"].as_array().unwrap().len(), 2);
    assert!(
        result["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["mutator"] == "Load"
                && c["original"] == "import('./lazy')"
                && c["ignored"].is_null())
    );
}

#[test]
fn import_clause_names_ranges_and_ignorer_anchors_keep_the_plugin_contract() {
    let source = "import type {T} from './types'; import main, {type U, value as local} from './mod'; import * as ns from './ns'; import {} from './empty'; import defer * as later from './later';";
    let output = generate(source, json!({"facts":true}));
    assert!(output.status.success());
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    let nodes = result["metadata"]["nodes"].as_array().unwrap();
    let clauses: Vec<_> = nodes
        .iter()
        .filter(|node| node["kind"] == "ImportClause")
        .collect();
    assert_eq!(clauses.len(), 5);
    let raw = |node: &Value| {
        &source[node["start"].as_u64().unwrap() as usize..node["end"].as_u64().unwrap() as usize]
    };
    assert_eq!(raw(clauses[0]), "type {T}");
    assert_eq!(raw(clauses[1]), "main, {type U, value as local}");
    assert_eq!(clauses[1]["name"], "main");
    assert_eq!(raw(clauses[2]), "* as ns");
    assert_eq!(
        nodes
            .iter()
            .find(|node| node["kind"] == "NamespaceImport")
            .unwrap()["name"],
        "ns"
    );
    assert_eq!(raw(clauses[3]), "{}");
    assert_eq!(raw(clauses[4]), "defer * as later");
    let named: Vec<_> = nodes
        .iter()
        .filter(|node| node["kind"] == "NamedImports")
        .map(raw)
        .collect();
    assert_eq!(named, ["{T}", "{type U, value as local}", "{}"]);
    let output = generate(
        source,
        json!({"pluginIgnores":[{"start":clauses[1]["start"],"end":clauses[1]["end"],"reason":"import configuration"}]}),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn switching_type_only_imports_or_exports_invalidates_module_effects() {
    for (before, after) in [
        (
            "import type {T} from './module';",
            "import {T} from './module';",
        ),
        (
            "import {type T} from './module';",
            "import {T} from './module';",
        ),
        (
            "export type {T} from './module';",
            "export {T} from './module';",
        ),
        (
            "export type * from './module';",
            "export * from './module';",
        ),
        ("export {type T};", "export {T};"),
    ] {
        let before: Value =
            serde_json::from_slice(&generate(before, json!({"facts":true})).stdout).unwrap();
        let after: Value =
            serde_json::from_slice(&generate(after, json!({"facts":true})).stdout).unwrap();
        assert_ne!(
            before["metadata"]["scopes"]["module"]["effectHash"],
            after["metadata"]["scopes"]["module"]["effectHash"]
        );
    }
}
