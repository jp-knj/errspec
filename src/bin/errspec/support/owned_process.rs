//! Bounded acquisition commands. Never signal a PID after reaping its owner.
use std::{
    fs, io,
    os::unix::process::CommandExt,
    path::Path,
    process::{Child, Command, Output, Stdio},
    time::{Duration, Instant},
};

pub(crate) fn retain_children() -> io::Result<()> {
    // An inherited SIG_IGN / SA_NOCLDWAIT would let the kernel recycle a child
    // PID before our group termination. Keep children waitable until we reap.
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = libc::SIG_DFL;
    unsafe {
        libc::sigemptyset(&mut action.sa_mask);
    }
    if unsafe { libc::sigaction(libc::SIGCHLD, &action, std::ptr::null_mut()) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

struct OwnedChild(Option<Child>);
impl OwnedChild {
    fn completed(&mut self) -> io::Result<Option<std::process::ExitStatus>> {
        let child = self.0.as_ref().unwrap();
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        if unsafe {
            libc::waitid(
                libc::P_PID,
                child.id(),
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        } == -1
        {
            return Err(io::Error::last_os_error());
        }
        if unsafe { info.si_pid() } == 0 {
            return Ok(None);
        }
        let mut child = self.0.take().unwrap();
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
        child.wait().map(Some)
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub(crate) struct CaptureOptions {
    pub stdin: Stdio,
    pub timeout: Duration,
    pub stdout_limit: u64,
    pub stderr_limit: u64,
    pub poll_interval: Duration,
}

impl CaptureOptions {
    fn bounded(timeout: Duration, stdout_limit: u64, stderr_limit: u64) -> Self {
        Self {
            stdin: Stdio::null(),
            timeout,
            stdout_limit,
            stderr_limit,
            poll_interval: Duration::from_millis(10),
        }
    }
}

#[derive(Debug)]
pub(crate) enum CaptureFailure {
    Interrupted,
    TimedOut,
    OutputLimit,
    Io(io::Error),
}

impl From<io::Error> for CaptureFailure {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Preserve the existing bounded-command contract for config and Git acquisition.
pub(crate) fn capture(
    command: &mut Command,
    timeout: Duration,
    limit: u64,
    interrupted: impl Fn() -> bool,
) -> io::Result<Output> {
    capture_with_options(
        command,
        CaptureOptions::bounded(timeout, limit, limit),
        interrupted,
    )
    .map_err(|failure| match failure {
        CaptureFailure::Interrupted => {
            io::Error::new(io::ErrorKind::Interrupted, "Command interrupted")
        }
        CaptureFailure::TimedOut => io::Error::new(io::ErrorKind::TimedOut, "Command timed out"),
        CaptureFailure::OutputLimit => io::Error::other(format!(
            "Command output exceeds {} MiB",
            limit / 1024 / 1024
        )),
        CaptureFailure::Io(error) => error,
    })
}

fn check_output(
    stdout: &Path,
    stderr: &Path,
    stdout_limit: u64,
    stderr_limit: u64,
) -> Result<(), CaptureFailure> {
    if fs::metadata(stdout)?.len() > stdout_limit || fs::metadata(stderr)?.len() > stderr_limit {
        return Err(CaptureFailure::OutputLimit);
    }
    Ok(())
}

/// Own one process group until output is bounded and descendants are stopped.
pub(crate) fn capture_with_options(
    command: &mut Command,
    options: CaptureOptions,
    interrupted: impl Fn() -> bool,
) -> Result<Output, CaptureFailure> {
    let CaptureOptions {
        stdin,
        timeout,
        stdout_limit,
        stderr_limit,
        poll_interval,
    } = options;
    let directory = tempfile::tempdir()?;
    let stdout = directory.path().join("stdout");
    let stderr = directory.path().join("stderr");
    let mut child = OwnedChild(Some(
        command
            .stdin(stdin)
            .stdout(fs::File::create(&stdout)?)
            .stderr(fs::File::create(&stderr)?)
            .process_group(0)
            .spawn()?,
    ));
    let start = Instant::now();
    let status = loop {
        if interrupted() {
            return Err(CaptureFailure::Interrupted);
        }
        if start.elapsed() > timeout {
            return Err(CaptureFailure::TimedOut);
        }
        check_output(&stdout, &stderr, stdout_limit, stderr_limit)?;
        if let Some(status) = child.completed()? {
            break status;
        }
        std::thread::sleep(poll_interval);
    };
    check_output(&stdout, &stderr, stdout_limit, stderr_limit)?;
    Ok(Output {
        status,
        stdout: fs::read(stdout)?,
        stderr: fs::read(stderr)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Seek, Write};

    fn shell(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        command
    }

    #[test]
    fn configured_stdin_separate_streams_and_nonzero_status_are_preserved() {
        let mut input = tempfile::tempfile().unwrap();
        input.write_all(b"request\n").unwrap();
        input.rewind().unwrap();
        let mut options = CaptureOptions::bounded(Duration::from_secs(3), 8, 4);
        options.stdin = Stdio::from(input);
        let output =
            capture_with_options(&mut shell("cat; printf warn >&2; exit 7"), options, || {
                false
            })
            .unwrap();
        assert_eq!(output.stdout, b"request\n");
        assert_eq!(output.stderr, b"warn");
        assert_eq!(output.status.code(), Some(7));
    }

    #[test]
    fn independent_stream_limits_reject_only_bytes_over_the_limit() {
        for (script, stdout_limit, stderr_limit, exceeded) in [
            ("printf 1234; printf abcd >&2", 4, 4, false),
            ("printf 12345", 4, 8, true),
            ("printf 12345 >&2", 8, 4, true),
        ] {
            let result = capture_with_options(
                &mut shell(script),
                CaptureOptions::bounded(Duration::from_secs(3), stdout_limit, stderr_limit),
                || false,
            );
            if exceeded {
                assert!(matches!(result, Err(CaptureFailure::OutputLimit)));
            } else {
                let output = result.unwrap();
                assert!(output.status.success());
                assert_eq!(output.stdout, b"1234");
                assert_eq!(output.stderr, b"abcd");
            }
        }
    }

    #[test]
    fn legacy_capture_keeps_its_failure_kinds_and_messages() {
        let error =
            capture(&mut shell("sleep 10"), Duration::from_secs(3), 16, || true).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert_eq!(error.to_string(), "Command interrupted");
        let error = capture(&mut shell("sleep 10"), Duration::ZERO, 16, || false).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(error.to_string(), "Command timed out");
        let error = capture(
            &mut shell("printf 12345"),
            Duration::from_secs(3),
            4,
            || false,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(error.to_string(), "Command output exceeds 0 MiB");
    }

    fn writer(path: &Path, finish: bool) -> Command {
        let ending = if finish { "exit 0" } else { "wait" };
        let mut command = shell(&format!(
            "(while :; do printf x >> \"$1\"; sleep 0.01; done) & \
             while [ ! -s \"$1\" ]; do sleep 0.01; done; {ending}"
        ));
        command.arg("test").arg(path);
        command
    }

    fn assert_writer_stopped(path: &Path) {
        let length = fs::metadata(path).unwrap().len();
        assert!(
            length > 0,
            "the descendant must run before capture finishes"
        );
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(fs::metadata(path).unwrap().len(), length);
    }

    #[test]
    fn successful_parent_exit_stops_its_ordinary_descendants_before_returning() {
        let directory = tempfile::tempdir().unwrap();
        let ticks = directory.path().join("ticks");
        let output = capture(
            &mut writer(&ticks, true),
            Duration::from_secs(3),
            16,
            || false,
        )
        .unwrap();
        assert!(output.status.success());
        assert_writer_stopped(&ticks);
    }

    #[test]
    fn interrupted_acquisition_stops_its_ordinary_descendants_before_returning() {
        let directory = tempfile::tempdir().unwrap();
        let ticks = directory.path().join("ticks");
        let result = capture_with_options(
            &mut writer(&ticks, false),
            CaptureOptions::bounded(Duration::from_secs(3), 16, 16),
            || fs::metadata(&ticks).is_ok_and(|metadata| metadata.len() > 0),
        );
        assert!(matches!(result, Err(CaptureFailure::Interrupted)));
        assert_writer_stopped(&ticks);
    }

    #[test]
    fn spawn_failures_remain_io_errors() {
        let directory = tempfile::tempdir().unwrap();
        let error = capture_with_options(
            &mut Command::new(directory.path().join("missing")),
            CaptureOptions::bounded(Duration::from_secs(3), 16, 16),
            || false,
        )
        .unwrap_err();
        assert!(matches!(error, CaptureFailure::Io(e) if e.kind() == io::ErrorKind::NotFound));
    }
}
