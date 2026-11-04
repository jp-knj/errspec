//! Bounded acquisition commands. Never signal a PID after reaping its owner.
use std::{
    fs, io,
    os::unix::process::CommandExt,
    process::{Child, Command, Output, Stdio},
    time::{Duration, Instant},
};

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

pub(crate) fn capture(
    command: &mut Command,
    timeout: Duration,
    limit: u64,
    interrupted: impl Fn() -> bool,
) -> io::Result<Output> {
    let directory = tempfile::tempdir()?;
    let stdout = directory.path().join("stdout");
    let stderr = directory.path().join("stderr");
    let mut child = OwnedChild(Some(
        command
            .stdin(Stdio::null())
            .stdout(fs::File::create(&stdout)?)
            .stderr(fs::File::create(&stderr)?)
            .process_group(0)
            .spawn()?,
    ));
    let start = Instant::now();
    let status = loop {
        if interrupted() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Command interrupted",
            ));
        }
        if start.elapsed() > timeout {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "Command timed out"));
        }
        if fs::metadata(&stdout)?.len() > limit || fs::metadata(&stderr)?.len() > limit {
            return Err(io::Error::other(format!(
                "Command output exceeds {} MiB",
                limit / 1024 / 1024
            )));
        }
        if let Some(status) = child.completed()? {
            break status;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if fs::metadata(&stdout)?.len() > limit || fs::metadata(&stderr)?.len() > limit {
        return Err(io::Error::other(format!(
            "Command output exceeds {} MiB",
            limit / 1024 / 1024
        )));
    }
    Ok(Output {
        status,
        stdout: fs::read(stdout)?,
        stderr: fs::read(stderr)?,
    })
}
