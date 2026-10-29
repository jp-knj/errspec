use serde_json::Value;
use std::{fs, process::Command};
use tempfile::TempDir;
fn fixture() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("tsconfig.json"),
        r#"{"compilerOptions":{"strict":true,"skipLibCheck":true},"files":["errors.ts"]}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("errors.ts"),
        "export type Failure={_tag:'Offline'}|{_tag:'Denied'};",
    )
    .unwrap();
    dir
}
fn init(dir: &TempDir, runner: &str) -> (i32, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_errspec"))
        .current_dir(dir.path())
        .args([
            "init",
            "--source",
            "errors.ts",
            "--export",
            "Failure",
            "--runner",
            runner,
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    (
        output.status.code().unwrap(),
        serde_json::from_slice(&output.stdout).unwrap(),
    )
}
#[test]
fn generates_each_tag_and_failing_tests_for_both_runners() {
    for runner in ["bun", "vitest"] {
        let dir = fixture();
        let (code, report) = init(&dir, runner);
        assert_eq!(code, 0, "{report}");
        assert_eq!(report["ready"], false);
        assert_eq!(report["testsExecuted"], false);
        let config: Value =
            serde_json::from_slice(&fs::read(dir.path().join("errspec.json")).unwrap()).unwrap();
        assert_eq!(config["contracts"][0]["cases"].as_array().unwrap().len(), 2);
        let test = fs::read_to_string(dir.path().join("errspec.generated.test.ts")).unwrap();
        assert_eq!(test.matches("throw new Error").count(), 2);
        assert!(test.contains(if runner == "bun" {
            "bun:test"
        } else {
            "vitest"
        }));
        let checked = Command::new(env!("CARGO_BIN_EXE_errspec"))
            .current_dir(dir.path())
            .args(["check", "--format", "json"])
            .output()
            .unwrap();
        assert!(
            checked.status.success(),
            "{}",
            String::from_utf8_lossy(&checked.stdout)
        );
    }
}
#[test]
fn refuses_either_existing_output_without_leaving_the_other() {
    for existing in ["errspec.json", "errspec.generated.test.ts"] {
        let dir = fixture();
        fs::write(dir.path().join(existing), "keep me").unwrap();
        let (code, report) = init(&dir, "bun");
        assert_eq!(code, 2, "{report}");
        assert_eq!(
            fs::read_to_string(dir.path().join(existing)).unwrap(),
            "keep me"
        );
        let other = if existing == "errspec.json" {
            "errspec.generated.test.ts"
        } else {
            "errspec.json"
        };
        assert!(!dir.path().join(other).exists());
    }
}
#[test]
fn unsupported_type_creates_nothing() {
    let dir = fixture();
    fs::write(
        dir.path().join("errors.ts"),
        "export type Failure={_tag:string};",
    )
    .unwrap();
    assert_eq!(init(&dir, "bun").0, 2);
    assert!(!dir.path().join("errspec.json").exists());
    assert!(!dir.path().join("errspec.generated.test.ts").exists());
}
