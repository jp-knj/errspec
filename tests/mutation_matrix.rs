use errspec::{
    model::Location,
    mutation::{
        self, Candidate, Execution, Mutant, Placement, Status,
        kill_matrix::{self, Observation},
    },
};
use serde_json::json;

fn mutant(key: &str, selected: &[&str], killed: &[&str]) -> Mutant {
    let selected: Vec<String> = selected.iter().map(|id| (*id).into()).collect();
    let killed: Vec<String> = killed.iter().map(|id| (*id).into()).collect();
    Mutant {
        candidate: Candidate {
            key: key.into(),
            location: Location {
                file: "src/value.ts".into(),
                line: 1,
                byte_offset: 0,
            },
            end: 4,
            mutator: "BooleanLiteral".into(),
            original: "true".into(),
            replacement: "false".into(),
            placement: Placement::Expression,
            ignored: None,
            regex: None,
            guard_offset: None,
            weak: None,
        },
        status: if killed.is_empty() {
            Status::Survived
        } else {
            Status::Killed
        },
        covered_by: selected.clone(),
        killed_by: killed.clone(),
        failed_modules: vec![],
        attempts: vec![],
        reason: None,
        accepted: None,
        critical: false,
        reused: false,
        infected_by: None,
        matrix: Some(Observation {
            complete: true,
            selected: selected.clone(),
            executed: selected,
            killed_by: killed,
            incomplete_reason: None,
        }),
    }
}
#[test]
fn relationships_use_only_complete_current_rows_and_expose_partial_causes() {
    let mut timeout = mutant("timeout", &["t#timeout"], &[]);
    timeout.status = Status::Timeout;
    timeout.matrix = None;
    let mut partial = mutant("partial", &["t#module"], &[]);
    partial.status = Status::RuntimeError;
    partial.matrix = None;
    let mut pending = mutant("budget", &["t#pending"], &[]);
    pending.status = Status::Pending;
    pending.reason = Some("Time budget exhausted before execution".into());
    pending.matrix = None;
    let rows = vec![
        mutant("one", &["t#a", "t#b", "t#c", "t#d"], &["t#a", "t#b", "t#c"]),
        mutant("two", &["t#a", "t#b", "t#c", "t#d"], &["t#a", "t#b"]),
        timeout,
        partial,
        pending,
    ];
    let analysis = kill_matrix::analyze(&rows);
    assert_eq!(analysis.mutants.full, 2);
    assert_eq!(analysis.mutants.timeout, 1);
    assert_eq!(analysis.mutants.incomplete, 2);
    assert_eq!(analysis.no_kills, vec!["t#d"]);
    assert_eq!(analysis.duplicates[0].tests, vec!["t#a", "t#b"]);
    assert_eq!(analysis.subsumed[0].test, "t#c");
    insta::assert_json_snapshot!(analysis);
}
#[test]
fn incomplete_tests_timeouts_modules_and_failed_run_validation_never_become_complete_cells() {
    let complete:Execution=serde_json::from_value(json!({"selected":["a","b"],"tests":[{"id":"a","taskId":"1","file":"a.test.ts","name":"a","status":"failed","started":true,"duration":1,"errors":["AssertionError: expected"],"hits":{}},{"id":"b","taskId":"2","file":"a.test.ts","name":"b","status":"passed","started":true,"duration":1,"errors":[],"hits":{}}],"failedModules":[],"errors":[],"staticHits":{},"hits":{},"wallTimeout":false})).unwrap();
    let selected = vec!["a".into(), "b".into()];
    assert!(Observation::acquire(&complete, &selected).complete);
    for kind in 0..4 {
        let mut incomplete = complete.clone();
        match kind {
            0 => incomplete.tests[1].started = false,
            1 => incomplete.wall_timeout = true,
            2 => incomplete.failed_modules.push("a.test.ts".into()),
            _ => incomplete.tests[0].errors = vec!["Test timed out in 100ms".into()],
        };
        assert!(!Observation::acquire(&incomplete, &selected).complete);
    }
    let mut report = mutation::Report::new(false);
    report.kill_matrix = Some(kill_matrix::analyze(&[]));
    report.mutants = vec![mutant("one", &["a", "b"], &["a"])];
    report.finish();
    assert_eq!(report.kill_matrix.as_ref().unwrap().mutants.full, 0);
    let mut corrupt = mutant("corrupt", &["a", "b"], &["a"]);
    corrupt.matrix.as_mut().unwrap().executed = vec!["a".into()];
    assert_eq!(kill_matrix::analyze(&[corrupt]).mutants.full, 0);
}
