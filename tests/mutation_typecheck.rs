use errspec::mutation::{
    Status, TypecheckInfo,
    typecheck::{Error, Facts, Outcome, Update, evaluate},
};
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn keys(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|key| (*key).into()).collect()
}

fn outcome(key: &str, diagnostic: Value, equivalent: Value) -> Outcome {
    Outcome {
        key: Some(json!(key)),
        diagnostic: Some(diagnostic),
        equivalent_reason: Some(equivalent),
    }
}

fn valid(key: &str) -> Outcome {
    outcome(key, Value::Null, Value::Null)
}

fn invalid_code(key: &str) -> Outcome {
    outcome(key, json!("TS2322: invalid assignment"), Value::Null)
}

fn equivalent(key: &str) -> Outcome {
    outcome(key, Value::Null, json!("equivalent: non-null receiver"))
}

fn facts(outcomes: Vec<Outcome>, errors: usize, equivalent: usize) -> Facts {
    Facts {
        info: TypecheckInfo {
            version: "6.0.3".into(),
            package: "typescript".into(),
            project: "tsconfig.json".into(),
            checked: outcomes.len(),
            compile_errors: errors,
            equivalent,
        },
        outcomes,
    }
}

fn compile_error(key: &str) -> Update {
    Update {
        key: key.into(),
        status: Status::CompileError,
        reason: "TS2322: invalid assignment".into(),
    }
}

fn ignored(key: &str) -> Update {
    Update {
        key: key.into(),
        status: Status::Ignored,
        reason: "equivalent: non-null receiver".into(),
    }
}

#[test]
fn successful_updates_follow_response_order_and_keep_aggregate_info() {
    let facts = facts(vec![equivalent("c"), valid("b"), invalid_code("a")], 1, 1);
    let info = serde_json::to_value(&facts.info).unwrap();
    let result = evaluate(&keys(&["a", "b", "c"]), facts);
    assert_eq!(result.updates, vec![ignored("c"), compile_error("a")]);
    assert_eq!(
        serde_json::to_value(result.completion.unwrap()).unwrap(),
        info
    );
}

#[test]
fn zero_targets_complete_without_updates() {
    let result = evaluate(&keys(&[]), facts(vec![], 0, 0));
    assert!(result.updates.is_empty());
    assert_eq!(result.completion.unwrap().checked, 0);
}

#[test]
fn explicit_null_facts_preserve_the_existing_status_and_reason() {
    let result = evaluate(&keys(&["a"]), facts(vec![valid("a")], 0, 0));
    assert!(result.updates.is_empty());
    assert!(result.completion.is_ok());
}

#[test]
fn missing_null_and_non_string_keys_stop_before_later_candidates() {
    for key in [
        None,
        Some(Value::Null),
        Some(json!(1)),
        Some(json!({})),
        Some(json!([])),
    ] {
        let mut bad = valid("b");
        bad.key = key;
        let result = evaluate(
            &keys(&["a", "b", "c"]),
            facts(vec![invalid_code("a"), bad, equivalent("c")], 1, 1),
        );
        assert_eq!(result.updates, vec![compile_error("a")]);
        assert_eq!(result.completion.unwrap_err(), Error::MissingKey);
    }
}

#[test]
fn unknown_and_duplicate_keys_keep_only_preceding_updates() {
    for key in ["unknown", "", "a"] {
        let result = evaluate(
            &keys(&["a", "b", "c"]),
            facts(
                vec![invalid_code("a"), equivalent(key), invalid_code("c")],
                2,
                1,
            ),
        );
        assert_eq!(result.updates, vec![compile_error("a")]);
        assert_eq!(result.completion.unwrap_err(), Error::UnknownOrDuplicateKey);
    }
    // No-op candidates also participate in duplicate detection.
    let result = evaluate(
        &keys(&["a"]),
        facts(vec![valid("a"), invalid_code("a")], 1, 0),
    );
    assert!(result.updates.is_empty());
    assert_eq!(result.completion.unwrap_err(), Error::UnknownOrDuplicateKey);
}

#[test]
fn invalid_facts_preserve_the_valid_prefix_and_skip_the_suffix() {
    let mut invalid = vec![
        (Some(json!("")), Some(Value::Null)),
        (Some(Value::Null), Some(json!(""))),
        (Some(Value::Null), Some(json!("not equivalent"))),
        (
            Some(json!("compiler error")),
            Some(json!("equivalent: receiver")),
        ),
    ];
    for value in [
        None,
        Some(json!(false)),
        Some(json!(1)),
        Some(json!([])),
        Some(json!({})),
    ] {
        invalid.push((value.clone(), Some(Value::Null)));
        invalid.push((Some(Value::Null), value));
    }
    for (diagnostic, equivalent_reason) in invalid {
        let bad = Outcome {
            key: Some(json!("b")),
            diagnostic,
            equivalent_reason,
        };
        let result = evaluate(
            &keys(&["a", "b", "c"]),
            facts(vec![invalid_code("a"), bad, equivalent("c")], 1, 1),
        );
        assert_eq!(result.updates, vec![compile_error("a")]);
        assert_eq!(result.completion.unwrap_err(), Error::InvalidCompilerFacts);
    }
}

#[test]
fn existing_reason_validation_accepts_whitespace_and_the_bare_equivalent_prefix() {
    let result = evaluate(
        &keys(&["a", "b"]),
        facts(
            vec![
                outcome("a", json!(" "), Value::Null),
                outcome("b", Value::Null, json!("equivalent:")),
            ],
            1,
            1,
        ),
    );
    assert_eq!(
        result.updates,
        vec![
            Update {
                key: "a".into(),
                status: Status::CompileError,
                reason: " ".into()
            },
            Update {
                key: "b".into(),
                status: Status::Ignored,
                reason: "equivalent:".into()
            },
        ]
    );
    assert!(result.completion.is_ok());
}

#[test]
fn final_count_and_version_checks_keep_all_confirmed_updates() {
    for field in ["checked", "errors", "equivalent", "version", "missing"] {
        let mut facts = facts(vec![equivalent("b"), invalid_code("a")], 1, 1);
        let mut expected = keys(&["a", "b"]);
        match field {
            "checked" => facts.info.checked = 3,
            "errors" => facts.info.compile_errors = 0,
            "equivalent" => facts.info.equivalent = 0,
            "version" => facts.info.version.clear(),
            "missing" => {
                expected.insert("c".into());
            }
            _ => unreachable!(),
        }
        let result = evaluate(&expected, facts);
        assert_eq!(
            result.updates,
            vec![ignored("b"), compile_error("a")],
            "{field}"
        );
        assert_eq!(
            result.completion.unwrap_err(),
            Error::IncompleteResults,
            "{field}"
        );
    }
    let result = evaluate(&keys(&["a"]), facts(vec![], 0, 0));
    assert!(result.updates.is_empty());
    assert_eq!(result.completion.unwrap_err(), Error::IncompleteResults);
}

#[test]
fn candidate_validation_precedes_final_counts_and_keys_precede_facts() {
    let mut bad = valid("unknown");
    bad.diagnostic = None;
    let mut facts = facts(vec![bad], 99, 99);
    facts.info.version.clear();
    let result = evaluate(&keys(&["a"]), facts);
    assert!(result.updates.is_empty());
    assert_eq!(result.completion.unwrap_err(), Error::UnknownOrDuplicateKey);
}

#[test]
fn errors_keep_existing_protocol_diagnostics() {
    for (error, message) in [
        (Error::MissingKey, "Missing typecheck key"),
        (
            Error::UnknownOrDuplicateKey,
            "Unknown or duplicate typecheck key",
        ),
        (
            Error::InvalidCompilerFacts,
            "Invalid typecheck compiler facts",
        ),
        (
            Error::IncompleteResults,
            "Incomplete typecheck results or counts",
        ),
    ] {
        assert_eq!(error.to_string(), message);
    }
}
