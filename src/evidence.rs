//! Runner-independent evidence. Registration and passing tests remain separate facts.
use crate::model::{Finding, Report};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub struct RunEvidence {
    pub runner: &'static str,
    pub success: bool,
    pub tests: Vec<TestEvidence>,
}

pub struct TestEvidence {
    pub file: String,
    pub name: String,
    pub status: String,
}

fn canonical(root: &Path, file: &str) -> Option<PathBuf> {
    root.join(file).canonicalize().ok()
}

pub fn apply(report: &mut Report, root: &Path, evidence: RunEvidence, process_ok: bool) {
    report.tests_executed = true;
    report.test_runner = Some(evidence.runner);
    let mut results: BTreeMap<(PathBuf, String), Vec<String>> = BTreeMap::new();
    for test in evidence.tests {
        if let Some(path) = canonical(root, &test.file) {
            results
                .entry((path, test.name))
                .or_default()
                .push(test.status);
        }
    }
    for contract in &mut report.contracts {
        for case in &mut contract.cases {
            let Some(test) = &case.test else { continue };
            let matches = canonical(root, &test.file)
                .and_then(|path| results.get(&(path, test.name.clone())));
            case.test_status = match matches {
                None => "not_found".into(),
                Some(values) if values.len() != 1 => "ambiguous".into(),
                Some(values) => values[0].clone(),
            };
            if case.test_status != "passed" {
                report.diagnostics.push(
                    Finding::new(
                        "EC005",
                        format!("Referenced test is {}: {}", case.test_status, test.name),
                    )
                    .for_case(&contract.id, &case.tag)
                    .for_scenario(case.id.as_deref()),
                );
            }
        }
    }
    if !process_ok || !evidence.success {
        report.diagnostics.push(Finding::new(
            "EC006",
            format!(
                "{} run failed; passing individual tests do not make the run successful",
                evidence.runner
            ),
        ));
    }
}
