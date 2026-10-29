//! Run the project's Vitest with the assertion recorder and judge scanned error tags.
use crate::{input::read_json, request::CoverageRequest, scan};
use errspec::{
    coverage::{self, AssertionRecord, CoverageReport},
    model::Finding,
    vitest,
};
use std::{
    collections::BTreeSet,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
};

const RECORDER: &str = include_str!("coverage/vitest-recorder.mjs");
const CONFIG_NAMES: &[&str] = &[
    "vitest.config.ts",
    "vitest.config.mts",
    "vitest.config.cts",
    "vitest.config.js",
    "vitest.config.mjs",
    "vitest.config.cjs",
    "vite.config.ts",
    "vite.config.mts",
    "vite.config.js",
    "vite.config.mjs",
];

fn runner_error(message: impl Into<String>) -> Finding {
    Finding::new("EC_RUNNER", message)
}

/// Nearest directory at or above `start` that contains `relative`.
fn find_up(start: &Path, relative: &str) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|dir| dir.join(relative))
        .find(|path| path.exists())
}

/// Workspaces often keep the Vitest config and binary at the repository root, not in the package.
fn vitest_config(project_dir: &Path) -> Option<PathBuf> {
    project_dir.ancestors().find_map(|dir| {
        CONFIG_NAMES
            .iter()
            .map(|name| dir.join(name))
            .find(|p| p.is_file())
    })
}

fn js_string(path: &Path) -> String {
    serde_json::to_string(&path.to_string_lossy()).expect("paths serialize as JSON strings")
}

/// Load the user's config unchanged and append the recorder to its setup files.
fn wrapper_config(base: Option<&Path>, root: &Path, recorder: &Path) -> String {
    let (import, loaded) = match base {
        Some(base) => (
            format!("import base from {};\n", js_string(base)),
            "typeof base === \"function\" ? await base({ mode: \"test\", command: \"serve\" }) : base",
        ),
        None => (String::new(), "{}"),
    };
    format!(
        r#"import {{ resolve }} from "node:path";
{import}const config = (await ({loaded})) ?? {{}};
const test = config.test ?? {{}};
// A relative root was relative to the original config file, not to this temporary one.
const root = config.root ? resolve({root}, config.root) : {root};
export default {{ ...config, root, test: {{ ...test, setupFiles: [...[test.setupFiles ?? []].flat(), {recorder}] }} }};
"#,
        root = js_string(root),
        recorder = js_string(recorder),
    )
}

fn read_records(directory: &Path) -> Result<Vec<AssertionRecord>, Finding> {
    let mut records = vec![];
    let entries = fs::read_dir(directory).map_err(|e| runner_error(e.to_string()))?;
    for entry in entries {
        let path = entry.map_err(|e| runner_error(e.to_string()))?.path();
        let text = fs::read_to_string(&path).map_err(|e| runner_error(e.to_string()))?;
        for line in text.lines().filter(|l| !l.is_empty()) {
            records.push(serde_json::from_str(line).map_err(|e| {
                runner_error(format!(
                    "Malformed assertion record in {}: {e}",
                    path.display()
                ))
            })?);
        }
    }
    Ok(records)
}

pub(crate) fn run(request: &CoverageRequest) -> CoverageReport {
    let mut report = CoverageReport::default();
    let scanned = scan::run(&request.project, &request.discriminants);
    if !scanned.scan_complete {
        report.diagnostics = scanned.diagnostics;
        return report;
    }
    if let Err(finding) = execute(request, &scanned.candidates, &mut report) {
        report.diagnostics.push(finding);
    }
    report
}

fn execute(
    request: &CoverageRequest,
    candidates: &[errspec::discovery::Candidate],
    report: &mut CoverageReport,
) -> Result<(), Finding> {
    let project = request
        .project
        .canonicalize()
        .map_err(|e| Finding::new("EC_CONFIG", e.to_string()))?;
    let project_dir = project.parent().expect("canonical project parent");
    let config = match &request.vitest_config {
        Some(path) => Some(path.canonicalize().map_err(|e| {
            Finding::new(
                "EC_CONFIG",
                format!("Cannot open Vitest config {}: {e}", path.display()),
            )
        })?),
        None => vitest_config(project_dir),
    };
    let root = config
        .as_deref()
        .and_then(Path::parent)
        .unwrap_or(project_dir)
        .to_path_buf();
    let executable = match &request.vitest {
        Some(path) => path.canonicalize().map_err(|e| {
            runner_error(format!(
                "Cannot open test runner executable {}: {e}",
                path.display()
            ))
        })?,
        None => find_up(&root, "node_modules/.bin/vitest").ok_or_else(|| {
            runner_error(format!(
                "No node_modules/.bin/vitest at or above {}; pass --vitest PATH",
                root.display()
            ))
        })?,
    };
    let package = find_up(&root, "node_modules/vitest/package.json")
        .and_then(|manifest| manifest.parent()?.canonicalize().ok())
        .ok_or_else(|| runner_error(format!("No vitest package at or above {}", root.display())))?;

    let temp = tempfile::tempdir().map_err(|e| runner_error(e.to_string()))?;
    let recorder = temp.path().join("vitest-recorder.mjs");
    let wrapper = temp.path().join("vitest.config.mjs");
    let records = temp.path().join("records");
    let output_file = temp.path().join("vitest.json");
    fs::create_dir(&records).map_err(|e| runner_error(e.to_string()))?;
    fs::write(&recorder, RECORDER).map_err(|e| runner_error(e.to_string()))?;
    fs::write(
        &wrapper,
        wrapper_config(config.as_deref(), &root, &recorder),
    )
    .map_err(|e| runner_error(e.to_string()))?;

    let output = Command::new(&executable)
        .current_dir(&root)
        .env("ERRSPEC_COVERAGE", &records)
        .env("ERRSPEC_VITEST_PACKAGE", &package)
        .env("ERRSPEC_DISCRIMINANTS", request.discriminants.join(","))
        .args(["run", "--config"])
        .arg(&wrapper)
        .args(["--reporter=json", "--outputFile"])
        .arg(&output_file)
        .args(&request.runner_args)
        .output()
        .map_err(|e| runner_error(e.to_string()))?;
    // Keep stdout machine-readable even if the test runner writes logs.
    let mut stderr = io::stderr().lock();
    let _ = stderr.write_all(&output.stdout);
    let _ = stderr.write_all(&output.stderr);

    let evidence = vitest::normalize(read_json(&output_file, "EC_RUNNER")?);
    report.tests_executed = true;
    report.runner_ok = output.status.success() && evidence.success;
    if !report.runner_ok {
        report.diagnostics.push(runner_error(
            "vitest run failed; only assertions that passed before the failure count",
        ));
    }
    let test_sources: Vec<String> = evidence
        .tests
        .iter()
        .map(|t| t.file.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|file| fs::read_to_string(file).ok())
        .collect();
    let (types, summary) = coverage::judge(candidates, &read_records(&records)?, &test_sources);
    report.types = types;
    report.summary = summary;
    Ok(())
}
