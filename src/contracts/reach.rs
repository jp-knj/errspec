//! Reachability of failure paths from line coverage. No filesystem access: callers resolve paths.
use crate::model::{FailurePath, Finding};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// Line hit counts per source file, as written in an lcov report (`SF` paths unresolved).
pub type LineHits = BTreeMap<String, BTreeMap<usize, u64>>;

/// Read `SF` and `DA` records. Other records are ignored; malformed `DA` lines are errors,
/// because a silently dropped line could turn a reached path into an unreached one or back.
pub fn parse_lcov(text: &str) -> Result<LineHits, String> {
    let mut hits = LineHits::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if let Some(file) = line.strip_prefix("SF:") {
            hits.entry(file.to_owned()).or_default();
            current = Some(file.to_owned());
        } else if let Some(data) = line.strip_prefix("DA:") {
            let file = current.as_ref().ok_or("lcov DA record before SF")?;
            let mut fields = data.split(',');
            let parse = |value: Option<&str>| value.and_then(|v| v.trim().parse().ok());
            let (Some(number), Some(count)) = (parse(fields.next()), parse(fields.next())) else {
                return Err(format!("Invalid lcov DA record: {line}"));
            };
            let entry = hits
                .get_mut(file)
                .expect("file recorded at SF")
                .entry(number as usize)
                .or_insert(0);
            *entry = entry.saturating_add(count);
        } else if line == "end_of_record" {
            current = None;
        }
    }
    Ok(hits)
}

/// Mark each path reached or not and report the unreached ones as EC019.
/// A file absent from the coverage was never loaded, so its paths are unreached.
/// A line the coverage does not list is not judged and counts as reached.
pub fn judge(
    contract: &str,
    paths: &mut [FailurePath],
    hits: &BTreeMap<PathBuf, BTreeMap<usize, u64>>,
    resolve: impl Fn(&str) -> Option<PathBuf>,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for path in paths {
        let lines = resolve(&path.file).and_then(|file| hits.get(Path::new(&file)));
        let reached = match lines {
            None => false,
            Some(lines) => lines.get(&path.line).is_none_or(|&count| count > 0),
        };
        path.reached = Some(reached);
        if !reached {
            let mut finding = Finding::new(
                "EC019",
                format!(
                    "No scenario reached this return of {} (line {})",
                    path.tags.join(" | "),
                    path.line
                ),
            );
            finding.contract_id = Some(contract.into());
            finding.file = Some(path.file.as_str().into());
            finding.byte_offset = Some(path.byte_offset);
            finding.line = Some(path.line as u32);
            findings.push(finding);
        }
    }
    findings
}
