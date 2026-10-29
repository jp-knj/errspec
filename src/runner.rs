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
