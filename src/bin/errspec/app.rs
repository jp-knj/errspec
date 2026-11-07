use crate::{
    analysis_backend::{self, Backend, Plan},
    coverage_runner, failure_paths,
    input::{existing_tests, read_json},
    onboard,
    request::{ContractAction, ContractRequest, Request, Runner},
    runner, scan, scenario_source, verification,
};
use errspec::{config::ValidatedConfig, contract, model::*, report::ReportData, snapshot};
use std::{
    fs,
    io::{self, Write},
};

fn run(request: &ContractRequest, mode: &str) -> Result<Report, Finding> {
    if request.strict_baseline && matches!(request.command, ContractAction::Snapshot { .. }) {
        return Err(Finding::new(
            "EC_CONFIG",
            "--strict-baseline is supported only by check and verify",
        ));
    }
    let path = request
        .config
        .canonicalize()
        .map_err(|e| Finding::new("EC_CONFIG", e.to_string()))?;
    let root = path
        .parent()
        .ok_or_else(|| Finding::new("EC_CONFIG", "Config has no parent directory"))?;
    let config: Config = read_json(&path, "EC_CONFIG")?;
    let config = ValidatedConfig::try_from(config)?;
    if request.strict_baseline && config.schema_version() != 2 {
        return Err(Finding::new(
            "EC_CONFIG",
            "--strict-baseline requires schemaVersion: 2 configuration",
        ));
    }
    let mut data = ReportData::default();
    let result = (|| {
        let has_entries = config.contracts().iter().any(|c| c.entry().is_some());
        let plan = Plan {
            sources: config
                .contracts()
                .iter()
                .map(|c| c.source().clone())
                .collect(),
            scenario_files: scenario_source::files(root, &config),
            discriminants: has_entries.then(Vec::new),
            entries: config
                .contracts()
                .iter()
                .filter_map(|c| {
                    Some(analysis_backend::EntryRequest {
                        entry: c.entry()?.clone(),
                        discriminant: c.source().discriminant.clone(),
                    })
                })
                .collect(),
        };
        let mut session =
            match analysis_backend::open(&request.analysis, root, config.project(), plan) {
                Ok(session) => session,
                Err(error) => {
                    data.analysis = error.analysis;
                    data.diagnostics.extend(error.diagnostics);
                    return Ok(());
                }
            };
        data.analysis = session.analysis.clone();
        data.analysis_complete = true;
        if let Backend::Native(adapter) = &session.backend {
            data.assumptions.clone_from(&adapter.assumptions);
        }
        for (index, c) in config.contracts().iter().enumerate() {
            match session.tags(index, root, c.source()) {
                Ok(tags) => {
                    let (result, findings) = contract::compare(c, tags, &existing_tests(root, c));
                    data.registrations.push(result);
                    data.diagnostics.extend(findings);
                }
                Err(error) => {
                    data.analysis_complete = false;
                    let mut f = Finding::new("EC002", error);
                    f.contract_id = Some(c.id().into());
                    f.file = Some(c.source().file.as_str().into());
                    data.diagnostics.push(f);
                }
            }
        }
        if !data.analysis_complete {
            return Ok(());
        }
        data.diagnostics.extend(match &session.backend {
            Backend::Native(adapter) => scenario_source::inspect(adapter, root, &config),
            Backend::Official(response) => {
                scenario_source::inspect_facts(root, &config, &response.scenarios)
            }
        });
        if has_entries {
            let propagation = match &mut session.backend {
                Backend::Native(adapter) => scan::collect(adapter, root, &[]),
                Backend::Official(response) => response
                    .facts
                    .take()
                    .expect("validated scan facts")
                    .into_report(),
            };
            for (index, registration) in data
                .registrations
                .iter()
                .filter(|r| r.contract.entry().is_some())
                .enumerate()
            {
                let contract = registration.contract;
                let entry = contract.entry().expect("entry contract");
                match session.entry(index, root, entry, &contract.source().discriminant) {
                    Ok(facts) => {
                        let analysis =
                            failure_paths::judge(facts, contract, &registration.tags, &propagation);
                        data.paths.insert(contract.id().to_owned(), analysis.paths);
                        data.diagnostics.extend(analysis.findings);
                    }
                    Err(error) => {
                        data.analysis_complete = false;
                        let mut finding = Finding::new("EC002", error);
                        finding.contract_id = Some(contract.id().into());
                        finding.file = Some(entry.file.as_str().into());
                        data.diagnostics.push(finding);
                    }
                }
            }
            if !data.analysis_complete {
                return Ok(());
            }
        }
        if let Some(baseline) = &request.baseline {
            let previous: Snapshot = read_json(baseline, "EC_BASELINE")?;
            if request.strict_baseline
                && (previous.schema_version != 2
                    || previous.contracts.values().any(|c| c.scenarios.is_none()))
            {
                return Err(Finding::new(
                    "EC_BASELINE",
                    "--strict-baseline requires a v2 snapshot with scenario history for every contract",
                ));
            }
            let delta = snapshot::diff(&snapshot::capture(&data.registrations), &previous)?;
            for id in &delta.removed_contracts {
                data.diagnostics.push(Finding::new(
                    "EC007",
                    format!("Contract removed from baseline: {id}"),
                ));
            }
            for (id, scenarios) in &delta.removed_scenarios {
                for scenario in scenarios {
                    let mut finding = Finding::new(
                        "EC013",
                        "Scenario removed from baseline; review before creating a new baseline",
                    )
                    .for_scenario(Some(scenario));
                    finding.contract_id = Some(id.as_str().into());
                    data.diagnostics.push(finding);
                }
            }
            if request.strict_baseline {
                for (id, scenarios) in &delta.changed_scenarios {
                    for scenario in scenarios {
                        let mut finding = Finding::new(
                            "EC014",
                            "Scenario changed from baseline; review before creating a new baseline",
                        )
                        .for_scenario(Some(scenario));
                        finding.contract_id = Some(id.as_str().into());
                        data.diagnostics.push(finding);
                    }
                }
            }
            data.delta = Some(delta);
        }
        match &request.command {
            ContractAction::Check => (),
            ContractAction::Verify {
                runner,
                vitest,
                bun,
                only_contract_tests,
            } => {
                let files = if *only_contract_tests {
                    config
                        .contracts()
                        .iter()
                        .flat_map(|c| c.cases())
                        .map(|c| root.join(&c.test().file))
                        .collect::<std::collections::BTreeSet<_>>()
                        .into_iter()
                        .collect::<Vec<_>>()
                } else {
                    vec![]
                };
                if *only_contract_tests && files.is_empty() {
                    return Err(Finding::new(
                        "EC_CONFIG",
                        "No registered test files to select",
                    ));
                }
                let verification = verification::verify(
                    &data.registrations,
                    root,
                    *runner,
                    vitest.as_deref(),
                    bun.as_deref(),
                    &files,
                )?;
                data.tests = Some(verification.tests);
                data.diagnostics.extend(verification.diagnostics);
                let (observations, findings) = verification.observations?;
                data.observations = Some(observations);
                data.diagnostics.extend(findings);
                if matches!(runner, Runner::Bun) && !data.paths.is_empty() {
                    record_reach(&mut data, &config, root, bun.as_deref())?;
                }
            }
            ContractAction::Snapshot { out } => {
                // A snapshot inventories declared tags; registration gaps remain in the report.
                let data = serde_json::to_vec_pretty(&snapshot::capture(&data.registrations))
                    .map_err(|e| Finding::new("EC_SNAPSHOT", e.to_string()))?;
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(out)
                    .map_err(|e| Finding::new("EC_SNAPSHOT", format!("{}: {e}", out.display())))?;
                file.write_all(&data)
                    .map_err(|e| Finding::new("EC_SNAPSHOT", e.to_string()))?;
            }
        }
        Ok(())
    })();
    if let Err(finding) = result {
        data.fail(finding);
    }
    Ok(data.finish(mode))
}

/// Mark failure paths reached by any SDK scenario in a separate Bun coverage run.
fn record_reach(
    data: &mut ReportData<'_>,
    config: &ValidatedConfig,
    root: &std::path::Path,
    bun: Option<&std::path::Path>,
) -> Result<(), Finding> {
    let files: Vec<_> = config
        .contracts()
        .iter()
        .flat_map(|c| c.cases())
        .map(|c| root.join(&c.test().file))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let lcov = runner::bun_coverage(root, bun, &files)?;
    let resolve = |file: &str| root.join(file).canonicalize().ok();
    let resolve_owned = |file: String| root.join(file).canonicalize().ok();
    let hits = lcov
        .into_iter()
        .filter_map(|(file, lines)| Some((resolve_owned(file)?, lines)))
        .collect();
    for (contract, paths) in &mut data.paths {
        let findings = errspec::reach::judge(contract, paths, &hits, resolve);
        data.diagnostics.extend(findings);
    }
    Ok(())
}

/// Results crossing the execution/output boundary contain no compiler handles.
pub(crate) enum Outcome {
    TestSelection(Box<errspec::mutation::test_selection::Report>),
    Command(crate::command_output::Output),
    Mutation(Box<errspec::mutation::Report>),
    Contract(Report),
    Scan {
        report: errspec::discovery::ScanReport,
        fail_on_unknown: bool,
    },
    Init(Result<onboard::InitReport, onboard::InitError>),
    Coverage {
        report: errspec::coverage::CoverageReport,
        fail_on_missing: bool,
    },
}

pub(crate) fn execute(request: Request) -> Outcome {
    match request {
        Request::InitSkill { directory, force } => {
            Outcome::Command(crate::skill_actions::run(&directory, force))
        }
        Request::Accept {
            root,
            file,
            keys,
            reason,
        } => Outcome::Command(crate::mutation_actions::accept(
            &root, &file, &keys, &reason,
        )),
        Request::Mutation(request) => {
            Outcome::Mutation(Box::new(crate::mutation_config::run(*request)))
        }
        Request::SelectTests(request) => {
            Outcome::TestSelection(Box::new(crate::mutation_config::select_tests(*request)))
        }
        Request::MutationSource
        | Request::MutationApi
        | Request::FormatReport
        | Request::MutationSupervisor(_) => {
            unreachable!("hidden source entry handled by main")
        }
        Request::Contract(request) => {
            let mode = request.command.mode();
            let report = worker(Some("errspec"), move || {
                run(&request, mode).unwrap_or_else(|finding| failed_report(mode, finding))
            })
            .unwrap_or_else(|error| {
                let message = match error {
                    WorkerError::Spawn(error) => error.to_string(),
                    WorkerError::Panicked => "tsrs analysis panicked".into(),
                };
                failed_report(mode, Finding::new("EC_INTERNAL", message))
            });
            Outcome::Contract(report)
        }
        Request::Scan(request) => {
            let fail_on_unknown = request.fail_on_unknown;
            let report = worker(None, move || {
                scan::run(&request.project, &request.discriminants, &request.analysis)
            })
            .unwrap_or_else(|_| {
                let mut report = errspec::discovery::ScanReport::default();
                report
                    .diagnostics
                    .push(Finding::new("EC_INTERNAL", "Scan worker failed"));
                report
            });
            Outcome::Scan {
                report,
                fail_on_unknown,
            }
        }
        Request::Coverage(request) => {
            let fail_on_missing = request.fail_on_missing;
            let report =
                worker(None, move || coverage_runner::run(&request)).unwrap_or_else(|_| {
                    let mut report = errspec::coverage::CoverageReport::default();
                    report
                        .diagnostics
                        .push(Finding::new("EC_INTERNAL", "Coverage worker failed"));
                    report
                });
            Outcome::Coverage {
                report,
                fail_on_missing,
            }
        }
        Request::Init(request) => {
            let result = worker(None, move || onboard::run(request))
                .map_err(|error| {
                    onboard::InitError::from(match error {
                        WorkerError::Spawn(error) => error.to_string(),
                        WorkerError::Panicked => "Init worker failed".into(),
                    })
                })
                .and_then(|result| result);
            Outcome::Init(result)
        }
    }
}

fn failed_report(mode: &str, finding: Finding) -> Report {
    let mut data = ReportData::default();
    data.fail(finding);
    data.finish(mode)
}

enum WorkerError {
    Spawn(io::Error),
    Panicked,
}

fn worker<T: Send + 'static>(
    name: Option<&str>,
    task: impl FnOnce() -> T + Send + 'static,
) -> Result<T, WorkerError> {
    // Deep compiler traversals need more stack than the platform default.
    // All compiler work and arena handles stay inside this one-shot worker.
    let mut builder = std::thread::Builder::new().stack_size(32 * 1024 * 1024);
    if let Some(name) = name {
        builder = builder.name(name.to_owned());
    }
    builder
        .spawn(task)
        .map_err(WorkerError::Spawn)?
        .join()
        .map_err(|_| WorkerError::Panicked)
}
