//! Owned, compiler-independent facts and a finite may-propagate analysis.
use serde::Serialize;
mod propagation;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    pub file: String,
    pub byte_offset: u32,
    pub line: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct Candidate {
    pub source: crate::model::Source,
    pub tags: Vec<String>,
    /// Other public paths to the same declaration. `source` names the declaring file when it exports the type.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reexports: Vec<Export>,
    /// Every discriminant property is declared outside the project directory or in node_modules.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub external: bool,
    /// Channels of origins whose error type is this declaration or a union containing it.
    pub flows: Vec<Channel>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Export {
    pub file: String,
    pub export: String,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    Throw,
    Rejection,
    Result,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Origin {
    pub id: String,
    pub function: String,
    pub location: Location,
    pub channel: Channel,
    pub error_type: String,
    pub caught: bool,
    pub escapes: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Effect {
    pub origin: String,
    pub channel: Channel,
    /// Finite witness to the origin. Recursion may revisit a function in a different channel.
    pub via: Vec<String>,
}

/// Collected function identity, before propagation has computed any effects.
#[derive(Debug)]
pub struct FunctionFacts {
    pub id: String,
    pub name: String,
    pub location: Location,
    pub is_async: bool,
}

/// Owned facts collected by a frontend. No compiler handles or derived summaries.
#[derive(Debug, Default)]
pub struct ScanFacts {
    pub files: Vec<String>,
    pub candidates: Vec<Candidate>,
    pub origins: Vec<Origin>,
    pub functions: Vec<FunctionFacts>,
    pub edges: Vec<Edge>,
    pub unknowns: Vec<Unknown>,
}

impl ScanFacts {
    /// Analyze a successfully collected scan and assemble its wire report once.
    /// Completion does not imply that every runtime error or call target is known.
    pub fn into_report(mut self) -> ScanReport {
        self.files.sort();
        self.files.dedup();
        self.candidates.sort_by(|a, b| {
            (&a.source.file, &a.source.export, &a.source.discriminant).cmp(&(
                &b.source.file,
                &b.source.export,
                &b.source.discriminant,
            ))
        });
        let summaries = propagation::evaluate(
            self.functions.iter().map(|f| (f.id.as_str(), f.is_async)),
            &self.origins,
            &self.edges,
            &self.unknowns,
        );
        ScanReport {
            scan_complete: true,
            files: self.files,
            candidates: self.candidates,
            origins: self.origins,
            functions: self
                .functions
                .into_iter()
                .zip(summaries)
                .map(|(f, summary)| Function {
                    id: f.id,
                    name: f.name,
                    location: f.location,
                    is_async: f.is_async,
                    effects: summary.effects,
                    has_unknown: summary.has_unknown,
                })
                .collect(),
            edges: self.edges,
            unknowns: self.unknowns,
            ..ScanReport::default()
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Function {
    pub id: String,
    pub name: String,
    pub location: Location,
    pub is_async: bool,
    pub effects: Vec<Effect>,
    pub has_unknown: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Edge {
    pub caller: String,
    pub callee: String,
    pub location: Location,
    pub awaited: bool,
    pub returned: bool,
    pub catches: bool,
    pub may_rethrow: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Unknown {
    pub function: String,
    pub location: Location,
    pub expression: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub kind: &'static str,
    pub schema_version: u32,
    pub scan_complete: bool,
    pub all_runtime_errors_known: bool,
    pub files: Vec<String>,
    pub candidates: Vec<Candidate>,
    pub origins: Vec<Origin>,
    pub functions: Vec<Function>,
    pub edges: Vec<Edge>,
    pub unknowns: Vec<Unknown>,
    pub assumptions: Vec<&'static str>,
    pub diagnostics: Vec<crate::model::Finding>,
}

impl Default for ScanReport {
    fn default() -> Self {
        Self {
            kind: "errspec/scan",
            schema_version: 1,
            scan_complete: false,
            all_runtime_errors_known: false,
            files: vec![],
            candidates: vec![],
            origins: vec![],
            functions: vec![],
            edges: vec![],
            unknowns: vec![],
            assumptions: vec![
                "May-propagate analysis, not a proof of runtime reachability or exhaustiveness.",
                "Dependencies in node_modules and declaration-only implementations are not traversed.",
                "Implicit exceptions, getters/proxies, eval and arbitrary callback execution are not modeled.",
                "Return error properties and finite discriminants are candidates; their domain meaning requires review.",
                "Conditional catches/rethrows and finally overrides are conservatively approximated.",
                "Static calls assume function declarations and built-in globals are not reassigned or monkey-patched; mutable aliases and callbacks are unresolved.",
            ],
            diagnostics: vec![],
        }
    }
}

/// Recompute an existing report's function summaries from its recorded facts.
/// Kept for callers of the original Rust API; CLI collection uses `ScanFacts`.
pub fn propagate(report: &mut ScanReport) {
    let summaries = propagation::evaluate(
        report.functions.iter().map(|f| (f.id.as_str(), f.is_async)),
        &report.origins,
        &report.edges,
        &report.unknowns,
    );
    for (function, summary) in report.functions.iter_mut().zip(summaries) {
        function.effects = summary.effects;
        function.has_unknown = summary.has_unknown;
    }
}
