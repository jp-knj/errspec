//! Acquire report source snapshots and persist rendered artifacts; rendering stays in the library.
use errspec::mutation::{
    Report,
    reporting::{self, Input, Reporter, Source, Sources},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read, Write},
    path::Path,
    process::ExitCode,
};

fn acquire(
    input: &Input,
    root: &Path,
    snapshots: &BTreeMap<String, String>,
    reporter: Reporter,
) -> Result<Sources, String> {
    let mut sources = Sources::default();
    if reporter == Reporter::Text
        || reporter == Reporter::Annotations
            && input.selection.is_none()
            && !input.mutants.iter().any(|m| m.critical)
    {
        return Ok(sources);
    }
    for mutant in &input.mutants {
        let file = &mutant.candidate.location.file;
        if sources.files.contains_key(file) {
            continue;
        }
        let text = if let Some(source) = snapshots.get(file) {
            source.clone()
        } else {
            fs::read_to_string(root.join(file)).map_err(|e|format!("Cannot read report source {file}: {e}. Supply sources with the original snapshots."))?
        };
        let scopes = if matches!(reporter, Reporter::Agent | Reporter::Simplify) {
            crate::mutation_source::generate(crate::mutation_source::SourceRequest {
                file: file.clone(),
                source: text.clone(),
                validate_only: false,
                exclude_mutators: vec![],
                weak_mutation: false,
                facts: true,
                arid: false,
                arid_callees: None,
            })?
            .metadata
            .ok_or("Missing report syntax facts")?
            .scopes
        } else {
            BTreeMap::new()
        };
        sources.files.insert(file.clone(), Source { text, scopes });
    }
    Ok(sources)
}
fn render(
    input: &Input,
    reporter: Reporter,
    root: &Path,
    snapshots: &BTreeMap<String, String>,
) -> Result<String, String> {
    let sources = acquire(input, root, snapshots, reporter)?;
    reporting::format(input, reporter, &sources, &root.to_string_lossy())
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    protocol: u8,
    report: Value,
    reporter: Reporter,
    #[serde(default = "default_root")]
    root: std::path::PathBuf,
    #[serde(default)]
    sources: BTreeMap<String, String>,
}
fn default_root() -> std::path::PathBuf {
    ".".into()
}
pub(crate) fn entry() -> ExitCode {
    let result = std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| -> Result<String, String> {
            let mut bytes = vec![];
            io::stdin()
                .take(64 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() > 64 * 1024 * 1024 {
                return Err("Report request exceeds 64 MiB".into());
            }
            let request: Request = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if request.protocol != 1 {
                return Err("Unsupported formatter protocol".into());
            }
            let input: Input = serde_json::from_value(request.report).map_err(|e| e.to_string())?;
            let root = std::env::current_dir()
                .map_err(|e| e.to_string())?
                .join(request.root);
            render(&input, request.reporter, &root, &request.sources)
        })
        .map_err(|e| e.to_string())
        .and_then(|worker| {
            worker
                .join()
                .map_err(|_| "Report formatter panicked".to_owned())
        })
        .and_then(|r| r);
    match result {
        Ok(output) => {
            println!("{}", json!({"protocol":1,"output":output}));
            ExitCode::SUCCESS
        }
        Err(error) => {
            println!("{}", json!({"protocol":1,"error":error}));
            ExitCode::from(2)
        }
    }
}

pub(crate) fn write(report: &Report) -> ExitCode {
    let result = || -> Result<(), String> {
        let input: Input = serde_json::from_value(serde_json::to_value(report).unwrap())
            .map_err(|e| e.to_string())?;
        let root = std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(&report.presentation.root);
        let directory = report
            .presentation
            .directory
            .clone()
            .unwrap_or_else(|| root.join(".errspec/report"));
        let mut stdout = String::new();
        for reporter in &report.presentation.reporters {
            let output = render(&input, *reporter, &root, &BTreeMap::new())?;
            if let Some(name) = match reporter {
                Reporter::Json => Some("mutation.json"),
                Reporter::Html => Some("index.html"),
                _ => None,
            } {
                fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
                fs::write(directory.join(name), output).map_err(|e| e.to_string())?;
                eprintln!("report written to {}", directory.join(name).display());
            } else {
                stdout = output;
            }
        }
        // Workflow commands never contaminate the agent's JSON document.
        if std::env::var("GITHUB_ACTIONS").is_ok_and(|v| v == "true")
            && !report.presentation.reporters.contains(&Reporter::Agent)
            && !report
                .presentation
                .reporters
                .contains(&Reporter::Annotations)
        {
            let annotations = render(&input, Reporter::Annotations, &root, &BTreeMap::new())?;
            if !annotations.is_empty() {
                if !stdout.is_empty() {
                    stdout.push('\n');
                }
                stdout.push_str(&annotations);
            }
        }
        if !stdout.is_empty() {
            writeln!(io::stdout().lock(), "{stdout}").map_err(|e| e.to_string())?;
        }
        Ok(())
    };
    match result() {
        Ok(()) => ExitCode::from(report.exit_code()),
        Err(error) => {
            eprintln!("EC_MUTATION_REPORT: {error}");
            ExitCode::from(2)
        }
    }
}

/// Emit workflow commands only alongside human-readable default output.
pub(crate) fn automatic_annotations(report: &Report) -> Result<(), String> {
    if !std::env::var("GITHUB_ACTIONS").is_ok_and(|v| v == "true") {
        return Ok(());
    }
    let input: Input =
        serde_json::from_value(serde_json::to_value(report).unwrap()).map_err(|e| e.to_string())?;
    let root = std::env::current_dir()
        .map_err(|e| e.to_string())?
        .join(&report.presentation.root);
    let output = render(&input, Reporter::Annotations, &root, &BTreeMap::new())?;
    if !output.is_empty() {
        writeln!(io::stdout().lock(), "{output}").map_err(|e| e.to_string())?;
    }
    Ok(())
}
