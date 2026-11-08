//! Acquire owned typecheck values through the driver, without accessing a report.
use super::{
    Failure,
    driver::{Driver, Response, expected, only},
};
use errspec::mutation::typecheck::{Facts, Outcome};
use serde_json::json;
use std::time::Duration;

pub(super) fn acquire(driver: &mut Driver, keys: &[&str]) -> Result<Facts, Failure> {
    let response = driver.request(
        json!({"op":"typecheck", "keys":keys}),
        Duration::MAX,
        Duration::from_secs(120),
    )?;
    decode(response)
}

pub(super) fn decode(response: Response) -> Result<Facts, Failure> {
    if response.timeout {
        return Err(Failure::new(
            "EC_MUTATION_TYPECHECK",
            "Mutant typechecker timed out",
        ));
    }
    response.check_error()?;
    let result = only(&response.messages, "typechecked")?;
    let outcomes = result["outcomes"]
        .as_array()
        .ok_or_else(|| Failure::protocol("Missing typecheck outcomes"))?;
    expected(
        &response.messages,
        &["progress", "typechecked", "done"],
        outcomes.len(),
    )?;
    let info = serde_json::from_value(result["info"].clone())
        .map_err(|e| Failure::protocol(e.to_string()))?;
    Ok(Facts {
        info,
        outcomes: outcomes
            .iter()
            .map(|outcome| Outcome {
                key: outcome.get("key").cloned(),
                diagnostic: outcome.get("diagnostic").cloned(),
                equivalent_reason: outcome.get("equivalentReason").cloned(),
            })
            .collect(),
    })
}
