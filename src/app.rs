use crate::{Action, Cli, input::read_json, runner, tsrs_adapter};
use errspec::{config::ValidatedConfig, contract, model::*};
use std::{
    fs,
    io::{self, Write},
};

fn run(cli: &Cli, report: &mut Report) -> Result<(), Finding> {
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
    let adapter = match tsrs_adapter::TsrsAdapter::open(root, config.project()) {
        Ok(adapter) => adapter,
        Err(errors) => {
            report.diagnostics.extend(errors);
            return Ok(());
        }
    };
    report.analysis_complete = true;
    for c in config.contracts() {
        match adapter.tags(root, c.source()) {
            Ok(tags) => {
                let (result, findings) = contract::compare(root, c, tags);
                report.contracts.push(result);
                report.diagnostics.extend(findings);
            }
            Err(error) => {
                report.analysis_complete = false;
                let mut f = Finding::new("EC002", error);
                f.contract_id = Some(c.id().into());
                f.file = Some(c.source().file.clone());
                report.diagnostics.push(f);
            }
        }
    }
    if !report.analysis_complete {
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
        let delta = contract::diff(&contract::snapshot(report), &previous)?;
        for id in &delta.removed_contracts {
            report.diagnostics.push(Finding::new(
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
                report.diagnostics.push(finding);
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
                    report.diagnostics.push(finding);
                }
            }
        }
        report.delta = Some(delta);
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
            runner::verify(
                report,
                root,
                *runner,
                vitest.as_deref(),
                bun.as_deref(),
                &files,
            )?;
        }
        Action::Snapshot { out } => {
            // A snapshot inventories declared tags; missing registrations remain visible in this report.
            let data = serde_json::to_vec_pretty(&contract::snapshot(report))
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
            let mut report = Report::new(mode);
            if let Err(finding) = run(&cli, &mut report) {
                report.analysis_complete = false;
                report.diagnostics.push(finding);
            }
            report.ok = report.analysis_complete && report.diagnostics.is_empty();
            report
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
