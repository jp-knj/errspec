//! Source request validation shared by single-shot and persistent acquisition.
use crate::mutation_source::{self, SourceRequest};
use errspec::mutation::cache::ENGINE_FINGERPRINT;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Batch {
    requests: Vec<SourceRequest>,
}

fn authenticate(input: &mut Value) -> Result<(), String> {
    if let Some(expected) = input
        .as_object_mut()
        .and_then(|object| object.remove("engineFingerprint"))
        && expected.as_str() != Some(ENGINE_FINGERPRINT)
    {
        return Err("EC_MUTATION_INPUT: Native mutation engine changed during execution".into());
    }
    Ok(())
}
pub(super) fn decode(mut input: Value) -> Result<SourceRequest, String> {
    authenticate(&mut input)?;
    serde_json::from_value(input)
        .map_err(|e| format!("EC_MUTATION_PROTOCOL: Invalid generator request: {e}"))
}
pub(super) fn respond(input: &str) -> Result<String, String> {
    let mut input: Value = serde_json::from_str(input).map_err(|e| e.to_string())?;
    authenticate(&mut input)?;
    if input.get("requests").is_some() {
        let batch: Batch = serde_json::from_value(input).map_err(|e| e.to_string())?;
        if batch.requests.is_empty() || batch.requests.len() > 8 {
            return Err("Native parser batches require 1–8 requests".into());
        }
        let results: Vec<_> = batch
            .requests
            .into_iter()
            .map(mutation_source::generate)
            .collect();
        serde_json::to_string(&results).map_err(|e| e.to_string())
    } else {
        let request = serde_json::from_value(input).map_err(|e| e.to_string())?;
        let result = mutation_source::generate(request)?;
        serde_json::to_string(&result).map_err(|e| e.to_string())
    }
}
