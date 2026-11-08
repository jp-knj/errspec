//! Resolve request inputs before acquiring driver resources.
use super::{Failure, MutationRequest};
use errspec::mutation::{self, Report};
use std::{fs, io, path::PathBuf, sync::atomic::AtomicI32, time::Instant};

pub(super) struct Configuration {
    pub(super) request: MutationRequest,
    pub(super) changes: Option<mutation::selection::Changes>,
    pub(super) accepted: std::collections::BTreeMap<String, mutation::policy::Acceptance>,
    pub(super) beginning: Instant,
    pub(super) selection_only: bool,
}

pub(super) fn prepare(
    mut request: MutationRequest,
    report: &mut Report,
    signal: &AtomicI32,
    selection_only: bool,
) -> Result<Configuration, Failure> {
    if selection_only && request.since.is_none() {
        return Err(Failure::new(
            "EC_MUTATION_CONFIG",
            "selectTests requires since",
        ));
    }
    if request.kill_matrix {
        report.kill_matrix = Some(mutation::kill_matrix::analyze(&[]));
    }
    request
        .policy
        .validate()
        .map_err(|e| Failure::new("EC_MUTATION_CONFIG", e))?;
    if request.policy.active() {
        report.policy = Some(mutation::policy::PolicyReport::new(request.policy.clone()));
    }
    let beginning = Instant::now();
    for family in &request.exclude_mutator {
        if !mutation::MUTATORS.contains(&family.as_str()) && request.plugins.is_empty() {
            return Err(Failure::new(
                "EC_MUTATION_CONFIG",
                format!("Unknown mutator: {family}"),
            ));
        }
    }
    request.root = absolute(request.root)?
        .canonicalize()
        .map_err(|e| Failure::new("EC_MUTATION_CONFIG", e.to_string()))?;
    let recovery = crate::mutation_ownership::recover(signal)?;
    if !recovery.is_empty() {
        report.workers = Some(recovery);
    }
    if request.cache {
        let directory = crate::mutation_cache::directory(&request);
        report.cache = Some(mutation::cache::Info {
            experimental_callgraph: request.experimental_callgraph,
            scope: request.cache_scope,
            directory: directory.display().to_string(),
            relocation: request.cache_relocation,
            ..Default::default()
        });
        if request.experimental_callgraph {
            report.cache.as_mut().unwrap().warnings.push(
                "Experimental static-call reuse can miss dependencies carried through values or shared state; only old survivors may be pruned".into()
            );
        }
        fs::create_dir_all(&directory)
            .map_err(|e| Failure::new("EC_MUTATION_CACHE", e.to_string()))?;
        let directory = directory
            .canonicalize()
            .map_err(|e| Failure::new("EC_MUTATION_CACHE", e.to_string()))?;
        if request.root.starts_with(&directory) {
            return Err(Failure::new(
                "EC_MUTATION_CONFIG",
                "cacheDir must not contain the project root",
            ));
        }
        crate::mutation_cache::check_directory(&directory)
            .map_err(|e| Failure::new("EC_MUTATION_CONFIG", e))?;
        report.cache.as_mut().unwrap().directory = directory
            .strip_prefix(&request.root)
            .unwrap_or(&directory)
            .display()
            .to_string();
        request.cache_dir = Some(directory);
    }
    let changes = request
        .since
        .as_ref()
        .map(|since| {
            crate::mutation_git::acquire(
                &request.root,
                since,
                signal,
                request.cache_dir.as_deref().filter(|_| request.cache),
            )
        })
        .transpose()
        .map_err(|e| Failure::new("EC_MUTATION_CONFIG", e))?;
    if !selection_only
        && request.policy.accepted_file.is_none()
        && request.root.join("errspec.accepted.json").is_file()
    {
        request.policy.accepted_file = Some("errspec.accepted.json".into());
        report.policy = Some(mutation::policy::PolicyReport::new(request.policy.clone()));
    }
    let accepted = if let Some(file) = &request.policy.accepted_file {
        let file = request.root.join(file);
        let accepted: std::collections::BTreeMap<String, mutation::policy::Acceptance> =
            match fs::read(&file) {
                Ok(bytes) => serde_json::from_slice(&bytes)
                    .map_err(|e| Failure::new("EC_MUTATION_CONFIG", e.to_string()))?,
                Err(e) if e.kind() == io::ErrorKind::NotFound => Default::default(),
                Err(e) => return Err(e.into()),
            };
        if accepted
            .values()
            .any(|entry| entry.reason.trim().is_empty())
        {
            return Err(Failure::new(
                "EC_MUTATION_CONFIG",
                "Acceptance reasons must not be empty",
            ));
        }
        accepted
    } else {
        std::collections::BTreeMap::new()
    };
    request.runner_config = request.runner_config.map(absolute).transpose()?;
    request.runner_package = request.runner_package.map(absolute).transpose()?;
    request.vitest_config = request.vitest_config.map(absolute).transpose()?;
    request.vitest_package = request.vitest_package.map(absolute).transpose()?;
    request.typecheck_project = request.typecheck_project.map(absolute).transpose()?;
    request.typecheck_package = request.typecheck_package.map(absolute).transpose()?;
    if request.node.components().count() > 1 {
        request.node = absolute(request.node)?;
    }
    Ok(Configuration {
        request,
        changes,
        accepted,
        beginning,
        selection_only,
    })
}

fn absolute(path: PathBuf) -> Result<PathBuf, Failure> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}
