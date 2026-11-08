//! Acquire and validate inventory, ordinary baseline, probe and reusable evidence.
mod inventory;
use super::{
    Configuration, Failure, MutationRequest,
    driver::{Response, execution, expected, only},
    session::Session,
};
use errspec::mutation::{self, Execution, Report};
pub(super) use inventory::Inventory;
use serde_json::json;
use std::{collections::BTreeSet, time::Duration};

pub(super) struct Prepared {
    pub(super) baseline: Execution,
    pub(super) inventory: Inventory,
    pub(super) projections: Option<crate::mutation_cache::Projections>,
    pub(super) cache_store: Option<crate::mutation_cache::Store>,
}

pub(super) fn run(
    configuration: &Configuration,
    session: &mut Session,
    report: &mut Report,
) -> Result<Prepared, Failure> {
    let request = &configuration.request;
    let (inventory, mut prepared) = inventory::acquire(
        configuration,
        &mut session.driver,
        &session.resources.custom_mutators,
        report,
    )?;
    let projections: crate::mutation_cache::Projections = inventory
        .syntax
        .iter()
        .map(|(name, metadata)| {
            let module = metadata
                .scopes
                .get("module")
                .ok_or_else(|| Failure::protocol("Missing syntax module"))?;
            Ok((
                request.root.join(name).canonicalize()?,
                crate::mutation_cache::Projection {
                    source_hash: metadata.source_hash.clone(),
                    semantic_hash: module.effect_hash.clone(),
                },
            ))
        })
        .collect::<Result<_, Failure>>()?;
    let projections = (request.cache && request.cache_scope).then_some(projections);
    let mut cache_store = probe(
        request,
        session,
        &inventory,
        projections.as_ref(),
        report,
        &mut prepared,
    )?;
    let baseline = validate_baselines(request, &inventory, &prepared, report)?;
    load_scope(request, &inventory, &baseline, &mut cache_store, report)?;
    if request.reuse_session {
        let mode = only(&prepared.messages, "prepared")?["sessionReuse"]
            .as_str()
            .filter(|mode| {
                matches!(
                    *mode,
                    "available" | "fresh_global_setup" | "fresh_lifecycle_unknown"
                )
            })
            .ok_or_else(|| Failure::protocol("Invalid session reuse capability"))?;
        report.engine.session_reuse = Some(mode.to_owned());
    }
    Ok(Prepared {
        baseline,
        inventory,
        projections,
        cache_store,
    })
}

fn probe(
    request: &MutationRequest,
    session: &mut Session,
    inventory: &Inventory,
    projections: Option<&crate::mutation_cache::Projections>,
    report: &mut Report,
    prepared: &mut Response,
) -> Result<Option<crate::mutation_cache::Store>, Failure> {
    let driver = &mut session.driver;
    let temp = &session.resources.directory;
    let files = &inventory.files;
    let keys = &inventory.keys;
    let mut cache_store = None;
    if request.cache {
        expected(
            &prepared.messages,
            &["progress", "baseline", "file", "discovered", "done"],
            files.len(),
        )?;
        let plain = execution(only(&prepared.messages, "baseline")?)?;
        mutation::validate_execution(&plain, &[]).map_err(Failure::protocol)?;
        mutation::validate_baseline(&plain, "Unmodified baseline")
            .map_err(|message| Failure::new("EC_MUTATION_BASELINE", message))?;
        let facts: crate::mutation_cache::Facts =
            serde_json::from_value(only(&prepared.messages, "discovered")?["cacheFacts"].clone())
                .map_err(|e| Failure::protocol(e.to_string()))?;
        cache_store = match crate::mutation_cache::acquire_scoped(
            request,
            &facts,
            temp.path(),
            None,
            &driver.signal,
            projections,
        ) {
            Ok(stamp) => Some(crate::mutation_cache::load(
                crate::mutation_cache::directory(request),
                stamp,
                report.cache.as_mut().unwrap(),
            )),
            Err(crate::mutation_cache::Error::Unavailable(reason)) => {
                report.cache.as_mut().unwrap().disabled_reason = Some(reason);
                None
            }
            Err(crate::mutation_cache::Error::Changed(reason)) => {
                return Err(Failure::new("EC_MUTATION_INPUT", reason));
            }
        };
        let coverage = if request.full_dry_run {
            None
        } else {
            cache_store.as_ref().and_then(|store| {
                crate::mutation_cache::load_coverage(
                    &store.directory,
                    report.cache.as_mut().unwrap(),
                )
                .and_then(|file| {
                    let reused = file.reuse(&store.stamp.hash, &plain, keys);
                    if reused.is_none() {
                        report.cache.as_mut().unwrap().warnings.push(
                            "Coverage inputs or observations changed; refreshing observations"
                                .into(),
                        );
                    }
                    reused
                })
            })
        };
        report.cache.as_mut().unwrap().coverage_reused = coverage.is_some();
        let probe = driver.request(
            json!({"op":"probe","coverage":coverage}),
            Duration::MAX,
            Duration::MAX,
        )?;
        if probe.timeout {
            return Err(Failure::new(
                "EC_MUTATION_BASELINE",
                "Probe baseline timed out",
            ));
        }
        probe.check_error()?;
        expected(&probe.messages, &["progress", "prepared", "done"], 0)?;
        prepared
            .messages
            .retain(|m| m["type"] != "done" && m["type"] != "discovered");
        prepared.messages.extend(probe.messages);
    }
    Ok(cache_store)
}

fn validate_baselines(
    request: &MutationRequest,
    inventory: &Inventory,
    prepared: &Response,
    report: &mut Report,
) -> Result<Execution, Failure> {
    let files = &inventory.files;
    let keys = &inventory.keys;
    expected(
        &prepared.messages,
        &["progress", "baseline", "file", "prepared", "done"],
        if request.cache { 0 } else { files.len() },
    )?;
    let plain = execution(only(&prepared.messages, "baseline")?)?;
    let baseline = execution(only(&prepared.messages, "prepared")?)?;
    mutation::validate_execution(&plain, &[]).map_err(Failure::protocol)?;
    mutation::validate_execution(&baseline, &[]).map_err(Failure::protocol)?;
    mutation::validate_baseline(&plain, "Unmodified baseline")
        .map_err(|message| Failure::new("EC_MUTATION_BASELINE", message))?;
    mutation::validate_baseline(&baseline, "Instrumented baseline")
        .map_err(|message| Failure::new("EC_MUTATION_BASELINE", message))?;
    if baseline
        .hits
        .keys()
        .chain(baseline.static_hits.keys())
        .chain(baseline.tests.iter().flat_map(|t| t.hits.keys()))
        .chain(baseline.tests.iter().flat_map(|t| t.infected.iter()))
        .any(|k| !keys.contains(k))
    {
        return Err(Failure::protocol("Baseline contains an unknown mutant key"));
    }
    report.baseline = baseline.tests.clone();
    report.child_processes = plain
        .child_processes
        .iter()
        .chain(&baseline.child_processes)
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok(baseline)
}

fn load_scope(
    request: &MutationRequest,
    inventory: &Inventory,
    baseline: &Execution,
    cache_store: &mut Option<crate::mutation_cache::Store>,
    report: &mut Report,
) -> Result<(), Failure> {
    let syntax = &inventory.syntax;
    let sources = &inventory.sources;
    if let Some(store) = cache_store
        && let Some(envelope) = &store.stamp.scope_envelope
    {
        let mut grouped = std::collections::BTreeMap::new();
        for mutant in &report.mutants {
            grouped
                .entry(mutant.candidate.location.file.clone())
                .or_insert_with(Vec::new)
                .push(mutant.candidate.clone());
        }
        let source_facts = syntax
            .iter()
            .map(|(file, metadata)| {
                (
                    file.clone(),
                    mutation::scope_cache::Source {
                        text: &sources[file],
                        metadata,
                        candidates: grouped.get(file).map(Vec::as_slice).unwrap_or(&[]),
                    },
                )
            })
            .collect();
        let mut context =
            mutation::scope_cache::from_probe(envelope.clone(), &source_facts, baseline, true)
                .map_err(Failure::protocol)?;
        if request.experimental_callgraph {
            context.static_calls = mutation::callgraph::Graph::build(
                &syntax
                    .iter()
                    .map(|(file, metadata)| (file.clone(), metadata))
                    .collect(),
            );
            if context.static_calls.is_none() {
                report
                    .cache
                    .as_mut()
                    .unwrap()
                    .warnings
                    .push("Static calls were not acquired; keeping full scope revalidation".into());
            }
        }
        crate::mutation_cache::load_scope(store, context, report.cache.as_mut().unwrap());
    }
    Ok(())
}
