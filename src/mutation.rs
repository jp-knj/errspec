//! Mutation results and judgments. No compiler handles, processes, or file I/O.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const RULES_VERSION: &str = "1";
pub const MUTATORS: [&str; 18] = [
    "ArithmeticOperator",
    "ArrayDeclaration",
    "ArrowFunction",
    "AssignmentOperator",
    "BlockStatement",
    "BooleanLiteral",
    "CallExpression",
    "ConditionalExpression",
    "EqualityOperator",
    "FnValue",
    "LogicalOperator",
    "MethodExpression",
    "ObjectLiteral",
    "OptionalChaining",
    "Regex",
    "StringLiteral",
    "UnaryOperator",
    "UpdateOperator",
];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Candidate {
    pub key: String,
    #[serde(flatten)]
    pub location: crate::model::Location,
    pub end: u32,
    pub mutator: String,
    pub original: String,
    pub replacement: String,
    pub placement: Placement,
    pub ignored: Option<String>,
    pub regex: Option<(String, String)>,
    pub guard_offset: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Placement {
    Expression,
    Statement,
    Body,
    Block,
    EmptyCondition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum Status {
    Killed,
    Survived,
    NoCoverage,
    Timeout,
    RuntimeError,
    Ignored,
    Pending,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TestResult {
    pub id: String,
    pub task_id: String,
    pub file: String,
    pub name: String,
    pub status: String,
    pub started: bool,
    pub duration: f64,
    pub errors: Vec<String>,
    pub hits: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Execution {
    pub selected: Vec<String>,
    pub tests: Vec<TestResult>,
    pub failed_modules: Vec<String>,
    pub errors: Vec<String>,
    pub static_hits: BTreeMap<String, Vec<String>>,
    pub hits: BTreeMap<String, u64>,
    pub wall_timeout: bool,
    #[serde(default)]
    pub module_durations: BTreeMap<String, f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attempt {
    pub number: usize,
    pub status: Status,
    pub reason: String,
    pub killed_by: Vec<String>,
    pub failed_modules: Vec<String>,
}

/// Validate wire identities before interpreting missing executions as a runtime failure.
pub fn validate_execution(result: &Execution, selected: &[String]) -> Result<(), String> {
    if result.selected != selected {
        return Err("Driver selected IDs differ from request".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    if result
        .module_durations
        .values()
        .any(|d| !d.is_finite() || *d < 0.0)
    {
        return Err("Invalid module duration".into());
    }
    for test in &result.tests {
        if !ids.insert(&test.id) {
            return Err(format!("Duplicate test ID: {}", test.id));
        }
        if !selected.is_empty() && !selected.contains(&test.id) && test.started {
            return Err(format!("Unplanned test started: {}", test.id));
        }
        if !matches!(
            test.status.as_str(),
            "passed" | "failed" | "skipped" | "pending"
        ) || !test.duration.is_finite()
            || test.duration < 0.0
        {
            return Err("Invalid test result".into());
        }
    }
    Ok(())
}

fn is_timeout(error: &str) -> bool {
    error.contains("ERRSPEC_HIT_LIMIT")
        || error.contains("Test timed out in ")
        || error.contains("Hook timed out in ")
        || error.contains("hook timed out after ")
}

pub fn judge(result: &Execution, selected: &[String], number: usize) -> Attempt {
    let killed_by: Vec<_> = result
        .tests
        .iter()
        .filter(|t| {
            selected.contains(&t.id)
                && t.status == "failed"
                && t.errors.iter().any(|e| !is_timeout(e))
        })
        .map(|t| t.id.clone())
        .collect();
    let (status, reason) = if !killed_by.is_empty() {
        (Status::Killed, "Selected test failed")
    } else if result
        .errors
        .iter()
        .chain(
            result
                .tests
                .iter()
                .filter(|t| t.status == "failed")
                .flat_map(|t| &t.errors),
        )
        .any(|e| e.contains("ERRSPEC_HIT_LIMIT"))
    {
        (Status::Timeout, "Execution exceeded the hit limit")
    } else if result.wall_timeout
        || result.errors.iter().any(|e| is_timeout(e))
        || result
            .tests
            .iter()
            .filter(|t| t.status == "failed")
            .flat_map(|t| &t.errors)
            .any(|e| is_timeout(e))
    {
        (Status::Timeout, "Execution exceeded a timeout")
    } else if !result.failed_modules.is_empty()
        || !result.errors.is_empty()
        || selected.is_empty()
        || selected.iter().any(|id| {
            !result
                .tests
                .iter()
                .any(|t| &t.id == id && t.started && t.status == "passed")
        })
    {
        (
            Status::RuntimeError,
            "Selected tests did not all execute successfully",
        )
    } else {
        (Status::Survived, "All selected tests passed")
    };
    Attempt {
        number,
        status,
        reason: reason.into(),
        killed_by,
        failed_modules: result.failed_modules.clone(),
    }
}

/// Bounded confirmation: a different failure category is inconclusive, never a detection.
pub fn confirmed(attempts: &[Attempt]) -> Status {
    match attempts {
        [] => Status::Pending,
        [one] if matches!(one.status, Status::Killed | Status::Survived) => one.status,
        [one] if one.reason.contains("hit limit") => Status::Timeout,
        [first, second]
            if matches!(second.status, Status::Killed | Status::Survived)
                || first.status == second.status =>
        {
            second.status
        }
        _ => Status::Pending,
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mutant {
    #[serde(flatten)]
    pub candidate: Candidate,
    pub status: Status,
    pub covered_by: Vec<String>,
    pub killed_by: Vec<String>,
    pub failed_modules: Vec<String>,
    pub attempts: Vec<Attempt>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Engine {
    pub tsrs_revision: &'static str,
    pub rules_version: &'static str,
    pub node: Option<String>,
    pub vitest: Option<String>,
    pub typecheck: &'static str,
    pub coverage_scope: &'static str,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub total: usize,
    pub killed: usize,
    pub survived: usize,
    pub no_coverage: usize,
    pub timeout: usize,
    pub runtime_error: usize,
    pub ignored: usize,
    pub pending: usize,
    pub score: Option<f64>,
}
#[derive(Debug, Serialize)]
pub struct Gate {
    pub enabled: bool,
    pub passed: Option<bool>,
}
#[derive(Debug, Serialize)]
pub struct Diagnostic {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub kind: &'static str,
    pub schema_version: u8,
    pub engine: Engine,
    pub run_complete: bool,
    pub ok: bool,
    pub tests_executed: bool,
    pub baseline: Vec<TestResult>,
    pub summary: Summary,
    pub mutants: Vec<Mutant>,
    pub gate: Gate,
    pub diagnostics: Vec<Diagnostic>,
    #[serde(skip)]
    pub interrupted: Option<u8>,
}
impl Report {
    pub fn new(gate: bool) -> Self {
        Self {
            kind: "errspec/mutation",
            schema_version: 1,
            engine: Engine {
                tsrs_revision: "1bfd1e65be95131f9242a03687d37006214a96f6",
                rules_version: RULES_VERSION,
                node: None,
                vitest: None,
                typecheck: "not_run",
                coverage_scope: "vitest_vite_workers",
            },
            run_complete: false,
            ok: false,
            tests_executed: false,
            baseline: vec![],
            summary: Summary::default(),
            mutants: vec![],
            gate: Gate {
                enabled: gate,
                passed: None,
            },
            diagnostics: vec![],
            interrupted: None,
        }
    }
    pub fn error(&mut self, code: &str, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            code: code.into(),
            message: message.into(),
        });
    }
    pub fn finish(&mut self) {
        let mut s = Summary {
            total: self.mutants.len(),
            ..Summary::default()
        };
        for m in &self.mutants {
            match m.status {
                Status::Killed => s.killed += 1,
                Status::Survived => s.survived += 1,
                Status::NoCoverage => s.no_coverage += 1,
                Status::Timeout => s.timeout += 1,
                Status::RuntimeError => s.runtime_error += 1,
                Status::Ignored => s.ignored += 1,
                Status::Pending => s.pending += 1,
            }
        }
        let denominator = s.killed + s.survived + s.no_coverage + s.timeout + s.runtime_error;
        if self.run_complete && denominator > 0 {
            s.score = Some((s.killed + s.timeout + s.runtime_error) as f64 / denominator as f64);
        }
        if self.run_complete && self.gate.enabled {
            let passed = s.survived + s.no_coverage == 0;
            self.gate.passed = Some(passed);
            if !passed {
                self.error(
                    "EC_MUTATION_UNDETECTED",
                    "Survived or uncovered mutants remain",
                );
            }
        }
        self.ok = self.run_complete && self.diagnostics.is_empty();
        self.summary = s;
    }
    pub fn exit_code(&self) -> u8 {
        self.interrupted.unwrap_or(if !self.run_complete {
            2
        } else if self.gate.passed == Some(false) {
            1
        } else {
            0
        })
    }
}
