//! Acquire Git facts. Selection judgments live in the shared mutation model.
use errspec::mutation::selection::{Changes, Lines, hunks};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    os::unix::fs::OpenOptionsExt,
    path::Path,
    process::Command,
    sync::atomic::{AtomicI32, Ordering},
    time::Duration,
};

fn git(root: &Path, args: &[&str], signal: &AtomicI32) -> Result<Vec<u8>, String> {
    let result = crate::owned_process::capture(
        Command::new("git")
            .current_dir(root)
            .args(["-c", "core.fsmonitor=false", "--literal-pathspecs"])
            .args(args),
        Duration::from_secs(30),
        16 * 1024 * 1024,
        || signal.load(Ordering::SeqCst) != 0,
    )
    .map_err(|e| format!("Cannot acquire Git changes: {e}"))?;
    if !result.status.success() {
        return Err(format!(
            "Git changes unavailable: {}",
            String::from_utf8_lossy(&result.stderr).trim()
        ));
    }
    Ok(result.stdout)
}
fn paths(bytes: &[u8]) -> Result<Vec<&str>, String> {
    bytes
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| std::str::from_utf8(p).map_err(|_| "Git paths must be UTF-8".into()))
        .collect()
}
fn fingerprint(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}

pub(crate) fn acquire(
    root: &Path,
    since: &str,
    signal: &AtomicI32,
    cache_directory: Option<&Path>,
) -> Result<Changes, String> {
    let revision = format!("{since}^{{commit}}");
    let base = git(
        root,
        &["rev-parse", "--verify", "--end-of-options", &revision],
        signal,
    )?;
    let base = std::str::from_utf8(&base)
        .map_err(|e| e.to_string())?
        .trim()
        .to_owned();
    let mut hash = Sha256::new();
    fingerprint(&mut hash, base.as_bytes());
    let names = git(
        root,
        &[
            "diff",
            "--no-color",
            "--ignore-submodules=none",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            "--name-only",
            "-z",
            "--relative",
            &base,
            "--",
            ".",
        ],
        signal,
    )?;
    let mut files = BTreeMap::new();
    for name in paths(&names)? {
        if managed_cache_file(root, name, cache_directory) {
            continue;
        }
        let patch = git(
            root,
            &[
                "diff",
                "--no-color",
                "--ignore-submodules=none",
                "--no-ext-diff",
                "--no-textconv",
                "--no-renames",
                "--unified=0",
                "--relative",
                &base,
                "--",
                name,
            ],
            signal,
        )?;
        fingerprint(&mut hash, name.as_bytes());
        fingerprint(&mut hash, &patch);
        let ranges = hunks(std::str::from_utf8(&patch).map_err(|_| "Git patch must be UTF-8")?)?;
        files.insert(name.to_owned(), ranges);
    }
    let untracked = git(
        root,
        &[
            "ls-files",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            ".",
        ],
        signal,
    )?;
    for name in paths(&untracked)? {
        if managed_cache_file(root, name, cache_directory) {
            continue;
        }
        fingerprint(&mut hash, name.as_bytes());
        let path = root.join(name);
        let info = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if info.file_type().is_symlink() {
            fingerprint(
                &mut hash,
                fs::read_link(&path)
                    .map_err(|e| e.to_string())?
                    .as_os_str()
                    .as_encoded_bytes(),
            );
            files.insert(
                name.to_owned(),
                vec![Lines {
                    start: 1,
                    end: usize::MAX,
                }],
            );
            continue;
        }
        if !info.is_file() {
            return Err(format!("Unsupported untracked file kind: {name}"));
        }
        let mut input = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .map_err(|e| e.to_string())?;
        if !input.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err(format!(
                "Untracked input is no longer a regular file: {name}"
            ));
        }
        let mut buffer = [0u8; 65536];
        let mut contents = Sha256::new();
        loop {
            if signal.load(Ordering::SeqCst) != 0 {
                return Err("Git acquisition interrupted".into());
            }
            let count = input.read(&mut buffer).map_err(|e| e.to_string())?;
            if count == 0 {
                break;
            }
            contents.update(&buffer[..count]);
        }
        fingerprint(&mut hash, &contents.finalize());
        files.insert(
            name.to_owned(),
            vec![Lines {
                start: 1,
                end: usize::MAX,
            }],
        );
    }
    Ok(Changes {
        base,
        fingerprint: format!("{:x}", hash.finalize()),
        files,
    })
}

fn managed_cache_file(root: &Path, name: &str, directory: Option<&Path>) -> bool {
    let path = root.join(name);
    directory.is_some_and(|directory| {
        path.parent() == Some(directory)
            && path
                .file_name()
                .is_some_and(crate::mutation_cache::managed_file)
    })
}
