//! Owned subprocess and validated request/response protocol.
use super::Failure;
use errspec::mutation::Execution;
use serde_json::{Value, json};
use std::{
    io::{self, BufRead, BufReader, Write},
    os::unix::process::CommandExt,
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI32, Ordering},
        mpsc::{self, Receiver},
    },
    time::{Duration, Instant},
};

pub(super) struct Driver {
    pub(super) child: Child,
    pub(super) input: ChildStdin,
    pub(super) output: Receiver<Result<String, io::Error>>,
    pub(super) run_id: String,
    pub(super) sequence: u64,
    pub(super) signal: Arc<AtomicI32>,
    pub(super) stopped: bool,
    pub(super) started: Arc<AtomicBool>,
    pub(super) ownership: crate::mutation_ownership::Launch,
    pub(super) endpoint_available: Option<bool>,
    pub(super) cancelled: Arc<AtomicBool>,
}
pub(super) struct Response {
    pub(super) messages: Vec<Value>,
    pub(super) timeout: bool,
}
impl Response {
    pub(super) fn check_error(&self) -> Result<(), Failure> {
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
    pub(super) fn start(
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
    pub(super) fn stop(&mut self) {
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
    pub(super) fn request(
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

pub(super) fn only<'a>(messages: &'a [Value], kind: &str) -> Result<&'a Value, Failure> {
    let values: Vec<_> = messages.iter().filter(|m| m["type"] == kind).collect();
    if values.len() != 1 {
        return Err(Failure::protocol(format!(
            "Expected exactly one {kind} response"
        )));
    }
    Ok(values[0])
}
pub(super) fn execution(value: &Value) -> Result<Execution, Failure> {
    serde_json::from_value(value["execution"].clone()).map_err(|e| Failure::protocol(e.to_string()))
}
pub(super) fn expected(messages: &[Value], allowed: &[&str], count: usize) -> Result<(), Failure> {
    if messages
        .iter()
        .any(|m| !allowed.contains(&m["type"].as_str().unwrap_or("")))
        || only(messages, "done")?["count"] != json!(count)
    {
        return Err(Failure::protocol("Unexpected response or terminal count"));
    }
    Ok(())
}
