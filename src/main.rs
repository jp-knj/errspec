mod tsrs_adapter;
use clap::{Parser, Subcommand, ValueEnum};
use errspec::{bun, contract, evidence, model::*, observation, vitest};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

#[derive(Parser)]
#[command(
    version,
    about = "Connect TypeScript error variants to explicit failure contracts and tests"
)]
struct Cli {
    #[command(subcommand)]
    command: Action,
    #[arg(long, global = true, default_value = "errspec.json")]
    config: PathBuf,
    #[arg(long, global = true, value_enum, default_value = "text")]
    format: Format,
    #[arg(long, global = true)]
    baseline: Option<PathBuf>,
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Json,
    Text,
}

#[derive(Clone, Copy, ValueEnum)]
enum Runner {
    Vitest,
    Bun,
}

#[derive(Subcommand)]
enum Action {
    /// Resolve error tags and check registrations. Does not execute tests.
    Check,
    /// Run Vitest or Bun test and match exact test file + full name.
    Verify {
        #[arg(long, value_enum, default_value = "vitest")]
        runner: Runner,
        /// Vitest executable; defaults to <config directory>/node_modules/.bin/vitest.
        #[arg(long, conflicts_with = "bun")]
        vitest: Option<PathBuf>,
        /// Bun executable; defaults to bun on PATH.
        #[arg(long, conflicts_with = "vitest")]
        bun: Option<PathBuf>,
    },
    /// Write an explicit baseline. Refuses to overwrite an existing file.
    Snapshot {
        #[arg(long)]
        out: PathBuf,
    },
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path, code: &str) -> Result<T, Finding> {
    let bytes =
        fs::read(path).map_err(|e| Finding::new(code, format!("{}: {e}", path.display())))?;
    serde_json::from_slice(&bytes)
        .map_err(|e| Finding::new(code, format!("{}: {e}", path.display())))
}

fn verify(
    report: &mut Report,
    root: &Path,
    runner: Runner,
    vitest: Option<&Path>,
    bun: Option<&Path>,
) -> Result<(), Finding> {
    if matches!(runner, Runner::Bun) && vitest.is_some()
        || matches!(runner, Runner::Vitest) && bun.is_some()
    {
        return Err(Finding::new(
            "EC_CONFIG",
            "Executable option does not match --runner",
        ));
    }
    let explicit = match runner {
        Runner::Vitest => Some(
            vitest
                .map(Path::to_path_buf)
                .unwrap_or_else(|| root.join("node_modules/.bin/vitest")),
        ),
        Runner::Bun => bun.map(Path::to_path_buf),
    };
    let executable = if let Some(path) = explicit {
        path.canonicalize().map_err(|e| {
            Finding::new(
                "EC_RUNNER",
                format!("Cannot open test runner executable: {e}"),
            )
        })?
    } else {
        PathBuf::from("bun")
    };
    let temp = tempfile::tempdir().map_err(|e| Finding::new("EC_RUNNER", e.to_string()))?;
    let output_file = temp.path().join(match runner {
        Runner::Vitest => "vitest.json",
        Runner::Bun => "bun.xml",
    });
    let mut command = Command::new(executable);
    command.current_dir(root);
    let observations = temp.path().join("observations");
    fs::create_dir(&observations).map_err(|e| Finding::new("EC_RUNNER", e.to_string()))?;
    // Private directory and run id prevent accidental reuse across executions.
    // The test process is trusted; these are not credentials or attestations.
    let run_id = temp.path().to_string_lossy().into_owned();
    command
        .env("ERRSPEC_OBSERVATIONS", &observations)
        .env("ERRSPEC_RUN_ID", &run_id);
    match runner {
        Runner::Vitest => {
            command
                .args(["run", "--reporter=json", "--outputFile"])
                .arg(&output_file);
        }
        Runner::Bun => {
            command
                .args(["test", "--reporter=junit", "--reporter-outfile"])
                .arg(&output_file);
        }
    }
    let output = command
        .output()
        .map_err(|e| Finding::new("EC_RUNNER", e.to_string()))?;
    // Keep stdout machine-readable even if the test runner writes logs.
    let mut stderr = io::stderr().lock();
    let _ = stderr.write_all(&output.stdout);
    let _ = stderr.write_all(&output.stderr);
    let result = match runner {
        Runner::Vitest => vitest::normalize(read_json(&output_file, "EC_RUNNER")?),
        Runner::Bun => {
            let xml = fs::read_to_string(&output_file)
                .map_err(|e| Finding::new("EC_RUNNER", format!("Bun report: {e}")))?;
            bun::parse_junit(&xml).map_err(|e| Finding::new("EC_RUNNER", e))?
        }
    };
    evidence::apply(report, root, result, output.status.success());
    let records = observation::read(&observations, &run_id)?;
    observation::apply(report, root, &records);
    Ok(())
}

fn run(cli: &Cli, report: &mut Report) -> Result<(), Finding> {
    let path = cli
        .config
        .canonicalize()
        .map_err(|e| Finding::new("EC_CONFIG", e.to_string()))?;
    let root = path
        .parent()
        .ok_or_else(|| Finding::new("EC_CONFIG", "Config has no parent directory"))?;
    let config: Config = read_json(&path, "EC_CONFIG")?;
    contract::validate(&config)?;
    let adapter = match tsrs_adapter::TsrsAdapter::open(root, &config) {
        Ok(adapter) => adapter,
        Err(errors) => {
            report.diagnostics.extend(errors);
            return Ok(());
        }
    };
    report.analysis_complete = true;
    for c in &config.contracts {
        match adapter.tags(root, &c.source) {
            Ok(tags) => {
                let (result, findings) = contract::compare(root, c, tags);
                report.contracts.push(result);
                report.diagnostics.extend(findings);
            }
            Err(error) => {
                report.analysis_complete = false;
                let mut f = Finding::new("EC002", error);
                f.contract_id = Some(c.id.as_str().into());
                f.file = Some(c.source.file.clone());
                report.diagnostics.push(f);
            }
        }
    }
    if !report.analysis_complete {
        return Ok(());
    }
    if let Some(baseline) = &cli.baseline {
        let previous: Snapshot = read_json(baseline, "EC_BASELINE")?;
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
        report.delta = Some(delta);
    }
    match &cli.command {
        Action::Check => (),
        Action::Verify {
            runner,
            vitest,
            bun,
        } => verify(report, root, *runner, vitest.as_deref(), bun.as_deref())?,
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

fn main() -> ExitCode {
    let cli = Cli::parse();
    let format = cli.format;
    let mode = match cli.command {
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
    let report = match worker.and_then(|t| {
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
    };
    let output = match format {
        Format::Json => serde_json::to_string_pretty(&report)
            .expect("report serialization uses only JSON primitives"),
        Format::Text => {
            let mut lines = vec![format!(
                "{}: {} (analysis complete: {}, tests executed: {})",
                report.mode,
                if report.ok { "pass" } else { "fail" },
                report.analysis_complete,
                report.tests_executed
            )];
            for c in &report.contracts {
                lines.push(format!("{}: {}", c.id, c.tags.join(", ")));
                for case in &c.cases {
                    lines.push(format!(
                        "  {} {}: {}, test={}, observation={:?}",
                        case.tag,
                        case.id.as_deref().unwrap_or(""),
                        case.registration,
                        case.test_status,
                        case.observation_status
                    ));
                }
            }
            for d in &report.diagnostics {
                lines.push(format!(
                    "{} {} {} {}: {}",
                    d.code,
                    d.contract_id.as_deref().unwrap_or(""),
                    d.tag.as_deref().unwrap_or(""),
                    d.scenario_id.as_deref().unwrap_or(""),
                    d.message
                ));
            }
            if let Some(delta) = &report.delta {
                lines.push(format!("delta: {}", serde_json::to_string(delta).unwrap()));
            }
            lines.join("\n")
        }
    };
    if writeln!(io::stdout().lock(), "{output}").is_err() {
        return ExitCode::from(2);
    }
    ExitCode::from(if report.ok {
        0
    } else if report.analysis_complete {
        1
    } else {
        2
    })
}
