use sha2::{Digest, Sha256};
use std::{env, fs, path::Path, process::Command};

fn sources(path: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(path).expect("read native sources") {
        let path = entry.expect("native source entry").path();
        if path.is_dir() {
            sources(&path, out)
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}
fn main() {
    // Bind cached evidence to the code actually compiled into this binary. A
    // runtime hash of current_exe's pathname could observe a concurrent rebuild.
    let root = std::path::PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let mut paths = vec![];
    sources(&root.join("src"), &mut paths);
    paths.extend(
        [
            "build.rs",
            "Cargo.toml",
            "Cargo.lock",
            "packages/mutation-driver/dist/driver.mjs",
            "packages/mutation-driver/setup.mjs",
            "packages/mutation-driver/weak-runtime.mjs",
            "packages/mutation-driver/child-process.mjs",
        ]
        .map(|name| root.join(name)),
    );
    paths.sort();
    let mut hash = Sha256::new();
    for path in paths {
        let name = path.strip_prefix(&root).unwrap().to_string_lossy();
        println!("cargo:rerun-if-changed={name}");
        let data = fs::read(&path).expect("read mutation engine input");
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update((data.len() as u64).to_le_bytes());
        hash.update(data);
    }
    let mut configuration: Vec<_> = env::vars()
        .filter(|(name, _)| {
            matches!(
                name.as_str(),
                "TARGET" | "PROFILE" | "CARGO_ENCODED_RUSTFLAGS"
            ) || name.starts_with("CARGO_CFG_")
                || name.starts_with("CARGO_FEATURE_")
        })
        .collect();
    configuration.sort();
    for (name, value) in configuration {
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name);
        hash.update((value.len() as u64).to_le_bytes());
        hash.update(value);
    }
    let compiler = Command::new(env::var_os("RUSTC").unwrap())
        .arg("--version")
        .output()
        .expect("compiler identity");
    assert!(compiler.status.success(), "compiler identity failed");
    hash.update(compiler.stdout);
    println!(
        "cargo:rustc-env=ERRSPEC_ENGINE_FINGERPRINT={:x}",
        hash.finalize()
    );
}
