//! Select and acquire semantic facts. Registration and propagation judgments stay in Rust.
use crate::{
    failure_paths,
    owned_process::{self, CaptureFailure, CaptureOptions},
    scenario_source,
    tsrs_adapter::TsrsAdapter,
};
use errspec::{
    discovery::ScanFacts,
    model::{AnalysisInfo, EntryRef, Finding, Source, TSRS_REVISION},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Engine {
    #[default]
    Auto,
    Tsrs,
    Typescript,
}

pub(crate) struct Options {
    pub engine: Engine,
    pub package: Option<PathBuf>,
    pub node: PathBuf,
}

#[derive(Default)]
pub(crate) struct Plan {
    pub sources: Vec<Source>,
    pub discriminants: Option<Vec<String>>,
    pub scenario_files: Vec<PathBuf>,
    pub entries: Vec<EntryRequest>,
}

#[derive(Serialize)]
pub(crate) struct EntryRequest {
    pub entry: EntryRef,
    pub discriminant: String,
}

pub(crate) struct Failure {
    pub analysis: Option<AnalysisInfo>,
    pub diagnostics: Vec<Finding>,
}

pub(crate) struct Session {
    pub analysis: Option<AnalysisInfo>,
    pub backend: Backend,
}

pub(crate) enum Backend {
    Native(TsrsAdapter),
    Official(Box<Response>),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Response {
    protocol: u32,
    engine: String,
    version: String,
    diagnostics: Vec<Finding>,
    tags: Vec<Tags>,
    pub facts: Option<ScanFacts>,
    pub scenarios: Vec<scenario_source::ScenarioCall>,
    pub entries: Vec<EntryResult>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EntryResult {
    id: usize,
    facts: Option<failure_paths::Facts>,
    error: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Tags {
    id: usize,
    tags: Vec<String>,
    error: Option<String>,
}

impl Session {
    pub fn entry(
        &self,
        index: usize,
        root: &Path,
        entry: &EntryRef,
        discriminant: &str,
    ) -> Result<failure_paths::Facts, String> {
        match &self.backend {
            Backend::Native(adapter) => failure_paths::collect(adapter, root, entry, discriminant),
            Backend::Official(response) => {
                let value = &response.entries[index];
                value
                    .facts
                    .clone()
                    .ok_or_else(|| value.error.clone().expect("validated entry result"))
            }
        }
    }
    pub fn tags(&self, index: usize, root: &Path, source: &Source) -> Result<Vec<String>, String> {
        match &self.backend {
            Backend::Native(adapter) => adapter.tags(root, source),
            Backend::Official(response) => {
                let value = &response.tags[index]; // IDs and cardinality validated before exposure.
                match &value.error {
                    Some(error) => Err(error.clone()),
                    None => Ok(value.tags.clone()),
                }
            }
        }
    }
}

fn error(message: impl Into<String>) -> Finding {
    Finding::new("EC_ANALYSIS_BACKEND", message)
}

pub(crate) fn open(
    options: &Options,
    root: &Path,
    project: &str,
    plan: Plan,
) -> Result<Session, Failure> {
    let mut info = None;
    let result = (|| {
        let project = root
            .join(project)
            .canonicalize()
            .map_err(|e| vec![error(e.to_string())])?;
        let explicit = options.package.is_some();
        let package = if options.engine == Engine::Tsrs {
            None
        } else {
            options.package.clone().or_else(|| {
                project
                    .parent()?
                    .ancestors()
                    .map(|dir| dir.join("node_modules/typescript"))
                    .find(|dir| dir.join("package.json").is_file())
            })
        };
        if let Some(package) = package {
            let package = package
                .canonicalize()
                .map_err(|e| vec![error(format!("TypeScript package: {e}"))])?;
            let metadata: serde_json::Value = serde_json::from_slice(
                &fs::read(package.join("package.json")).map_err(|e| vec![error(e.to_string())])?,
            )
            .map_err(|e| vec![error(e.to_string())])?;
            let version = metadata
                .get("version")
                .and_then(|v| v.as_str())
                .ok_or_else(|| vec![error("TypeScript package has no version")])?;
            let supported = matches!(version.split('.').next(), Some("6" | "7"));
            if supported || explicit || options.engine == Engine::Typescript {
                info = Some(AnalysisInfo {
                    engine: "typescript".into(),
                    version: version.into(),
                    package: package.to_string_lossy().into(),
                    reason: if explicit {
                        "explicit TypeScript package"
                    } else if options.engine == Engine::Typescript {
                        "explicit TypeScript engine"
                    } else {
                        "installed TypeScript 6/7 selected to preserve project compiler semantics"
                    }
                    .into(),
                });
                if !supported {
                    return Err(vec![error(format!(
                        "Unsupported TypeScript {version}; expected major 6 or 7"
                    ))]);
                }
                let response = invoke(&options.node, root, &project, &package, &plan)
                    .map_err(|e| vec![error(e)])?;
                validate(&response, version, &plan).map_err(|e| vec![error(e)])?;
                if !response.diagnostics.is_empty() {
                    return Err(response.diagnostics);
                }
                return Ok(Backend::Official(Box::new(response)));
            }
        } else if options.engine == Engine::Typescript {
            return Err(vec![error(
                "TypeScript package not found; install TypeScript 6/7 or set --typescript-package",
            )]);
        }
        if options.engine == Engine::Tsrs {
            info = Some(AnalysisInfo {
                engine: "tsrs".into(),
                version: TSRS_REVISION.into(),
                package: "bundled".into(),
                reason: "explicit tsrs engine".into(),
            });
        }
        TsrsAdapter::open(root, &project.to_string_lossy()).map(Backend::Native)
    })();
    match result {
        Ok(backend) => Ok(Session {
            analysis: info,
            backend,
        }),
        Err(diagnostics) => Err(Failure {
            analysis: info,
            diagnostics,
        }),
    }
}

fn validate(response: &Response, version: &str, plan: &Plan) -> Result<(), String> {
    let malformed = || "Incomplete or incompatible TypeScript analysis response".to_owned();
    if response.protocol != 1 || response.engine != "typescript" || response.version != version {
        return Err(malformed());
    }
    if !response.diagnostics.is_empty() {
        return Ok(());
    }
    if response.tags.len() != plan.sources.len()
        || response.tags.iter().enumerate().any(|(i, t)| {
            t.id != i
                || t.tags.windows(2).any(|w| w[0] >= w[1])
                || t.error.is_some() && !t.tags.is_empty()
        })
    {
        return Err(malformed());
    }
    if response.entries.len() != plan.entries.len()
        || response
            .entries
            .iter()
            .enumerate()
            .any(|(i, entry)| entry.id != i || entry.facts.is_some() == entry.error.is_some())
    {
        return Err(malformed());
    }
    if response.facts.is_some() != plan.discriminants.is_some() {
        return Err(malformed());
    }
    if let Some(facts) = &response.facts {
        let functions: BTreeSet<_> = facts.functions.iter().map(|f| f.id.as_str()).collect();
        let origins: BTreeSet<_> = facts.origins.iter().map(|o| o.id.as_str()).collect();
        if functions.len() != facts.functions.len()
            || origins.len() != facts.origins.len()
            || facts
                .origins
                .iter()
                .any(|o| !functions.contains(o.function.as_str()))
            || facts.edges.iter().any(|e| {
                !functions.contains(e.caller.as_str()) || !functions.contains(e.callee.as_str())
            })
            || facts
                .unknowns
                .iter()
                .any(|u| !functions.contains(u.function.as_str()))
        {
            return Err(malformed());
        }
        for entry in response.entries.iter().filter_map(|e| e.facts.as_ref()) {
            if !functions.contains(entry.function.as_str())
                || entry.paths.iter().any(|p| {
                    p.line == 0 || p.reached.is_some() || p.tags.windows(2).any(|w| w[0] >= w[1])
                })
            {
                return Err(malformed());
            }
        }
    } else if !response.entries.is_empty() {
        return Err(malformed());
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Input<'a> {
    protocol: u32,
    package: &'a Path,
    project: &'a Path,
    root: &'a Path,
    sources: &'a [Source],
    discriminants: &'a Option<Vec<String>>,
    scenario_files: &'a [PathBuf],
    entries: &'a [EntryRequest],
}

struct Interrupts(Vec<signal_hook::SigId>);
impl Drop for Interrupts {
    fn drop(&mut self) {
        for id in self.0.drain(..) {
            signal_hook::low_level::unregister(id);
        }
    }
}

fn invoke(
    node: &Path,
    root: &Path,
    project: &Path,
    package: &Path,
    plan: &Plan,
) -> Result<Response, String> {
    let run = || -> Result<Response, Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        for (name, source) in [
            (
                "driver.mjs",
                include_str!("../packages/analysis-driver/driver.mjs"),
            ),
            (
                "compiler.mjs",
                include_str!("../packages/analysis-driver/compiler.mjs"),
            ),
            (
                "scan.mjs",
                include_str!("../packages/analysis-driver/scan.mjs"),
            ),
            (
                "scenarios.mjs",
                include_str!("../packages/analysis-driver/scenarios.mjs"),
            ),
            (
                "entries.mjs",
                include_str!("../packages/analysis-driver/entries.mjs"),
            ),
        ] {
            fs::write(directory.path().join(name), source)?;
        }
        let input = directory.path().join("input.json");
        fs::write(
            &input,
            serde_json::to_vec(&Input {
                protocol: 1,
                package,
                project,
                root,
                sources: &plan.sources,
                discriminants: &plan.discriminants,
                scenario_files: &plan.scenario_files,
                entries: &plan.entries,
            })?,
        )?;
        let interrupted = Arc::new(AtomicBool::new(false));
        let mut signals = Interrupts(vec![]);
        for number in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
            signals.0.push(signal_hook::flag::register(
                number,
                Arc::clone(&interrupted),
            )?);
        }
        let mut command = Command::new(node);
        // Resolve explicit relative executable paths before changing the working directory.
        if node.components().count() > 1 && node.is_relative() {
            command = Command::new(std::env::current_dir()?.join(node));
        }
        command
            .arg(directory.path().join("driver.mjs"))
            .current_dir(root);
        let output = owned_process::capture_with_options(
            &mut command,
            CaptureOptions {
                stdin: Stdio::from(File::open(input)?),
                timeout: Duration::from_secs(300),
                stdout_limit: 64 * 1024 * 1024,
                stderr_limit: 4 * 1024 * 1024,
                poll_interval: Duration::from_millis(20),
            },
            || interrupted.load(Ordering::SeqCst),
        )
        .map_err(|failure| -> Box<dyn std::error::Error> {
            match failure {
                CaptureFailure::Interrupted => "TypeScript analysis interrupted".into(),
                CaptureFailure::TimedOut => {
                    "TypeScript analysis timed out after 300 seconds".into()
                }
                CaptureFailure::OutputLimit => {
                    "TypeScript analysis output exceeded its limit".into()
                }
                CaptureFailure::Io(error) => error.into(),
            }
        })?;
        if !output.status.success() {
            let mut stderr = String::new();
            output.stderr.as_slice().read_to_string(&mut stderr)?;
            return Err(format!(
                "TypeScript analysis exited {}: {}",
                output.status,
                stderr.chars().take(8000).collect::<String>()
            )
            .into());
        }
        Ok(serde_json::from_slice(&output.stdout)?)
    };
    run().map_err(|e| format!("TypeScript API backend failed: {e}; use --analysis-node for Node.js 22+ and --typescript-package for an installed compiler"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn source() -> Source {
        Source {
            file: "errors.ts".into(),
            export: "Failure".into(),
            discriminant: "_tag".into(),
        }
    }

    fn response() -> serde_json::Value {
        json!({"protocol":1,"engine":"typescript","version":"7.0.2","diagnostics":[],
            "tags":[{"id":0,"tags":["Offline"],"error":null}],"facts":null,"scenarios":[],"entries":[]})
    }

    fn valid(value: serde_json::Value, plan: &Plan) -> bool {
        serde_json::from_value::<Response>(value).is_ok_and(|r| validate(&r, "7.0.2", plan).is_ok())
    }

    #[test]
    fn protocol_requires_matching_identity_and_complete_tag_requests() {
        let plan = Plan {
            sources: vec![source()],
            ..Plan::default()
        };
        assert!(valid(response(), &plan));
        for (field, value) in [
            ("protocol", json!(2)),
            ("engine", json!("tsrs")),
            ("version", json!("7.0.1")),
            ("tags", json!([])),
            ("scenarios", json!([{}])),
        ] {
            let mut malformed = response();
            malformed[field] = value;
            assert!(!valid(malformed, &plan), "{field}");
        }
        for tags in [
            json!([{"id":1,"tags":[],"error":null}]),
            json!([{"id":0,"tags":["Offline","Offline"],"error":null}]),
            json!([{"id":0,"tags":["Z","A"],"error":null}]),
            json!([{"id":0,"tags":["Offline"],"error":"failed"}]),
        ] {
            let mut malformed = response();
            malformed["tags"] = tags;
            assert!(!valid(malformed, &plan));
        }
        // never legitimately has no variants. It is distinct from a missing request.
        let mut never = response();
        never["tags"][0]["tags"] = json!([]);
        assert!(valid(never, &plan));
    }

    #[test]
    fn scan_requires_owned_facts_with_valid_function_references() {
        let plan = Plan {
            sources: vec![source()],
            discriminants: Some(vec!["_tag".into()]),
            ..Plan::default()
        };
        assert!(!valid(response(), &plan));
        let mut complete = response();
        complete["facts"] = json!({"files":["errors.ts"],"candidates":[],"functions":[{
            "id":"f","name":"f","location":{"file":"errors.ts","byteOffset":0,"line":1},"isAsync":false
        }],"origins":[],"edges":[],"unknowns":[]});
        assert!(valid(complete.clone(), &plan));
        let mut orphan = complete.clone();
        orphan["facts"]["unknowns"] = json!([{"function":"missing","location":{"file":"errors.ts","byteOffset":0,"line":1},"expression":"callback()","reason":"unresolved_call"}]);
        assert!(!valid(orphan, &plan));
        let duplicate = complete["facts"]["functions"][0].clone();
        complete["facts"]["functions"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
        assert!(!valid(complete, &plan));
    }

    #[test]
    fn entry_requests_require_results_and_valid_return_locations() {
        let plan = Plan {
            sources: vec![source()],
            discriminants: Some(vec![]),
            entries: vec![EntryRequest {
                entry: EntryRef {
                    file: "errors.ts".into(),
                    export: "run".into(),
                },
                discriminant: "_tag".into(),
            }],
            ..Plan::default()
        };
        let mut complete = response();
        complete["facts"] = json!({"files":["errors.ts"],"candidates":[],"functions":[{
            "id":"f","name":"run","location":{"file":"errors.ts","byteOffset":0,"line":1},"isAsync":false
        }],"origins":[],"edges":[],"unknowns":[]});
        assert!(!valid(complete.clone(), &plan));
        complete["entries"] = json!([{"id":0,"facts":{"function":"f","paths":[{
            "file":"errors.ts","byteOffset":10,"line":2,"tags":["Offline"]
        }]},"error":null}]);
        assert!(valid(complete.clone(), &plan));
        for (field, value) in [("id", json!(1)), ("error", json!("missing"))] {
            let mut bad = complete.clone();
            bad["entries"][0][field] = value;
            assert!(!valid(bad, &plan));
        }
        let mut bad = complete.clone();
        bad["entries"][0]["facts"]["function"] = json!("unknown");
        assert!(!valid(bad, &plan));
        for (field, value) in [
            ("line", json!(0)),
            ("reached", json!(true)),
            ("tags", json!(["Z", "A"])),
        ] {
            let mut bad = complete.clone();
            bad["entries"][0]["facts"]["paths"][0][field] = value;
            assert!(!valid(bad, &plan));
        }
        complete["entries"] = json!([{"id":0,"facts":null,"error":"entry export not found"}]);
        assert!(valid(complete, &plan));
    }
}
