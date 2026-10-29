//! Graph semantics without a TypeScript compiler, subprocess, or filesystem.
use errspec::discovery::{
    Candidate, Channel, Edge, FunctionFacts, Location, Origin, ScanFacts, Unknown, propagate,
};
use errspec::model::Source;
use serde_json::{Value, json};

fn location() -> Location {
    Location {
        file: "graph.ts".into(),
        byte_offset: 0,
        line: 1,
    }
}

fn function(id: &str, is_async: bool) -> FunctionFacts {
    FunctionFacts {
        id: id.into(),
        name: id.into(),
        location: location(),
        is_async,
    }
}

fn origin(owner: &str, channel: Channel) -> Origin {
    Origin {
        id: "failure".into(),
        function: owner.into(),
        location: location(),
        channel,
        error_type: "Failure".into(),
        caught: false,
        escapes: true,
    }
}

fn edge(caller: &str, callee: &str) -> Edge {
    Edge {
        caller: caller.into(),
        callee: callee.into(),
        location: location(),
        awaited: false,
        returned: false,
        catches: false,
        may_rethrow: false,
    }
}

fn effects(report: &errspec::discovery::ScanReport, id: &str) -> Value {
    serde_json::to_value(
        &report
            .functions
            .iter()
            .find(|f| f.id == id)
            .unwrap()
            .effects,
    )
    .unwrap()
}

#[test]
fn channels_follow_catch_await_return_and_async_boundaries() {
    use Channel::{Rejection, Result, Throw};
    // Each row specifies the expected escaping channel, independently of the solver.
    // Input channel, async caller, await, return, catch, rethrow, expected channel.
    let cases = [
        (Throw, false, false, false, false, false, Some("throw")),
        (Throw, true, false, false, false, false, Some("rejection")),
        (Throw, true, false, true, true, false, None),
        (Throw, false, false, false, true, true, Some("throw")),
        (
            Rejection,
            true,
            true,
            false,
            false,
            false,
            Some("rejection"),
        ),
        (Rejection, true, true, false, true, false, None),
        (Rejection, true, true, false, true, true, Some("rejection")),
        (
            Rejection,
            false,
            false,
            true,
            true,
            false,
            Some("rejection"),
        ),
        (Rejection, true, false, false, false, false, None),
        (Result, false, false, true, true, false, Some("result")),
        (Result, true, true, true, false, false, Some("result")),
        (Result, false, false, false, false, false, None),
    ];
    for (channel, is_async, awaited, returned, catches, may_rethrow, expected) in cases {
        let report = ScanFacts {
            functions: vec![function("caller", is_async), function("source", false)],
            origins: vec![origin("source", channel)],
            edges: vec![Edge {
                awaited,
                returned,
                catches,
                may_rethrow,
                ..edge("caller", "source")
            }],
            ..ScanFacts::default()
        }
        .into_report();
        let expected = expected.map_or_else(
            || json!([]),
            |channel| json!([{"origin":"failure", "channel":channel, "via":["caller","source"]}]),
        );
        assert_eq!(
            effects(&report, "caller"),
            expected,
            "{channel:?} async={is_async} await={awaited} return={returned} catch={catches} rethrow={may_rethrow}"
        );
        assert_eq!(report.functions[0].id, "caller");
        assert!(report.scan_complete);
        assert!(!report.all_runtime_errors_known);
    }
}

#[test]
fn recursive_channel_changes_have_finite_witnesses_and_keep_unknowns() {
    let report = ScanFacts {
        functions: vec![
            function("a", false),
            function("b", true),
            function("isolated", false),
        ],
        origins: vec![origin("a", Channel::Throw)],
        edges: vec![
            Edge {
                returned: true,
                ..edge("a", "b")
            },
            Edge {
                awaited: true,
                ..edge("b", "a")
            },
            edge("a", "a"),
        ],
        unknowns: vec![Unknown {
            function: "b".into(),
            location: location(),
            expression: "callback()".into(),
            reason: "unresolved_call".into(),
        }],
        ..ScanFacts::default()
    }
    .into_report();
    assert_eq!(
        effects(&report, "a"),
        json!([
            {"origin":"failure","channel":"throw","via":["a"]},
            {"origin":"failure","channel":"rejection","via":["a","b","a"]},
        ])
    );
    assert_eq!(
        effects(&report, "b"),
        json!([
            {"origin":"failure","channel":"rejection","via":["b","a"]},
        ])
    );
    assert_eq!(effects(&report, "isolated"), json!([]));
    assert_eq!(
        report
            .functions
            .iter()
            .map(|f| f.has_unknown)
            .collect::<Vec<_>>(),
        [true, true, false]
    );
}

#[test]
fn caught_origins_do_not_escape_but_unknowns_still_reach_callers() {
    let report = ScanFacts {
        functions: vec![function("caller", false), function("source", false)],
        origins: vec![Origin {
            caught: true,
            escapes: false,
            ..origin("source", Channel::Throw)
        }],
        edges: vec![Edge {
            catches: true,
            ..edge("caller", "source")
        }],
        unknowns: vec![Unknown {
            function: "source".into(),
            location: location(),
            expression: "external()".into(),
            reason: "implementation_outside_scan_or_dynamic_signature".into(),
        }],
        ..ScanFacts::default()
    }
    .into_report();
    assert!(
        report
            .functions
            .iter()
            .all(|f| f.effects.is_empty() && f.has_unknown)
    );
    assert!(report.origins[0].caught);
    assert!(!report.origins[0].escapes);
    assert_eq!(report.unknowns.len(), 1);
}

#[test]
fn first_witness_is_stable_and_recomputing_replaces_stale_summaries_only() {
    let candidate = |file: &str| Candidate {
        source: Source {
            file: file.into(),
            export: "Failure".into(),
            discriminant: "_tag".into(),
        },
        tags: vec!["Offline".into()],
    };
    let mut report = ScanFacts {
        files: vec!["z.ts".into(), "a.ts".into(), "z.ts".into()],
        candidates: vec![candidate("z.ts"), candidate("a.ts")],
        functions: vec![
            function("entry", false),
            function("left", false),
            function("right", false),
            function("source", false),
        ],
        origins: vec![origin("source", Channel::Throw)],
        edges: vec![
            edge("left", "source"),
            edge("right", "source"),
            edge("entry", "right"),
            edge("entry", "left"),
        ],
        ..ScanFacts::default()
    }
    .into_report();
    assert_eq!(report.files, ["a.ts", "z.ts"]);
    assert_eq!(report.candidates[0].source.file, "a.ts");
    assert_eq!(
        effects(&report, "entry"),
        json!([
            {"origin":"failure","channel":"throw","via":["entry","right","source"]},
        ])
    );
    let expected = serde_json::to_value(&report).unwrap();
    for f in &mut report.functions {
        f.effects.clear();
        f.has_unknown = true;
    }
    propagate(&mut report);
    assert_eq!(serde_json::to_value(&report).unwrap(), expected);

    // Removing the recorded cause must also remove previously computed effects.
    report.origins.clear();
    let mut expected = serde_json::to_value(&report).unwrap();
    for f in expected["functions"].as_array_mut().unwrap() {
        f["effects"] = json!([]);
    }
    propagate(&mut report);
    assert_eq!(serde_json::to_value(&report).unwrap(), expected);
}
