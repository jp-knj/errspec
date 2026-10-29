//! Owned, compiler-independent facts and a finite may-propagate analysis.
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

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

/// Monotone fixed point over (function, origin, channel). Each fact is inserted once.
/// Witnesses do not participate in equality, so recursive graphs cannot grow paths forever.
pub fn propagate(report: &mut ScanReport) {
    let indices: BTreeMap<_, _> = report
        .functions
        .iter()
        .enumerate()
        .map(|(i, f)| (f.id.clone(), i))
        .collect();
    let mut facts: Vec<BTreeMap<(String, Channel), Vec<String>>> =
        vec![BTreeMap::new(); report.functions.len()];
    let mut unknown: BTreeSet<String> =
        report.unknowns.iter().map(|u| u.function.clone()).collect();
    for origin in &report.origins {
        if origin.escapes
            && let Some(&i) = indices.get(&origin.function)
        {
            facts[i].insert(
                (origin.id.clone(), origin.channel),
                vec![origin.function.clone()],
            );
        }
    }
    loop {
        let mut changed = false;
        for edge in &report.edges {
            let (Some(&caller), Some(&callee)) =
                (indices.get(&edge.caller), indices.get(&edge.callee))
            else {
                continue;
            };
            if unknown.contains(&edge.callee) && unknown.insert(edge.caller.clone()) {
                changed = true;
            }
            for ((origin, channel), path) in facts[callee].clone() {
                let channel = match channel {
                    Channel::Throw if edge.catches && !edge.may_rethrow => continue,
                    Channel::Throw if report.functions[caller].is_async => Channel::Rejection,
                    Channel::Rejection if edge.awaited && edge.catches && !edge.may_rethrow => {
                        continue;
                    }
                    Channel::Rejection if !edge.awaited && !edge.returned => continue,
                    Channel::Result if !edge.returned => continue,
                    other => other,
                };
                if let std::collections::btree_map::Entry::Vacant(entry) =
                    facts[caller].entry((origin, channel))
                {
                    let mut witness = vec![edge.caller.clone()];
                    witness.extend(path);
                    entry.insert(witness);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    for (i, function) in report.functions.iter_mut().enumerate() {
        function.effects = std::mem::take(&mut facts[i])
            .into_iter()
            .map(|((origin, channel), via)| Effect {
                origin,
                channel,
                via,
            })
            .collect();
        function.has_unknown = unknown.contains(&function.id);
    }
}
