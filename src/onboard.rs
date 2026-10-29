use crate::{request::InitRequest, scaffold, tsrs_adapter::TsrsAdapter};
use errspec::model::Source;
use serde::Serialize;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InitReport {
    kind: &'static str,
    schema_version: u32,
    pub files: [PathBuf; 2],
    tags: Vec<String>,
    tests_executed: bool,
    ready: bool,
    next_steps: [&'static str; 3],
}

fn relative(base: &Path, path: &Path) -> String {
    let left: Vec<_> = base.components().collect();
    let right: Vec<_> = path.components().collect();
    let common = left.iter().zip(&right).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return path.to_string_lossy().replace('\\', "/");
    }
    let mut result = PathBuf::new();
    for _ in common..left.len() {
        result.push("..");
    }
    for item in &right[common..] {
        result.push(item.as_os_str());
    }
    result.to_string_lossy().replace('\\', "/")
}

fn output_path(path: &Path) -> Result<PathBuf, String> {
    let absolute = std::env::current_dir()
        .map_err(|e| e.to_string())?
        .join(path);
    let parent = absolute
        .parent()
        .ok_or("Output needs a parent")?
        .canonicalize()
        .map_err(|e| format!("Output directory must exist: {e}"))?;
    let name = absolute.file_name().ok_or("Output needs a filename")?;
    Ok(parent.join(name))
}

pub(crate) fn run(request: InitRequest) -> Result<InitReport, String> {
    let project = request.project.canonicalize().map_err(|e| e.to_string())?;
    let root = project.parent().ok_or("Project needs a parent")?;
    let config_path = output_path(&request.config)?;
    let test_path = output_path(&request.test)?;
    if config_path == test_path {
        return Err("Config and test paths must differ".into());
    }
    for path in [&config_path, &test_path] {
        if path.try_exists().map_err(|e| e.to_string())? || path.symlink_metadata().is_ok() {
            return Err(format!("Refusing to overwrite {}", path.display()));
        }
    }
    let contract = request.contract.unwrap_or_else(|| request.export.clone());
    if contract.trim().is_empty() {
        return Err("Contract id must not be empty".into());
    }
    let source_path = root
        .join(&request.source)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let source = Source {
        file: source_path.to_string_lossy().into_owned(),
        export: request.export,
        discriminant: request.discriminant,
    };
    let adapter = TsrsAdapter::open(root, &project.to_string_lossy()).map_err(|errors| {
        errors
            .iter()
            .map(|d| format!("{} {}", d.code, d.message))
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    let tags = adapter.tags(root, &source).map_err(|error| {
        if error.starts_with("Missing discriminant") {
            // init defaults to _tag while scan also tries kind, type and code.
            format!("{error}; select the type's property with --discriminant")
        } else {
            error
        }
    })?;
    if tags.is_empty() {
        return Err("Selected type has no error tags".into());
    }
    let config_root = config_path.parent().unwrap();
    let files = scaffold::render(
        &relative(config_root, &project),
        &Source {
            file: relative(config_root, &source_path),
            ..source
        },
        &relative(config_root, &test_path),
        &contract,
        request.runner,
        &tags,
    )?;
    write_pair(
        &config_path,
        &files.config,
        &test_path,
        files.test.as_bytes(),
    )?;
    Ok(InitReport {
        kind: "errspec/init",
        schema_version: 1,
        files: [config_path, test_path],
        tags,
        tests_executed: false,
        ready: false,
        next_steps: [
            "Install @errspec/testkit and your selected test runner.",
            "Replace the failing TODOs with application calls and required comparisons.",
            "Run errspec verify; keep a reviewed baseline for subsequent changes.",
        ],
    })
}

fn write_pair(
    first: &Path,
    first_data: &[u8],
    second: &Path,
    second_data: &[u8],
) -> Result<(), String> {
    let mut a = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(first)
        .map_err(|e| format!("{}: {e}", first.display()))?;
    let mut b = match OpenOptions::new().write(true).create_new(true).open(second) {
        Ok(file) => file,
        Err(e) => {
            drop(a);
            let _ = fs::remove_file(first);
            return Err(format!("{}: {e}", second.display()));
        }
    };
    let result = a
        .write_all(first_data)
        .and_then(|_| a.sync_all())
        .and_then(|_| b.write_all(second_data))
        .and_then(|_| b.sync_all());
    drop(a);
    drop(b);
    if let Err(e) = result {
        let _ = fs::remove_file(first);
        let _ = fs::remove_file(second);
        return Err(e.to_string());
    }
    Ok(())
}
