use crate::{
    Action, Cli,
    input::{existing_tests, read_json},
    runner, tsrs_adapter,
};
use errspec::{config::ValidatedConfig, contract, model::*, report::ReportData, snapshot};
use std::{
    fs,
    io::{self, Write},
};

fn run(cli: &Cli, mode: &str) -> Result<Report, Finding> {
    if cli.strict_baseline && matches!(cli.command, Action::Snapshot { .. }) {
        return Err(Finding::new(
            "EC_CONFIG",
            "--strict-baseline is supported only by check and verify",
        ));
    }
    let path = cli
        .config
        .canonicalize()
        .map_err(|e| Finding::new("EC_CONFIG", e.to_string()))?;
    let root = path
        .parent()
        .ok_or_else(|| Finding::new("EC_CONFIG", "Config has no parent directory"))?;
    let config: Config = read_json(&path, "EC_CONFIG")?;
    let config = ValidatedConfig::try_from(config)?;
    if cli.strict_baseline && config.schema_version() != 2 {
        return Err(Finding::new(
            "EC_CONFIG",
            "--strict-baseline requires schemaVersion: 2 configuration",
        ));
    }
    let mut data = ReportData::default();
    let result = (|| {
        let adapter = match tsrs_adapter::TsrsAdapter::open(root, config.project()) {
            Ok(adapter) => adapter,
            Err(errors) => {
                data.diagnostics.extend(errors);
                return Ok(());
            }
        };
        data.analysis_complete = true;
        for c in config.contracts() {
            match adapter.tags(root, c.source()) {
                Ok(tags) => {
                    let (result, findings) = contract::compare(c, tags, &existing_tests(root, c));
                    data.registrations.push(result);
                    data.diagnostics.extend(findings);
                }
                Err(error) => {
                    data.analysis_complete = false;
                    let mut f = Finding::new("EC002", error);
                    f.contract_id = Some(c.id().into());
                    f.file = Some(c.source().file.clone());
                    data.diagnostics.push(f);
                }
            }
        }
        if !data.analysis_complete {
            return Ok(());
        }
        if let Some(baseline) = &cli.baseline {
            let previous: Snapshot = read_json(baseline, "EC_BASELINE")?;
            if cli.strict_baseline
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
            if cli.strict_baseline {
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
        match &cli.command {
            Action::Scan { .. } | Action::Init { .. } => unreachable!("separate report"),
            Action::Check => (),
            Action::Verify {
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
                let verification = runner::verify(
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
            }
            Action::Snapshot { out } => {
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

pub(crate) fn execute(cli: Cli) -> Report {
    let mode = match cli.command {
        Action::Scan { .. } | Action::Init { .. } => unreachable!("separate report"),
        Action::Check => "check",
        Action::Verify { .. } => "verify",
        Action::Snapshot { .. } => "snapshot",
    };
    // Deep compiler traversals need more stack than the platform default.
    let worker = std::thread::Builder::new()
        .name("errspec".into())
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            run(&cli, mode).unwrap_or_else(|finding| {
                let mut data = ReportData::default();
                data.fail(finding);
                data.finish(mode)
            })
        });
    match worker.and_then(|t| {
        t.join()
            .map_err(|_| io::Error::other("tsrs analysis panicked"))
    }) {
        Ok(report) => report,
        Err(error) => {
            let mut r = Report::new(mode);
            r.diagnostics
                .push(Finding::new("EC_INTERNAL", error.to_string()));
            r
        }
    }
}
