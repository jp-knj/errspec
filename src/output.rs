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
        Format::Text | Format::Markdown => {
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
            for note in &report.assumptions {
                lines.push(format!("assumption: {note}"));
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
        Outcome::Mutation(report) => write_mutation(&report, format),
        Outcome::Contract(report) => write_contract(&report, format),
        Outcome::Scan {
            report,
            fail_on_unknown,
        } => write_scan(&report, fail_on_unknown, format),
        Outcome::Init(result) => write_init(result, format),
        Outcome::Coverage {
            report,
            fail_on_missing,
        } => write_coverage(&report, fail_on_missing, format),
    }
}

fn write_mutation(report: &errspec::mutation::Report, format: Format) -> ExitCode {
    let output = match format {
        Format::Json => serde_json::to_string_pretty(report).unwrap(),
        Format::Text | Format::Markdown => {
            let mut lines = vec![format!(
                "mutate: {} (complete: {}, tests executed: {})",
                if report.ok { "pass" } else { "fail" },
                report.run_complete,
                report.tests_executed
            )];
            let s = &report.summary;
            lines.push(format!("{} mutants: {} killed, {} survived, {} uncovered, {} timeout, {} runtime errors, {} ignored, {} pending",
                s.total,s.killed,s.survived,s.no_coverage,s.timeout,s.runtime_error,s.ignored,s.pending));
            if let Some(score) = s.score {
                lines.push(format!("score: {:.2}%", score * 100.0));
            }
            for m in &report.mutants {
                lines.push(format!(
                    "{}:{} {} {:?}: {} -> {}",
                    m.candidate.location.file,
                    m.candidate.location.line,
                    m.candidate.mutator,
                    m.status,
                    m.candidate.original.replace('\n', " "),
                    m.candidate.replacement.replace('\n', " ")
                ));
                if let Some(reason) = &m.candidate.ignored {
                    lines.push(format!("  ignored: {reason}"));
                }
                if !m.covered_by.is_empty() {
                    lines.push(format!("  covered by: {}", m.covered_by.join(", ")));
                }
                if !m.killed_by.is_empty() {
                    lines.push(format!("  killed by: {}", m.killed_by.join(", ")));
                }
                if !m.failed_modules.is_empty() {
                    lines.push(format!("  failed modules: {}", m.failed_modules.join(", ")));
                }
                for attempt in &m.attempts {
                    lines.push(format!(
                        "  attempt {} {:?}: {}",
                        attempt.number, attempt.status, attempt.reason
                    ));
                }
            }
            for d in &report.diagnostics {
                lines.push(format!("{}: {}", d.code, d.message));
            }
            lines.join("\n")
        }
    };
    if writeln!(io::stdout().lock(), "{output}").is_err() {
        return ExitCode::from(2);
    }
    ExitCode::from(report.exit_code())
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
                Format::Text | Format::Markdown => {
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
                Format::Text | Format::Markdown => eprintln!("EC_INIT {error}"),
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
        Format::Text | Format::Markdown => {
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
                let flows: Vec<_> = c
                    .flows
                    .iter()
                    .map(|f| format!("{f:?}").to_lowercase())
                    .collect();
                println!(
                    "{}{} export {} [{}]{}: {}",
                    if c.external { "external " } else { "" },
                    c.source.file,
                    c.source.export,
                    c.source.discriminant,
                    if flows.is_empty() {
                        String::new()
                    } else {
                        format!(" (flows: {})", flows.join(", "))
                    },
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

fn write_coverage(
    report: &errspec::coverage::CoverageReport,
    fail_on_missing: bool,
    format: Format,
) -> ExitCode {
    use errspec::coverage::TagStatus;
    // An unreadable, foreign or unwritable baseline is a usage error, like a failed scan.
    let baseline_error = report.diagnostics.iter().any(|d| d.code == "EC_BASELINE");
    let code = if !report.tests_executed || !report.evaluation_complete || baseline_error {
        2
    } else if !report.runner_ok
        || fail_on_missing && report.summary.missing > 0
        || report.regressed()
    {
        1
    } else {
        0
    };
    match format {
        Format::Json => println!("{}", serde_json::to_string_pretty(report).unwrap()),
        Format::Markdown => print!("{}", coverage_markdown(report)),
        Format::Text => {
            let s = &report.summary;
            println!(
                "coverage: {} error types, {} tags: {} asserted, {} mentioned only, {} missing",
                s.types, s.tags, s.asserted, s.mentioned, s.missing
            );
            if report.skipped_external > 0 {
                println!(
                    "skipped {} types declared only in dependencies",
                    report.skipped_external
                );
            }
            for t in &report.types {
                let open: Vec<_> = t
                    .tags
                    .iter()
                    .filter(|tag| tag.status != TagStatus::Asserted)
                    .collect();
                if open.is_empty() {
                    continue;
                }
                println!(
                    "{} {} [{}]: {}/{} asserted{}",
                    t.source.file,
                    t.source.export,
                    t.source.discriminant,
                    t.tags.len() - open.len(),
                    t.tags.len(),
                    if t.flows.is_empty() {
                        " (never seen returned or thrown)"
                    } else {
                        ""
                    }
                );
                for tag in open {
                    println!("  {:<9} {}", tag.status.as_str(), tag.tag);
                }
            }
            if let Some(delta) = &report.delta {
                for line in delta_lines(delta) {
                    println!("{line}");
                }
            }
            for d in &report.diagnostics {
                eprintln!("{} {}", d.code, d.message);
            }
        }
    }
    ExitCode::from(code)
}

/// For a CI job summary or a pull request comment. Fully asserted types are omitted.
fn coverage_markdown(report: &errspec::coverage::CoverageReport) -> String {
    use errspec::coverage::TagStatus;
    let s = &report.summary;
    let mut out = format!(
        "**errspec coverage**: {} error types, {} tags: {} asserted, {} mentioned only, {} missing\n",
        s.types, s.tags, s.asserted, s.mentioned, s.missing
    );
    if report.skipped_external > 0 {
        out.push_str(&format!(
            "\nSkipped {} types declared only in dependencies.\n",
            report.skipped_external
        ));
    }
    for d in &report.diagnostics {
        out.push_str(&format!("\n> {} {}\n", d.code, d.message));
    }
    if let Some(delta) = &report.delta {
        let lines = delta_lines(delta);
        if !lines.is_empty() {
            out.push_str("\n**Changes from baseline**\n\n");
            for line in lines {
                out.push_str(&format!("- {}\n", line.replace('|', "\\|")));
            }
        }
    }
    let cell = |text: &str| text.replace('|', "\\|");
    for t in &report.types {
        let open: Vec<_> = t
            .tags
            .iter()
            .filter(|tag| tag.status != TagStatus::Asserted)
            .collect();
        if open.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "\n### `{}` {} (`{}`): {}/{} asserted\n\n| Tag | Status | Note |\n|---|---|---|\n",
            cell(&t.source.file),
            cell(&t.source.export),
            cell(&t.source.discriminant),
            t.tags.len() - open.len(),
            t.tags.len()
        ));
        for tag in open {
            let note = match tag.status {
                TagStatus::Mentioned => "Written in a test, but no passing assertion required it",
                _ => "No test refers to it",
            };
            out.push_str(&format!(
                "| `{}` | {} | {note} |\n",
                cell(&tag.tag),
                tag.status.as_str()
            ));
        }
    }
    out
}

fn delta_lines(delta: &errspec::coverage::CoverageDelta) -> Vec<String> {
    let change = |label: &str, c: &errspec::coverage::TagChange| {
        format!(
            "{label}: {} {} {} -> {}",
            c.type_key,
            c.tag,
            c.from.as_str(),
            c.to.as_str()
        )
    };
    delta
        .regressed
        .iter()
        .map(|c| change("regressed", c))
        .chain(
            delta
                .new_missing
                .iter()
                .map(|t| format!("new missing: {} {}", t.type_key, t.tag)),
        )
        .chain(delta.improved.iter().map(|c| change("improved", c)))
        .collect()
}
