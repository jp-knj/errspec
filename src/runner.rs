use crate::{input::read_json, request::Runner};
use errspec::{bun, evidence, model::Finding, observation, observation_io, vitest};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
};

/// Normalized runner output plus ownership of this execution's observation files.
pub(crate) struct Execution {
    pub evidence: evidence::RunEvidence,
    pub process_ok: bool,
    pub observations: ObservationSession,
}

/// Keep the workspace alive until all observation reads and judgments finish.
/// Its directory and run identity cannot be paired with another execution.
pub(crate) struct ObservationSession {
    temp: tempfile::TempDir,
    run: observation::RunContext,
}

impl ObservationSession {
    pub(crate) fn context(&self) -> &observation::RunContext {
        &self.run
    }

    pub(crate) fn read(&self) -> Result<observation::ValidatedObservations, Finding> {
        observation_io::read(&self.temp.path().join("observations"), &self.run)
    }
}

/// Bun before 1.2.19 omits the file attribute from JUnit suites, so no case could match.
const MINIMUM_BUN: (u64, u64, u64) = (1, 2, 19);

/// Fail before running tests when the version is known to be too old.
/// An unknown version still runs; the report parser rejects unusable output.
fn reject_unsupported_bun(executable: &Path, root: &Path) -> Result<(), Finding> {
    let Ok(output) = Command::new(executable)
        .arg("--version")
        .current_dir(root)
        .output()
    else {
        return Ok(());
    };
    let reported = String::from_utf8_lossy(&output.stdout);
    let reported = reported.trim();
    let Some(version) = parse_version(reported).filter(|_| output.status.success()) else {
        return Ok(());
    };
    if version >= MINIMUM_BUN {
        return Ok(());
    }
    let (major, minor, patch) = MINIMUM_BUN;
    Err(Finding::new(
        "EC_RUNNER",
        format!(
            "Bun {reported} does not report test files in JUnit; errspec requires Bun {major}.{minor}.{patch} or later"
        ),
    ))
}

fn parse_version(value: &str) -> Option<(u64, u64, u64)> {
    let core = value.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|part| part.parse().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

pub(crate) fn run(
    root: &Path,
    runner: Runner,
    vitest: Option<&Path>,
    bun: Option<&Path>,
    test_files: &[PathBuf],
) -> Result<Execution, Finding> {
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
            // Workspaces often install vitest only at the repository root.
            let hint = if matches!(runner, Runner::Vitest) && vitest.is_none() {
                "; pass --vitest PATH when vitest is installed elsewhere"
            } else {
                ""
            };
            Finding::new(
                "EC_RUNNER",
                format!(
                    "Cannot open test runner executable {}: {e}{hint}",
                    path.display()
                ),
            )
        })?
    } else {
        PathBuf::from("bun")
    };
    if matches!(runner, Runner::Bun) {
        reject_unsupported_bun(&executable, root)?;
    }
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
    command.args(test_files);
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
    Ok(Execution {
        evidence: result,
        process_ok: output.status.success(),
        observations: ObservationSession {
            temp,
            run: observation::RunContext::new(run_id),
        },
    })
}

/// Run only the SDK scenarios of the registered files with Bun line coverage.
/// Observation variables are not passed, so the testkit runs as plain tests and
/// this execution never contributes to the verification verdict.
pub(crate) fn bun_coverage(
    root: &Path,
    bun: Option<&Path>,
    test_files: &[PathBuf],
) -> Result<errspec::reach::LineHits, Finding> {
    let executable = match bun {
        Some(path) => path
            .canonicalize()
            .map_err(|e| Finding::new("EC_RUNNER", format!("Bun coverage: {e}")))?,
        None => PathBuf::from("bun"),
    };
    let temp = tempfile::tempdir().map_err(|e| Finding::new("EC_RUNNER", e.to_string()))?;
    let output = Command::new(executable)
        .current_dir(root)
        .env_remove("ERRSPEC_OBSERVATIONS")
        .env_remove("ERRSPEC_RUN_ID")
        .args([
            "test",
            "--coverage",
            "--coverage-reporter=lcov",
            "--coverage-dir",
        ])
        .arg(temp.path())
        .args(["-t", r"errspec \["])
        .args(test_files)
        .output()
        .map_err(|e| Finding::new("EC_RUNNER", format!("Bun coverage: {e}")))?;
    let mut stderr = io::stderr().lock();
    let _ = stderr.write_all(&output.stdout);
    let _ = stderr.write_all(&output.stderr);
    let text = fs::read_to_string(temp.path().join("lcov.info"))
        .map_err(|e| Finding::new("EC_RUNNER", format!("Bun coverage report: {e}")))?;
    errspec::reach::parse_lcov(&text).map_err(|e| Finding::new("EC_RUNNER", e))
}
