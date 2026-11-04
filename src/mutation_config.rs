//! Acquire mutation configuration, then merge explicit overrides into shared options.
use crate::request::MutationRequest;
use errspec::mutation::{Report, options::Options};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs, io,
    path::Path,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

pub(crate) fn run(request: MutationRequest) -> Report {
    let gate = request.options.fail_on_undetected;
    match resolve(request) {
        Ok(options) => crate::mutate::run(options),
        Err(error) => {
            let mut report = Report::new(gate);
            report.interrupted = error.interrupted;
            report.error("EC_MUTATION_CONFIG", error.message);
            report.finish();
            report
        }
    }
}

struct ConfigFailure {
    message: String,
    interrupted: Option<u8>,
}
impl From<String> for ConfigFailure {
    fn from(message: String) -> Self {
        Self {
            message,
            interrupted: None,
        }
    }
}
impl From<&str> for ConfigFailure {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}

fn resolve(request: MutationRequest) -> Result<Options, ConfigFailure> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let root = cwd.join(&request.options.root);
    let path = if let Some(path) = request.config_file {
        Some(cwd.join(path))
    } else {
        let found: Vec<_> = ["ts", "mts", "mjs", "js", "json"]
            .iter()
            .map(|extension| root.join(format!("errspec.mutation.config.{extension}")))
            .filter(|p| p.is_file())
            .collect();
        if found.len() > 1 {
            return Err("Multiple mutation configurations; select one with --config-file".into());
        }
        found.into_iter().next()
    };
    let Some(path) = path else {
        request.options.validate()?;
        return Ok(request.options);
    };
    let path = path
        .canonicalize()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let bytes = if path.extension().is_some_and(|e| e == "json") {
        if fs::metadata(&path).map_err(|e| e.to_string())?.len() > 1024 * 1024 {
            return Err("Mutation configuration exceeds 1 MiB".into());
        }
        fs::read(&path).map_err(|e| e.to_string())?
    } else {
        script(&request.options.node, &path)?
    };
    let mut config: Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let object = config
        .as_object_mut()
        .ok_or("Mutation configuration must be an object")?;
    if let Some(schema) = object.remove("$schema")
        && !schema.is_string()
    {
        return Err("$schema must be a string".into());
    }
    // Validate the entire file, including fields later overridden by the CLI/API.
    let file_options: Options =
        serde_json::from_value(config.clone()).map_err(|e| e.to_string())?;
    file_options.validate()?;
    let explicit_root = request.overrides.contains("root");
    let effective_root = if explicit_root {
        root
    } else if object_has_root(&config) {
        path.parent().unwrap().join(&file_options.root)
    } else {
        root
    };
    let mut config = serde_json::to_value(file_options).unwrap();
    config["root"] = serde_json::json!(effective_root);
    for name in [
        "vitestConfig",
        "vitestPackage",
        "typecheckProject",
        "typecheckPackage",
    ] {
        if let Some(value) = config[name].as_str() {
            config[name] = serde_json::json!(effective_root.join(value));
        }
    }
    if let Some(value) = config["node"]
        .as_str()
        .filter(|value| Path::new(value).components().count() > 1)
    {
        config["node"] = serde_json::json!(effective_root.join(value));
    }
    let cli = serde_json::to_value(&request.options).unwrap();
    overlay(&mut config, &cli, &request.overrides);
    if config["typecheck"] == false
        && (request.overrides.contains("no_typecheck") || request.overrides.contains("typecheck"))
    {
        if !request.overrides.contains("typecheck_project") {
            config["typecheckProject"] = Value::Null;
        }
        if !request.overrides.contains("typecheck_package") {
            config["typecheckPackage"] = Value::Null;
        }
    }
    let options: Options = serde_json::from_value(config).map_err(|e| e.to_string())?;
    options.validate()?;
    Ok(options)
}

fn object_has_root(value: &Value) -> bool {
    value.get("root").is_some()
}

pub(crate) fn overlay(config: &mut Value, cli: &Value, keys: &BTreeSet<String>) {
    for (id, key) in [
        ("root", "root"),
        ("since", "since"),
        ("diff_unit", "diffUnit"),
        ("node", "node"),
        ("include", "include"),
        ("exclude", "exclude"),
        ("exclude_mutator", "excludeMutator"),
        ("vitest_config", "vitestConfig"),
        ("vitest_project", "vitestProject"),
        ("vitest_package", "vitestPackage"),
        ("fail_on_undetected", "failOnUndetected"),
        ("no_fail_on_undetected", "failOnUndetected"),
        ("typecheck", "typecheck"),
        ("no_typecheck", "typecheck"),
        ("weak_mutation", "weakMutation"),
        ("reuse_session", "reuseSession"),
        ("cache", "cache"),
        ("no_cache", "cache"),
        ("cache_dir", "cacheDir"),
        ("no_reuse_session", "reuseSession"),
        ("no_weak_mutation", "weakMutation"),
        ("typecheck_project", "typecheckProject"),
        ("typecheck_package", "typecheckPackage"),
    ] {
        if keys.contains(id) {
            config[key] = cli[key].clone();
        }
    }
    for (id, key) in [
        ("only", "only"),
        ("mutants_per_line", "mutantsPerLine"),
        ("time_budget", "timeBudget"),
        ("fail_on_survived", "failOnSurvived"),
        ("no_fail_on_survived", "failOnSurvived"),
        ("threshold_high", "thresholdHigh"),
        ("threshold_low", "thresholdLow"),
        ("threshold_break", "thresholdBreak"),
        ("critical", "critical"),
        ("accepted_file", "acceptedFile"),
    ] {
        if keys.contains(id) {
            config["policy"][key] = cli["policy"][key].clone();
        }
    }
}

struct Signals(Vec<signal_hook::SigId>);
impl Drop for Signals {
    fn drop(&mut self) {
        for id in self.0.drain(..) {
            signal_hook::low_level::unregister(id);
        }
    }
}
fn script(node: &Path, path: &Path) -> Result<Vec<u8>, ConfigFailure> {
    let signal = Arc::new(AtomicUsize::new(0));
    let invoke = || -> Result<Vec<u8>, io::Error> {
        let dir = tempfile::tempdir()?;
        let source = dir.path().join("config.mjs");
        fs::write(
            &source,
            include_str!("../packages/mutation-driver/config.mjs"),
        )?;
        let mut handlers = Signals(vec![]);
        for number in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
            handlers.0.push(signal_hook::flag::register_usize(
                number,
                Arc::clone(&signal),
                number as usize,
            )?);
        }
        let output = crate::owned_process::capture(
            Command::new(node).arg(source).arg(path),
            Duration::from_secs(10),
            1024 * 1024,
            || signal.load(Ordering::SeqCst) != 0,
        )?;
        let logs = String::from_utf8_lossy(&output.stderr);
        if !logs.is_empty() {
            eprint!("{logs}");
        }
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "Configuration loader exited {}",
                output.status
            )));
        }
        Ok(output.stdout)
    };
    invoke().map_err(|e| ConfigFailure {
        message: format!("{}: {e}", path.display()),
        interrupted: match signal.load(Ordering::SeqCst) {
            0 => None,
            number => Some((128 + number) as u8),
        },
    })
}
