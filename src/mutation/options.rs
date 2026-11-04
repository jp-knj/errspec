//! Shared, resolved mutation options. Acquisition and CLI syntax stay outside the model.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Options {
    pub root: PathBuf,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub exclude_mutator: Vec<String>,
    pub vitest_config: Option<PathBuf>,
    pub vitest_project: Option<String>,
    pub vitest_package: Option<PathBuf>,
    pub node: PathBuf,
    pub fail_on_undetected: bool,
    pub typecheck: bool,
    pub weak_mutation: bool,
    pub typecheck_project: Option<PathBuf>,
    pub typecheck_package: Option<PathBuf>,
    pub policy: super::policy::Policy,
    pub since: Option<String>,
    pub diff_unit: Option<super::selection::Unit>,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            root: ".".into(),
            include: vec![],
            exclude: vec![],
            exclude_mutator: vec![],
            vitest_config: None,
            vitest_project: None,
            vitest_package: None,
            node: "node".into(),
            fail_on_undetected: false,
            typecheck: false,
            weak_mutation: false,
            typecheck_project: None,
            typecheck_package: None,
            policy: Default::default(),
            since: None,
            diff_unit: None,
        }
    }
}
impl Options {
    pub fn validate(&self) -> Result<(), String> {
        self.policy.validate()?;
        if self.since.as_ref().is_some_and(|s| s.is_empty()) {
            return Err("since must not be empty".into());
        }
        if self.diff_unit.is_some() && self.since.is_none() {
            return Err("diffUnit requires since".into());
        }
        if !self.typecheck && (self.typecheck_project.is_some() || self.typecheck_package.is_some())
        {
            return Err("typecheckProject and typecheckPackage require typecheck".into());
        }
        if self.root.as_os_str().is_empty() || self.node.as_os_str().is_empty() {
            return Err("root and node must not be empty".into());
        }
        if self
            .include
            .iter()
            .chain(&self.exclude)
            .any(|s| s.is_empty())
        {
            return Err("include and exclude patterns must not be empty".into());
        }
        if [
            &self.vitest_config,
            &self.vitest_package,
            &self.typecheck_project,
            &self.typecheck_package,
        ]
        .into_iter()
        .flatten()
        .any(|p| p.as_os_str().is_empty())
            || self.vitest_project.as_ref().is_some_and(|p| p.is_empty())
        {
            return Err("Configured paths and project names must not be empty".into());
        }
        for name in &self.exclude_mutator {
            if !super::MUTATORS.contains(&name.as_str()) {
                return Err(format!("Unknown mutator: {name}"));
            }
        }
        Ok(())
    }
}
