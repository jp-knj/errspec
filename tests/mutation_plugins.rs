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
