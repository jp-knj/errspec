use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const TSRS_REVISION: &str = "1bfd1e65be95131f9242a03687d37006214a96f6";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    pub file: String,
    pub byte_offset: u32,
    pub line: usize,
}

/// Provenance for a selected analysis backend. Native defaults retain their original wire format.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnalysisInfo {
    pub engine: String,
    pub version: String,
    pub package: String,
    pub reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub project: String,
    pub contracts: Vec<Contract>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Contract {
    pub id: String,
    pub source: Source,
    #[serde(default)]
    pub boundaries: Vec<String>,
    /// The function that returns the error type; enables failure path checks.
    #[serde(default)]
    pub entry: Option<EntryRef>,
    pub cases: Vec<Case>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntryRef {
    pub file: String,
    pub export: String,
}

/// A `return` of the entry function that yields one or more of the contract's tags.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FailurePath {
    pub file: String,
    pub line: usize,
    pub byte_offset: u32,
    pub tags: Vec<String>,
    /// Present only when an execution recorded reachability.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reached: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Source {
    pub file: String,
    pub export: String,
    pub discriminant: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Case {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub tag: String,
    pub expect: String,
    pub test: TestRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observe: Option<ObservationRequirements>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ObservationRequirements {
    pub faults: Vec<String>,
    pub assertions: Vec<String>,
}

/// Shared with the SDK. The JSON tuple prevents delimiter collisions in identities.
pub fn scenario_test_name(contract: &str, scenario: &str) -> String {
    format!(
        "errspec {}",
        serde_json::to_string(&[contract, scenario]).unwrap()
    )
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TestRef {
    pub file: String,
    /// Full test name: describe ancestors and leaf name joined with a single space.
    #[serde(default)]
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub code: String,
    pub message: Box<str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contract_id: Option<Box<str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<Box<str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scenario_id: Option<Box<str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<Box<str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte_offset: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

impl Finding {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into().into_boxed_str(),
            contract_id: None,
            tag: None,
            scenario_id: None,
            file: None,
            byte_offset: None,
            line: None,
        }
    }
    pub fn for_case(mut self, id: &str, tag: &str) -> Self {
        self.contract_id = Some(id.into());
        self.tag = Some(tag.into());
        self
    }
    pub fn for_scenario(mut self, id: Option<&str>) -> Self {
        self.scenario_id = id.map(Into::into);
        self
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub schema_version: u32,
    pub engine: Engine,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analysis: Option<AnalysisInfo>,
    pub mode: String,
    pub ok: bool,
    pub analysis_complete: bool,
    pub tests_executed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_runner: Option<&'static str>,
    pub contracts: Vec<ContractResult>,
    pub diagnostics: Vec<Finding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<Delta>,
    /// Analysis choices that differ from the project's tsconfig. They do not affect `ok`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub assumptions: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct Engine {
    pub name: String,
    pub revision: String,
}

impl Report {
    pub fn new(mode: &str) -> Self {
        Self {
            schema_version: 2,
            engine: Engine {
                name: "tsrs".into(),
                revision: TSRS_REVISION.into(),
            },
            analysis: None,
            mode: mode.into(),
            ok: false,
            analysis_complete: false,
            tests_executed: false,
            test_runner: None,
            contracts: Vec::new(),
            diagnostics: Vec::new(),
            delta: None,
            assumptions: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractResult {
    pub id: String,
    pub source: Source,
    /// User declarations, not inferred call-graph reachability.
    pub declared_boundaries: Vec<String>,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry: Option<EntryRef>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<FailurePath>,
    pub cases: Vec<CaseResult>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaseResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub tag: String,
    pub registration: &'static str,
    pub test_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test: Option<TestRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expect: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observe: Option<ObservationRequirements>,
    pub observation_status: ObservationStatus,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub observed_faults: Vec<FaultObservation>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub observed_assertions: Vec<AssertionObservation>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ObservationStatus {
    NotRequired,
    NotRun,
    Missing,
    Invalid,
    Incomplete,
    Failed,
    Passed,
}

impl ObservationStatus {
    /// The same name as the JSON value, for text output.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotRequired => "not_required",
            Self::NotRun => "not_run",
            Self::Missing => "missing",
            Self::Invalid => "invalid",
            Self::Incomplete => "incomplete",
            Self::Failed => "failed",
            Self::Passed => "passed",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FaultObservation {
    pub id: String,
    pub calls: u32,
    pub completed: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssertionStatus {
    Passed,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AssertionObservation {
    pub id: String,
    pub status: AssertionStatus,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Snapshot {
    pub schema_version: u32,
    pub kind: String,
    pub contracts: BTreeMap<String, SnapshotContract>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotContract {
    pub source: Source,
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scenarios: Option<BTreeMap<String, SnapshotScenario>>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SnapshotScenario {
    pub tag: String,
    pub expect: String,
    pub test: TestRef,
    pub observe: ObservationRequirements,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Delta {
    pub added: BTreeMap<String, Vec<String>>,
    pub removed: BTreeMap<String, Vec<String>>,
    pub removed_contracts: Vec<String>,
    pub added_scenarios: BTreeMap<String, Vec<String>>,
    pub removed_scenarios: BTreeMap<String, Vec<String>>,
    pub changed_scenarios: BTreeMap<String, Vec<String>>,
}
