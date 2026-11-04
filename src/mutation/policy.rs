//! Selection, acceptance and quality judgments over acquired mutation facts.
use super::{Mutant, Status};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Policy {
    #[serde(default)]
    pub only: Vec<String>,
    pub mutants_per_line: Option<usize>,
    pub time_budget: Option<f64>,
    #[serde(default)]
    pub fail_on_survived: bool,
    pub threshold_high: Option<f64>,
    pub threshold_low: Option<f64>,
    pub threshold_break: Option<f64>,
    #[serde(default)]
    pub critical: Vec<String>,
    pub accepted_file: Option<String>,
}

impl Policy {
    pub fn active(&self) -> bool {
        !self.only.is_empty()
            || self.mutants_per_line.is_some()
            || self.time_budget.is_some()
            || self.fail_on_survived
            || self.threshold_high.is_some()
            || self.threshold_low.is_some()
            || self.threshold_break.is_some()
            || !self.critical.is_empty()
            || self.accepted_file.is_some()
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.mutants_per_line == Some(0) {
            return Err("--mutants-per-line must be positive".into());
        }
        if self.time_budget.is_some_and(|n| !n.is_finite() || n < 0.0) {
            return Err("--time-budget must be a finite nonnegative number".into());
        }
        for threshold in [
            self.threshold_high,
            self.threshold_low,
            self.threshold_break,
        ]
        .into_iter()
        .flatten()
        {
            if !threshold.is_finite() || !(0.0..=100.0).contains(&threshold) {
                return Err("Score thresholds must be between 0 and 100".into());
            }
        }
        if self.threshold_low.unwrap_or(60.0) > self.threshold_high.unwrap_or(80.0) {
            return Err("The low score threshold must not exceed the high threshold".into());
        }
        if self
            .only
            .iter()
            .any(|key| key.len() != 64 || !key.bytes().all(|c| c.is_ascii_hexdigit()))
        {
            return Err("--only expects complete mutant keys".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Acceptance {
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#where: Option<String>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptanceSummary {
    pub applied: Vec<String>,
    pub detected: Vec<String>,
    pub stale: Vec<String>,
}

pub fn accept_results(
    mutants: &mut [Mutant],
    accepted: &BTreeMap<String, Acceptance>,
    all_keys: &BTreeSet<String>,
) -> AcceptanceSummary {
    let mut summary = AcceptanceSummary {
        stale: accepted
            .keys()
            .filter(|key| !all_keys.contains(*key))
            .cloned()
            .collect(),
        ..Default::default()
    };
    for mutant in mutants {
        let Some(entry) = accepted.get(&mutant.candidate.key) else {
            continue;
        };
        match mutant.status {
            Status::Survived | Status::NoCoverage => {
                mutant.accepted = Some(entry.reason.clone());
                mutant.status = Status::Ignored;
                summary.applied.push(mutant.candidate.key.clone());
            }
            Status::Killed | Status::Timeout | Status::RuntimeError => {
                summary.detected.push(mutant.candidate.key.clone())
            }
            _ => (),
        }
    }
    summary
}

// Stable productivity ordering; keys resolve ties independently of traversal order.
const ORDER: [&str; 18] = [
    "EqualityOperator",
    "ConditionalExpression",
    "ArithmeticOperator",
    "LogicalOperator",
    "UnaryOperator",
    "UpdateOperator",
    "AssignmentOperator",
    "OptionalChaining",
    "MethodExpression",
    "BooleanLiteral",
    "StringLiteral",
    "ArrayDeclaration",
    "ObjectLiteral",
    "ArrowFunction",
    "CallExpression",
    "BlockStatement",
    "Regex",
    "FnValue",
];

pub fn sample_per_line(mutants: &[Mutant], maximum: usize) -> BTreeSet<String> {
    let mut groups = BTreeMap::<(&str, usize), Vec<&Mutant>>::new();
    for mutant in mutants.iter().filter(|m| m.status == Status::Pending) {
        groups
            .entry((
                &mutant.candidate.location.file,
                mutant.candidate.location.line,
            ))
            .or_default()
            .push(mutant);
    }
    let mut selected = BTreeSet::new();
    for group in groups.values_mut() {
        group.sort_by_key(|m| {
            (
                ORDER
                    .iter()
                    .position(|name| *name == m.candidate.mutator)
                    .unwrap_or(ORDER.len()),
                &m.candidate.mutator,
                &m.candidate.key,
            )
        });
        selected.extend(group.iter().take(maximum).map(|m| m.candidate.key.clone()));
    }
    selected
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyReport {
    pub options: Policy,
    pub sampled: usize,
    pub budget: usize,
    pub acceptance: AcceptanceSummary,
    pub score_level: Option<&'static str>,
    pub quality_failures: Vec<String>,
}

impl PolicyReport {
    pub fn new(options: Policy) -> Self {
        Self {
            options,
            sampled: 0,
            budget: 0,
            acceptance: AcceptanceSummary::default(),
            score_level: None,
            quality_failures: vec![],
        }
    }
    pub fn judge(&mut self, mutants: &[Mutant], complete: bool, score: Option<f64>) {
        self.quality_failures.clear();
        self.score_level = None;
        if !complete {
            return;
        }
        // A score for the executed subset cannot establish a threshold for all
        // selected candidates when sampling or a budget left others unexecuted.
        let score = if mutants.iter().any(|m| m.status == Status::Pending) {
            None
        } else {
            score
        };
        // As in the reference tool, no detected/undetected candidates gives 100%.
        if score.is_none()
            && mutants.iter().any(|m| m.status == Status::Pending)
            && self.options.threshold_break.is_some()
        {
            self.quality_failures
                .push("Mutation score is unavailable while mutants remain unexecuted".into());
        }
        let percent = score.unwrap_or(1.0) * 100.0;
        self.score_level = if score.is_none() && mutants.iter().any(|m| m.status == Status::Pending)
        {
            None
        } else {
            Some(if percent >= self.options.threshold_high.unwrap_or(80.0) {
                "high"
            } else if percent >= self.options.threshold_low.unwrap_or(60.0) {
                "low"
            } else {
                "danger"
            })
        };
        if let Some(threshold) = self.options.threshold_break
            && percent < threshold
        {
            self.quality_failures.push(format!(
                "Mutation score {percent:.2}% is below {threshold}%"
            ));
        }
        let survived = mutants
            .iter()
            .filter(|m| m.status == Status::Survived)
            .count();
        if self.options.fail_on_survived && survived > 0 {
            self.quality_failures
                .push(format!("{survived} mutants survived"));
        }
        let critical = mutants
            .iter()
            .filter(|m| {
                m.critical
                    && matches!(
                        m.status,
                        Status::Survived | Status::NoCoverage | Status::Pending
                    )
            })
            .count();
        if critical > 0 {
            self.quality_failures.push(format!("{critical} critical mutants are undetected or not run; each must be detected or accepted"));
        }
    }
}
