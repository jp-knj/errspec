use crate::{
    Runner,
    input::{read_json, resolve_paths},
};
use errspec::{
    bun, contract::Registration, evidence, model::*, observation, observation_io, vitest,
};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) struct Verification {
    pub tests: evidence::TestRun,
    pub diagnostics: Vec<Finding>,
    // Observation read failure must retain the successfully parsed runner results.
    pub observations: Result<(observation::ObservationJudgments, Vec<Finding>), Finding>,
}

pub(crate) fn verify(
    registrations: &[Registration<'_>],
    root: &Path,
    runner: Runner,
    vitest: Option<&Path>,
    bun: Option<&Path>,
    test_files: &[PathBuf],
) -> Result<Verification, Finding> {
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
    let registered_files = || {
        registrations
            .iter()
            .flat_map(|r| &r.cases)
            .filter_map(|case| case.test().map(|test| test.file.as_str()))
    };
    let mut paths = resolve_paths(root, registered_files());
    paths.extend(resolve_paths(
        root,
        result.tests.iter().map(|test| test.file.as_str()),
    ));
    let (tests, diagnostics) =
        evidence::judge(registrations, &paths, result, output.status.success());
    let run = observation::RunContext::new(run_id);
    let observations = (|| {
        let records = observation_io::read(&observations, &run)?;
        // Resolve again after reading observations, preserving the existing phase boundary.
        let mut paths = resolve_paths(root, registered_files());
        paths.extend(resolve_paths(
            root,
            records.records().iter().map(|record| record.file.as_str()),
        ));
        observation::judge(registrations, &tests, &paths, &run, &records)
    })();
    Ok(Verification {
        tests,
        diagnostics,
        observations,
    })
}
