use errspec::{
    model::Location,
    mutation::{self, Attempt, Candidate, Mutant, Placement, Status, cache::File},
};
use serde_json::json;

fn mutant(status: Status) -> Mutant {
    let killed = if status == Status::Killed {
        vec!["test-a".into()]
    } else {
        vec![]
    };
    Mutant {
        candidate: Candidate {
            key: "a".repeat(64),
            location: Location {
                file: "src/a.ts".into(),
                line: 1,
                byte_offset: 0,
            },
            end: 1,
            mutator: "BooleanLiteral".into(),
            original: "true".into(),
            replacement: "false".into(),
            placement: Placement::Expression,
            ignored: None,
            regex: None,
            guard_offset: None,
            weak: None,
        },
        status,
        covered_by: vec!["test-a".into(), "test-b".into()],
        killed_by: killed.clone(),
        failed_modules: vec![],
        attempts: if status == Status::Pending {
            vec![]
        } else {
            vec![Attempt {
                number: 1,
                status,
                reason: if status == Status::Killed {
                    "Selected test failed"
                } else {
                    "All selected tests passed"
                }
                .into(),
                killed_by: killed,
                failed_modules: vec![],
            }]
        },
        reason: None,
        accepted: None,
        critical: false,
        reused: false,
        infected_by: None,
    }
}
fn selected() -> Vec<String> {
    vec!["test-a".into(), "test-b".into()]
}
fn stored(status: Status) -> serde_json::Value {
    let mut file = File::new("f".repeat(64));
    assert!(file.record(&mutant(status), &selected()));
    serde_json::to_value(file).unwrap()
}
#[test]
fn stable_assertion_outcomes_reuse_only_matching_current_evidence() {
    for status in [Status::Killed, Status::Survived] {
        let file: File = serde_json::from_value(stored(status)).unwrap();
        let mut current = mutant(Status::Pending);
        assert!(file.reuse(&"f".repeat(64), &mut current, &selected()));
        assert_eq!(current.status, status);
        assert!(current.reused);
        assert_eq!(current.attempts.len(), 1);
        let mut report = mutation::Report::new(true);
        report.run_complete = true;
        report.mutants.push(current);
        report.finish();
        assert_eq!(
            report.exit_code(),
            if status == Status::Killed { 0 } else { 1 }
        );
    }
}
#[test]
fn changed_inputs_modes_and_removed_killing_tests_never_reuse() {
    for (field, value) in [
        ("fingerprint", json!("e".repeat(64))),
        ("kind", json!("errspec/snapshot")),
        ("schemaVersion", json!(2)),
        ("acquisitionMode", json!("complete_matrix")),
    ] {
        let mut data = stored(Status::Killed);
        data[field] = value;
        let file: File = serde_json::from_value(data).unwrap();
        assert!(!file.reuse(&"f".repeat(64), &mut mutant(Status::Pending), &selected()));
    }
    let file: File = serde_json::from_value(stored(Status::Killed)).unwrap();
    let mut current = mutant(Status::Pending);
    current.covered_by = vec!["test-b".into()];
    assert!(!file.reuse(&"f".repeat(64), &mut current, &["test-b".into()]));
    assert!(!file.reuse(
        &"f".repeat(64),
        &mut mutant(Status::Pending),
        &["test-b".into()]
    ));
}
#[test]
fn corrupt_attempts_and_inconclusive_statuses_are_not_detection_evidence() {
    let key = "a".repeat(64);
    for (field, value) in [
        ("status", json!("Pending")),
        ("status", json!("Timeout")),
        ("status", json!("RuntimeError")),
        ("number", json!(2)),
        ("killedBy", json!([])),
        ("killedBy", json!(["absent"])),
        ("killedBy", json!(["test-a", "test-a"])),
        ("failedModules", json!(["failed.test.ts"])),
        ("reason", json!("changed")),
    ] {
        let mut data = stored(Status::Killed);
        data["records"][&key]["attempt"][field] = value;
        let file: File = serde_json::from_value(data).unwrap();
        let mut current = mutant(Status::Pending);
        assert!(!file.reuse(&"f".repeat(64), &mut current, &selected()));
        assert!(!current.reused);
    }
    for status in [
        Status::Timeout,
        Status::RuntimeError,
        Status::NoCoverage,
        Status::Ignored,
        Status::CompileError,
        Status::Pending,
    ] {
        assert!(!File::new("f".repeat(64)).record(&mutant(status), &selected()));
    }
    let mut retried = mutant(Status::Killed);
    retried.attempts.push(retried.attempts[0].clone());
    assert!(!File::new("f".repeat(64)).record(&retried, &selected()));
}
