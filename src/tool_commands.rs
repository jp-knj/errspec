use crate::{Action, Cli, Format, onboard, scan};
use std::process::ExitCode;

pub(crate) fn execute(cli: &Cli) -> Option<ExitCode> {
    if let Action::Init {
        project,
        source,
        export,
        discriminant,
        runner,
        test,
        contract,
    } = &cli.command
    {
        if cli.baseline.is_some() || cli.strict_baseline {
            eprintln!("init does not accept baseline options");
            return Some(ExitCode::from(2));
        }
        let request = onboard::InitRequest {
            project: project.clone(),
            source: source.clone(),
            export: export.clone(),
            discriminant: discriminant.clone(),
            runner: *runner,
            test: test.clone(),
            contract: contract.clone(),
            config: cli.config.clone(),
        };
        let result = std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn(move || onboard::run(request))
            .map_err(|e| e.to_string())
            .and_then(|w| w.join().map_err(|_| "Init worker failed".into()))
            .and_then(|r| r);
        return Some(match result {
            Ok(report) => {
                match cli.format {
                    Format::Json => println!("{}", serde_json::to_string_pretty(&report).unwrap()),
                    Format::Text => {
                        println!("Created configuration and tests: {}", report["files"]);
                        println!(
                            "Tests intentionally fail until you specify the expected behavior. Install @errspec/testkit and your test runner, then run errspec verify."
                        );
                    }
                };
                ExitCode::SUCCESS
            }
            Err(error) => {
                match cli.format {
                    Format::Json => println!(
                        "{}",
                        serde_json::json!({"kind":"errspec/init","schemaVersion":1,"ready":false,"error":error})
                    ),
                    Format::Text => eprintln!("EC_INIT {error}"),
                };
                ExitCode::from(2)
            }
        });
    }
    if let Action::Scan {
        project,
        discriminant,
        fail_on_unknown,
    } = &cli.command
    {
        if cli.baseline.is_some() || cli.strict_baseline {
            eprintln!("scan does not accept baseline options");
            return Some(ExitCode::from(2));
        }
        let project = project.clone();
        let discriminant = discriminant.clone();
        let report = std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn(move || scan::run(&project, &discriminant))
            .ok()
            .and_then(|worker| worker.join().ok())
            .unwrap_or_else(|| {
                let mut report = errspec::discovery::ScanReport::default();
                report.diagnostics.push(errspec::model::Finding::new(
                    "EC_INTERNAL",
                    "Scan worker failed",
                ));
                report
            });
        let code = if !report.scan_complete {
            2
        } else if *fail_on_unknown && !report.unknowns.is_empty() {
            1
        } else {
            0
        };
        match cli.format {
            Format::Json => println!("{}", serde_json::to_string_pretty(&report).unwrap()),
            Format::Text => {
                println!(
                    "{} files, {} candidates, {} origins, {} calls, {} unknowns",
                    report.files.len(),
                    report.candidates.len(),
                    report.origins.len(),
                    report.edges.len(),
                    report.unknowns.len()
                );
                for c in &report.candidates {
                    println!(
                        "{} export {} [{}]: {}",
                        c.source.file,
                        c.source.export,
                        c.source.discriminant,
                        c.tags.join(", ")
                    );
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
        return Some(ExitCode::from(code));
    }
    None
}
