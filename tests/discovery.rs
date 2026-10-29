use serde_json::Value;
use std::{fs, process::Command};
use tempfile::TempDir;

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
        (
            "leaf.ts",
            r#"export type Failure = { _tag: 'Offline' } | { _tag: 'Denied' }; export function leaf(): never { throw new Error('offline'); }"#,
        ),
        (
            "barrel.ts",
            "export { leaf as renamed } from './leaf'; export type { Failure as PublicFailure } from './leaf';",
        ),
        (
            "api.ts",
            r#"import { renamed } from './barrel'; const alias = renamed;
export function api(): void { alias(); }
export function a(n:number): void { if(n>0) b(n-1); else api(); }
export function b(n:number): void { a(n); }
export function caught(): void { try { api(); } catch {} }
export function rethrow(): void { try { api(); } catch(e) { throw e; } }
export function shadow(renamed: () => void): void { renamed(); }
"#,
        ),
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
}

#[test]
fn await_return_and_aliases_respect_rejection_catch_boundaries() {
    let dir = project(&[(
        "async.ts",
        r#"
export async function source(): Promise<void> { throw new Error('offline'); }
export async function outward(): Promise<void> { await source(); }
export async function caught(): Promise<void> { try { await source(); } catch {} }
export function returned() { try { return source(); } catch { return Promise.resolve(); } }
export async function aliasCaught() { const p = source(); try { await p; } catch {} }
export async function aliasReturned() { const p = source(); const q = p; return q; }
export function reject() { return Promise.reject(new Error('reject')); }
export function dangling() { source(); }
export function recursive(): Promise<void> { if (Math.random() > 0) throw new Error('cycle'); return bounce(); }
export async function bounce(): Promise<void> { await recursive(); }
"#,
    )]);
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
}

#[test]
fn structural_result_errors_and_overloaded_implementations_are_found() {
    let dir = project(&[(
        "result.ts",
        r#"
export type Failure={kind:'Missing'}|{kind:'Invalid'};
type Result<T,E>={ok:true,value:T}|{ok:false,error:E};
export function parse(x:string): Result<string,Failure> { return {ok:false,error:{kind:'Missing'}}; }
export async function wrapper() { return parse(''); }
export function overload(x:string): void;
export function overload(x:number): void;
export function overload(x:string|number): void { throw new TypeError('bad'); }
export function caller() { overload(''); }
"#,
    )]);
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
}

#[test]
fn invalid_types_fail_without_claiming_a_complete_scan() {
    let dir = project(&[("bad.ts", "export const value: number = 'wrong';")]);
    let (status, r) = scan(&dir, &[]);
    assert_eq!(status, 2);
    assert_eq!(r["scanComplete"], false);
    assert!(!r["diagnostics"].as_array().unwrap().is_empty());
}
