//! Revalidate all inputs after workers join, then persist and apply acceptance.
use super::{
    Configuration, Failure,
    driver::{Driver, expected, only},
    execute::Executed,
    prepare::Prepared,
};
use errspec::mutation::{self, Report};
use serde_json::json;
use std::time::Duration;

pub(super) fn run(
    configuration: &Configuration,
    executed: Executed,
    prepared: &mut Prepared,
    report: &mut Report,
) -> Result<(), Failure> {
    let Executed {
        mut session,
        additional_facts,
    } = executed;
    let driver = &mut session.driver;
    let request = &configuration.request;
    let changes = &configuration.changes;
    let accepted = &configuration.accepted;
    let selection_only = configuration.selection_only;
    let baseline = &prepared.baseline;
    let keys = &prepared.inventory.keys;
    let projections = prepared.projections.as_ref();
    let cache_store = &mut prepared.cache_store;
    let temp = &session.resources.directory;
    let script = &session.resources.script;
    let run_id = &session.resources.run_id;
    let options = &session.resources.options;
    let startup = Duration::from_secs(120);
    if driver.stopped {
        *driver = Driver::start(
            &request.node,
            script,
            run_id,
            driver.signal.clone(),
            driver.started.clone(),
            driver.cancelled.clone(),
        )?;
        let result = driver.request(
            json!({"op":"open","options":options,"restore":true}),
            startup,
            startup,
        )?;
        if result.timeout {
            return Err(Failure::new(
                "EC_MUTATION_RUNNER",
                "Final input validation timed out",
            ));
        }
        result.check_error()?;
        expected(&result.messages, &["done"], 0)?;
    }
    let close = driver.request(json!({"op":"close"}), startup, startup)?;
    if close.timeout {
        return Err(Failure::new("EC_MUTATION_RUNNER", "Driver close timed out"));
    }
    close.check_error()?;
    expected(&close.messages, &["done"], 0)?;
    if let Some(original) = changes {
        let current = crate::mutation_git::acquire(
            &request.root,
            request.since.as_ref().unwrap(),
            &driver.signal,
            request.cache_dir.as_deref().filter(|_| request.cache),
        )
        .map_err(|e| Failure::new("EC_MUTATION_INPUT", e))?;
        if &current != original {
            return Err(Failure::new(
                "EC_MUTATION_INPUT",
                "Git selection inputs changed during execution",
            ));
        }
    }
    if let Some(store) = cache_store {
        let mut facts: crate::mutation_cache::Facts =
            serde_json::from_value(only(&close.messages, "done")?["cacheFacts"].clone())
                .map_err(|e| Failure::protocol(e.to_string()))?;
        let mut merge_error = None;
        for worker_facts in additional_facts {
            if let Err(error) = facts.merge(worker_facts) {
                merge_error = Some(error);
                break;
            }
        }
        let current = if let Some(error) = merge_error {
            Err(error)
        } else {
            crate::mutation_cache::acquire_scoped(
                request,
                &facts,
                temp.path(),
                Some(&store.stamp.baseline_paths),
                &driver.signal,
                projections,
            )
        };
        let problem = match current {
            Ok(stamp) if stamp.hash == store.stamp.hash => None,
            Ok(_) => Some("Cache inputs changed during execution".to_owned()),
            Err(crate::mutation_cache::Error::Changed(reason)) => {
                return Err(Failure::new("EC_MUTATION_INPUT", reason));
            }
            Err(crate::mutation_cache::Error::Unavailable(reason)) => Some(reason),
        };
        if let Some(reason) = problem {
            if report.cache.as_ref().unwrap().reused > 0
                || report.cache.as_ref().unwrap().coverage_reused
            {
                report.cache.as_mut().unwrap().invalidated = true;
                return Err(Failure::new("EC_MUTATION_INPUT", reason));
            }
            report.cache.as_mut().unwrap().disabled_reason = Some(reason);
        } else {
            store.data.retain(keys);
            for mutant in report
                .mutants
                .iter()
                .filter(|_| !request.kill_matrix && !selection_only)
            {
                store.data.record(
                    mutant,
                    mutant.infected_by.as_ref().unwrap_or(&mutant.covered_by),
                );
            }
            if !selection_only && !request.kill_matrix {
                crate::mutation_cache::save(store)
                    .map_err(|e| Failure::new("EC_MUTATION_CACHE", e.to_string()))?;
            }
            crate::mutation_cache::save_coverage(store, baseline)
                .map_err(|e| Failure::new("EC_MUTATION_CACHE", e.to_string()))?;
            let scoped = if request.kill_matrix || selection_only {
                0
            } else {
                crate::mutation_cache::save_scope(store, &report.mutants)
                    .map_err(|e| Failure::new("EC_MUTATION_CACHE", e.to_string()))?
            };
            report.cache.as_mut().unwrap().saved = if request.kill_matrix || selection_only {
                0
            } else {
                store.data.len().max(scoped)
            };
        }
    }
    report.run_complete = true;
    if !accepted.is_empty() {
        report.policy.as_mut().unwrap().acceptance =
            mutation::policy::accept_results(&mut report.mutants, accepted, keys);
    }
    Ok(())
}
