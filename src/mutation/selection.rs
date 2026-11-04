//! Pure judgments over acquired Git changes, source text and owned syntax facts.
use super::{Candidate, syntax::Metadata};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    Node,
    Scope,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lines {
    pub start: usize,
    pub end: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Changes {
    pub base: String,
    pub fingerprint: String,
    pub files: BTreeMap<String, Vec<Lines>>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionReport {
    pub since: String,
    pub base: String,
    pub unit: Unit,
    pub total: usize,
    pub selected: usize,
    pub omitted: usize,
    pub changed_files: Vec<String>,
    pub full_run_reason: Option<String>,
    pub warnings: Vec<Warning>,
}
#[derive(Debug, Serialize)]
pub struct Warning {
    pub code: &'static str,
    pub file: String,
    pub message: String,
}

pub fn test_file(file: &str) -> bool {
    file.split('/').any(|p| p == "__tests__")
        || file
            .rsplit('/')
            .next()
            .is_some_and(|name| name.contains(".test.") || name.contains(".spec."))
}
fn overlaps(start: usize, end: usize, changes: &[Lines]) -> bool {
    changes
        .iter()
        .any(|range| range.start <= end && start <= range.end)
}
fn line(source: &str, offset: usize) -> usize {
    source.as_bytes()[..offset]
        .iter()
        .filter(|c| **c == b'\n')
        .count()
        + 1
}

/// Validate compiler facts at the acquisition boundary before selecting anything.
pub fn validate(metadata: &Metadata, source: &str, candidates: &[Candidate]) -> Result<(), String> {
    if metadata.version != 2 {
        return Err("Unsupported syntax metadata version".into());
    }
    if !super::syntax::source_matches(metadata, source) {
        return Err("Syntax source digest mismatch".into());
    }
    for (name, scope) in &metadata.scopes {
        if [&scope.hash, &scope.effect_hash]
            .iter()
            .any(|value| value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            return Err("Invalid syntax scope hash".into());
        }
        if scope.start > scope.end
            || !source.is_char_boundary(scope.start)
            || !source.is_char_boundary(scope.end)
            || scope.line != line(source, scope.start)
            || scope.end_line != line(source, scope.end)
            || scope
                .parent
                .as_ref()
                .is_some_and(|p| !metadata.scopes.contains_key(p))
        {
            return Err("Invalid syntax scope".into());
        }
        let mut parent = Some(name);
        let mut seen = BTreeSet::new();
        while let Some(key) = parent {
            if !seen.insert(key) {
                return Err("Cyclic syntax scope".into());
            }
            parent = metadata
                .scopes
                .get(key)
                .and_then(|scope| scope.parent.as_ref());
        }
        if scope
            .parent
            .as_ref()
            .and_then(|key| metadata.scopes.get(key))
            .is_some_and(|parent| parent.start > scope.start || parent.end < scope.end)
        {
            return Err("Syntax scope outside its parent".into());
        }
    }
    for c in candidates {
        let Some(scope) = metadata
            .owners
            .get(&c.key)
            .and_then(|key| metadata.scopes.get(key))
        else {
            return Err("Missing mutant owner".into());
        };
        if (c.location.byte_offset as usize) < scope.start
            || c.end as usize > scope.end
            || source.get(c.location.byte_offset as usize..c.end as usize)
                != Some(c.original.as_str())
            || !source.is_char_boundary(c.location.byte_offset as usize)
            || !source.is_char_boundary(c.end as usize)
            || c.location.line != line(source, c.location.byte_offset as usize)
        {
            return Err("Mutant outside its syntax scope".into());
        }
    }
    let mut linked = BTreeSet::new();
    for (index, node) in metadata.nodes.iter().enumerate() {
        if node.start > node.end
            || !source.is_char_boundary(node.start)
            || !source.is_char_boundary(node.end)
            || (index == 0) != (node.parent.is_none())
            || node.parent.is_some_and(|parent| parent >= index)
            || node.children.iter().any(|child| {
                *child <= index
                    || !linked.insert(*child)
                    || metadata
                        .nodes
                        .get(*child)
                        .is_none_or(|child| child.parent != Some(index))
            })
        {
            return Err("Invalid syntax node graph".into());
        }
    }
    if linked.len() != metadata.nodes.len().saturating_sub(1) {
        return Err("Incomplete syntax node graph".into());
    }
    let call_ranges = metadata
        .nodes
        .iter()
        .filter(|node| matches!(node.kind.as_str(), "CallExpression" | "NewExpression"))
        .map(|node| (node.start, node.end))
        .collect::<BTreeSet<_>>();
    let mut seen_calls = BTreeSet::new();
    for call in &metadata.calls {
        let scope = metadata
            .scopes
            .get(&call.scope)
            .ok_or("Unknown call owner")?;
        if !seen_calls.insert((call.start, call.end))
            || !call_ranges.contains(&(call.start, call.end))
            || call.start < scope.start
            || call.end > scope.end
            || call.callee.as_ref().is_some_and(|callee| callee.is_empty())
        {
            return Err("Invalid acquired call site".into());
        }
    }
    for (owner, scope) in &metadata.scopes {
        if let Some(key) = &scope.entry_key {
            let candidate = candidates
                .iter()
                .find(|candidate| &candidate.key == key)
                .ok_or("Missing scope entry candidate")?;
            let body = super::syntax::function_body(metadata, scope)
                .ok_or("Missing function entry body")?;
            if metadata.owners.get(key) != Some(owner)
                || candidate.ignored.is_some()
                || candidate.location.byte_offset as usize != body.start
                || candidate.end as usize != body.end
                || !(body.kind == "Block"
                    && candidate.mutator == "BlockStatement"
                    && candidate.placement == super::Placement::Body
                    || body.kind != "Block"
                        && candidate.mutator == "ArrowFunction"
                        && candidate.placement == super::Placement::Expression)
            {
                return Err("Invalid scope entry candidate".into());
            }
        }
    }
    Ok(())
}

pub fn select<'a>(
    since: &str,
    unit: Unit,
    changes: &Changes,
    candidates: impl Iterator<Item = &'a Candidate>,
    metadata: &BTreeMap<String, Metadata>,
    sources: &BTreeMap<String, String>,
    tests: &BTreeSet<String>,
) -> (BTreeSet<String>, SelectionReport) {
    let candidates: Vec<_> = candidates.collect();
    let mut warnings = vec![];
    let mut full_run_reason = None;
    for file in changes.files.keys() {
        if test_file(file) || tests.contains(file) {
            full_run_reason =
                Some("Tests changed or were removed; recheck all candidate results".into());
            if !tests.contains(file) {
                warnings.push(Warning {
                    code: "EC_MUTATION_CHANGED_TEST_UNCOLLECTED",
                    file: file.clone(),
                    message: "Changed or removed test was not collected by the baseline".into(),
                });
            }
        } else if !metadata.contains_key(file) {
            full_run_reason.get_or_insert_with(||"Changes outside the mutation inventory may affect dependencies or configuration".into());
        }
    }
    let selected: BTreeSet<_> = candidates
        .iter()
        .filter(|c| {
            if full_run_reason.is_some() {
                return true;
            }
            let Some(changed) = changes.files.get(&c.location.file) else {
                return false;
            };
            let source = &sources[&c.location.file];
            match unit {
                Unit::Node => overlaps(
                    c.location.line,
                    line(
                        source,
                        (c.end as usize)
                            .saturating_sub(1)
                            .max(c.location.byte_offset as usize),
                    ),
                    changed,
                ),
                Unit::Scope => {
                    let facts = &metadata[&c.location.file];
                    let mut name = facts.owners.get(&c.key).unwrap();
                    loop {
                        let scope = &facts.scopes[name];
                        if scope.function || scope.parent.is_none() {
                            return overlaps(scope.line, scope.end_line, changed);
                        }
                        name = scope.parent.as_ref().unwrap();
                    }
                }
            }
        })
        .map(|c| c.key.clone())
        .collect();
    let report = SelectionReport {
        since: since.into(),
        base: changes.base.clone(),
        unit,
        total: candidates.len(),
        selected: selected.len(),
        omitted: candidates.len() - selected.len(),
        changed_files: changes.files.keys().cloned().collect(),
        full_run_reason,
        warnings,
    };
    (selected, report)
}

/// Git -U0 hunk headers, independent of quoted paths and patch content.
pub fn hunks(patch: &str) -> Result<Vec<Lines>, String> {
    let mut ranges = vec![];
    for header in patch.lines().filter(|line| line.starts_with("@@ ")) {
        let new = header
            .split_whitespace()
            .nth(2)
            .and_then(|s| s.strip_prefix('+'))
            .ok_or("Malformed Git hunk")?;
        let (start, count) = new.split_once(',').unwrap_or((new, "1"));
        let start: usize = start.parse().map_err(|_| "Malformed Git hunk start")?;
        let count: usize = count.parse().map_err(|_| "Malformed Git hunk count")?;
        let end = if count == 0 {
            start.saturating_add(1)
        } else {
            start.saturating_add(count - 1)
        };
        ranges.push(Lines {
            start: start.max(1),
            end: end.max(1),
        });
    }
    Ok(ranges)
}
