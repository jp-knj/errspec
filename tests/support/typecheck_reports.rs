//! Expectations captured from the coordinator before extracting typecheck judgments.
use super::super::driver::Response;
use super::*;
use errspec::mutation::Mutant;
use serde_json::{Value, json};

fn apply_typecheck_response(response: Response, report: &mut Report) -> Result<(), Failure> {
    let facts = typecheck::decode(response)?;
    let evaluation = mutation::typecheck::evaluate(&expected_typecheck_keys(report), facts);
    apply_typecheck(evaluation, report)
}

fn mutant(key: &str, file: &str, status: Status) -> Mutant {
    serde_json::from_value(json!({
        "key": key, "file": file, "line": 1, "byteOffset": 0,
        "end": 4, "mutator": "BooleanLiteral", "original": "true",
        "replacement": "false", "placement": "expression", "ignored": null,
        "regex": null, "guardOffset": null, "status": status,
        "coveredBy": [], "killedBy": [], "failedModules": [], "attempts": [],
        "reason": "existing reason"
    }))
    .unwrap()
}

fn report() -> Report {
    let mut report = Report::new(false);
    report.mutants = vec![
        mutant("a", "src/a.ts", Status::Pending),
        mutant("b", "src/b.tsx", Status::Pending),
        mutant("c", "src/c.mts", Status::Pending),
        mutant("d", "src/d.cts", Status::Pending),
        mutant("js", "src/plain.js", Status::Pending),
        mutant("ignored", "src/ignored.ts", Status::Ignored),
    ];
    report
}

fn response(outcomes: Value, errors: usize, equivalent: usize) -> Response {
    Response {
        timeout: false,
        messages: vec![
            json!({"type":"progress", "event":"typecheck", "key":"a"}),
            json!({"type":"typechecked", "outcomes":outcomes, "info":{
                "version":"6.0.3", "package":"typescript", "project":"tsconfig.json",
                "checked":outcomes.as_array().unwrap().len(),
                "compileErrors":errors, "equivalent":equivalent
            }}),
            json!({"type":"done", "count":outcomes.as_array().unwrap().len()}),
        ],
    }
}

fn valid_response() -> Response {
    response(
        json!([
            {"key":"c", "diagnostic":null, "equivalentReason":"equivalent: non-null receiver"},
            {"key":"a", "diagnostic":"TS2322: invalid assignment", "equivalentReason":null},
            {"key":"d", "diagnostic":null, "equivalentReason":null},
            {"key":"b", "diagnostic":null, "equivalentReason":null}
        ]),
        1,
        1,
    )
}

fn finish(response: Response, mut report: Report) -> Report {
    if let Err(error) = apply_typecheck_response(response, &mut report) {
        report.error(&error.code, error.message);
    }
    report.finish();
    report
}

#[test]
fn typecheck_report_and_diagnostics() {
    let mut partial = valid_response();
    partial.messages[1]["outcomes"][1]["diagnostic"] = json!(false);
    let mut counts = valid_response();
    counts.messages[1]["info"]["checked"] = json!(5);
    let mut info = valid_response();
    info.messages[1]["info"]
        .as_object_mut()
        .unwrap()
        .remove("version");
    // Envelope errors must take precedence over invalid aggregate/candidate data.
    let mut envelope = valid_response();
    envelope.messages[2]["count"] = json!(99);
    envelope.messages[1]["info"] = Value::Null;
    let reports = json!({
        "success_reordered": finish(valid_response(), report()),
        "second_invalid": finish(partial, report()),
        "count_mismatch": finish(counts, report()),
        "invalid_info": finish(info, report()),
        "invalid_envelope": finish(envelope, report()),
        "zero_targets": finish(response(json!([]), 0, 0), Report::new(false))
    });
    insta::with_settings!({snapshot_path => "../snapshots", prepend_module_to_snapshot => false}, {
        insta::assert_json_snapshot!("typecheck_reports", reports);
    });
}

#[test]
fn typecheck_protocol_errors_do_not_update_reports() {
    let mut cases = vec![];
    let mut timeout = valid_response();
    timeout.timeout = true;
    cases.push((
        timeout,
        "EC_MUTATION_TYPECHECK",
        "Mutant typechecker timed out",
    ));
    let mut driver = valid_response();
    driver
        .messages
        .push(json!({"type":"error", "code":"EC_MUTATION_TYPECHECK", "message":"compiler failed"}));
    cases.push((driver, "EC_MUTATION_TYPECHECK", "compiler failed"));
    let mut missing = valid_response();
    missing.messages.remove(1);
    cases.push((
        missing,
        "EC_MUTATION_PROTOCOL",
        "Expected exactly one typechecked response",
    ));
    let mut duplicate = valid_response();
    duplicate.messages.push(duplicate.messages[1].clone());
    cases.push((
        duplicate,
        "EC_MUTATION_PROTOCOL",
        "Expected exactly one typechecked response",
    ));
    let mut outcomes = valid_response();
    outcomes.messages[1]["outcomes"] = Value::Null;
    cases.push((
        outcomes,
        "EC_MUTATION_PROTOCOL",
        "Missing typecheck outcomes",
    ));
    let mut unexpected = valid_response();
    unexpected.messages.push(json!({"type":"prepared"}));
    cases.push((
        unexpected,
        "EC_MUTATION_PROTOCOL",
        "Unexpected response or terminal count",
    ));
    let mut missing_done = valid_response();
    missing_done.messages.pop();
    cases.push((
        missing_done,
        "EC_MUTATION_PROTOCOL",
        "Expected exactly one done response",
    ));
    for (response, code, message) in cases {
        let mut report = report();
        let before = serde_json::to_value(&report).unwrap();
        let error = apply_typecheck_response(response, &mut report)
            .err()
            .unwrap();
        assert_eq!(error.code, code);
        assert_eq!(error.message, message);
        assert_eq!(serde_json::to_value(report).unwrap(), before);
    }
}

#[test]
fn typecheck_decoding_preserves_unvalidated_candidate_fields() {
    for value in [
        Value::Null,
        json!(false),
        json!(1),
        json!(""),
        json!([]),
        json!({}),
    ] {
        let mut response = valid_response();
        response.messages[1]["outcomes"][1]["diagnostic"] = value.clone();
        let facts = typecheck::decode(response).ok().unwrap();
        assert_eq!(facts.outcomes[1].diagnostic, Some(value));
    }
    let mut response = valid_response();
    response.messages[1]["outcomes"][1]
        .as_object_mut()
        .unwrap()
        .remove("diagnostic");
    let facts = typecheck::decode(response).ok().unwrap();
    assert_eq!(facts.outcomes[1].diagnostic, None);

    for value in [Value::Null, json!(false), json!(1), json!(""), json!([])] {
        let mut response = valid_response();
        response.messages[1]["outcomes"][1] = value;
        let mut report = report();
        let error = apply_typecheck_response(response, &mut report)
            .err()
            .unwrap();
        assert_eq!(error.message, "Missing typecheck key");
        assert_eq!(report.mutants[2].status, Status::Ignored);
        assert_eq!(report.mutants[0].status, Status::Pending);
        assert_eq!(report.engine.typecheck, "not_run");
        assert!(report.typecheck.is_none());
    }
}

#[test]
fn typecheck_info_is_decoded_before_candidate_validation() {
    let mut response = valid_response();
    response.messages[1]["info"]["compileErrors"] = json!(false);
    response.messages[1]["outcomes"][0] = Value::Null;
    let mut report = report();
    let before = serde_json::to_value(&report).unwrap();
    let error = apply_typecheck_response(response, &mut report)
        .err()
        .unwrap();
    assert_eq!(error.code, "EC_MUTATION_PROTOCOL");
    assert_eq!(
        error.message,
        "invalid type: boolean `false`, expected usize"
    );
    assert_eq!(serde_json::to_value(report).unwrap(), before);
}

#[test]
fn typecheck_targets_include_only_non_ignored_typescript_candidates() {
    let mut report = report();
    report.mutants.extend([
        mutant("jsx", "src/view.jsx", Status::Pending),
        mutant("suffix", "src/a.ts.bak", Status::Pending),
        mutant("upper", "src/a.TS", Status::Pending),
    ]);
    assert_eq!(
        expected_typecheck_keys(&report),
        BTreeSet::from_iter(["a", "b", "c", "d"].map(str::to_owned))
    );
    for key in ["js", "ignored", "jsx", "suffix", "upper"] {
        let response = response(
            json!([
                {"key":key, "diagnostic":null, "equivalentReason":null}
            ]),
            0,
            0,
        );
        let before = serde_json::to_value(&report).unwrap();
        let error = apply_typecheck_response(response, &mut report)
            .err()
            .unwrap();
        assert_eq!(error.message, "Unknown or duplicate typecheck key");
        assert_eq!(serde_json::to_value(&report).unwrap(), before);
    }
}
