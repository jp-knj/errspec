//! Acquire cache inputs and persist results. Reuse judgments live in mutation::cache.
use errspec::mutation::{cache, options::Options};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicI32, Ordering},
};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Facts {
    version: u8,
    inputs: Vec<(PathBuf, String)>,
    runner_package: PathBuf,
    runtime: Runtime,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Runtime {
    node: String,
    platform: String,
    arch: String,
    executable: PathBuf,
    cwd: PathBuf,
    exec_argv: Vec<String>,
    configuration_hash: Option<String>,
    env_hash: String,
}
impl Facts {
    pub(crate) fn merge(&mut self, additional: Self) -> Result<(), Error> {
        if self.version != additional.version
            || self.runner_package != additional.runner_package
            || self.runtime != additional.runtime
        {
            return Err(Error::Unavailable(
                "Parallel workers resolved different runtime inputs".into(),
            ));
        }
        if self
            .inputs
            .iter()
            .map(|(path, _)| path)
            .collect::<BTreeSet<_>>()
            .len()
            != self.inputs.len()
            || additional
                .inputs
                .iter()
                .map(|(path, _)| path)
                .collect::<BTreeSet<_>>()
                .len()
                != additional.inputs.len()
        {
            return Err(Error::Unavailable("Duplicate worker input facts".into()));
        }
        let mut inputs: BTreeMap<_, _> = self.inputs.iter().cloned().collect();
        for (path, hash) in additional.inputs {
            if inputs.get(&path).is_some_and(|old| old != &hash) {
                return Err(Error::Changed(format!(
                    "Workers observed different input contents: {}",
                    path.display()
                )));
            }
            inputs.insert(path, hash);
        }
        self.inputs = inputs.into_iter().collect();
        Ok(())
    }
}
pub(crate) struct Stamp {
    pub hash: String,
    pub baseline_paths: BTreeSet<PathBuf>,
}
pub(crate) struct Store {
    pub directory: PathBuf,
    pub data: cache::File,
    pub stamp: Stamp,
}
#[derive(Debug)]
pub(crate) enum Error {
    Unavailable(String),
    Changed(String),
}
impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Unavailable(error.to_string())
    }
}
impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Unavailable(error.to_string())
    }
}

fn part(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}
fn digest(path: &Path) -> Result<String, Error> {
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(Error::Unavailable(format!(
            "Not a regular cache input: {}",
            path.display()
        )));
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
struct Inputs<'a> {
    hash: Sha256,
    files: BTreeMap<PathBuf, String>,
    packages: BTreeSet<PathBuf>,
    cache_directory: &'a Path,
    temp: &'a Path,
    signal: &'a AtomicI32,
}
impl Inputs<'_> {
    fn interrupted(&self) -> Result<(), Error> {
        if self.signal.load(Ordering::SeqCst) != 0 {
            return Err(Error::Changed("Cache input acquisition interrupted".into()));
        }
        Ok(())
    }
    fn tree(
        &mut self,
        root: &Path,
        path: &Path,
        visited: &mut BTreeSet<PathBuf>,
    ) -> Result<(), Error> {
        self.interrupted()?;
        if path.starts_with(self.cache_directory) || path.starts_with(self.temp) {
            return Ok(());
        }
        let info = fs::symlink_metadata(path)?;
        let relative = path.strip_prefix(root).unwrap();
        if info.file_type().is_symlink() {
            let target = fs::canonicalize(path)?;
            if !target.starts_with(root) {
                return Err(Error::Unavailable(format!(
                    "Cache input symlink leaves its package: {}",
                    path.display()
                )));
            }
            part(&mut self.hash, b"symlink");
            part(&mut self.hash, relative.as_os_str().as_encoded_bytes());
            part(
                &mut self.hash,
                fs::read_link(path)?.as_os_str().as_encoded_bytes(),
            );
            return self.tree(root, &target, visited);
        }
        if info.is_dir() {
            if !visited.insert(path.to_owned()) {
                return Ok(());
            }
            part(&mut self.hash, b"directory");
            part(&mut self.hash, relative.as_os_str().as_encoded_bytes());
            let mut entries = fs::read_dir(path)?
                .map(|entry| entry.map(|e| e.path()))
                .collect::<Result<Vec<_>, _>>()?;
            entries.sort();
            for entry in entries {
                if entry
                    .file_name()
                    .is_some_and(|name| name == "node_modules" || name == ".git")
                {
                    continue;
                }
                self.tree(root, &entry, visited)?;
            }
        } else if info.is_file() {
            let value = digest(path)?;
            part(&mut self.hash, b"file");
            part(&mut self.hash, relative.as_os_str().as_encoded_bytes());
            part(&mut self.hash, value.as_bytes());
            self.files.insert(path.to_owned(), value);
        } else {
            return Err(Error::Unavailable(format!(
                "Unsupported cache input: {}",
                path.display()
            )));
        }
        Ok(())
    }
    fn package(&mut self, path: &Path) -> Result<(), Error> {
        self.interrupted()?;
        let path = path.canonicalize()?;
        // Bind every dependency edge, including references to a package already
        // visited through another parent or a cycle.
        part(&mut self.hash, b"package-reference");
        part(&mut self.hash, path.as_os_str().as_encoded_bytes());
        if !self.packages.insert(path.clone()) {
            return Ok(());
        }
        part(&mut self.hash, b"package");
        part(&mut self.hash, path.as_os_str().as_encoded_bytes());
        self.tree(&path, &path, &mut BTreeSet::new())?;
        let manifest_path = path.join("package.json");
        if fs::metadata(&manifest_path)?.len() > 1024 * 1024 {
            return Err(Error::Unavailable("Package manifest exceeds 1 MiB".into()));
        }
        let manifest: serde_json::Value = serde_json::from_slice(&fs::read(manifest_path)?)?;
        let mut names = BTreeSet::new();
        for field in [
            "dependencies",
            "devDependencies",
            "optionalDependencies",
            "peerDependencies",
        ] {
            if let Some(value) = manifest.get(field) {
                let object = value.as_object().ok_or_else(|| {
                    Error::Unavailable(format!("Invalid package dependencies: {}", path.display()))
                })?;
                names.extend(object.keys().cloned());
            }
        }
        for name in names {
            if !package_name(&name) {
                return Err(Error::Unavailable(format!(
                    "Unsupported dependency name: {name}"
                )));
            }
            part(&mut self.hash, b"dependency");
            part(&mut self.hash, name.as_bytes());
            let mut directory = Some(path.as_path());
            let mut found = None;
            while let Some(parent) = directory {
                let candidate = parent.join("node_modules").join(&name);
                if candidate.join("package.json").is_file() {
                    found = Some(candidate);
                    break;
                }
                directory = parent.parent();
            }
            if let Some(dependency) = found {
                self.package(&dependency)?;
            } else {
                part(&mut self.hash, b"missing");
            }
        }
        Ok(())
    }
}
fn package_name(name: &str) -> bool {
    let parts: Vec<_> = name.split('/').collect();
    let segment = |s: &str| {
        !s.is_empty()
            && s != "."
            && s != ".."
            && s.bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    };
    match parts.as_slice() {
        [name] => segment(name),
        [scope, name] => scope.strip_prefix('@').is_some_and(segment) && segment(name),
        _ => false,
    }
}
fn package_for(path: &Path) -> Option<PathBuf> {
    let mut directory = path.parent();
    while let Some(parent) = directory {
        if parent.join("package.json").is_file() {
            return Some(parent.to_owned());
        }
        directory = parent.parent();
    }
    None
}

pub(crate) fn directory(options: &Options) -> PathBuf {
    options.root.join(
        options
            .cache_dir
            .as_deref()
            .unwrap_or_else(|| Path::new(".errspec/mutation-cache")),
    )
}
pub(crate) fn acquire(
    options: &Options,
    facts: &Facts,
    temp: &Path,
    baseline_paths: Option<&BTreeSet<PathBuf>>,
    signal: &AtomicI32,
) -> Result<Stamp, Error> {
    // Node records real paths. macOS TMPDIR commonly goes through /var while
    // its canonical path starts with /private/var; compare in one namespace.
    let temp = temp.canonicalize()?;
    if facts.version != 1
        || !facts.runner_package.is_absolute()
        || !facts.runtime.executable.is_absolute()
        || !facts.runtime.cwd.is_absolute()
        || facts.runtime.node.is_empty()
        || facts.runtime.platform.is_empty()
        || facts.runtime.arch.is_empty()
        || facts.runtime.env_hash.len() != 64
        || !facts
            .runtime
            .env_hash
            .bytes()
            .all(|c| c.is_ascii_hexdigit())
    {
        return Err(Error::Unavailable("Invalid cache input facts".into()));
    }
    if !facts.runtime.exec_argv.is_empty() {
        return Err(Error::Unavailable(
            "Custom Node execution flags prevent complete input acquisition".into(),
        ));
    }
    if facts
        .runtime
        .configuration_hash
        .as_ref()
        .is_none_or(|value| value.len() != 64 || !value.bytes().all(|c| c.is_ascii_hexdigit()))
    {
        return Err(Error::Unavailable(
            "Resolved runner configuration cannot be fingerprinted".into(),
        ));
    }
    let cache_directory = directory(options);
    let mut input = Inputs {
        hash: Sha256::new(),
        files: BTreeMap::new(),
        packages: BTreeSet::new(),
        cache_directory: &cache_directory,
        temp: &temp,
        signal,
    };
    part(&mut input.hash, cache::ENGINE_FINGERPRINT.as_bytes());
    let mut settings = serde_json::to_value(options).unwrap();
    for field in [
        "policy",
        "since",
        "diffUnit",
        "cache",
        "cacheDir",
        "failOnUndetected",
        "fullDryRun",
        "shard",
        "concurrency",
    ] {
        settings.as_object_mut().unwrap().remove(field);
    }
    part(&mut input.hash, &serde_json::to_vec(&settings).unwrap());
    part(
        &mut input.hash,
        std::env::current_dir()?.as_os_str().as_encoded_bytes(),
    );
    let mut environment: Vec<_> = std::env::vars_os().collect();
    environment.sort();
    for (name, value) in environment {
        part(&mut input.hash, name.as_encoded_bytes());
        part(&mut input.hash, value.as_encoded_bytes());
    }
    // NODE_PATH and preload hooks can acquire undeclared dependencies outside the
    // package graph. Keep execution fresh rather than guessing that graph.
    if std::env::var_os("NODE_PATH").is_some_and(|value| !value.is_empty()) {
        return Err(Error::Unavailable(
            "NODE_PATH prevents complete dependency acquisition".into(),
        ));
    }
    if std::env::var("NODE_OPTIONS").is_ok_and(|value| {
        ["--require", "--import", "--loader", "-r "]
            .iter()
            .any(|flag| value.contains(flag))
    }) {
        return Err(Error::Unavailable(
            "Node preload hooks prevent complete dependency acquisition".into(),
        ));
    }
    part(
        &mut input.hash,
        &serde_json::to_vec(&facts.runtime).unwrap(),
    );
    if !options.root.join("package.json").is_file() {
        return Err(Error::Unavailable(
            "A project package.json is required for dependency acquisition".into(),
        ));
    }
    input.package(&options.root)?;
    input.package(&facts.runner_package)?;
    if let Some(package) = &options.typecheck_package {
        input.package(package)?;
    }
    let mut known = BTreeMap::new();
    for (path, value) in &facts.inputs {
        if path.starts_with(&temp) {
            continue;
        }
        if path.starts_with(&cache_directory) {
            return Err(Error::Unavailable(
                "Cache directory overlaps runtime inputs".into(),
            ));
        }
        if !path.is_absolute()
            || value.len() != 64
            || !value.bytes().all(|c| c.is_ascii_hexdigit())
            || known.insert(path.clone(), value.clone()).is_some()
        {
            return Err(Error::Unavailable("Malformed cache input identity".into()));
        }
    }
    let baseline_paths = baseline_paths
        .cloned()
        .unwrap_or_else(|| known.keys().cloned().collect());
    let mut additional_packages = BTreeSet::new();
    for path in known
        .keys()
        .filter(|path| path.file_name().is_none_or(|name| name != "package.json"))
    {
        if let Some(package) = package_for(path) {
            // Ancestor manifests are hashed individually. Scanning an entire
            // monorepo because a config file lives at its root is not bounded.
            if !options.root.starts_with(&package) {
                additional_packages.insert(package);
            }
        }
    }
    for package in additional_packages {
        if !input.packages.contains(&package.canonicalize()?) {
            input.package(&package)?;
        }
    }
    for (path, expected) in known {
        input.interrupted()?;
        let value = digest(&path)?;
        if value != expected {
            return Err(Error::Changed(format!(
                "Input changed during cache acquisition: {}",
                path.display()
            )));
        }
        if baseline_paths.contains(&path) {
            part(&mut input.hash, b"resolved");
            part(&mut input.hash, path.as_os_str().as_encoded_bytes());
            part(&mut input.hash, value.as_bytes());
        } else if input.files.get(&path) != Some(&value) {
            return Err(Error::Unavailable(format!(
                "New runtime input was not acquired before execution: {}",
                path.display()
            )));
        }
    }
    Ok(Stamp {
        hash: format!("{:x}", input.hash.finalize()),
        baseline_paths,
    })
}

pub(crate) fn load(directory: PathBuf, stamp: Stamp, info: &mut cache::Info) -> Store {
    let path = directory.join("results-v1.json");
    let read = || -> Result<cache::File, String> {
        let mut file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&path)
            .map_err(|e| e.to_string())?;
        if !file.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("Cache is not a regular file".into());
        }
        let mut bytes = vec![];
        Read::by_ref(&mut file)
            .take(64 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 64 * 1024 * 1024 {
            return Err("Result cache exceeds 64 MiB".into());
        }
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())
    };
    let data = if !path.exists() {
        cache::File::new(stamp.hash.clone())
    } else {
        match read() {
            Ok(data) if data.compatible(&stamp.hash) => data,
            Ok(_) => {
                info.invalidated = true;
                cache::File::new(stamp.hash.clone())
            }
            Err(error) => {
                info.invalidated = true;
                info.warnings.push(format!(
                    "Result cache was not reusable: {error}; revalidating"
                ));
                cache::File::new(stamp.hash.clone())
            }
        }
    };
    info.fingerprint = Some(stamp.hash.clone());
    Store {
        directory,
        data,
        stamp,
    }
}
pub(crate) fn save(store: &Store) -> Result<(), std::io::Error> {
    save_document(
        &store.directory,
        "results-v1.json",
        "errspec-result-",
        &store.data,
    )
}

pub(crate) fn load_coverage(
    directory: &Path,
    info: &mut cache::Info,
) -> Option<errspec::mutation::coverage_cache::File> {
    let result = (|| -> Result<_, String> {
        let path = directory.join("coverage-v1.json");
        if !path.exists() {
            return Ok(None);
        }
        let file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .map_err(|e| e.to_string())?;
        if !file.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("Coverage cache is not a regular file".into());
        }
        let mut bytes = vec![];
        file.take(64 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 64 * 1024 * 1024 {
            return Err("Coverage cache exceeds 64 MiB".into());
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| e.to_string())
    })();
    match result {
        Ok(value) => value,
        Err(reason) => {
            info.warnings.push(format!(
                "Coverage cache was not reusable: {reason}; refreshing observations"
            ));
            None
        }
    }
}
pub(crate) fn save_coverage(
    store: &Store,
    execution: &errspec::mutation::Execution,
) -> Result<(), std::io::Error> {
    let data =
        errspec::mutation::coverage_cache::File::new(store.stamp.hash.clone(), execution.clone());
    save_document(
        &store.directory,
        "coverage-v1.json",
        "errspec-coverage-",
        &data,
    )
}
fn save_document(
    directory: &Path,
    name: &str,
    prefix: &str,
    data: &impl Serialize,
) -> Result<(), std::io::Error> {
    fs::create_dir_all(directory)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(prefix)
        .suffix(".tmp")
        .tempfile_in(directory)?;
    serde_json::to_writer(&mut temporary, data)?;
    temporary.write_all(b"\n")?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(directory.join(name))
        .map_err(|error| error.error)?;
    Ok(())
}

pub(crate) fn managed_file(name: &std::ffi::OsStr) -> bool {
    name.to_str().is_some_and(|name| {
        matches!(name, "results-v1.json" | "coverage-v1.json")
            || (name.starts_with("errspec-result-") || name.starts_with("errspec-coverage-"))
                && name.ends_with(".tmp")
    })
}
pub(crate) fn check_directory(path: &Path) -> Result<(), String> {
    for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if !managed_file(&entry.file_name()) {
            return Err("cacheDir must be dedicated to errspec mutation results".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn worker_facts_require_matching_runtime_and_cannot_hide_conflicting_contents() {
        let facts = Facts {
            version: 1,
            inputs: vec![(PathBuf::from("/project/a.ts"), "a".repeat(64))],
            runner_package: "/runner".into(),
            runtime: Runtime {
                node: "24".into(),
                platform: "test".into(),
                arch: "test".into(),
                executable: "/node".into(),
                cwd: "/project".into(),
                exec_argv: vec![],
                configuration_hash: Some("f".repeat(64)),
                env_hash: "e".repeat(64),
            },
        };
        let mut merged = facts.clone();
        let mut additional = facts.clone();
        additional
            .inputs
            .push(("/project/b.ts".into(), "b".repeat(64)));
        merged.merge(additional).unwrap();
        assert_eq!(merged.inputs.len(), 2);
        let mut different = facts.clone();
        different.runtime.env_hash = "d".repeat(64);
        assert!(matches!(
            merged.merge(different),
            Err(Error::Unavailable(_))
        ));
        let before = merged.inputs.clone();
        let mut conflict = facts.clone();
        conflict.inputs[0].1 = "c".repeat(64);
        assert!(matches!(merged.merge(conflict), Err(Error::Changed(_))));
        assert_eq!(merged.inputs, before);
        let mut duplicate = facts.clone();
        duplicate.inputs.push(duplicate.inputs[0].clone());
        assert!(matches!(
            merged.merge(duplicate),
            Err(Error::Unavailable(_))
        ));
    }
    use std::os::unix::fs::symlink;
    fn package(path: &Path, dependencies: serde_json::Value) {
        fs::create_dir_all(path).unwrap();
        fs::write(path.join("package.json"),serde_json::to_vec(&serde_json::json!({"name":"fixture","version":"1.0.0","dependencies":dependencies})).unwrap()).unwrap();
        fs::write(path.join("index.js"), "export const value=true;").unwrap();
    }
    #[test]
    fn dependency_edges_remain_bound_when_both_targets_were_already_visited() {
        let base = tempfile::tempdir().unwrap();
        let base = base.path().canonicalize().unwrap();
        let root = base.join("project");
        let temp = base.join("tool");
        fs::create_dir(&temp).unwrap();
        package(
            &root,
            serde_json::json!({"00-global":"1","01-local":"1","a":"1","b":"1"}),
        );
        let modules = root.join("node_modules");
        package(&modules.join("shared"), serde_json::json!({}));
        package(
            &modules.join("b/node_modules/shared"),
            serde_json::json!({}),
        );
        package(&modules.join("a"), serde_json::json!({"shared":"1"}));
        package(&modules.join("b"), serde_json::json!({"shared":"1"}));
        package(&modules.join("runner"), serde_json::json!({}));
        symlink(modules.join("shared"), modules.join("00-global")).unwrap();
        symlink(
            modules.join("b/node_modules/shared"),
            modules.join("01-local"),
        )
        .unwrap();
        let options = Options {
            root: root.clone(),
            cache: true,
            ..Default::default()
        };
        let facts = Facts {
            version: 1,
            inputs: vec![],
            runner_package: modules.join("runner"),
            runtime: Runtime {
                node: "24".into(),
                platform: "test".into(),
                arch: "test".into(),
                executable: std::env::current_exe().unwrap(),
                cwd: root.clone(),
                exec_argv: vec![],
                configuration_hash: Some("f".repeat(64)),
                env_hash: "f".repeat(64),
            },
        };
        let before = acquire(&options, &facts, &temp, None, &AtomicI32::new(0)).unwrap();
        fs::create_dir_all(modules.join("a/node_modules")).unwrap();
        symlink(
            modules.join("b/node_modules/shared"),
            modules.join("a/node_modules/shared"),
        )
        .unwrap();
        let after = acquire(&options, &facts, &temp, None, &AtomicI32::new(0)).unwrap();
        assert_ne!(before.hash, after.hash);
    }
}
