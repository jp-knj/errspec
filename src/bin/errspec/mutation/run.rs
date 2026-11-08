//! Mutation process ownership, protocol validation, and acquisition.
mod configuration;
mod driver;
mod execute;
mod finalize;
mod plan;
mod prepare;
mod session;
mod source_protocol;
mod worker_pool;
use configuration::Configuration;
use errspec::mutation::options::Options as MutationRequest;
use errspec::mutation::{self, Report};
use std::{
    io::{self, Read},
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI32, Ordering},
    },
};

pub(crate) fn source_entry() -> ExitCode {
    let result = std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            let mut input = String::new();
            io::stdin()
                .read_to_string(&mut input)
                .map_err(|e| e.to_string())?;
            source_protocol::respond(&input)
        })
        .map_err(|e| e.to_string())
        .and_then(|worker| worker.join().map_err(|_| "Mutation parser panicked".into()))
        .and_then(|r| r);
    match result {
        Ok(result) => {
            println!("{result}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

pub(crate) fn run(request: MutationRequest) -> Report {
    run_mode(request, false)
}
pub(crate) fn select_tests(mut request: MutationRequest) -> mutation::test_selection::Report {
    request.policy = Default::default();
    request.shard = None;
    request.concurrency = 1;
    request.fail_on_undetected = false;
    request.kill_matrix = false;
    request.experimental_callgraph = false;
    request.weak_mutation = false;
    request.typecheck = false;
    request.typecheck_package = None;
    request.typecheck_project = None;
    mutation::test_selection::Report::from_mutation(run_mode(request, true))
}
fn run_mode(request: MutationRequest, selection_only: bool) -> Report {
    let mut report = Report::new(request.fail_on_undetected);
    report.presentation = mutation::reporting::Presentation {
        root: request.root.clone(),
        reporters: request.reporters.clone(),
        directory: request.report_dir.clone(),
    };
    let signal = Arc::new(AtomicI32::new(0));
    let started = Arc::new(AtomicBool::new(false));
    let mut registrations = vec![];
    for number in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        // Only an atomic write is performed by the signal handler.
        let signal = signal.clone();
        match unsafe {
            signal_hook::low_level::register(number, move || signal.store(number, Ordering::SeqCst))
        } {
            Ok(id) => registrations.push(id),
            Err(e) => {
                report.error("EC_MUTATION_RUNNER", e.to_string());
                report.finish();
                return report;
            }
        }
    }
    if let Err(error) = run_inner(
        request,
        &mut report,
        signal.clone(),
        started.clone(),
        selection_only,
    ) {
        report.run_complete = false;
        report.error(&error.code, error.message);
    }
    report.tests_executed |= started.load(Ordering::SeqCst);
    let interrupted = signal.load(Ordering::SeqCst);
    if interrupted != 0 {
        report.interrupted = Some((128 + interrupted) as u8);
        report.run_complete = false;
    }
    for registration in registrations {
        signal_hook::low_level::unregister(registration);
    }
    if selection_only {
        report.policy = None;
    }
    report.finish();
    report
}

struct Failure {
    code: String,
    message: String,
}
impl Failure {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
    fn protocol(message: impl Into<String>) -> Self {
        Self::new("EC_MUTATION_PROTOCOL", message)
    }
}
impl From<io::Error> for Failure {
    fn from(e: io::Error) -> Self {
        Self::new("EC_MUTATION_RUNNER", e.to_string())
    }
}

fn run_inner(
    request: MutationRequest,
    report: &mut Report,
    signal: Arc<AtomicI32>,
    started: Arc<AtomicBool>,
    selection_only: bool,
) -> Result<(), Failure> {
    let configuration = configuration::prepare(request, report, &signal, selection_only)?;
    let mut session = session::open(&configuration, report, signal, started)?;
    let mut prepared = prepare::run(&configuration, &mut session, report)?;
    plan::apply(&configuration, &mut session, &prepared, report)?;
    let executed = execute::run(&configuration, session, &prepared, report)?;
    finalize::run(&configuration, executed, &mut prepared, report)
}
