//! Judge which error tags existing tests assert, from recorded assertion values.
//! Pure: callers supply candidates, records and test sources; nothing here reads files.
use crate::{model::Finding, model::Source};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// A type to judge. Kept apart from scan candidates so their reporting fields can grow.
#[derive(Clone, Debug)]
pub struct ErrorType {
    pub source: Source,
    pub tags: Vec<String>,
}

/// One passing, non-negated assertion written by the Vitest recorder.
#[derive(Debug, Deserialize)]
pub struct AssertionRecord {
    pub file: String,
    pub test: Option<String>,
    /// `[discriminant, value]`; an empty discriminant is a bare string the test received.
    pub values: Vec<(String, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TagStatus {
    /// A passing test required this literal tag in a successful assertion.
    Asserted,
    /// The tag appears in a test file, but no passing test required its value.
    Mentioned,
    Missing,
}

impl TagStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Asserted => "asserted",
            Self::Mentioned => "mentioned",
            Self::Missing => "missing",
        }
    }

    /// Stronger evidence ranks higher; a lower rank than the baseline is a regression.
    fn rank(self) -> u8 {
        match self {
            Self::Asserted => 2,
            Self::Mentioned => 1,
            Self::Missing => 0,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct TagCoverage {
    pub tag: String,
    pub status: TagStatus,
    /// Up to three tests that asserted the tag, as `file > test name`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tests: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct TypeCoverage {
    pub source: Source,
    pub tags: Vec<TagCoverage>,
}

#[derive(Debug, Default, Serialize)]
pub struct Summary {
    pub types: usize,
    pub tags: usize,
    pub asserted: usize,
    pub mentioned: usize,
    pub missing: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverageReport {
    pub kind: &'static str,
    pub schema_version: u32,
    pub tests_executed: bool,
    pub runner_ok: bool,
    /// Internal completion state, separate from the test runner's result.
    #[serde(skip)]
    pub evaluation_complete: bool,
    pub summary: Summary,
    pub types: Vec<TypeCoverage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<CoverageDelta>,
    pub diagnostics: Vec<Finding>,
}

impl CoverageReport {
    /// Regressions and new missing tags fail; improvements and unchanged gaps do not.
    pub fn regressed(&self) -> bool {
        self.delta
            .as_ref()
            .is_some_and(|d| !d.regressed.is_empty() || !d.new_missing.is_empty())
    }
}

impl Default for CoverageReport {
    fn default() -> Self {
        Self {
            kind: "errspec/coverage",
            schema_version: 1,
            tests_executed: false,
            runner_ok: false,
            evaluation_complete: false,
            summary: Summary::default(),
            types: vec![],
            delta: None,
            diagnostics: vec![],
        }
    }
}

const SHOWN_TESTS: usize = 3;

/// Re-exports list the same declaration under several files; judge each tag set once.
fn distinct(candidates: &[ErrorType]) -> Vec<&ErrorType> {
    let mut seen = BTreeSet::new();
    candidates
        .iter()
        .filter(|c| seen.insert((&c.source.export, &c.source.discriminant, &c.tags)))
        .collect()
}

fn quoted(source: &str, tag: &str) -> bool {
    ['\'', '"', '`']
        .iter()
        .any(|q| source.contains(&format!("{q}{tag}{q}")))
}

pub fn judge(
    candidates: &[ErrorType],
    records: &[AssertionRecord],
    test_sources: &[String],
) -> (Vec<TypeCoverage>, Summary) {
    let mut summary = Summary::default();
    let types: Vec<TypeCoverage> = distinct(candidates)
        .into_iter()
        .map(|candidate| {
            let key = candidate.source.discriminant.as_str();
            let tags = candidate
                .tags
                .iter()
                .map(|tag| {
                    let tests: Vec<String> = records
                        .iter()
                        .filter(|r| {
                            r.values
                                .iter()
                                .any(|(k, v)| v == tag && (k == key || k.is_empty()))
                        })
                        .map(|r| match &r.test {
                            Some(test) => format!("{} > {test}", r.file),
                            None => r.file.clone(),
                        })
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .take(SHOWN_TESTS)
                        .collect();
                    let status = if !tests.is_empty() {
                        TagStatus::Asserted
                    } else if test_sources.iter().any(|s| quoted(s, tag)) {
                        TagStatus::Mentioned
                    } else {
                        TagStatus::Missing
                    };
                    TagCoverage {
                        tag: tag.clone(),
                        status,
                        tests,
                    }
                })
                .collect();
            TypeCoverage {
                source: candidate.source.clone(),
                tags,
            }
        })
        .collect();
    // A discriminant/value pair shared by several types counts once.
    let mut counted = BTreeSet::new();
    for ty in &types {
        for tag in &ty.tags {
            if !counted.insert((&ty.source.discriminant, &tag.tag)) {
                continue;
            }
            summary.tags += 1;
            match tag.status {
                TagStatus::Asserted => summary.asserted += 1,
                TagStatus::Mentioned => summary.mentioned += 1,
                TagStatus::Missing => summary.missing += 1,
            }
        }
    }
    summary.types = types.len();
    (types, summary)
}

/// Per-tag statuses saved for gradual adoption, keyed by `file#export#discriminant`.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CoverageBaseline {
    pub kind: String,
    pub schema_version: u32,
    pub types: BTreeMap<String, BTreeMap<String, TagStatus>>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct TagChange {
    #[serde(rename = "type")]
    pub type_key: String,
    pub tag: String,
    pub from: TagStatus,
    pub to: TagStatus,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct TagRef {
    #[serde(rename = "type")]
    pub type_key: String,
    pub tag: String,
}

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverageDelta {
    pub regressed: Vec<TagChange>,
    pub new_missing: Vec<TagRef>,
    pub improved: Vec<TagChange>,
}

const BASELINE_KIND: &str = "errspec/coverage-baseline";

pub fn type_key(source: &Source) -> String {
    format!("{}#{}#{}", source.file, source.export, source.discriminant)
}

pub fn capture(types: &[TypeCoverage]) -> CoverageBaseline {
    CoverageBaseline {
        kind: BASELINE_KIND.into(),
        schema_version: 1,
        types: types
            .iter()
            .map(|t| {
                let tags = t
                    .tags
                    .iter()
                    .map(|tag| (tag.tag.clone(), tag.status))
                    .collect();
                (type_key(&t.source), tags)
            })
            .collect(),
    }
}

pub fn compare(
    previous: &CoverageBaseline,
    types: &[TypeCoverage],
) -> Result<CoverageDelta, Finding> {
    if previous.kind != BASELINE_KIND || previous.schema_version != 1 {
        return Err(Finding::new(
            "EC_BASELINE",
            format!(
                "Not a coverage baseline: {} v{}",
                previous.kind, previous.schema_version
            ),
        ));
    }
    let mut delta = CoverageDelta::default();
    for t in types {
        let key = type_key(&t.source);
        let before = previous.types.get(&key);
        for tag in &t.tags {
            match before.and_then(|tags| tags.get(&tag.tag)) {
                Some(&from) if tag.status.rank() < from.rank() => delta.regressed.push(TagChange {
                    type_key: key.clone(),
                    tag: tag.tag.clone(),
                    from,
                    to: tag.status,
                }),
                Some(&from) if tag.status.rank() > from.rank() => delta.improved.push(TagChange {
                    type_key: key.clone(),
                    tag: tag.tag.clone(),
                    from,
                    to: tag.status,
                }),
                // A tag the baseline never saw must not arrive untested.
                None if tag.status == TagStatus::Missing => delta.new_missing.push(TagRef {
                    type_key: key.clone(),
                    tag: tag.tag.clone(),
                }),
                _ => {}
            }
        }
    }
    Ok(delta)
}
