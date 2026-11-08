//! Judge acquired compiler facts without processes, file I/O, or report mutation.
use super::{Status, TypecheckInfo};
use serde_json::Value;
use std::{collections::BTreeSet, fmt};

/// Aggregate information decoded after validating the response envelope, followed
/// by candidate facts in response order. Candidate fields are not yet validated.
#[derive(Debug)]
pub struct Facts {
    pub info: TypecheckInfo,
    pub outcomes: Vec<Outcome>,
}

/// Keep missing fields distinct from explicit null and invalid values so that
/// validation can stop at the first invalid candidate, retaining earlier updates.
#[derive(Clone, Debug)]
pub struct Outcome {
    pub key: Option<Value>,
    pub diagnostic: Option<Value>,
    pub equivalent_reason: Option<Value>,
}

/// A confirmed status/reason change. A valid pair of null facts emits no update.
#[derive(Debug, PartialEq, Eq)]
pub struct Update {
    pub key: String,
    pub status: Status,
    pub reason: String,
}

/// Apply all updates in order before handling completion, including on failure.
#[derive(Debug)]
pub struct Evaluation {
    pub updates: Vec<Update>,
    pub completion: Result<TypecheckInfo, Error>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    MissingKey,
    UnknownOrDuplicateKey,
    InvalidCompilerFacts,
    IncompleteResults,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MissingKey => "Missing typecheck key",
            Self::UnknownOrDuplicateKey => "Unknown or duplicate typecheck key",
            Self::InvalidCompilerFacts => "Invalid typecheck compiler facts",
            Self::IncompleteResults => "Incomplete typecheck results or counts",
        })
    }
}

impl std::error::Error for Error {}

/// Validate candidates in response order, then check completeness and counts.
/// `expected_keys` contains the selected, non-ignored TypeScript candidates.
pub fn evaluate(expected_keys: &BTreeSet<String>, facts: Facts) -> Evaluation {
    let mut updates = Vec::new();
    let completion = validate(expected_keys, &facts, &mut updates).map(|()| facts.info);
    Evaluation {
        updates,
        completion,
    }
}

fn validate(
    expected_keys: &BTreeSet<String>,
    facts: &Facts,
    updates: &mut Vec<Update>,
) -> Result<(), Error> {
    let mut seen = BTreeSet::new();
    let mut errors = 0;
    let mut equivalent = 0;
    for outcome in &facts.outcomes {
        let key = outcome
            .key
            .as_ref()
            .and_then(Value::as_str)
            .ok_or(Error::MissingKey)?;
        if !expected_keys.contains(key) || !seen.insert(key.to_owned()) {
            return Err(Error::UnknownOrDuplicateKey);
        }
        let (status, reason) = match (&outcome.diagnostic, &outcome.equivalent_reason) {
            (Some(Value::Null), Some(Value::Null)) => continue,
            (Some(Value::String(reason)), Some(Value::Null)) if !reason.is_empty() => {
                errors += 1;
                (Status::CompileError, reason)
            }
            (Some(Value::Null), Some(Value::String(reason)))
                if reason.starts_with("equivalent:") =>
            {
                equivalent += 1;
                (Status::Ignored, reason)
            }
            _ => return Err(Error::InvalidCompilerFacts),
        };
        updates.push(Update {
            key: key.to_owned(),
            status,
            reason: reason.clone(),
        });
    }
    if *expected_keys != seen
        || facts.info.checked != facts.outcomes.len()
        || facts.info.compile_errors != errors
        || facts.info.equivalent != equivalent
        || facts.info.version.is_empty()
    {
        return Err(Error::IncompleteResults);
    }
    Ok(())
}
