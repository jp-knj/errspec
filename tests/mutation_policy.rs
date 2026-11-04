use errspec::{
    model::Location,
    mutation::{self, Candidate, Mutant, Placement, Status, policy::*},
};
use std::collections::{BTreeMap, BTreeSet};

fn mutant(key: &str, family: &str, status: Status) -> Mutant {
    Mutant {
        candidate: Candidate {
            key: key.into(),
            location: Location {
                file: "src/value.ts".into(),
                byte_offset: 0,
                line: 1,
            },
            end: 1,
            mutator: family.into(),
            original: "x".into(),
            replacement: "y".into(),
            placement: Placement::Expression,
            ignored: None,
            regex: None,
            guard_offset: None,
            weak: None,
        },
        status,
        covered_by: vec![],
        killed_by: vec![],
        failed_modules: vec![],
        attempts: vec![],
        reason: None,
        accepted: None,
        reused: false,
        critical: false,
        infected_by: None,
    }
}

#[test]
fn sampling_is_order_independent_and_excludes_static_decisions() {
    let mut mutants = vec![
        mutant("boolean", "BooleanLiteral", Status::Pending),
        mutant("equality", "EqualityOperator", Status::Pending),
        mutant("invalid", "EqualityOperator", Status::CompileError),
        mutant("ignored", "EqualityOperator", Status::Ignored),
    ];
    assert_eq!(
        sample_per_line(&mutants, 1),
        BTreeSet::from(["equality".into()])
    );
    mutants.reverse();
    assert_eq!(
        sample_per_line(&mutants, 1),
        BTreeSet::from(["equality".into()])
    );
    mutants[3].candidate.location.line = 2;
    assert_eq!(sample_per_line(&mutants, 1).len(), 2);
}

#[test]
fn approval_applies_only_to_undetected_mutants_and_reports_stale_ids() {
    let mut mutants = vec![
        mutant("survived", "BooleanLiteral", Status::Survived),
        mutant("uncovered", "BooleanLiteral", Status::NoCoverage),
        mutant("killed", "BooleanLiteral", Status::Killed),
        mutant("pending", "BooleanLiteral", Status::Pending),
    ];
    let accepted: BTreeMap<_, _> = ["survived", "uncovered", "killed", "pending", "stale"]
        .into_iter()
        .map(|key| {
            (
                key.into(),
                Acceptance {
                    reason: "Equivalent for the documented domain".into(),
                    r#where: None,
                },
            )
        })
        .collect();
    let keys = mutants.iter().map(|m| m.candidate.key.clone()).collect();
    let summary = accept_results(&mut mutants, &accepted, &keys);
    assert_eq!(summary.applied, ["survived", "uncovered"]);
    assert_eq!(summary.detected, ["killed"]);
    assert_eq!(summary.stale, ["stale"]);
    assert_eq!(mutants[3].status, Status::Pending);
    assert_eq!(mutants[2].status, Status::Killed);
    assert_eq!(mutants[0].status, Status::Ignored);
    insta::assert_json_snapshot!(serde_json::json!({"summary":summary,"mutants":mutants}));
}

#[test]
fn compile_errors_are_excluded_from_the_score_and_quality_gate_is_distinct() {
    let mut report = mutation::Report::new(false);
    report.run_complete = true;
    report.policy = Some(PolicyReport::new(Policy {
        threshold_break: Some(60.0),
        ..Default::default()
    }));
    report.mutants = vec![
        mutant("invalid", "BlockStatement", Status::CompileError),
        mutant("killed", "BooleanLiteral", Status::Killed),
        mutant("survived", "BooleanLiteral", Status::Survived),
    ];
    report.finish();
    assert_eq!(report.summary.score, Some(0.5));
    assert_eq!(report.summary.compile_error, 1);
    assert_eq!(report.gate.passed, Some(false));
    assert_eq!(report.exit_code(), 1);
    assert_eq!(report.diagnostics[0].code, "EC_MUTATION_QUALITY");
    insta::assert_json_snapshot!(report);
}

#[test]
fn critical_pending_mutants_fail_even_without_a_score_threshold() {
    let mut mutant = mutant("pending", "BooleanLiteral", Status::Pending);
    mutant.critical = true;
    let mut policy = PolicyReport::new(Policy::default());
    policy.judge(&[mutant], true, None);
    assert_eq!(policy.quality_failures.len(), 1);
    assert!(policy.quality_failures[0].contains("critical"));
}

#[test]
fn no_coverage_is_distinct_from_survival_and_incomplete_runs_have_no_quality_verdict() {
    let mutants = vec![mutant("uncovered", "BooleanLiteral", Status::NoCoverage)];
    let mut policy = PolicyReport::new(Policy {
        fail_on_survived: true,
        ..Default::default()
    });
    policy.judge(&mutants, true, Some(0.0));
    assert!(policy.quality_failures.is_empty());
    policy.judge(&mutants, false, None);
    assert!(policy.score_level.is_none());
}

#[test]
fn invalid_limits_and_thresholds_are_rejected_before_execution() {
    for options in [
        Policy {
            mutants_per_line: Some(0),
            ..Default::default()
        },
        Policy {
            time_budget: Some(f64::NAN),
            ..Default::default()
        },
        Policy {
            time_budget: Some(-1.0),
            ..Default::default()
        },
        Policy {
            threshold_break: Some(101.0),
            ..Default::default()
        },
        Policy {
            threshold_low: Some(90.0),
            threshold_high: Some(80.0),
            ..Default::default()
        },
    ] {
        assert!(options.validate().is_err());
    }
    assert!(
        Policy {
            time_budget: Some(0.0),
            ..Default::default()
        }
        .validate()
        .is_ok()
    );
}

#[test]
fn pending_candidates_never_establish_a_complete_score_or_detection_gate() {
    for known in [Status::Killed, Status::NoCoverage, Status::Survived] {
        let mut report = mutation::Report::new(true);
        report.run_complete = true;
        report.policy = Some(PolicyReport::new(Policy {
            threshold_break: Some(50.0),
            ..Default::default()
        }));
        report.mutants = vec![
            mutant("known", "BooleanLiteral", known),
            mutant("pending", "BooleanLiteral", Status::Pending),
        ];
        report.finish();
        assert_eq!(report.summary.score, None);
        assert_eq!(report.gate.passed, Some(false));
        assert_eq!(report.exit_code(), 1);
        assert!(
            report
                .policy
                .as_ref()
                .unwrap()
                .quality_failures
                .iter()
                .any(|reason| reason.contains("unavailable"))
        );
    }
    let mut report = mutation::Report::new(true);
    report.run_complete = true;
    report.mutants = vec![mutant("pending", "BooleanLiteral", Status::Pending)];
    report.finish();
    assert_eq!(report.gate.passed, Some(false));
    assert_eq!(report.exit_code(), 1);
}

#[test]
fn policy_cannot_accept_a_subset_score_when_pending_candidates_remain() {
    let mutants = vec![
        mutant("killed", "BooleanLiteral", Status::Killed),
        mutant("pending", "BooleanLiteral", Status::Pending),
    ];
    let mut policy = PolicyReport::new(Policy {
        threshold_break: Some(90.0),
        ..Default::default()
    });
    policy.judge(&mutants, true, Some(1.0));
    assert_eq!(policy.score_level, None);
    assert!(
        policy
            .quality_failures
            .iter()
            .any(|reason| reason.contains("unavailable"))
    );
}
