//! A native group leader retains ownership while Node runs project code.
//! Recovery asks that live leader to terminate its own group, never an old PID.
use errspec::mutation::ownership::{Decision, Info, Record, Response};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Read, Write},
    os::fd::{AsRawFd, FromRawFd},
    os::unix::{
        ffi::OsStrExt,
        fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
    sync::atomic::{AtomicI32, Ordering},
    time::{Duration, Instant},
};

fn directory() -> io::Result<PathBuf> {
    // Keep socket paths below sockaddr_un's limit even with a long TMPDIR.
    let directory = PathBuf::from(format!("/tmp/errspec-workers-v1-{}", unsafe {
        libc::geteuid()
    }));
    match fs::DirBuilder::new().mode(0o700).create(&directory) {
        Ok(()) => (),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
        Err(error) => return Err(error),
    }
    let metadata = fs::symlink_metadata(&directory)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(io::Error::other(
            "Worker registry must be a private directory owned by the current user",
        ));
    }
    directory.canonicalize()
}
fn owned_file(path: &Path) -> io::Result<fs::File> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return Err(io::Error::other(
            "Worker record is not a private regular file",
        ));
    }
    Ok(file)
}
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<T> {
    let mut bytes = vec![];
    owned_file(path)?.take(16385).read_to_end(&mut bytes)?;
    if bytes.len() > 16384 {
        return Err(io::Error::other("Worker record exceeds 16 KiB"));
    }
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}
fn write_json(path: &Path, data: &impl Serialize) -> io::Result<()> {
    let mut file = tempfile::Builder::new()
        .prefix("ownership-")
        .suffix(".tmp")
        .tempfile_in(path.parent().unwrap())?;
    serde_json::to_writer(&mut file, data)?;
    file.write_all(b"\n")?;
    file.as_file().sync_all()?;
    file.persist_noclobber(path).map_err(|error| error.error)?;
    Ok(())
}
fn key(record: &Record) -> &str {
    &record.token[..32]
}
fn valid_location(record: &Record, directory: &Path, path: &Path) -> bool {
    record.valid()
        && path == directory.join(format!("{}.json", key(record)))
        && record.socket == directory.join(format!("{}.sock", key(record)))
}
fn exchange(record: &Record) -> io::Result<Response> {
    let metadata = fs::symlink_metadata(&record.socket)?;
    if !metadata.file_type().is_socket() || metadata.uid() != unsafe { libc::geteuid() } {
        return Err(io::Error::other("Worker endpoint is not an owned socket"));
    }
    let start = Instant::now();
    let budget = Duration::from_millis(250);
    let mut stream = connect(&record.socket, start, budget)?;
    send(
        &mut stream,
        &serde_json::json!({"version":1,"runId":record.run_id,"token":record.token}),
        start,
        budget,
    )?;
    serde_json::from_slice(&receive(&mut stream, start, budget)?).map_err(io::Error::other)
}
fn ready(
    stream: &UnixStream,
    events: libc::c_short,
    start: Instant,
    budget: Duration,
) -> io::Result<()> {
    let remaining = budget.saturating_sub(start.elapsed());
    if remaining.is_zero() {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "Worker endpoint deadline elapsed",
        ));
    }
    let mut fd = libc::pollfd {
        fd: stream.as_raw_fd(),
        events,
        revents: 0,
    };
    match unsafe {
        libc::poll(
            &mut fd,
            1,
            (remaining.as_millis() + 1).min(i32::MAX as u128) as i32,
        )
    } {
        -1 => Err(io::Error::last_os_error()),
        0 => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "Worker endpoint deadline elapsed",
        )),
        _ => Ok(()),
    }
}
fn connect(path: &Path, start: Instant, budget: Duration) -> io::Result<UnixStream> {
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    address.sun_family = libc::AF_UNIX as _;
    let bytes = path.as_os_str().as_bytes();
    if bytes.len() >= address.sun_path.len() || bytes.contains(&0) {
        return Err(io::Error::other("Invalid worker socket path"));
    }
    for (target, source) in address.sun_path.iter_mut().zip(bytes) {
        *target = *source as libc::c_char;
    }
    #[cfg(target_os = "macos")]
    {
        address.sun_len = std::mem::size_of_val(&address) as _;
    }
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    stream.set_nonblocking(true)?;
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe {
        libc::connect(
            fd,
            (&address as *const libc::sockaddr_un).cast(),
            std::mem::size_of_val(&address) as libc::socklen_t,
        )
    } < 0
    {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EINPROGRESS) {
            return Err(error);
        }
        ready(&stream, libc::POLLOUT, start, budget)?;
        let mut status: libc::c_int = 0;
        let mut length = std::mem::size_of_val(&status) as libc::socklen_t;
        if unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_ERROR,
                (&mut status as *mut libc::c_int).cast(),
                &mut length,
            )
        } < 0
        {
            return Err(io::Error::last_os_error());
        }
        if status != 0 {
            return Err(io::Error::from_raw_os_error(status));
        }
    }
    Ok(stream)
}
fn send(
    stream: &mut UnixStream,
    data: &impl Serialize,
    start: Instant,
    budget: Duration,
) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(data)?;
    bytes.push(b'\n');
    if bytes.len() > 4096 {
        return Err(io::Error::other("Worker message exceeds 4 KiB"));
    }
    let mut offset = 0;
    while offset < bytes.len() {
        ready(stream, libc::POLLOUT, start, budget)?;
        match stream.write(&bytes[offset..]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "Worker endpoint closed",
                ));
            }
            Ok(count) => offset += count,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => (),
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
fn receive(stream: &mut UnixStream, start: Instant, budget: Duration) -> io::Result<Vec<u8>> {
    let mut bytes = vec![];
    let mut buffer = [0u8; 1024];
    loop {
        ready(stream, libc::POLLIN, start, budget)?;
        match stream.read(&mut buffer) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Worker endpoint closed",
                ));
            }
            Ok(count) => {
                bytes.extend_from_slice(&buffer[..count]);
                if bytes.len() > 4096 {
                    return Err(io::Error::other("Worker message exceeds 4 KiB"));
                }
                if bytes.contains(&b'\n') {
                    return Ok(bytes);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => (),
            Err(error) => return Err(error),
        }
    }
}
pub(crate) fn recover(signal: &AtomicI32) -> io::Result<Info> {
    let directory = directory()?;
    let mut info = Info::default();
    let mut count = 0;
    let start = Instant::now();
    for entry in fs::read_dir(&directory)? {
        if signal.load(Ordering::SeqCst) != 0 {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Worker recovery interrupted",
            ));
        }
        if start.elapsed() > Duration::from_secs(3) {
            info.warnings
                .push("Worker recovery reached its 3-second deadline".into());
            break;
        }
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !name
            .strip_suffix(".json")
            .is_some_and(|stem| stem.len() == 32 && stem.bytes().all(|c| c.is_ascii_hexdigit()))
        {
            continue;
        }
        count += 1;
        if count > 256 {
            info.warnings
                .push("Worker recovery stopped after 256 records".into());
            break;
        }
        let result = (|| {
            let record: Record = read_json(&path)?;
            if !valid_location(&record, &directory, &path) {
                return Err(io::Error::other("Invalid worker identity or endpoint"));
            }
            Ok(record.judge(&exchange(&record)?))
        })();
        match result {
            Ok(Decision::Active) => info.active += 1,
            Ok(Decision::Recovered) => info.recovered += 1,
            Ok(Decision::Unverified) => {
                info.unverified += 1;
                info.warnings.push(format!(
                    "Worker identity could not be verified: {name}; no PID was signalled"
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => (), // Concurrent owner cleanup.
            Err(error) => {
                info.unverified += 1;
                info.warnings.push(format!(
                    "Worker recovery unavailable for {name}: {error}; no PID was signalled"
                ));
            }
        }
    }
    Ok(info)
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Configuration {
    engine_fingerprint: String,
    endpoint: bool,
    node: PathBuf,
    script: PathBuf,
    record: PathBuf,
    identity: Record,
}
pub(crate) struct Launch {
    pub configuration: PathBuf,
    record: PathBuf,
    socket: PathBuf,
}
impl Launch {
    pub(crate) fn new(node: &Path, script: &Path, run_id: &str) -> io::Result<Self> {
        let directory = directory()?;
        let mut random = [0u8; 32];
        fs::File::open("/dev/urandom")?.read_exact(&mut random)?;
        let token: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        let record = directory.join(format!("{}.json", &token[..32]));
        let socket = directory.join(format!("{}.sock", &token[..32]));
        let configuration = script
            .parent()
            .unwrap()
            .join(format!("supervisor-{}.json", &token[..16]));
        write_json(
            &configuration,
            &Configuration {
                engine_fingerprint: errspec::mutation::cache::ENGINE_FINGERPRINT.into(),
                endpoint: std::env::var_os("ERRSPEC_WORKER_ENDPOINT").as_deref()
                    != Some(std::ffi::OsStr::new("0")),
                node: node.into(),
                script: script.into(),
                record: record.clone(),
                identity: Record {
                    version: 1,
                    run_id: run_id.into(),
                    token,
                    owner: std::process::id(),
                    supervisor: 0,
                    socket: socket.clone(),
                },
            },
        )?;
        Ok(Self {
            configuration,
            record,
            socket,
        })
    }
    pub(crate) fn cleanup(&self) {
        for path in [&self.record, &self.socket, &self.configuration] {
            let _ = fs::remove_file(path);
        }
    }
}
impl Drop for Launch {
    fn drop(&mut self) {
        self.cleanup();
    }
}

fn own_group() -> bool {
    unsafe { libc::getpgrp() as u32 == std::process::id() }
}
fn orphaned(owner: u32) -> bool {
    unsafe { libc::getppid() as u32 != owner }
}
fn stop(configuration: &Configuration) -> ! {
    // The caller is the live group leader. Its own group cannot have been
    // replaced by an unrelated process after a stored PID was recycled.
    let _ = fs::remove_file(&configuration.record);
    let _ = fs::remove_file(&configuration.identity.socket);
    if orphaned(configuration.identity.owner) {
        cleanup_temporary(configuration);
    }
    if own_group() {
        unsafe {
            libc::kill(0, libc::SIGKILL);
        }
    }
    std::process::exit(2)
}
fn cleanup_temporary(configuration: &Configuration) {
    let Some(directory) = configuration.script.parent() else {
        return;
    };
    let Some(name) = directory.file_name().and_then(|s| s.to_str()) else {
        return;
    };
    let Ok(metadata) = fs::symlink_metadata(directory) else {
        return;
    };
    if !name.starts_with("errspec-mutation-")
        || name.replace('-', "") != configuration.identity.run_id
        || !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o077 != 0
    {
        return;
    }
    for name in [
        "driver.mjs".to_owned(),
        "setup.mjs".into(),
        "weak-runtime.mjs".into(),
        "child-process.mjs".into(),
        "state.json".into(),
        format!("supervisor-{}.json", &configuration.identity.token[..16]),
    ] {
        let _ = fs::remove_file(directory.join(name));
    }
    // Leave unexpected project-created files alone, rather than recursively
    // deleting a directory after an unclean owner exit.
    if fs::remove_dir(directory).is_ok()
        && let Some(parent) = directory.parent()
        && parent
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("errspec-mutation-"))
        && let Ok(metadata) = fs::symlink_metadata(parent)
        && metadata.is_dir()
        && !metadata.file_type().is_symlink()
        && metadata.uid() == unsafe { libc::geteuid() }
        && metadata.mode() & 0o077 == 0
    {
        let _ = fs::remove_dir(parent);
    }
}
fn respond(mut stream: UnixStream, configuration: &Configuration) -> io::Result<bool> {
    stream.set_nonblocking(true)?;
    let start = Instant::now();
    let budget = Duration::from_millis(100);
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Request {
        version: u8,
        run_id: String,
        token: String,
    }
    let request: Request = serde_json::from_slice(&receive(&mut stream, start, budget)?)?;
    let identity = &configuration.identity;
    if request.version != 1 || request.run_id != identity.run_id || request.token != identity.token
    {
        return Ok(false);
    }
    let group_owned = own_group();
    let orphaned = orphaned(identity.owner);
    let terminating = group_owned && orphaned;
    send(
        &mut stream,
        &Response {
            version: 1,
            run_id: identity.run_id.clone(),
            owner: identity.owner,
            supervisor: identity.supervisor,
            group_owned,
            orphaned,
            terminating,
        },
        start,
        budget,
    )?;
    Ok(terminating)
}
fn supervise(path: &Path) -> io::Result<()> {
    let mut configuration: Configuration = read_json(path)?;
    if configuration.engine_fingerprint != errspec::mutation::cache::ENGINE_FINGERPRINT {
        println!(
            "{}",
            serde_json::json!({"version":1,"runId":configuration.identity.run_id,"requestId":1,"type":"error","code":"EC_MUTATION_INPUT","message":"Native mutation engine changed before worker startup"})
        );
        return Err(io::Error::other("Native engine fingerprint changed"));
    }
    if configuration.identity.owner != unsafe { libc::getppid() } as u32 || !own_group() {
        return Err(io::Error::other(
            "Supervisor must be started by its recorded owner as a new group leader",
        ));
    }
    configuration.identity.supervisor = std::process::id();
    let directory = directory()?;
    if !valid_location(&configuration.identity, &directory, &configuration.record) {
        return Err(io::Error::other("Invalid supervisor configuration"));
    }
    let listener = if !configuration.endpoint {
        None
    } else {
        match UnixListener::bind(&configuration.identity.socket) {
            Ok(listener) => {
                listener.set_nonblocking(true)?;
                write_json(&configuration.record, &configuration.identity)?;
                Some(listener)
            }
            Err(error)
                if error.kind() == io::ErrorKind::PermissionDenied
                    || matches!(
                        error.raw_os_error(),
                        Some(libc::EAFNOSUPPORT | libc::EOPNOTSUPP | libc::ENOSYS)
                    ) =>
            {
                None
            }
            Err(error) => return Err(error),
        }
    };
    println!(
        "{}",
        serde_json::json!({"version":1,"runId":configuration.identity.run_id,"requestId":1,"type":"ownership","supervisor":std::process::id(),"mode":if listener.is_some(){"endpoint"}else{"parent_monitor"}})
    );
    let mut child = match Command::new(&configuration.node)
        .arg(&configuration.script)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            let _ = fs::remove_file(&configuration.record);
            let _ = fs::remove_file(&configuration.identity.socket);
            // The native owner is waiting for the first open response.
            println!(
                "{}",
                serde_json::json!({"version":1,"runId":configuration.identity.run_id,"requestId":1,"type":"error","code":"EC_MUTATION_CONFIG","message":format!("Cannot start Node: {error}")})
            );
            return Err(error);
        }
    };
    let mut parent_check = Instant::now();
    loop {
        if let Some(listener) = &listener {
            match listener.accept() {
                Ok((stream, _)) => {
                    if respond(stream, &configuration).unwrap_or(false) {
                        stop(&configuration);
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => (),
                Err(error) => {
                    eprintln!("Mutation supervisor endpoint: {error}");
                    stop(&configuration);
                }
            }
        }
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
            eprintln!("Mutation supervisor wait: {}", io::Error::last_os_error());
            stop(&configuration);
        }
        if unsafe { info.si_pid() } != 0 {
            let _ = child.wait();
            stop(&configuration);
        }
        if parent_check.elapsed() >= Duration::from_secs(1) {
            if orphaned(configuration.identity.owner) {
                stop(&configuration);
            }
            parent_check = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
pub(crate) fn entry(path: &Path) -> ExitCode {
    match supervise(path) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Mutation supervisor: {error}");
            ExitCode::from(2)
        }
    }
}
