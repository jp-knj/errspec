//! Select candidates, acquire typecheck facts and map current tests before trials.
use super::{
    Configuration, Failure, MutationRequest,
    driver::Driver,
    prepare::{Inventory, Prepared},
    session::Session,
    typecheck,
};
use errspec::mutation::{self, Execution, Report, Status};
use std::{collections::BTreeSet, sync::atomic::AtomicI32};

pub(super) fn apply(
    configuration: &Configuration,
    session: &mut Session,
    prepared: &Prepared,
    report: &mut Report,
) -> Result<(), Failure> {
    select(configuration, prepared, report)?;
    typecheck(&configuration.request, &mut session.driver, report)?;
    coverage(&configuration.request, &prepared.baseline, report)?;
    select_tests(configuration, &session.driver.signal, prepared, report);
    Ok(())
}

fn select(
    configuration: &Configuration,
    prepared: &Prepared,
    report: &mut Report,
) -> Result<(), Failure> {
    let request = &configuration.request;
    let changes = &configuration.changes;
    let selection_only = configuration.selection_only;
    let baseline = &prepared.baseline;
    let Inventory {
        keys,
        syntax,
        sources,
        ..
    } = &prepared.inventory;
    let selected_keys: BTreeSet<_> = request.policy.only.iter().cloned().collect();
    if let Some(key) = selected_keys.iter().find(|key| !keys.contains(*key)) {
        return Err(Failure::new(
            "EC_MUTATION_CONFIG",
            format!("Unknown --only mutant key: {key}"),
        ));
    }
    if !selected_keys.is_empty() {
        report
            .mutants
            .retain(|m| selected_keys.contains(&m.candidate.key));
    }
    if let Some(changes) = &changes {
        let tests = baseline
            .tests
            .iter()
            .map(|test| test.file.clone())
            .collect();
        let (selected, selection) = mutation::selection::select(
            request.since.as_ref().unwrap(),
            if selection_only {
                mutation::selection::Unit::Scope
            } else {
                request.diff_unit.unwrap_or(mutation::selection::Unit::Node)
            },
            changes,
            report.mutants.iter().map(|m| &m.candidate),
            syntax,
            sources,
            &tests,
        );
        report
            .mutants
            .retain(|m| selected.contains(&m.candidate.key));
        report.selection = Some(selection);
    }
    if let Some(shard) = request.shard {
        let (selected, info) = shard
            .select(
                report
                    .mutants
                    .iter()
                    .map(|mutant| mutant.candidate.key.as_str()),
            )
            .map_err(Failure::protocol)?;
        report
            .mutants
            .retain(|mutant| selected.contains(&mutant.candidate.key));
        report.shard = Some(info);
    }
    Ok(())
}

fn typecheck(
    request: &MutationRequest,
    driver: &mut Driver,
    report: &mut Report,
) -> Result<(), Failure> {
    if request.typecheck {
        let keys: Vec<_> = report
            .mutants
            .iter()
            .map(|m| m.candidate.key.as_str())
            .collect();
        let facts = typecheck::acquire(driver, &keys)?;
        let evaluation = mutation::typecheck::evaluate(&expected_typecheck_keys(report), facts);
        apply_typecheck(evaluation, report)?;
    }
    Ok(())
}

fn expected_typecheck_keys(report: &Report) -> BTreeSet<String> {
    report
        .mutants
        .iter()
        .filter(|m| {
            m.status != Status::Ignored
                && [".ts", ".tsx", ".mts", ".cts"]
                    .iter()
                    .any(|ext| m.candidate.location.file.ends_with(ext))
        })
        .map(|m| m.candidate.key.clone())
        .collect()
}

fn apply_typecheck(
    evaluation: mutation::typecheck::Evaluation,
    report: &mut Report,
) -> Result<(), Failure> {
    for update in evaluation.updates {
        let mutant = report
            .mutants
            .iter_mut()
            .find(|m| m.candidate.key == update.key)
            .unwrap();
        mutant.status = update.status;
        mutant.reason = Some(update.reason);
    }
    let info = evaluation
        .completion
        .map_err(|error| Failure::protocol(error.to_string()))?;
    report.engine.typecheck = "complete";
    report.typecheck = Some(info);
    Ok(())
}

fn coverage(
    request: &MutationRequest,
    baseline: &Execution,
    report: &mut Report,
) -> Result<(), Failure> {
    for mutant in &mut report.mutants {
        if mutant.status != Status::Pending {
            continue;
        }
        let key = &mutant.candidate.key;
        let static_files = baseline.static_hits.get(key);
        mutant.covered_by = baseline
            .tests
            .iter()
            .filter(|t| {
                t.started
                    && t.status == "passed"
                    && (t.hits.contains_key(key)
                        || static_files.is_some_and(|files| files.contains(&t.file)))
            })
            .map(|t| t.id.clone())
            .collect();
        if mutant.covered_by.is_empty() && !baseline.hits.contains_key(key) {
            mutant.status = Status::NoCoverage;
            continue;
        }
        if mutant.covered_by.is_empty() {
            return Err(Failure::protocol(
                "Hit mutant has no attributable baseline tests",
            ));
        }
        if request.weak_mutation && !request.kill_matrix && mutant.candidate.weak.is_some() {
            let infected = baseline
                .tests
                .iter()
                .filter(|test| {
                    mutant.covered_by.contains(&test.id)
                        && (static_files.is_some() || test.infected.contains(key))
                })
                .map(|test| test.id.clone())
                .collect::<Vec<_>>();
            if infected.is_empty() {
                mutant.status = Status::Survived;
                mutant.reason =
                    Some("not infected: no covering test observes a different value".into());
            }
            mutant.infected_by = Some(infected);
        }
    }
    Ok(())
}

fn select_tests(
    configuration: &Configuration,
    signal: &AtomicI32,
    prepared: &Prepared,
    report: &mut Report,
) {
    let request = &configuration.request;
    let changes = &configuration.changes;
    let selection_only = configuration.selection_only;
    let baseline = &prepared.baseline;
    let Inventory {
        syntax, sources, ..
    } = &prepared.inventory;
    if selection_only {
        let hints = crate::mutation_cache::historical(request)
            .map(|history| history.killer_hints(&report.mutants))
            .unwrap_or_default();
        let previous = changes
            .as_ref()
            .unwrap()
            .files
            .keys()
            .filter(|file| syntax.contains_key(*file))
            .filter_map(|file| {
                let text = crate::mutation_git::source_at(
                    &request.root,
                    &changes.as_ref().unwrap().base,
                    file,
                    signal,
                )
                .ok()?;
                let source =
                    crate::mutation_source::SourceRequest::for_facts(file.clone(), text.clone());
                let metadata = std::thread::Builder::new()
                    .stack_size(32 * 1024 * 1024)
                    .spawn(move || {
                        crate::mutation_source::generate(source)
                            .ok()
                            .and_then(|result| result.metadata)
                    })
                    .ok()?
                    .join()
                    .ok()??;
                Some((
                    file.clone(),
                    mutation::test_selection::PreviousSource { text, metadata },
                ))
            })
            .collect();
        let fallback = mutation::test_selection::fallback(
            changes.as_ref().unwrap(),
            &report.mutants,
            syntax,
            sources,
            &previous,
        );
        report.test_selection = Some(mutation::test_selection::rank(
            request.since.as_deref().unwrap(),
            changes.as_ref().unwrap(),
            &report.mutants,
            baseline,
            &hints,
            report
                .selection
                .as_ref()
                .and_then(|selection| selection.full_run_reason.as_deref())
                .or(fallback.as_deref()),
        ));
        report.test_selection.as_mut().unwrap().warnings = report
            .selection
            .as_ref()
            .map(|selection| selection.warnings.clone())
            .unwrap_or_default();
    }
}

#[cfg(test)]
#[path = "../../../../../tests/support/typecheck_reports.rs"]
mod typecheck_tests;
