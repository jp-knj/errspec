//! Convert Vitest JSON into runner-independent evidence.
use crate::evidence::{RunEvidence, TestEvidence};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VitestReport {
    pub success: bool,
    pub test_results: Vec<TestFile>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestFile {
    pub name: String,
    pub assertion_results: Vec<Assertion>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Assertion {
    pub full_name: String,
    pub status: String,
}

pub fn normalize(report: VitestReport) -> RunEvidence {
    RunEvidence {
        runner: "vitest",
        success: report.success,
        tests: report
            .test_results
            .into_iter()
            .flat_map(|file| {
                file.assertion_results
                    .into_iter()
                    .map(move |test| TestEvidence {
                        file: file.name.clone(),
                        name: test.full_name,
                        status: test.status,
                    })
            })
            .collect(),
    }
}
