//! Independent owned drivers; all verdicts still use the sequential trial path.
use super::{
    Failure, MutationRequest,
    driver::{Driver, execution, expected, only},
    execute::{TrialContext, execute_trial, reopen},
};
use errspec::mutation::{self, Mutant, Report, Status};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

struct Worker {
    driver: Driver,
    script: PathBuf,
    run_id: String,
    options: Value,
    primary: bool,
}
enum Event {
    Completed {
        index: usize,
        mutant: Box<Mutant>,
        executed: bool,
        budget: bool,
    },
    Failed {
        error: Failure,
        primary: bool,
    },
}
struct Finished {
    driver: Driver,
    facts: Option<crate::mutation_cache::Facts>,
}

fn failure(sender: &mpsc::Sender<Event>, cancelled: &AtomicBool, error: Failure) {
    let primary = !cancelled.swap(true, Ordering::SeqCst);
    let _ = sender.send(Event::Failed { error, primary });
}
fn worker(
    mut worker: Worker,
    queue: &Mutex<VecDeque<(usize, Mutant)>>,
    sender: &mpsc::Sender<Event>,
    request: &MutationRequest,
    base: &TrialContext<'_>,
    beginning: Instant,
) -> Finished {
    let mut measured = base.baseline.clone();
    let cancelled = worker.driver.cancelled.clone();
    let result = (|| -> Result<Option<crate::mutation_cache::Facts>, Failure> {
        if !worker.primary {
            let response =
                worker
                    .driver
                    .request(json!({"op":"prime"}), Duration::MAX, Duration::MAX)?;
            if response.timeout {
                return Err(Failure::new(
                    "EC_MUTATION_BASELINE",
                    "Worker probe timed out",
                ));
            }
            response.check_error()?;
            expected(&response.messages, &["progress", "primed", "done"], 0)?;
            measured = execution(only(&response.messages, "primed")?)?;
            mutation::parallel::compatible_probe(base.baseline, &measured)
                .map_err(|message| Failure::new("EC_MUTATION_INPUT", message))?;
        }
        let context = TrialContext {
            node: base.node,
            script: &worker.script,
            run_id: &worker.run_id,
            options: &worker.options,
            baseline: &measured,
            keys: base.keys,
            parallel: true,
        };
        loop {
            if cancelled.load(Ordering::SeqCst) || worker.driver.signal.load(Ordering::SeqCst) != 0
            {
                break;
            }
            let Some((index, mut mutant)) = queue
                .lock()
                .map_err(|_| Failure::new("EC_MUTATION_RUNNER", "Worker queue poisoned"))?
                .pop_front()
            else {
                break;
            };
            if request
                .policy
                .time_budget
                .is_some_and(|seconds| beginning.elapsed().as_secs_f64() >= seconds)
            {
                mutant.reason = Some("Time budget exhausted before execution".into());
                let _ = sender.send(Event::Completed {
                    index,
                    mutant: Box::new(mutant),
                    executed: false,
                    budget: true,
                });
                continue;
            }
            let outcome = execute_trial(&mut mutant, &mut worker.driver, &context);
            let _ = sender.send(Event::Completed {
                index,
                mutant: Box::new(mutant),
                executed: true,
                budget: false,
            });
            outcome?;
        }
        if !worker.primary && !cancelled.load(Ordering::SeqCst) {
            reopen(&mut worker.driver, &context)?;
            let response = worker.driver.request(
                json!({"op":"close"}),
                Duration::from_secs(120),
                Duration::from_secs(120),
            )?;
            if response.timeout {
                return Err(Failure::new("EC_MUTATION_RUNNER", "Worker close timed out"));
            }
            response.check_error()?;
            expected(&response.messages, &["done"], 0)?;
            if request.cache {
                return serde_json::from_value(
                    only(&response.messages, "done")?["cacheFacts"].clone(),
                )
                .map(Some)
                .map_err(|error| Failure::protocol(error.to_string()));
            }
        }
        Ok(None)
    })();
    let facts = match result {
        Ok(facts) => facts,
        Err(error) => {
            failure(sender, &cancelled, error);
            worker.driver.stop();
            None
        }
    };
    Finished {
        driver: worker.driver,
        facts,
    }
}

pub(super) fn run(
    primary: Driver,
    jobs: Vec<(usize, Mutant)>,
    report: &mut Report,
    request: &MutationRequest,
    base: &TrialContext<'_>,
    beginning: Instant,
) -> Result<(Driver, Vec<crate::mutation_cache::Facts>), Failure> {
    let count = request.concurrency.min(jobs.len());
    report.engine.parallel = Some(mutation::parallel::Info {
        requested: request.concurrency,
        workers: count,
    });
    if jobs.is_empty() {
        return Ok((primary, vec![]));
    }
    if request
        .policy
        .time_budget
        .is_some_and(|seconds| beginning.elapsed().as_secs_f64() >= seconds)
    {
        report.engine.parallel.as_mut().unwrap().workers = 0;
        for (index, _) in jobs {
            report.mutants[index].reason = Some("Time budget exhausted before execution".into());
            report.policy.as_mut().unwrap().budget += 1;
        }
        return Ok((primary, vec![]));
    }
    let cancelled = primary.cancelled.clone();
    let signal = primary.signal.clone();
    let started = primary.started.clone();
    let mut workers = vec![Worker {
        driver: primary,
        script: base.script.into(),
        run_id: base.run_id.into(),
        options: base.options.clone(),
        primary: true,
    }];
    let mut directories = vec![];
    for _ in 1..count {
        let directory = tempfile::Builder::new()
            .prefix("errspec-mutation-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(base.script.parent().unwrap())?;
        let script = directory.path().join("driver.mjs");
        fs::copy(base.script, &script)?;
        fs::copy(
            base.options["setup"]
                .as_str()
                .ok_or_else(|| Failure::protocol("Missing setup path"))?,
            directory.path().join("setup.mjs"),
        )?;
        fs::copy(
            base.script.parent().unwrap().join("weak-runtime.mjs"),
            directory.path().join("weak-runtime.mjs"),
        )?;
        fs::copy(
            base.script.parent().unwrap().join("child-process.mjs"),
            directory.path().join("child-process.mjs"),
        )?;
        let run_id = directory
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .replace('-', "");
        let mut options = base.options.clone();
        options["temp"] = json!(directory.path());
        options["setup"] = json!(directory.path().join("setup.mjs"));
        let mut driver = Driver::start(
            base.node,
            &script,
            &run_id,
            signal.clone(),
            started.clone(),
            cancelled.clone(),
        )?;
        let response = driver.request(
            json!({"op":"open","options":options,"restore":true}),
            Duration::from_secs(120),
            Duration::from_secs(120),
        )?;
        if response.timeout {
            return Err(Failure::new(
                "EC_MUTATION_RUNNER",
                "Worker startup timed out",
            ));
        }
        response.check_error()?;
        expected(&response.messages, &["done"], 0)?;
        if driver.endpoint_available == Some(false) {
            report.workers.get_or_insert_default().endpoint_unavailable += 1;
        }
        workers.push(Worker {
            driver,
            script,
            run_id,
            options,
            primary: false,
        });
        directories.push(directory);
    }
    let queue = Mutex::new(VecDeque::from(jobs));
    let (sender, receiver) = mpsc::channel();
    let mut first_error = None;
    let mut primary_driver = None;
    let mut facts = vec![];
    std::thread::scope(|scope| {
        let mut handles = vec![];
        for (index, runner) in workers.into_iter().enumerate() {
            let sender = sender.clone();
            let worker_cancelled = cancelled.clone();
            let queue = &queue;
            let spawned = std::thread::Builder::new()
                .name(format!("errspec-mutation-{index}"))
                .spawn_scoped(scope, move || {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        worker(runner, queue, &sender, request, base, beginning)
                    }));
                    match result {
                        Ok(finished) => Some((index, finished)),
                        Err(_) => {
                            failure(
                                &sender,
                                &worker_cancelled,
                                Failure::new("EC_MUTATION_RUNNER", "Mutation worker panicked"),
                            );
                            None
                        }
                    }
                });
            match spawned {
                Ok(handle) => handles.push(handle),
                Err(error) => {
                    cancelled.store(true, Ordering::SeqCst);
                    first_error = Some(Failure::new("EC_MUTATION_RUNNER", error.to_string()));
                    break;
                }
            }
        }
        drop(sender);
        for event in receiver {
            match event {
                Event::Completed {
                    index,
                    mutant,
                    executed,
                    budget,
                } => {
                    report.mutants[index] = *mutant;
                    if executed && let Some(cache) = &mut report.cache {
                        cache.executed += 1;
                    }
                    if budget {
                        report.policy.as_mut().unwrap().budget += 1;
                    }
                }
                Event::Failed { error, primary } => {
                    if primary && first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }
        for handle in handles {
            match handle.join() {
                Ok(Some((index, finished))) => {
                    if let Some(value) = finished.facts {
                        facts.push(value);
                    }
                    if index == 0 {
                        primary_driver = Some(finished.driver);
                    }
                }
                Ok(None) => (),
                Err(_) => {
                    cancelled.store(true, Ordering::SeqCst);
                    if first_error.is_none() {
                        first_error = Some(Failure::new(
                            "EC_MUTATION_RUNNER",
                            "Mutation worker panicked",
                        ));
                    }
                }
            }
        }
    });
    if signal.load(Ordering::SeqCst) != 0 && first_error.is_none() {
        first_error = Some(Failure::new(
            "EC_MUTATION_RUNNER",
            "Mutation execution interrupted",
        ));
    }
    if let Some(error) = first_error {
        for mutant in &mut report.mutants {
            if mutant.status == Status::Pending && mutant.reason.is_none() {
                mutant.reason = Some("Not completed after worker failure or interruption".into());
            }
        }
        return Err(error);
    }
    // Secondary drivers are stopped on join before their private files disappear.
    drop(directories);
    Ok((
        primary_driver
            .ok_or_else(|| Failure::new("EC_MUTATION_RUNNER", "Primary worker did not return"))?,
        facts,
    ))
}
