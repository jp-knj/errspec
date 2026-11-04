use errspec::mutation::{Execution, coverage_cache::File};
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn execution() -> Execution {
    serde_json::from_value(json!({
        "selected":[],"tests":[{"id":"test-a","taskId":"old-task","file":"a.test.ts","name":"works",
            "status":"passed","started":true,"duration":1.0,"errors":[],"hits":{"mutant":1},"infected":["mutant"]}],
        "failedModules":[],"errors":[],"staticHits":{"mutant":["a.test.ts"]},"hits":{"mutant":2},
        "wallTimeout":false,"moduleDurations":{"a.test.ts":2.0}
    })).unwrap()
}
fn keys() -> BTreeSet<String> {
    BTreeSet::from(["mutant".into()])
}
fn stored() -> Value {
    serde_json::to_value(File::new("f".repeat(64), execution())).unwrap()
}

#[test]
fn current_registration_is_required_and_current_task_ids_and_timings_are_retained() {
    let file: File = serde_json::from_value(stored()).unwrap();
    let mut current = execution();
    current.tests[0].task_id = "current-task".into();
    current.tests[0].duration = 3.0;
    current.tests[0].hits.clear();
    current.tests[0].infected.clear();
    current.hits.clear();
    current.static_hits.clear();
    current.module_durations.insert("a.test.ts".into(), 4.0);
    let reused = file.reuse(&"f".repeat(64), &current, &keys()).unwrap();
    assert_eq!(reused.tests[0].task_id, "current-task");
    assert_eq!(reused.tests[0].duration, 3.0);
    assert_eq!(reused.module_durations["a.test.ts"], 4.0);
    assert_eq!(reused.hits["mutant"], 2);
    assert!(reused.tests[0].infected.contains("mutant"));
}
#[test]
fn changed_inputs_candidates_and_removed_or_failed_tests_refresh_observations() {
    let file: File = serde_json::from_value(stored()).unwrap();
    assert!(file.reuse(&"e".repeat(64), &execution(), &keys()).is_none());
    assert!(
        file.reuse(&"f".repeat(64), &execution(), &BTreeSet::new())
            .is_none()
    );
    for field in ["id", "file", "name", "status"] {
        let mut current = serde_json::to_value(execution()).unwrap();
        current["tests"][0][field] = json!("changed");
        let current = serde_json::from_value(current).unwrap();
        assert!(file.reuse(&"f".repeat(64), &current, &keys()).is_none());
    }
    let mut current = execution();
    current.tests.clear();
    assert!(file.reuse(&"f".repeat(64), &current, &keys()).is_none());
    let mut current = execution();
    current.wall_timeout = true;
    assert!(file.reuse(&"f".repeat(64), &current, &keys()).is_none());
}
#[test]
fn corrupt_records_modes_and_unattributed_observations_never_supply_coverage() {
    for (pointer, value) in [
        ("/kind", json!("errspec/mutation-results")),
        ("/schemaVersion", json!(2)),
        ("/acquisitionMode", json!("selected_tests")),
        ("/execution/wallTimeout", json!(true)),
        ("/execution/hits/mutant", json!(0)),
        ("/execution/hits", json!({})),
        ("/execution/hits", json!({"mutant":2,"unattributed":1})),
        ("/execution/staticHits/mutant", json!(["missing.test.ts"])),
        ("/execution/tests/0/hits/mutant", json!(3)),
        ("/execution/tests/0/infected", json!(["unknown"])),
        (
            "/execution/tests/0/errors",
            json!(["caught infrastructure failure"]),
        ),
    ] {
        let mut data = stored();
        *data.pointer_mut(pointer).unwrap() = value;
        let file: File = serde_json::from_value(data).unwrap();
        assert!(
            file.reuse(&"f".repeat(64), &execution(), &keys()).is_none(),
            "{pointer}"
        );
    }
    let mut data = stored();
    let duplicate = data["execution"]["tests"][0].clone();
    data["execution"]["tests"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    let file: File = serde_json::from_value(data).unwrap();
    assert!(file.reuse(&"f".repeat(64), &execution(), &keys()).is_none());
    let mut data = stored();
    data["execution"]["tests"][0]["hits"] = json!({});
    data["execution"]["tests"][0]["infected"] = json!([]);
    data["execution"]["staticHits"] = json!({});
    let file: File = serde_json::from_value(data).unwrap();
    assert!(file.reuse(&"f".repeat(64), &execution(), &keys()).is_none());
}
