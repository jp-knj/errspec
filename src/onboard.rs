use crate::{Runner, tsrs_adapter::TsrsAdapter};
use errspec::model::Source;
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

pub(crate) struct InitRequest {
    pub project: PathBuf,
    pub source: String,
    pub export: String,
    pub discriminant: String,
    pub runner: Runner,
    pub test: PathBuf,
    pub config: PathBuf,
    pub contract: Option<String>,
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

pub(crate) fn run(request: InitRequest) -> Result<Value, String> {
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
    let tags = adapter.tags(root, &source)?;
    if tags.is_empty() {
        return Err("Selected type has no error tags".into());
    }
    let config_root = config_path.parent().unwrap();
    let cases: Vec<_>=tags.iter().enumerate().map(|(i,tag)|json!({
        "id":format!("case-{}",i+1), "tag":tag, "expect":format!("TODO: define the required behavior for {tag}"),
        "test":{"file":relative(config_root,&test_path)},
        "observe":{"faults":["dependency"],"assertions":["behavior"]}
    })).collect();
    let config = json!({"schemaVersion":2,"project":relative(config_root,&project),"contracts":[{
        "id":contract,"source":{"file":relative(config_root,&source_path),"export":source.export,"discriminant":source.discriminant},
        "boundaries":[],"cases":cases
    }]});
    let runner = match request.runner {
        Runner::Bun => "bun:test",
        Runner::Vitest => "vitest",
    };
    let mut test = format!(
        "import {{ test }} from {runner:?};\nimport {{ scenarioTest }} from \"@errspec/testkit\";\n\n// Replace each TODO with an application call through s.fault(\"dependency\", fn),\n// then compare its observable behavior with s.check(\"behavior\", actual, expected).\n// The generated tests intentionally fail until real behavior is specified.\n"
    );
    for (i, tag) in tags.iter().enumerate() {
        let identity = json!({"contract":contract,"id":format!("case-{}",i+1),"tag":tag});
        let message =
            serde_json::to_string(&format!("TODO: implement the {tag} failure scenario")).unwrap();
        test.push_str(&format!("\nscenarioTest(test, import.meta.url, {identity}, async (_s) => {{\n  throw new Error({message});\n}});\n"));
    }
    let data = serde_json::to_vec_pretty(&config).map_err(|e| e.to_string())?;
    write_pair(&config_path, &data, &test_path, test.as_bytes())?;
    Ok(
        json!({"kind":"errspec/init","schemaVersion":1,"files":[config_path,test_path],
        "tags":tags,"testsExecuted":false,"ready":false,
        "nextSteps":["Install @errspec/testkit and your selected test runner.","Replace the failing TODOs with application calls and required comparisons.","Run errspec verify; keep a reviewed baseline for subsequent changes."]}),
    )
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
