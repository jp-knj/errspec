//! Bounded, deterministic observation file input.
use crate::{
    model::Finding,
    observation::{Observation, RunContext, ValidatedObservations},
};
use std::{fs, path::Path};

fn invalid(message: impl Into<String>) -> Finding {
    Finding::new("EC_EVIDENCE", message)
}

pub fn read(directory: &Path, run: &RunContext) -> Result<ValidatedObservations, Finding> {
    let mut files = fs::read_dir(directory)
        .map_err(|e| invalid(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| invalid(e.to_string()))?;
    files.sort_by_key(|e| e.file_name());
    if files.len() > 10_000 {
        return Err(invalid("Too many observation files (limit 10000)"));
    }
    let mut total = 0;
    let mut records = ValidatedObservations::new(run);
    for entry in files {
        let path = entry.path();
        if !entry
            .file_type()
            .map_err(|e| invalid(e.to_string()))?
            .is_file()
            || path.extension().is_none_or(|e| e != "json")
        {
            return Err(invalid("Incomplete or unexpected observation file"));
        }
        let size = entry.metadata().map_err(|e| invalid(e.to_string()))?.len();
        total += size;
        if size > 1_048_576 || total > 33_554_432 {
            return Err(invalid("Observation size limit exceeded"));
        }
        let bytes = fs::read(&path).map_err(|e| invalid(e.to_string()))?;
        let record: Observation = serde_json::from_slice(&bytes)
            .map_err(|e| invalid(format!("Invalid observation JSON: {e}")))?;
        records.push(record)?;
    }
    Ok(records)
}
