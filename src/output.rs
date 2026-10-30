use crate::{app::Outcome, cli::Format, onboard::InitReport};
use errspec::model::Report;
use std::{
    io::{self, Write},
    process::ExitCode,
};

fn write_contract(report: &Report, format: Format) -> ExitCode {
    let output = match format {
        Format::Json => serde_json::to_string_pretty(report)
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
                        "  {} {}: {}, test={}, observation={}",
                        case.tag,
                        case.id.as_deref().unwrap_or(""),
                        case.registration,
                        case.test_status,
                        case.observation_status.as_str()
                    ));
                }
                for path in &c.paths {
                    let reached = match path.reached {
                        Some(true) => "reached",
                        Some(false) => "not reached",
                        None => "not recorded",
                    };
                    lines.push(format!(
                        "  path {}:{} {}: {reached}",
                        path.file,
                        path.line,
                        path.tags.join(" | ")
                    ));
                }
            }
            for d in &report.diagnostics {
                let location = d.file.as_ref().map(|file| match d.line {
                    Some(line) => format!("{file}:{line}"),
                    None => file.to_string(),
                });
                let subject = [
                    Some(d.code.as_str()),
                    d.contract_id.as_deref(),
                    d.tag.as_deref(),
                    d.scenario_id.as_deref(),
                    location.as_deref(),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ");
                lines.push(format!("{subject}: {}", d.message));
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

pub(crate) fn write(outcome: Outcome, format: Format) -> ExitCode {
    match outcome {
        Outcome::Contract(report) => write_contract(&report, format),
        Outcome::Scan {
            report,
            fail_on_unknown,
        } => write_scan(&report, fail_on_unknown, format),
        Outcome::Init(result) => write_init(result, format),
    }
}

fn write_init(result: Result<InitReport, String>, format: Format) -> ExitCode {
    match result {
        Ok(report) => {
            match format {
                // Init originally returned a JSON object. Keep its sorted keys and formatting.
                Format::Json => println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::to_value(&report).unwrap()).unwrap()
                ),
                Format::Text => {
                    println!(
                        "Created configuration and tests: {}",
                        serde_json::to_string(&report.files).unwrap()
                    );
                    println!(
                        "Tests intentionally fail until you specify the expected behavior. Install @errspec/testkit and your test runner, then run errspec verify."
                    );
                }
            };
            ExitCode::SUCCESS
        }
        Err(error) => {
            match format {
                Format::Json => println!(
                    "{}",
                    serde_json::json!({"kind":"errspec/init","schemaVersion":1,"ready":false,"error":error})
                ),
                Format::Text => eprintln!("EC_INIT {error}"),
            };
            ExitCode::from(2)
        }
    }
}

fn write_scan(
    report: &errspec::discovery::ScanReport,
    fail_on_unknown: bool,
    format: Format,
) -> ExitCode {
    let code = if !report.scan_complete {
        2
    } else if fail_on_unknown && !report.unknowns.is_empty() {
        1
    } else {
        0
    };
    match format {
        Format::Json => println!("{}", serde_json::to_string_pretty(report).unwrap()),
        Format::Text => {
            println!(
                "{} files, {} candidates, {} origins, {} calls, {} unknowns",
                report.files.len(),
                report.candidates.len(),
                report.origins.len(),
                report.edges.len(),
                report.unknowns.len()
            );
            // Local declarations first; dependency-only types are listed after them.
            let (local, external): (Vec<_>, Vec<_>) =
                report.candidates.iter().partition(|c| !c.external);
            for c in local.into_iter().chain(external) {
                println!(
                    "{}{} export {} [{}]: {}",
                    if c.external { "external " } else { "" },
                    c.source.file,
                    c.source.export,
                    c.source.discriminant,
                    c.tags.join(", ")
                );
                if !c.reexports.is_empty() {
                    let paths: Vec<_> = c
                        .reexports
                        .iter()
                        .map(|e| format!("{}#{}", e.file, e.export))
                        .collect();
                    println!("  also exported as {}", paths.join(", "));
                }
            }
            for f in &report.functions {
                if !f.effects.is_empty() {
                    println!(
                        "{}:{} {}: {} possible escaping errors",
                        f.location.file,
                        f.location.line,
                        f.name,
                        f.effects.len()
                    );
                }
            }
            for u in &report.unknowns {
                println!(
                    "UNKNOWN {}:{} {}: {}",
                    u.location.file, u.location.line, u.expression, u.reason
                );
            }
            for d in &report.diagnostics {
                eprintln!("{} {}", d.code, d.message);
            }
            println!("Conservative scan; all runtime errors known: false.");
        }
    }
    ExitCode::from(code)
}
