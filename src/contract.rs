//! Compare declared tags with validated registrations using pre-execution file facts.
use crate::{
    config::{CaseKind, ValidatedCase, ValidatedContract},
    model::{Finding, ObservationRequirements, TestRef},
};
use std::collections::BTreeSet;

#[derive(Debug)]
pub struct Registration<'a> {
    pub contract: &'a ValidatedContract,
    pub tags: Vec<String>,
    pub cases: Vec<CaseRegistration<'a>>,
}

#[derive(Debug)]
pub enum CaseRegistration<'a> {
    Missing(String),
    Registered(&'a ValidatedCase),
}

impl CaseRegistration<'_> {
    pub fn registered(&self) -> Option<&ValidatedCase> {
        match self {
            Self::Missing(_) => None,
            Self::Registered(case) => Some(case),
        }
    }
    pub fn tag(&self) -> &str {
        match self {
            Self::Missing(tag) => tag,
            Self::Registered(case) => case.tag(),
        }
    }
    pub fn test(&self) -> Option<&TestRef> {
        self.registered().map(ValidatedCase::test)
    }
    pub fn scenario(&self) -> Option<(&str, &ObservationRequirements)> {
        match self.registered()?.kind() {
            CaseKind::Legacy => None,
            CaseKind::Scenario { id, observe } => Some((id, observe)),
        }
    }
    pub fn finding(&self, contract: &str, code: &str, message: impl Into<String>) -> Finding {
        Finding::new(code, message)
            .for_case(contract, self.tag())
            .for_scenario(self.scenario().map(|(id, _)| id))
    }
}

/// Compare only contracts obtained through configuration validation.
///
/// ```compile_fail
/// use errspec::{contract, model::Contract};
/// fn unchecked(raw: &Contract) {
///     contract::compare(raw, vec![], &Default::default());
/// }
/// ```
pub fn compare<'a>(
    contract: &'a ValidatedContract,
    tags: Vec<String>,
    existing_files: &BTreeSet<String>,
) -> (Registration<'a>, Vec<Finding>) {
    let mut findings = Vec::new();
    let mut cases = Vec::new();
    for tag in &tags {
        let registered: Vec<_> = contract
            .cases()
            .iter()
            .filter(|case| case.tag() == tag)
            .collect();
        if registered.is_empty() {
            findings.push(
                Finding::new(
                    "EC001",
                    "Error variant has no registered failure contract and test",
                )
                .for_case(contract.id(), tag),
            );
            cases.push(CaseRegistration::Missing(tag.clone()));
        }
        for case in registered {
            let file = &case.test().file;
            let case = CaseRegistration::Registered(case);
            if !existing_files.contains(file) {
                findings.push(case.finding(
                    contract.id(),
                    "EC003",
                    format!("Referenced test file does not exist: {file}"),
                ));
            }
            cases.push(case);
        }
    }
    for case in contract.cases() {
        if !tags.iter().any(|tag| tag == case.tag()) {
            findings.push(CaseRegistration::Registered(case).finding(
                contract.id(),
                "EC004",
                "Registered tag no longer exists in the error type",
            ));
        }
    }
    (
        Registration {
            contract,
            tags,
            cases,
        },
        findings,
    )
}
