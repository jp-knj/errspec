use errspec::mutation::{Execution, parallel::compatible_probe};
use serde_json::json;
fn probe() -> Execution {
    serde_json::from_value(json!({"selected":[],"tests":[{"id":"test-a","taskId":"original","file":"a.test.ts","name":"works","status":"passed","started":true,"duration":1.0,"errors":[],"hits":{"key":1},"infected":["key"]}],"hits":{"key":1},"staticHits":{"key":["a.test.ts"]},"failedModules":[],"errors":[],"wallTimeout":false,"moduleDurations":{"a.test.ts":2.0}})).unwrap()
}
#[test]
fn fresh_task_ids_timings_and_hit_counts_do_not_change_worker_cohorts() {
    let expected = probe();
    let mut actual = probe();
    actual.tests[0].task_id = "current-worker".into();
    actual.tests[0].duration = 3.0;
    actual.tests[0].hits.insert("key".into(), 4);
    actual.hits.insert("key".into(), 4);
    assert!(compatible_probe(&expected, &actual).is_ok());
}
#[test]
fn changed_registration_coverage_infection_and_failed_baselines_cannot_mix() {
    for field in ["id", "name", "file", "status"] {
        let mut actual = serde_json::to_value(probe()).unwrap();
        actual["tests"][0][field] = json!("changed");
        let actual = serde_json::from_value(actual).unwrap();
        assert!(compatible_probe(&probe(), &actual).is_err());
    }
    let mut actual = probe();
    actual.tests[0].infected.clear();
    assert!(compatible_probe(&probe(), &actual).is_err());
    let mut actual = probe();
    actual.static_hits.clear();
    assert!(compatible_probe(&probe(), &actual).is_err());
    let mut actual = probe();
    actual.wall_timeout = true;
    assert!(compatible_probe(&probe(), &actual).is_err());
    let mut actual = probe();
    actual.module_durations.insert("a.test.ts".into(), -1.0);
    assert!(compatible_probe(&probe(), &actual).is_err());
    let mut actual = probe();
    actual.errors.push("worker failed".into());
    assert!(compatible_probe(&probe(), &actual).is_err());
}
