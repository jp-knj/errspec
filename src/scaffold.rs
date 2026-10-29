//! Render both init files from one scenario inventory, without compiler or filesystem access.
use crate::request::Runner;
use errspec::model::Source;
use serde_json::json;

pub(crate) struct Scaffold {
    pub config: Vec<u8>,
    pub test: String,
}

struct Scenario<'a> {
    id: String,
    tag: &'a str,
}

pub(crate) fn render(
    project: &str,
    source: &Source,
    test_file: &str,
    contract: &str,
    runner: Runner,
    tags: &[String],
) -> Result<Scaffold, String> {
    let scenarios: Vec<_> = tags
        .iter()
        .enumerate()
        .map(|(i, tag)| Scenario {
            id: format!("case-{}", i + 1),
            tag,
        })
        .collect();
    let cases: Vec<_> = scenarios.iter().map(|s| json!({
        "id":s.id, "tag":s.tag, "expect":format!("TODO: define the required behavior for {}",s.tag),
        "test":{"file":test_file},
        "observe":{"faults":["dependency"],"assertions":["behavior"]}
    })).collect();
    let config = json!({"schemaVersion":2,"project":project,"contracts":[{
        "id":contract,"source":source,"boundaries":[],"cases":cases
    }]});
    let runner = match runner {
        Runner::Bun => "bun:test",
        Runner::Vitest => "vitest",
    };
    let mut test = format!(
        "import {{ test }} from {runner:?};\nimport {{ scenarioTest }} from \"@errspec/testkit\";\n\n// Replace each TODO with an application call through s.fault(\"dependency\", fn),\n// then compare its observable behavior with s.check(\"behavior\", actual, expected).\n// The generated tests intentionally fail until real behavior is specified.\n"
    );
    for scenario in scenarios {
        let identity = json!({"contract":contract,"id":scenario.id,"tag":scenario.tag});
        let message = serde_json::to_string(&format!(
            "TODO: implement the {} failure scenario",
            scenario.tag
        ))
        .unwrap();
        test.push_str(&format!("\nscenarioTest(test, import.meta.url, {identity}, async (_s) => {{\n  throw new Error({message});\n}});\n"));
    }
    Ok(Scaffold {
        config: serde_json::to_vec_pretty(&config).map_err(|e| e.to_string())?,
        test,
    })
}
