//! Assemble the wire report only after the registration and execution judgments.
use crate::{
    contract::Registration,
    evidence::{TestRun, TestStatus},
    model::*,
    observation::ObservationJudgments,
};

#[derive(Default)]
pub struct ReportData<'a> {
    pub analysis: Option<AnalysisInfo>,
    pub analysis_complete: bool,
    pub registrations: Vec<Registration<'a>>,
    pub tests: Option<TestRun>,
    pub observations: Option<ObservationJudgments>,
    pub diagnostics: Vec<Finding>,
    pub delta: Option<Delta>,
}

impl ReportData<'_> {
    pub fn fail(&mut self, finding: Finding) {
        self.analysis_complete = false;
        self.diagnostics.push(finding);
    }

    pub fn finish(self, mode: &str) -> Report {
        let mut report = Report::new(mode);
        if let Some(info) = self.analysis {
            report.engine = Engine {
                name: info.engine.clone(),
                revision: info.version.clone(),
            };
            report.analysis = Some(info);
        }
        report.analysis_complete = self.analysis_complete;
        report.tests_executed = self.tests.is_some();
        report.test_runner = self.tests.as_ref().map(|tests| tests.runner);
        report.diagnostics = self.diagnostics;
        report.delta = self.delta;
        report.contracts = self
            .registrations
            .iter()
            .map(|registration| {
                let contract = registration.contract;
                ContractResult {
                    id: contract.id().to_owned(),
                    source: contract.source().clone(),
                    declared_boundaries: contract.boundaries().to_vec(),
                    tags: registration.tags.clone(),
                    cases: registration
                        .cases
                        .iter()
                        .map(|case| {
                            let registered = case.registered();
                            let scenario = case.scenario();
                            let observed = scenario.and_then(|(id, _)| {
                                self.observations.as_ref()?.get(contract.id(), id)
                            });
                            let test_status = case
                                .test()
                                .and_then(|test| {
                                    self.tests.as_ref().map(|tests| tests.status(test))
                                })
                                .unwrap_or(&TestStatus::NotRun);
                            CaseResult {
                                id: scenario.map(|(id, _)| id.to_owned()),
                                tag: case.tag().to_owned(),
                                registration: if registered.is_some() {
                                    "registered"
                                } else {
                                    "missing"
                                },
                                test_status: test_status.as_str().to_owned(),
                                test: case.test().cloned(),
                                expect: registered.map(|case| case.expect().to_owned()),
                                observe: scenario.map(|(_, required)| required.clone()),
                                observation_status: observed.map(|j| j.status).unwrap_or(
                                    if scenario.is_some() {
                                        ObservationStatus::NotRun
                                    } else {
                                        ObservationStatus::NotRequired
                                    },
                                ),
                                observed_faults: observed
                                    .map(|j| j.faults.clone())
                                    .unwrap_or_default(),
                                observed_assertions: observed
                                    .map(|j| j.assertions.clone())
                                    .unwrap_or_default(),
                            }
                        })
                        .collect(),
                }
            })
            .collect();
        report.ok = report.analysis_complete && report.diagnostics.is_empty();
        report
    }
}
