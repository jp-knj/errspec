//! Schedule current candidates through the same sequential and parallel trial path.
use super::{
    Configuration, Failure,
    driver::{Driver, execution, expected, only},
    prepare::Prepared,
    session::Session,
    worker_pool,
};
use errspec::mutation::{self, Execution, Mutant, Report, Status};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path, time::Duration};

pub(super) struct Executed {
    pub(super) session: Session,
    pub(super) additional_facts: Vec<crate::mutation_cache::Facts>,
}

pub(super) fn run(
    configuration: &Configuration,
    session: Session,
    prepared: &Prepared,
    report: &mut Report,
) -> Result<Executed, Failure> {
    // Bind files first so error unwinding stops the driver before deleting them.
    let resources = session.resources;
    let mut driver = session.driver;
    let request = &configuration.request;
    let selection_only = configuration.selection_only;
    let beginning = configuration.beginning;
    let baseline = &prepared.baseline;
    let keys = &prepared.inventory.keys;
    let cache_store = &prepared.cache_store;
    let script = &resources.script;
    let run_id = &resources.run_id;
    let options = &resources.options;
    let mut jobs = vec![];
    let sampled = request
        .policy
        .mutants_per_line
        .map(|maximum| mutation::policy::sample_per_line(&report.mutants, maximum));
    for (index, mutant) in report.mutants.iter_mut().enumerate() {
        if selection_only {
            continue;
        }
        if mutant.status != Status::Pending {
            continue;
        }
        let selected = mutant
            .infected_by
            .as_ref()
            .unwrap_or(&mutant.covered_by)
            .clone();
        if sampled
            .as_ref()
            .is_some_and(|selected| !selected.contains(&mutant.candidate.key))
        {
            mutant.reason = Some("Not selected by per-line sampling".into());
            report.policy.as_mut().unwrap().sampled += 1;
            continue;
        }
        if !request.kill_matrix
            && let Some(store) = &cache_store
            && store.data.reuse(&store.stamp.hash, mutant, &selected)
        {
            report.cache.as_mut().unwrap().reused += 1;
            continue;
        }
        if !request.kill_matrix
            && cache_store
                .as_ref()
                .and_then(|store| store.scope.as_ref())
                .is_some_and(|scope| {
                    scope
                        .previous
                        .as_ref()
                        .is_some_and(|data| data.reuse(&scope.context, mutant, &selected))
                })
        {
            let info = report.cache.as_mut().unwrap();
            info.reused += 1;
            info.scope_reused += 1;
            continue;
        }
        if request.experimental_callgraph
            && cache_store
                .as_ref()
                .and_then(|store| store.scope.as_ref())
                .is_some_and(|scope| {
                    scope.previous.as_ref().is_some_and(|data| {
                        data.reuse_with_static_calls(&scope.context, mutant, &selected)
                    })
                })
        {
            let info = report.cache.as_mut().unwrap();
            info.reused += 1;
            info.scope_reused += 1;
            info.callgraph_reused += 1;
            continue;
        }
        if request.concurrency > 1 {
            jobs.push((index, mutant.clone()));
            continue;
        }
        if request
            .policy
            .time_budget
            .is_some_and(|seconds| beginning.elapsed().as_secs_f64() >= seconds)
        {
            mutant.reason = Some("Time budget exhausted before execution".into());
            report.policy.as_mut().unwrap().budget += 1;
            continue;
        }
        if let Some(info) = &mut report.cache {
            info.executed += 1;
        }
        execute_trial(
            mutant,
            &mut driver,
            &TrialContext {
                node: &request.node,
                script,
                run_id,
                options,
                baseline,
                keys,
                parallel: false,
            },
        )?;
    }
    let mut additional_facts = vec![];
    if request.concurrency > 1 {
        (driver, additional_facts) = worker_pool::run(
            driver,
            jobs,
            report,
            request,
            &TrialContext {
                node: &request.node,
                script,
                run_id,
                options,
                baseline,
                keys,
                parallel: true,
            },
            beginning,
        )?;
    }
    Ok(Executed {
        session: Session { driver, resources },
        additional_facts,
    })
}

pub(super) struct TrialContext<'a> {
    pub(super) node: &'a Path,
    pub(super) script: &'a Path,
    pub(super) run_id: &'a str,
    pub(super) options: &'a Value,
    pub(super) baseline: &'a Execution,
    pub(super) keys: &'a BTreeSet<String>,
    pub(super) parallel: bool,
}
pub(super) fn reopen(driver: &mut Driver, context: &TrialContext<'_>) -> Result<(), Failure> {
    if driver.stopped {
        *driver = Driver::start(
            context.node,
            context.script,
            context.run_id,
            driver.signal.clone(),
            driver.started.clone(),
            driver.cancelled.clone(),
        )?;
    }
    if driver.sequence == 0 {
        let response = driver.request(
            json!({"op":"open","options":context.options,"restore":true}),
            Duration::from_secs(120),
            Duration::from_secs(120),
        )?;
        if response.timeout {
            return Err(Failure::new(
                "EC_MUTATION_RUNNER",
                "Driver restart timed out",
            ));
        }
        response.check_error()?;
        expected(&response.messages, &["done"], 0)?;
    }
    Ok(())
}
pub(super) fn execute_trial(
    mutant: &mut Mutant,
    driver: &mut Driver,
    context: &TrialContext<'_>,
) -> Result<(), Failure> {
    let baseline = context.baseline;
    let keys = context.keys;
    let key = &mutant.candidate.key;
    let selected = mutant
        .infected_by
        .as_ref()
        .unwrap_or(&mutant.covered_by)
        .clone();
    let static_files = baseline.static_hits.get(key);
    let mut durations: Vec<_> = baseline
        .tests
        .iter()
        .filter(|t| selected.contains(&t.id))
        .map(|t| t.duration)
        .collect();
    if let Some(files) = static_files {
        durations = files
            .iter()
            .map(|file| baseline.module_durations.get(file).copied().unwrap_or(0.0))
            .collect();
    }
    let total_ms = durations.iter().sum::<f64>() * 1.5 + 5000.0;
    let stall_ms = durations.iter().copied().fold(0.0, f64::max) * 3.0 + 10000.0;
    let complete = context.options["killMatrix"] == true;
    // Complete cohorts can restart a runner/browser after each failed test.
    // Passing baseline durations do not include those failure-path transitions.
    let total_ms = if complete {
        total_ms.max(stall_ms * selected.len().max(1) as f64)
    } else {
        total_ms
    };
    let hit_limit = baseline
        .hits
        .get(key)
        .copied()
        .unwrap_or(0)
        .saturating_mul(100)
        .max(10000);
    for number in 1..=2 {
        if driver.stopped {
            reopen(driver, context)?;
            if context.parallel {
                let primed = driver.request(json!({"op":"prime"}), Duration::MAX, Duration::MAX)?;
                if primed.timeout {
                    return Err(Failure::new(
                        "EC_MUTATION_BASELINE",
                        "Restarted worker probe timed out",
                    ));
                }
                primed.check_error()?;
                expected(&primed.messages, &["progress", "primed", "done"], 0)?;
                mutation::parallel::compatible_probe(
                    context.baseline,
                    &execution(only(&primed.messages, "primed")?)?,
                )
                .map_err(|message| Failure::new("EC_MUTATION_INPUT", message))?;
            }
        }
        let multiplier = if number == 2 && mutant.attempts[0].status == Status::Timeout {
            3
        } else {
            1
        };
        let response=driver.request(json!({"op":"run","key":key,"selected":selected,"hitLimit":hit_limit,"multiplier":multiplier,"complete":complete}),
            Duration::from_secs_f64(total_ms*multiplier as f64/1000.0),Duration::from_secs_f64(stall_ms*multiplier as f64/1000.0))?;
        response.check_error()?;
        let result = if response.timeout {
            Execution {
                selected: selected.clone(),
                wall_timeout: true,
                ..Execution::default()
            }
        } else {
            expected(&response.messages, &["progress", "attempt", "done"], 1)?;
            let value = only(&response.messages, "attempt")?;
            if value["key"] != json!(key) {
                return Err(Failure::protocol("Wrong mutant key in response"));
            }
            execution(value)?
        };
        mutation::validate_execution(&result, &selected).map_err(Failure::protocol)?;
        if context.options["killMatrix"] == true {
            let mut observation = mutation::kill_matrix::Observation::acquire(&result, &selected);
            if number > 1 {
                observation
                    .invalidate("Retried execution does not establish one complete matrix row");
            }
            mutant.matrix = Some(observation);
        }
        if result
            .hits
            .keys()
            .chain(result.static_hits.keys())
            .chain(result.tests.iter().flat_map(|t| t.hits.keys()))
            .chain(result.tests.iter().flat_map(|t| t.infected.iter()))
            .any(|k| !keys.contains(k))
        {
            return Err(Failure::protocol("Attempt contains an unknown mutant key"));
        }
        let attempt = mutation::judge(&result, &selected, number);
        let final_result = matches!(attempt.status, Status::Killed | Status::Survived)
            || attempt.reason.contains("hit limit");
        mutant.attempts.push(attempt);
        if final_result || number == 2 {
            break;
        }
    }
    mutant.status = mutation::confirmed(&mutant.attempts);
    mutant.killed_by = baseline
        .tests
        .iter()
        .filter(|t| {
            mutant
                .attempts
                .last()
                .is_some_and(|a| a.killed_by.contains(&t.id))
        })
        .map(|t| t.id.clone())
        .collect();
    mutant.failed_modules = mutant
        .attempts
        .last()
        .map(|a| a.failed_modules.clone())
        .unwrap_or_default();
    if mutant.status == Status::Pending {
        return Err(Failure::new(
            "EC_MUTATION_RUNNER",
            "Mutation attempts produced different failure categories",
        ));
    }
    Ok(())
}
