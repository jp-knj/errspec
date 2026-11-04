use errspec::{
    model::Location,
    mutation::{
        Attempt, Candidate, Mutant, Placement, Status,
        scope_cache::{Context, File, Observation, Scope},
    },
};
use std::collections::{BTreeMap, BTreeSet};
fn context() -> Context {
    Context {
        eligible: true,
        envelope: "f".repeat(64),
        scopes: BTreeMap::from([
            (
                "scope-a".into(),
                Scope {
                    file: "src/a.ts".into(),
                    hash: "1".repeat(64),
                    opaque: false,
                    observation: Some(Observation {
                        entry_key: "a".repeat(64),
                        files: BTreeSet::from(["a.test.ts".into()]),
                    }),
                },
            ),
            (
                "scope-b".into(),
                Scope {
                    file: "src/b.ts".into(),
                    hash: "2".repeat(64),
                    opaque: false,
                    observation: Some(Observation {
                        entry_key: "b".repeat(64),
                        files: BTreeSet::from(["b.test.ts".into()]),
                    }),
                },
            ),
        ]),
        candidates: BTreeMap::from([
            ("a".repeat(64), "scope-a".into()),
            ("b".repeat(64), "scope-b".into()),
        ]),
        tests: BTreeMap::from([
            ("test-a".into(), "a.test.ts".into()),
            ("test-b".into(), "b.test.ts".into()),
        ]),
    }
}
fn mutant(status: Status) -> Mutant {
    Mutant {
        candidate: Candidate {
            key: "a".repeat(64),
            location: Location {
                file: "src/a.ts".into(),
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
        status,
        covered_by: vec!["test-a".into()],
        killed_by: vec![],
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
                killed_by: if status == Status::Killed {
                    vec!["test-a".into()]
                } else {
                    vec![]
                },
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
fn stored(status: Status) -> File {
    let mut data = File::new(context()).unwrap();
    assert!(data.record(&mutant(status), &["test-a".into()]));
    data
}
#[test]
fn unchanged_scopes_reuse_only_when_changed_scope_cohorts_are_disjoint() {
    for status in [Status::Killed, Status::Survived] {
        let data = stored(status);
        let mut current = context();
        current.scopes.get_mut("scope-b").unwrap().hash = "3".repeat(64);
        let mut a = mutant(Status::Pending);
        assert!(data.reuse(&current, &mut a, &["test-a".into()]));
        assert_eq!(a.status, status);
        assert!(a.reused);
        // A changed function called by another test in the same file can affect
        // the candidate through shared module state, even with different IDs.
        current
            .scopes
            .get_mut("scope-b")
            .unwrap()
            .observation
            .as_mut()
            .unwrap()
            .files = BTreeSet::from(["a.test.ts".into()]);
        assert!(!data.reuse(&current, &mut mutant(Status::Pending), &["test-a".into()]));
    }
    let mut shared = context();
    shared.tests.insert("test-b".into(), "a.test.ts".into());
    shared
        .scopes
        .get_mut("scope-b")
        .unwrap()
        .observation
        .as_mut()
        .unwrap()
        .files = BTreeSet::from(["a.test.ts".into()]);
    let mut data = File::new(shared.clone()).unwrap();
    assert!(data.record(&mutant(Status::Killed), &["test-a".into()]));
    shared.scopes.get_mut("scope-b").unwrap().hash = "3".repeat(64);
    assert!(!data.reuse(&shared, &mut mutant(Status::Pending), &["test-a".into()]));
}
#[test]
fn positions_do_not_decide_reuse_but_changed_owner_or_missing_entry_evidence_does() {
    let data = stored(Status::Killed);
    let mut a = mutant(Status::Pending);
    a.candidate.location.line = 17;
    a.candidate.location.byte_offset = 50;
    a.candidate.end = 54;
    assert!(data.reuse(&context(), &mut a, &["test-a".into()]));
    for missing in [false, true] {
        let mut current = context();
        let changed = current.scopes.get_mut("scope-b").unwrap();
        changed.hash = "3".repeat(64);
        if missing {
            changed.observation = None;
        } else {
            changed.opaque = true;
        }
        assert!(!data.reuse(&current, &mut mutant(Status::Pending), &["test-a".into()]));
    }
    let mut current = context();
    current.scopes.get_mut("scope-a").unwrap().hash = "4".repeat(64);
    assert!(!data.reuse(&current, &mut mutant(Status::Pending), &["test-a".into()]));
}
#[test]
fn changed_envelopes_removed_tests_and_incomplete_acquisition_cannot_establish_detection() {
    let data = stored(Status::Killed);
    for change in 0..5 {
        let mut current = context();
        match change {
            0 => current.envelope = "e".repeat(64),
            1 => {
                current.tests.remove("test-a");
            }
            2 => current.eligible = false,
            3 => {
                current.scopes.remove("scope-b");
            }
            _ => {
                current
                    .scopes
                    .get_mut("scope-b")
                    .unwrap()
                    .observation
                    .as_mut()
                    .unwrap()
                    .files
                    .insert("unknown.test.ts".into());
            }
        }
        assert!(!data.reuse(&current, &mut mutant(Status::Pending), &["test-a".into()]));
    }
    let mut corrupt = serde_json::to_value(data).unwrap();
    corrupt["acquisitionMode"] = serde_json::json!("partial_tests");
    let corrupt: File = serde_json::from_value(corrupt).unwrap();
    assert!(!corrupt.reuse(&context(), &mut mutant(Status::Pending), &["test-a".into()]));
    let mut fresh = File::new(context()).unwrap();
    let mut retried = mutant(Status::Killed);
    retried.attempts[0].number = 2;
    assert!(!fresh.record(&retried, &["test-a".into()]));
    assert!(!fresh.record(&mutant(Status::Timeout), &["test-a".into()]));
}
