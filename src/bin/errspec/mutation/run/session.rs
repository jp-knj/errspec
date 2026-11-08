//! Keep embedded driver files alive until their owned process has stopped.
use super::{
    Configuration, Failure,
    driver::{Driver, expected, only},
};
use errspec::mutation::{self, Report};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI32},
    },
    time::Duration,
};

pub(super) struct Resources {
    pub(super) directory: tempfile::TempDir,
    pub(super) script: PathBuf,
    pub(super) run_id: String,
    pub(super) options: Value,
    pub(super) custom_mutators: Vec<String>,
}

pub(super) struct Session {
    // Rust drops fields in declaration order: stop the driver before its files.
    pub(super) driver: Driver,
    pub(super) resources: Resources,
}

pub(super) fn open(
    configuration: &Configuration,
    report: &mut Report,
    signal: Arc<AtomicI32>,
    started: Arc<AtomicBool>,
) -> Result<Session, Failure> {
    let request = &configuration.request;
    let temp = tempfile::Builder::new()
        .prefix("errspec-mutation-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()?;
    let script = temp.path().join("driver.mjs");
    let setup = temp.path().join("setup.mjs");
    fs::write(
        &script,
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/packages/mutation-driver/dist/driver.mjs"
        )),
    )?;
    fs::write(
        &setup,
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/packages/mutation-driver/setup.mjs"
        )),
    )?;
    fs::write(
        temp.path().join("weak-runtime.mjs"),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/packages/mutation-driver/weak-runtime.mjs"
        )),
    )?;
    let run_id = temp
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .replace('-', "");
    fs::write(
        temp.path().join("child-process.mjs"),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/packages/mutation-driver/child-process.mjs"
        )),
    )?;
    let mut options = serde_json::to_value(request).unwrap();
    options["binary"] = json!(std::env::current_exe()?);
    options["selectionOnly"] = json!(configuration.selection_only);
    options["engineFingerprint"] = json!(mutation::cache::ENGINE_FINGERPRINT);
    options["temp"] = json!(temp.path());
    options["toolTemp"] = json!(temp.path());
    options["setup"] = json!(setup);
    options["state"] = json!(temp.path().join("state.json"));
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut driver = Driver::start(
        &request.node,
        &script,
        &run_id,
        signal.clone(),
        started.clone(),
        cancelled.clone(),
    )?;
    let startup = Duration::from_secs(120);
    let open = driver.request(json!({"op":"open","options":options}), startup, startup)?;
    if open.timeout {
        return Err(Failure::new(
            "EC_MUTATION_RUNNER",
            "Driver startup timed out",
        ));
    }
    open.check_error()?;
    expected(&open.messages, &["done"], 0)?;
    let info = only(&open.messages, "done")?;
    if request.kill_matrix && info["capabilities"]["complete"] != true {
        return Err(Failure::new(
            "EC_MUTATION_CONFIG",
            "Runner does not support complete kill-matrix acquisition",
        ));
    }
    let custom_mutators: Vec<String> =
        serde_json::from_value(info.get("mutators").cloned().unwrap_or(json!([])))
            .map_err(|e| Failure::protocol(e.to_string()))?;
    report.engine.node = info["node"].as_str().map(String::from);
    report.engine.vitest = info["vitest"].as_str().map(String::from);
    report.engine.runner = info["runner"].as_str().map(String::from);
    report.engine.runner_version = info["runnerVersion"].as_str().map(String::from);
    report.engine.coverage_scope = match report.engine.runner.as_deref() {
        Some("jest") => "jest_native_esm_workers",
        Some("playwright") => "playwright_vite_browser",
        _ => "vitest_vite_workers",
    };
    if driver.endpoint_available == Some(false) {
        report.workers.get_or_insert_default().endpoint_unavailable += 1;
    }
    Ok(Session {
        driver,
        resources: Resources {
            directory: temp,
            script,
            run_id,
            options,
            custom_mutators,
        },
    })
}
