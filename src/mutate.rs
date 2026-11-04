//! Mutation process ownership, protocol validation, and acquisition.
mod worker_pool;
use crate::mutation_source;
use errspec::mutation::options::Options as MutationRequest;
use errspec::mutation::{self, Candidate, Execution, Mutant, Report, Status};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, ExitCode, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI32, Ordering},
        mpsc::{self, Receiver},
    },
    time::{Duration, Instant},
};

pub(crate) fn source_entry() -> ExitCode {
    let result = std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            let mut input = String::new();
            io::stdin()
                .read_to_string(&mut input)
                .map_err(|e| e.to_string())?;
            let mut input: Value = serde_json::from_str(&input).map_err(|e| e.to_string())?;
            if let Some(expected) = input
                .as_object_mut()
                .and_then(|object| object.remove("engineFingerprint"))
                && expected.as_str() != Some(mutation::cache::ENGINE_FINGERPRINT)
            {
                return Err(
                    "EC_MUTATION_INPUT: Native mutation engine changed during execution".into(),
                );
            }
            let request = serde_json::from_value(input).map_err(|e| e.to_string())?;
            mutation_source::generate(request)
        })
        .map_err(|e| e.to_string())
        .and_then(|worker| worker.join().map_err(|_| "Mutation parser panicked".into()))
        .and_then(|r| r);
    match result {
        Ok(result) => {
            println!("{}", serde_json::to_string(&result).unwrap());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

pub(crate) fn run(request: MutationRequest) -> Report {
    let mut report = Report::new(request.fail_on_undetected);
    let signal = Arc::new(AtomicI32::new(0));
    let started = Arc::new(AtomicBool::new(false));
    let mut registrations = vec![];
    for number in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        // Only an atomic write is performed by the signal handler.
        let signal = signal.clone();
        match unsafe {
            signal_hook::low_level::register(number, move || signal.store(number, Ordering::SeqCst))
        } {
            Ok(id) => registrations.push(id),
            Err(e) => {
                report.error("EC_MUTATION_RUNNER", e.to_string());
                report.finish();
                return report;
            }
        }
    }
    if let Err(error) = run_inner(request, &mut report, signal.clone(), started.clone()) {
        report.run_complete = false;
        report.error(&error.code, error.message);
    }
    report.tests_executed |= started.load(Ordering::SeqCst);
    let interrupted = signal.load(Ordering::SeqCst);
    if interrupted != 0 {
        report.interrupted = Some((128 + interrupted) as u8);
        report.run_complete = false;
    }
    for registration in registrations {
        signal_hook::low_level::unregister(registration);
    }
    report.finish();
    report
}

struct Failure {
    code: String,
    message: String,
}
impl Failure {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
    fn protocol(message: impl Into<String>) -> Self {
        Self::new("EC_MUTATION_PROTOCOL", message)
    }
}
impl From<io::Error> for Failure {
    fn from(e: io::Error) -> Self {
        Self::new("EC_MUTATION_RUNNER", e.to_string())
    }
}

struct Driver {
    child: Child,
    input: ChildStdin,
    output: Receiver<Result<String, io::Error>>,
    run_id: String,
    sequence: u64,
    signal: Arc<AtomicI32>,
    stopped: bool,
    started: Arc<AtomicBool>,
    ownership: crate::mutation_ownership::Launch,
    endpoint_available: Option<bool>,
    cancelled: Arc<AtomicBool>,
}
struct Response {
    messages: Vec<Value>,
    timeout: bool,
}
impl Response {
    fn check_error(&self) -> Result<(), Failure> {
        if let Some(message) = self.messages.iter().find(|m| m["type"] == "error") {
            return Err(Failure::new(
                message["code"].as_str().unwrap_or("EC_MUTATION_PROTOCOL"),
                message["message"]
                    .as_str()
                    .unwrap_or("Unknown driver error"),
            ));
        }
        Ok(())
    }
}
impl Driver {
    fn start(
        node: &Path,
        script: &Path,
        run_id: &str,
        signal: Arc<AtomicI32>,
        started: Arc<AtomicBool>,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self, Failure> {
        let ownership = crate::mutation_ownership::Launch::new(node, script, run_id)?;
        let mut child = Command::new(std::env::current_exe()?)
            .arg("__mutation-supervisor")
            .arg(&ownership.configuration)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .process_group(0)
            .spawn()
            .map_err(|e| Failure::new("EC_MUTATION_CONFIG", format!("Cannot start Node: {e}")))?;
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            input,
            output: receiver,
            run_id: run_id.into(),
            sequence: 0,
            signal,
            stopped: false,
            started,
            ownership,
            endpoint_available: None,
            cancelled,
        })
    }
    fn stop(&mut self) {
        if !self.stopped {
            // The child owns a process group; workers and native generators inherit it.
            unsafe {
                libc::kill(-(self.child.id() as i32), libc::SIGKILL);
            }
            let _ = self.child.wait();
            self.stopped = true;
            self.ownership.cleanup();
        }
    }
    fn request(
        &mut self,
        mut request: Value,
        total: Duration,
        stall: Duration,
    ) -> Result<Response, Failure> {
        self.sequence += 1;
        request["version"] = json!(1);
        request["runId"] = json!(self.run_id);
        request["requestId"] = json!(self.sequence);
        writeln!(self.input, "{}", request)?;
        self.input.flush()?;
        let start = Instant::now();
        let mut progress = start;
        let mut messages = vec![];
        loop {
            if self.cancelled.load(Ordering::SeqCst) {
                self.stop();
                return Err(Failure::new(
                    "EC_MUTATION_RUNNER",
                    "Execution cancelled after another worker failed",
                ));
            }
            if self.signal.load(Ordering::SeqCst) != 0 {
                self.stop();
                return Err(Failure::new(
                    "EC_MUTATION_RUNNER",
                    "Mutation run interrupted",
                ));
            }
            if start.elapsed() > total || progress.elapsed() > stall {
                self.stop();
                return Ok(Response {
                    messages,
                    timeout: true,
                });
            }
            let line = match self.output.recv_timeout(Duration::from_millis(50)) {
                Ok(line) => line?,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(_) => {
                    return Err(Failure::new(
                        "EC_MUTATION_RUNNER",
                        "Driver exited without a terminal response",
                    ));
                }
            };
            let message: Value = serde_json::from_str(&line)
                .map_err(|e| Failure::protocol(format!("Invalid driver JSON: {e}")))?;
            if message.get("version") != Some(&json!(1))
                || message.get("runId") != Some(&json!(self.run_id))
                || message.get("requestId") != Some(&json!(self.sequence))
            {
                return Err(Failure::protocol(
                    "Wrong protocol version, run ID, or request ID",
                ));
            }
            match message["type"].as_str() {
                Some("ownership") => {
                    if self.sequence != 1
                        || request["op"] != "open"
                        || self.endpoint_available.is_some()
                        || message["supervisor"] != json!(self.child.id())
                    {
                        return Err(Failure::protocol("Invalid worker ownership handshake"));
                    }
                    self.endpoint_available = Some(match message["mode"].as_str() {
                        Some("endpoint") => true,
                        Some("parent_monitor") => false,
                        _ => return Err(Failure::protocol("Invalid ownership capability")),
                    });
                }
                Some("progress") => {
                    match message["event"].as_str() {
                        Some("testStart") if request["op"] != "typecheck" => {
                            self.started.store(true, Ordering::SeqCst)
                        }
                        Some("typecheck")
                            if request["op"] == "typecheck" && message["key"].is_string() => {}
                        _ => return Err(Failure::protocol("Unknown progress event")),
                    }
                    progress = Instant::now();
                    messages.push(message);
                }
                Some("error") => {
                    messages.push(message);
                    return Ok(Response {
                        messages,
                        timeout: false,
                    });
                }
                Some("done") => {
                    messages.push(message);
                    return Ok(Response {
                        messages,
                        timeout: false,
                    });
                }
                Some(
                    "baseline" | "prepared" | "file" | "attempt" | "typechecked" | "discovered"
                    | "primed",
                ) => messages.push(message),
                _ => return Err(Failure::protocol("Unknown driver message")),
            }
        }
    }
}
impl Drop for Driver {
    fn drop(&mut self) {
        self.stop();
    }
}

fn absolute(path: PathBuf) -> Result<PathBuf, Failure> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}
fn only<'a>(messages: &'a [Value], kind: &str) -> Result<&'a Value, Failure> {
    let values: Vec<_> = messages.iter().filter(|m| m["type"] == kind).collect();
    if values.len() != 1 {
        return Err(Failure::protocol(format!(
            "Expected exactly one {kind} response"
        )));
    }
    Ok(values[0])
}
fn execution(value: &Value) -> Result<Execution, Failure> {
    serde_json::from_value(value["execution"].clone()).map_err(|e| Failure::protocol(e.to_string()))
}
fn expected(messages: &[Value], allowed: &[&str], count: usize) -> Result<(), Failure> {
    if messages
        .iter()
        .any(|m| !allowed.contains(&m["type"].as_str().unwrap_or("")))
        || only(messages, "done")?["count"] != json!(count)
    {
        return Err(Failure::protocol("Unexpected response or terminal count"));
    }
    Ok(())
}
fn run_inner(
    mut request: MutationRequest,
    report: &mut Report,
    signal: Arc<AtomicI32>,
    started: Arc<AtomicBool>,
) -> Result<(), Failure> {
    request
        .policy
        .validate()
        .map_err(|e| Failure::new("EC_MUTATION_CONFIG", e))?;
    if request.policy.active() {
        report.policy = Some(mutation::policy::PolicyReport::new(request.policy.clone()));
    }
    let beginning = Instant::now();
    for family in &request.exclude_mutator {
        if !mutation::MUTATORS.contains(&family.as_str()) {
            return Err(Failure::new(
                "EC_MUTATION_CONFIG",
                format!("Unknown mutator: {family}"),
            ));
        }
    }
    request.root = absolute(request.root)?
        .canonicalize()
        .map_err(|e| Failure::new("EC_MUTATION_CONFIG", e.to_string()))?;
    let recovery = crate::mutation_ownership::recover(&signal)?;
    if !recovery.is_empty() {
        report.workers = Some(recovery);
    }
    if request.cache {
        let directory = crate::mutation_cache::directory(&request);
        report.cache = Some(mutation::cache::Info {
            directory: directory.display().to_string(),
            ..Default::default()
        });
        fs::create_dir_all(&directory)
            .map_err(|e| Failure::new("EC_MUTATION_CACHE", e.to_string()))?;
        let directory = directory
            .canonicalize()
            .map_err(|e| Failure::new("EC_MUTATION_CACHE", e.to_string()))?;
        if request.root.starts_with(&directory) {
            return Err(Failure::new(
                "EC_MUTATION_CONFIG",
                "cacheDir must not contain the project root",
            ));
        }
        crate::mutation_cache::check_directory(&directory)
            .map_err(|e| Failure::new("EC_MUTATION_CONFIG", e))?;
        report.cache.as_mut().unwrap().directory = directory
            .strip_prefix(&request.root)
            .unwrap_or(&directory)
            .display()
            .to_string();
        request.cache_dir = Some(directory);
    }
    let changes = request
        .since
        .as_ref()
        .map(|since| {
            crate::mutation_git::acquire(
                &request.root,
                since,
                &signal,
                request.cache_dir.as_deref().filter(|_| request.cache),
            )
        })
        .transpose()
        .map_err(|e| Failure::new("EC_MUTATION_CONFIG", e))?;
    if request.policy.accepted_file.is_none()
        && request.root.join("errspec.accepted.json").is_file()
    {
        request.policy.accepted_file = Some("errspec.accepted.json".into());
        report.policy = Some(mutation::policy::PolicyReport::new(request.policy.clone()));
    }
    let accepted = if let Some(file) = &request.policy.accepted_file {
        let file = request.root.join(file);
        let accepted: std::collections::BTreeMap<String, mutation::policy::Acceptance> =
            match fs::read(&file) {
                Ok(bytes) => serde_json::from_slice(&bytes)
                    .map_err(|e| Failure::new("EC_MUTATION_CONFIG", e.to_string()))?,
                Err(e) if e.kind() == io::ErrorKind::NotFound => Default::default(),
                Err(e) => return Err(e.into()),
            };
        if accepted
            .values()
            .any(|entry| entry.reason.trim().is_empty())
        {
            return Err(Failure::new(
                "EC_MUTATION_CONFIG",
                "Acceptance reasons must not be empty",
            ));
        }
        accepted
    } else {
        std::collections::BTreeMap::new()
    };
    request.vitest_config = request.vitest_config.map(absolute).transpose()?;
    request.vitest_package = request.vitest_package.map(absolute).transpose()?;
    request.typecheck_project = request.typecheck_project.map(absolute).transpose()?;
    request.typecheck_package = request.typecheck_package.map(absolute).transpose()?;
    if request.node.components().count() > 1 {
        request.node = absolute(request.node)?;
    }
    let temp = tempfile::Builder::new()
        .prefix("errspec-mutation-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()?;
    let script = temp.path().join("driver.mjs");
    let setup = temp.path().join("setup.mjs");
    fs::write(
        &script,
        include_str!("../packages/mutation-driver/dist/driver.mjs"),
    )?;
    fs::write(
        &setup,
        include_str!("../packages/mutation-driver/setup.mjs"),
    )?;
    fs::write(
        temp.path().join("weak-runtime.mjs"),
        include_str!("../packages/mutation-driver/weak-runtime.mjs"),
    )?;
    let run_id = temp
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .replace('-', "");
    let mut options = serde_json::to_value(&request).unwrap();
    options["binary"] = json!(std::env::current_exe()?);
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
    report.engine.node = info["node"].as_str().map(String::from);
    report.engine.vitest = info["vitest"].as_str().map(String::from);
    if driver.endpoint_available == Some(false) {
        report.workers.get_or_insert_default().endpoint_unavailable += 1;
    }
    // Baselines use the project's runner timeouts, not a whole-run time budget.
    let mut prepared = driver.request(
        json!({"op":if request.cache {"discover"} else {"prepare"}}),
        Duration::MAX,
        Duration::MAX,
    )?;
    report.tests_executed |= prepared.messages.iter().any(|m| m["type"] == "progress");
    if let Some(value) = prepared.messages.iter().find(|m| m["type"] == "baseline") {
        report.baseline = execution(value)?.tests;
    }
    // Preserve successfully inventoried files even when prepare fails later.
    let mut files = BTreeSet::new();
    let mut keys = BTreeSet::new();
    let mut syntax = std::collections::BTreeMap::new();
    let mut sources = std::collections::BTreeMap::new();
    for file in prepared.messages.iter().filter(|m| m["type"] == "file") {
        let name = file["file"]
            .as_str()
            .ok_or_else(|| Failure::protocol("Missing inventory filename"))?;
        if !files.insert(name.to_owned()) {
            return Err(Failure::protocol("Duplicate inventory file"));
        }
        let candidates: Vec<Candidate> = serde_json::from_value(file["candidates"].clone())
            .map_err(|e| Failure::protocol(e.to_string()))?;
        if changes.is_some() {
            let metadata: mutation::syntax::Metadata =
                serde_json::from_value(file["metadata"].clone())
                    .map_err(|e| Failure::protocol(e.to_string()))?;
            let source = fs::read_to_string(request.root.join(name))?;
            mutation::selection::validate(&metadata, &source, &candidates)
                .map_err(Failure::protocol)?;
            syntax.insert(name.to_owned(), metadata);
            sources.insert(name.to_owned(), source);
        }
        let critical: BTreeSet<String> =
            serde_json::from_value(file.get("critical").cloned().unwrap_or(json!([])))
                .map_err(|e| Failure::protocol(e.to_string()))?;
        if critical
            .iter()
            .any(|key| !candidates.iter().any(|c| &c.key == key))
        {
            return Err(Failure::protocol("Unknown critical mutant key"));
        }
        if !critical.is_empty() && report.policy.is_none() {
            report.policy = Some(mutation::policy::PolicyReport::new(request.policy.clone()));
        }
        for candidate in candidates {
            if candidate.location.file != name
                || !keys.insert(candidate.key.clone())
                || !mutation::MUTATORS.contains(&candidate.mutator.as_str())
                || candidate.location.byte_offset > candidate.end
                || candidate.key.len() != 64
            {
                return Err(Failure::protocol("Invalid candidate identity or location"));
            }
            let status = if candidate.ignored.is_some() {
                Status::Ignored
            } else {
                Status::Pending
            };
            report.mutants.push(Mutant {
                critical: critical.contains(&candidate.key),
                accepted: None,
                reused: false,
                infected_by: None,
                candidate,
                status,
                covered_by: vec![],
                killed_by: vec![],
                failed_modules: vec![],
                attempts: vec![],
                reason: None,
            });
        }
    }
    if prepared.timeout {
        return Err(Failure::new(
            "EC_MUTATION_BASELINE",
            "Baseline or generation timed out",
        ));
    }
    if let Err(mut failure) = prepared.check_error() {
        if failure.code == "EC_MUTATION_BASELINE"
            && let Ok(value) = only(&prepared.messages, "baseline")
            && let Ok(plain) = execution(value)
            && let Err(message) = mutation::validate_baseline(&plain, "Unmodified baseline")
        {
            failure.message = message;
        }
        return Err(failure);
    }
    let mut cache_store = None;
    if request.cache {
        expected(
            &prepared.messages,
            &["progress", "baseline", "file", "discovered", "done"],
            files.len(),
        )?;
        let plain = execution(only(&prepared.messages, "baseline")?)?;
        mutation::validate_execution(&plain, &[]).map_err(Failure::protocol)?;
        mutation::validate_baseline(&plain, "Unmodified baseline")
            .map_err(|message| Failure::new("EC_MUTATION_BASELINE", message))?;
        let facts: crate::mutation_cache::Facts =
            serde_json::from_value(only(&prepared.messages, "discovered")?["cacheFacts"].clone())
                .map_err(|e| Failure::protocol(e.to_string()))?;
        cache_store = match crate::mutation_cache::acquire(
            &request,
            &facts,
            temp.path(),
            None,
            &driver.signal,
        ) {
            Ok(stamp) => Some(crate::mutation_cache::load(
                crate::mutation_cache::directory(&request),
                stamp,
                report.cache.as_mut().unwrap(),
            )),
            Err(crate::mutation_cache::Error::Unavailable(reason)) => {
                report.cache.as_mut().unwrap().disabled_reason = Some(reason);
                None
            }
            Err(crate::mutation_cache::Error::Changed(reason)) => {
                return Err(Failure::new("EC_MUTATION_INPUT", reason));
            }
        };
        let coverage = if request.full_dry_run {
            None
        } else {
            cache_store.as_ref().and_then(|store| {
                crate::mutation_cache::load_coverage(
                    &store.directory,
                    report.cache.as_mut().unwrap(),
                )
                .and_then(|file| {
                    let reused = file.reuse(&store.stamp.hash, &plain, &keys);
                    if reused.is_none() {
                        report.cache.as_mut().unwrap().warnings.push(
                            "Coverage inputs or observations changed; refreshing observations"
                                .into(),
                        );
                    }
                    reused
                })
            })
        };
        report.cache.as_mut().unwrap().coverage_reused = coverage.is_some();
        let probe = driver.request(
            json!({"op":"probe","coverage":coverage}),
            Duration::MAX,
            Duration::MAX,
        )?;
        if probe.timeout {
            return Err(Failure::new(
                "EC_MUTATION_BASELINE",
                "Probe baseline timed out",
            ));
        }
        probe.check_error()?;
        expected(&probe.messages, &["progress", "prepared", "done"], 0)?;
        prepared
            .messages
            .retain(|m| m["type"] != "done" && m["type"] != "discovered");
        prepared.messages.extend(probe.messages);
    }
    expected(
        &prepared.messages,
        &["progress", "baseline", "file", "prepared", "done"],
        if request.cache { 0 } else { files.len() },
    )?;
    let plain = execution(only(&prepared.messages, "baseline")?)?;
    let baseline = execution(only(&prepared.messages, "prepared")?)?;
    mutation::validate_execution(&plain, &[]).map_err(Failure::protocol)?;
    mutation::validate_execution(&baseline, &[]).map_err(Failure::protocol)?;
    mutation::validate_baseline(&plain, "Unmodified baseline")
        .map_err(|message| Failure::new("EC_MUTATION_BASELINE", message))?;
    mutation::validate_baseline(&baseline, "Instrumented baseline")
        .map_err(|message| Failure::new("EC_MUTATION_BASELINE", message))?;
    if baseline
        .hits
        .keys()
        .chain(baseline.static_hits.keys())
        .chain(baseline.tests.iter().flat_map(|t| t.hits.keys()))
        .chain(baseline.tests.iter().flat_map(|t| t.infected.iter()))
        .any(|k| !keys.contains(k))
    {
        return Err(Failure::protocol("Baseline contains an unknown mutant key"));
    }
    report.baseline = baseline.tests.clone();
    if request.reuse_session {
        let mode = only(&prepared.messages, "prepared")?["sessionReuse"]
            .as_str()
            .filter(|mode| {
                matches!(
                    *mode,
                    "available" | "fresh_global_setup" | "fresh_lifecycle_unknown"
                )
            })
            .ok_or_else(|| Failure::protocol("Invalid session reuse capability"))?;
        report.engine.session_reuse = Some(mode.to_owned());
    }
    let selected_keys: BTreeSet<_> = request.policy.only.iter().cloned().collect();
    if let Some(key) = selected_keys.iter().find(|key| !keys.contains(*key)) {
        return Err(Failure::new(
            "EC_MUTATION_CONFIG",
            format!("Unknown --only mutant key: {key}"),
        ));
    }
    if !selected_keys.is_empty() {
        report
            .mutants
            .retain(|m| selected_keys.contains(&m.candidate.key));
    }
    if let Some(changes) = &changes {
        let tests = baseline
            .tests
            .iter()
            .map(|test| test.file.clone())
            .collect();
        let (selected, selection) = mutation::selection::select(
            request.since.as_ref().unwrap(),
            request.diff_unit.unwrap_or(mutation::selection::Unit::Node),
            changes,
            report.mutants.iter().map(|m| &m.candidate),
            &syntax,
            &sources,
            &tests,
        );
        report
            .mutants
            .retain(|m| selected.contains(&m.candidate.key));
        report.selection = Some(selection);
    }
    if let Some(shard) = request.shard {
        let (selected, info) = shard
            .select(
                report
                    .mutants
                    .iter()
                    .map(|mutant| mutant.candidate.key.as_str()),
            )
            .map_err(Failure::protocol)?;
        report
            .mutants
            .retain(|mutant| selected.contains(&mutant.candidate.key));
        report.shard = Some(info);
    }
    if request.typecheck {
        let response = driver.request(json!({"op":"typecheck", "keys":report.mutants.iter().map(|m| &m.candidate.key).collect::<Vec<_>>()}), Duration::MAX, startup)?;
        if response.timeout {
            return Err(Failure::new(
                "EC_MUTATION_TYPECHECK",
                "Mutant typechecker timed out",
            ));
        }
        response.check_error()?;
        let result = only(&response.messages, "typechecked")?;
        let outcomes = result["outcomes"]
            .as_array()
            .ok_or_else(|| Failure::protocol("Missing typecheck outcomes"))?;
        expected(
            &response.messages,
            &["progress", "typechecked", "done"],
            outcomes.len(),
        )?;
        let info: mutation::TypecheckInfo = serde_json::from_value(result["info"].clone())
            .map_err(|e| Failure::protocol(e.to_string()))?;
        let wanted: BTreeSet<_> = report
            .mutants
            .iter()
            .filter(|m| {
                m.status != Status::Ignored
                    && [".ts", ".tsx", ".mts", ".cts"]
                        .iter()
                        .any(|ext| m.candidate.location.file.ends_with(ext))
            })
            .map(|m| m.candidate.key.clone())
            .collect();
        let mut seen = BTreeSet::new();
        let mut errors = 0;
        let mut equivalent = 0;
        for outcome in outcomes {
            let key = outcome["key"]
                .as_str()
                .ok_or_else(|| Failure::protocol("Missing typecheck key"))?;
            if !wanted.contains(key) || !seen.insert(key.to_owned()) {
                return Err(Failure::protocol("Unknown or duplicate typecheck key"));
            }
            let mutant = report
                .mutants
                .iter_mut()
                .find(|m| m.candidate.key == key)
                .unwrap();
            let reason = outcome["reason"].as_str();
            match outcome["status"].as_str() {
                Some("checked") if outcome["reason"].is_null() => (),
                Some("CompileError") if reason.is_some_and(|r| !r.is_empty()) => {
                    mutant.status = Status::CompileError;
                    mutant.reason = reason.map(String::from);
                    errors += 1;
                }
                Some("Ignored") if reason.is_some_and(|r| r.starts_with("equivalent:")) => {
                    mutant.status = Status::Ignored;
                    mutant.reason = reason.map(String::from);
                    equivalent += 1;
                }
                _ => return Err(Failure::protocol("Invalid typecheck outcome")),
            }
        }
        if wanted != seen
            || info.checked != outcomes.len()
            || info.compile_errors != errors
            || info.equivalent != equivalent
            || info.version.is_empty()
        {
            return Err(Failure::protocol("Incomplete typecheck results or counts"));
        }
        report.engine.typecheck = "complete";
        report.typecheck = Some(info);
    }
    let mut jobs = vec![];
    for mutant in &mut report.mutants {
        if mutant.status != Status::Pending {
            continue;
        }
        let key = &mutant.candidate.key;
        let static_files = baseline.static_hits.get(key);
        mutant.covered_by = baseline
            .tests
            .iter()
            .filter(|t| {
                t.started
                    && t.status == "passed"
                    && (t.hits.contains_key(key)
                        || static_files.is_some_and(|files| files.contains(&t.file)))
            })
            .map(|t| t.id.clone())
            .collect();
        if mutant.covered_by.is_empty() && !baseline.hits.contains_key(key) {
            mutant.status = Status::NoCoverage;
            continue;
        }
        if mutant.covered_by.is_empty() {
            return Err(Failure::protocol(
                "Hit mutant has no attributable baseline tests",
            ));
        }
        if request.weak_mutation && mutant.candidate.weak.is_some() {
            let infected = baseline
                .tests
                .iter()
                .filter(|test| {
                    mutant.covered_by.contains(&test.id)
                        && (static_files.is_some() || test.infected.contains(key))
                })
                .map(|test| test.id.clone())
                .collect::<Vec<_>>();
            if infected.is_empty() {
                mutant.status = Status::Survived;
                mutant.reason =
                    Some("not infected: no covering test observes a different value".into());
            }
            mutant.infected_by = Some(infected);
        }
    }
    let sampled = request
        .policy
        .mutants_per_line
        .map(|maximum| mutation::policy::sample_per_line(&report.mutants, maximum));
    for (index, mutant) in report.mutants.iter_mut().enumerate() {
        if mutant.status != Status::Pending {
            continue;
        }
        let selected = mutant
            .infected_by
            .as_ref()
            .unwrap_or(&mutant.covered_by)
            .clone();
        if sampled
            .as_ref()
            .is_some_and(|selected| !selected.contains(&mutant.candidate.key))
        {
            mutant.reason = Some("Not selected by per-line sampling".into());
            report.policy.as_mut().unwrap().sampled += 1;
            continue;
        }
        if let Some(store) = &cache_store
            && store.data.reuse(&store.stamp.hash, mutant, &selected)
        {
            report.cache.as_mut().unwrap().reused += 1;
            continue;
        }
        if request.concurrency > 1 {
            jobs.push((index, mutant.clone()));
            continue;
        }
        if request
            .policy
            .time_budget
            .is_some_and(|seconds| beginning.elapsed().as_secs_f64() >= seconds)
        {
            mutant.reason = Some("Time budget exhausted before execution".into());
            report.policy.as_mut().unwrap().budget += 1;
            continue;
        }
        if let Some(info) = &mut report.cache {
            info.executed += 1;
        }
        execute_trial(
            mutant,
            &mut driver,
            &TrialContext {
                node: &request.node,
                script: &script,
                run_id: &run_id,
                options: &options,
                baseline: &baseline,
                keys: &keys,
                parallel: false,
            },
        )?;
    }
    let mut additional_facts = vec![];
    if request.concurrency > 1 {
        (driver, additional_facts) = worker_pool::run(
            driver,
            jobs,
            report,
            &request,
            &TrialContext {
                node: &request.node,
                script: &script,
                run_id: &run_id,
                options: &options,
                baseline: &baseline,
                keys: &keys,
                parallel: true,
            },
            beginning,
        )?;
    }
    if driver.stopped {
        driver = Driver::start(&request.node, &script, &run_id, signal, started, cancelled)?;
        let result = driver.request(
            json!({"op":"open","options":options,"restore":true}),
            startup,
            startup,
        )?;
        if result.timeout {
            return Err(Failure::new(
                "EC_MUTATION_RUNNER",
                "Final input validation timed out",
            ));
        }
        result.check_error()?;
        expected(&result.messages, &["done"], 0)?;
    }
    let close = driver.request(json!({"op":"close"}), startup, startup)?;
    if close.timeout {
        return Err(Failure::new("EC_MUTATION_RUNNER", "Driver close timed out"));
    }
    close.check_error()?;
    expected(&close.messages, &["done"], 0)?;
    if let Some(original) = changes {
        let current = crate::mutation_git::acquire(
            &request.root,
            request.since.as_ref().unwrap(),
            &driver.signal,
            request.cache_dir.as_deref().filter(|_| request.cache),
        )
        .map_err(|e| Failure::new("EC_MUTATION_INPUT", e))?;
        if current != original {
            return Err(Failure::new(
                "EC_MUTATION_INPUT",
                "Git selection inputs changed during execution",
            ));
        }
    }
    if let Some(store) = &mut cache_store {
        let mut facts: crate::mutation_cache::Facts =
            serde_json::from_value(only(&close.messages, "done")?["cacheFacts"].clone())
                .map_err(|e| Failure::protocol(e.to_string()))?;
        let mut merge_error = None;
        for worker_facts in additional_facts {
            if let Err(error) = facts.merge(worker_facts) {
                merge_error = Some(error);
                break;
            }
        }
        let current = if let Some(error) = merge_error {
            Err(error)
        } else {
            crate::mutation_cache::acquire(
                &request,
                &facts,
                temp.path(),
                Some(&store.stamp.baseline_paths),
                &driver.signal,
            )
        };
        let problem = match current {
            Ok(stamp) if stamp.hash == store.stamp.hash => None,
            Ok(_) => Some("Cache inputs changed during execution".to_owned()),
            Err(crate::mutation_cache::Error::Changed(reason)) => {
                return Err(Failure::new("EC_MUTATION_INPUT", reason));
            }
            Err(crate::mutation_cache::Error::Unavailable(reason)) => Some(reason),
        };
        if let Some(reason) = problem {
            if report.cache.as_ref().unwrap().reused > 0
                || report.cache.as_ref().unwrap().coverage_reused
            {
                report.cache.as_mut().unwrap().invalidated = true;
                return Err(Failure::new("EC_MUTATION_INPUT", reason));
            }
            report.cache.as_mut().unwrap().disabled_reason = Some(reason);
        } else {
            store.data.retain(&keys);
            for mutant in &report.mutants {
                store.data.record(
                    mutant,
                    mutant.infected_by.as_ref().unwrap_or(&mutant.covered_by),
                );
            }
            crate::mutation_cache::save(store)
                .map_err(|e| Failure::new("EC_MUTATION_CACHE", e.to_string()))?;
            crate::mutation_cache::save_coverage(store, &baseline)
                .map_err(|e| Failure::new("EC_MUTATION_CACHE", e.to_string()))?;
            report.cache.as_mut().unwrap().saved = store.data.len();
        }
    }
    report.run_complete = true;
    if !accepted.is_empty() {
        report.policy.as_mut().unwrap().acceptance =
            mutation::policy::accept_results(&mut report.mutants, &accepted, &keys);
    }
    Ok(())
}

struct TrialContext<'a> {
    node: &'a Path,
    script: &'a Path,
    run_id: &'a str,
    options: &'a Value,
    baseline: &'a Execution,
    keys: &'a BTreeSet<String>,
    parallel: bool,
}
fn reopen(driver: &mut Driver, context: &TrialContext<'_>) -> Result<(), Failure> {
    if driver.stopped {
        *driver = Driver::start(
            context.node,
            context.script,
            context.run_id,
            driver.signal.clone(),
            driver.started.clone(),
            driver.cancelled.clone(),
        )?;
    }
    if driver.sequence == 0 {
        let response = driver.request(
            json!({"op":"open","options":context.options,"restore":true}),
            Duration::from_secs(120),
            Duration::from_secs(120),
        )?;
        if response.timeout {
            return Err(Failure::new(
                "EC_MUTATION_RUNNER",
                "Driver restart timed out",
            ));
        }
        response.check_error()?;
        expected(&response.messages, &["done"], 0)?;
    }
    Ok(())
}
fn execute_trial(
    mutant: &mut Mutant,
    driver: &mut Driver,
    context: &TrialContext<'_>,
) -> Result<(), Failure> {
    let baseline = context.baseline;
    let keys = context.keys;
    let key = &mutant.candidate.key;
    let selected = mutant
        .infected_by
        .as_ref()
        .unwrap_or(&mutant.covered_by)
        .clone();
    let static_files = baseline.static_hits.get(key);
    let mut durations: Vec<_> = baseline
        .tests
        .iter()
        .filter(|t| selected.contains(&t.id))
        .map(|t| t.duration)
        .collect();
    if let Some(files) = static_files {
        durations = files
            .iter()
            .map(|file| baseline.module_durations.get(file).copied().unwrap_or(0.0))
            .collect();
    }
    let total_ms = durations.iter().sum::<f64>() * 1.5 + 5000.0;
    let stall_ms = durations.iter().copied().fold(0.0, f64::max) * 3.0 + 10000.0;
    let hit_limit = baseline
        .hits
        .get(key)
        .copied()
        .unwrap_or(0)
        .saturating_mul(100)
        .max(10000);
    for number in 1..=2 {
        if driver.stopped {
            reopen(driver, context)?;
            if context.parallel {
                let primed = driver.request(json!({"op":"prime"}), Duration::MAX, Duration::MAX)?;
                if primed.timeout {
                    return Err(Failure::new(
                        "EC_MUTATION_BASELINE",
                        "Restarted worker probe timed out",
                    ));
                }
                primed.check_error()?;
                expected(&primed.messages, &["progress", "primed", "done"], 0)?;
                mutation::parallel::compatible_probe(
                    context.baseline,
                    &execution(only(&primed.messages, "primed")?)?,
                )
                .map_err(|message| Failure::new("EC_MUTATION_INPUT", message))?;
            }
        }
        let multiplier = if number == 2 && mutant.attempts[0].status == Status::Timeout {
            3
        } else {
            1
        };
        let response=driver.request(json!({"op":"run","key":key,"selected":selected,"hitLimit":hit_limit,"multiplier":multiplier}),
            Duration::from_secs_f64(total_ms*multiplier as f64/1000.0),Duration::from_secs_f64(stall_ms*multiplier as f64/1000.0))?;
        response.check_error()?;
        let result = if response.timeout {
            Execution {
                selected: selected.clone(),
                wall_timeout: true,
                ..Execution::default()
            }
        } else {
            expected(&response.messages, &["progress", "attempt", "done"], 1)?;
            let value = only(&response.messages, "attempt")?;
            if value["key"] != json!(key) {
                return Err(Failure::protocol("Wrong mutant key in response"));
            }
            execution(value)?
        };
        mutation::validate_execution(&result, &selected).map_err(Failure::protocol)?;
        if result
            .hits
            .keys()
            .chain(result.static_hits.keys())
            .chain(result.tests.iter().flat_map(|t| t.hits.keys()))
            .chain(result.tests.iter().flat_map(|t| t.infected.iter()))
            .any(|k| !keys.contains(k))
        {
            return Err(Failure::protocol("Attempt contains an unknown mutant key"));
        }
        let attempt = mutation::judge(&result, &selected, number);
        let final_result = matches!(attempt.status, Status::Killed | Status::Survived)
            || attempt.reason.contains("hit limit");
        mutant.attempts.push(attempt);
        if final_result || number == 2 {
            break;
        }
    }
    mutant.status = mutation::confirmed(&mutant.attempts);
    mutant.killed_by = baseline
        .tests
        .iter()
        .filter(|t| {
            mutant
                .attempts
                .last()
                .is_some_and(|a| a.killed_by.contains(&t.id))
        })
        .map(|t| t.id.clone())
        .collect();
    mutant.failed_modules = mutant
        .attempts
        .last()
        .map(|a| a.failed_modules.clone())
        .unwrap_or_default();
    if mutant.status == Status::Pending {
        return Err(Failure::new(
            "EC_MUTATION_RUNNER",
            "Mutation attempts produced different failure categories",
        ));
    }
    Ok(())
}
