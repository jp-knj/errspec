use serde_json::Value;
use std::{fs, process::Command};
use tempfile::TempDir;
#[path = "support/snapshots.rs"]
mod snapshots;

fn project(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("tsconfig.json"),r#"{"compilerOptions":{"strict":true,"target":"ES2022","module":"ESNext","moduleResolution":"Bundler","skipLibCheck":true},"include":["*.ts"]}"#).unwrap();
    for (name, contents) in files {
        fs::write(dir.path().join(name), contents).unwrap();
    }
    dir
}
fn scan(dir: &TempDir, args: &[&str]) -> (i32, Value) {
    let out = Command::new(env!("CARGO_BIN_EXE_errspec"))
        .current_dir(dir.path())
        .args(["scan", "--format", "json"])
        .args(args)
        .output()
        .unwrap();
    let value = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| {
        panic!(
            "stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    });
    (out.status.code().unwrap(), value)
}
fn function<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["functions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == name)
        .unwrap_or_else(|| panic!("missing {name}: {report}"))
}

#[test]
fn imported_aliases_recursion_and_catches_keep_origin_witnesses() {
    let dir = project(&[
        ("leaf.ts", include_str!("fixtures/analysis/leaf.ts")),
        (
            "barrel.ts",
            "export { leaf as renamed } from './leaf'; export type { Failure as PublicFailure } from './leaf';",
        ),
        ("api.ts", include_str!("fixtures/analysis/api.ts")),
    ]);
    let (status, r) = scan(&dir, &[]);
    assert_eq!(status, 0, "{r}");
    assert!(
        r["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["source"]["export"] == "PublicFailure"
                && c["tags"] == serde_json::json!(["Denied", "Offline"]))
    );
    for name in ["api", "a", "b", "rethrow"] {
        assert!(
            !function(&r, name)["effects"].as_array().unwrap().is_empty(),
            "{name}: {r}"
        );
    }
    assert!(
        function(&r, "caught")["effects"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        function(&r, "shadow")["effects"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(function(&r, "shadow")["hasUnknown"], true);
    assert_eq!(r["allRuntimeErrorsKnown"], false);
    assert_eq!(scan(&dir, &["--fail-on-unknown"]).0, 1);
    insta::assert_json_snapshot!("imported_aliases_recursion_and_catches", r);
}

#[test]
fn await_return_and_aliases_respect_rejection_catch_boundaries() {
    let dir = project(&[("async.ts", include_str!("fixtures/analysis/async.ts"))]);
    let (status, r) = scan(&dir, &[]);
    assert_eq!(status, 0, "{r}");
    for name in ["outward", "returned", "aliasReturned", "reject"] {
        assert!(
            function(&r, name)["effects"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["channel"] == "rejection"),
            "{name}: {r}"
        );
    }
    for name in ["caught", "aliasCaught"] {
        assert!(
            function(&r, name)["effects"].as_array().unwrap().is_empty(),
            "{name}: {r}"
        );
    }
    assert_eq!(function(&r, "dangling")["hasUnknown"], true);
    for f in r["functions"].as_array().unwrap() {
        for effect in f["effects"].as_array().unwrap() {
            let origin = r["origins"]
                .as_array()
                .unwrap()
                .iter()
                .find(|o| o["id"] == effect["origin"])
                .unwrap();
            assert_eq!(
                effect["via"].as_array().unwrap().last().unwrap(),
                &origin["function"]
            );
        }
    }
    insta::assert_json_snapshot!("rejection_boundaries", r);
}

#[test]
fn structural_result_errors_and_overloaded_implementations_are_found() {
    let dir = project(&[("result.ts", include_str!("fixtures/analysis/result.ts"))]);
    let (status, r) = scan(&dir, &[]);
    assert_eq!(status, 0, "{r}");
    assert!(
        function(&r, "parse")["effects"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["channel"] == "result")
    );
    assert!(
        !function(&r, "caller")["effects"]
            .as_array()
            .unwrap()
            .is_empty(),
        "{r}"
    );
    assert_eq!(
        scan(&dir, &["--discriminant", "kind"]).1["candidates"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    insta::assert_json_snapshot!("result_errors_and_overloads", r);
}

#[test]
fn matching_signatures_do_not_prove_runtime_call_targets() {
    let dir = project(&[("bindings.ts", include_str!("fixtures/analysis/bindings.ts"))]);
    let (status, r) = scan(&dir, &[]);
    assert_eq!(status, 0, "{r}");
    for name in [
        "callback",
        "reassigned",
        "member",
        "customPromise",
        "mutablePromise",
    ] {
        assert_eq!(function(&r, name)["hasUnknown"], true, "{name}: {r}");
    }
    for name in ["typed", "callback", "reassigned", "customPromise"] {
        assert!(
            function(&r, name)["effects"].as_array().unwrap().is_empty(),
            "{name}: {r}"
        );
    }
    for name in ["nativeAlias", "directArrow"] {
        assert_eq!(function(&r, name)["hasUnknown"], false, "{name}: {r}");
        assert!(
            !function(&r, name)["effects"].as_array().unwrap().is_empty(),
            "{name}: {r}"
        );
    }
    insta::assert_json_snapshot!("unresolved_runtime_bindings", r);
}

#[test]
fn invalid_types_fail_without_claiming_a_complete_scan() {
    let dir = project(&[("bad.ts", "export const value: number = 'wrong';")]);
    let (status, r) = scan(&dir, &[]);
    assert_eq!(status, 2);
    assert_eq!(r["scanComplete"], false);
    assert!(!r["diagnostics"].as_array().unwrap().is_empty());
    insta::assert_json_snapshot!("compiler_diagnostics", snapshots::project(&r, dir.path()));
}
