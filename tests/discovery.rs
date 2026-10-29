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
    assert_eq!(
        r["candidates"],
        serde_json::json!([{
            "source": {"file": "leaf.ts", "export": "Failure", "discriminant": "_tag"},
            "tags": ["Denied", "Offline"],
            "reexports": [{"file": "barrel.ts", "export": "PublicFailure"}],
            "flows": []
        }]),
        "{r}"
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
fn matching_signatures_do_not_prove_runtime_call_targets() {
    let dir = project(&[(
        "bindings.ts",
        r#"
function leaf(): void { throw new Error('leaf'); }
function safe(): void {}
const typedAlias: typeof leaf = safe;
let mutable = leaf;
export function typed() { typedAlias(); }
export function callback(f: typeof leaf) { f(); }
export function reassigned() { mutable(); }
export function member(obj: { f: typeof leaf }) { obj.f(); }
export function customPromise(P: PromiseConstructor) { return P.reject('x'); }
const NativePromise = Promise;
export function nativeAlias() { return NativePromise.reject('x'); }
const arrow = () => { throw new Error('arrow'); };
export function directArrow() { arrow(); }
export async function mutablePromise() { let p = nativeAlias(); try { await p; } catch {} }
"#,
    )]);
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
}

#[test]
fn invalid_types_fail_without_claiming_a_complete_scan() {
    let dir = project(&[("bad.ts", "export const value: number = 'wrong';")]);
    let (status, r) = scan(&dir, &[]);
    assert_eq!(status, 2);
    assert_eq!(r["scanComplete"], false);
    assert!(!r["diagnostics"].as_array().unwrap().is_empty());
}

#[test]
fn reexported_declarations_are_one_candidate_and_distinct_declarations_stay_apart() {
    let dir = project(&[
        (
            "errors.ts",
            "type Hidden = { kind: 'Lost' }; export { Hidden as Renamed }; export type Failure = { kind: 'Offline' } | { kind: 'Denied' };",
        ),
        (
            "index.ts",
            "export type { Failure, Renamed } from './errors'; export type { Failure as Alias } from './errors';",
        ),
        (
            "other.ts",
            "export type Failure = { kind: 'Offline' } | { kind: 'Denied' };",
        ),
    ]);
    let (status, r) = scan(&dir, &["--discriminant", "kind"]);
    assert_eq!(status, 0, "{r}");
    let summary: Vec<_> = r["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                format!(
                    "{}#{}",
                    c["source"]["file"].as_str().unwrap(),
                    c["source"]["export"].as_str().unwrap()
                ),
                c["reexports"].as_array().map(|a| a.len()).unwrap_or(0),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("errors.ts#Failure".to_owned(), 2),
            ("errors.ts#Renamed".to_owned(), 1),
            ("other.ts#Failure".to_owned(), 0),
        ],
        "{r}"
    );
}

#[test]
fn candidates_built_only_from_dependency_declarations_are_external() {
    let dir = project(&[(
        "errors.ts",
        "import type { SdkIssue } from 'sdk';
export type Issue = Omit<SdkIssue, 'at'>;
export type Passed = SdkIssue;
export type Mixed = SdkIssue | { code: 'local' };
export type Local = { code: 'local' } | { code: 'other' };",
    )]);
    let sdk = dir.path().join("node_modules/sdk");
    fs::create_dir_all(&sdk).unwrap();
    fs::write(
        sdk.join("package.json"),
        r#"{"name":"sdk","types":"index.d.ts"}"#,
    )
    .unwrap();
    fs::write(
        sdk.join("index.d.ts"),
        "export type SdkIssue = { code: 'unknown_page'; at: number } | { code: 'invalid_url'; at: number };",
    )
    .unwrap();
    let (status, r) = scan(&dir, &["--discriminant", "code"]);
    assert_eq!(status, 0, "{r}");
    let external: Vec<_> = r["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["source"]["file"] == "errors.ts")
        .map(|c| {
            (
                c["source"]["export"].as_str().unwrap(),
                c.get("external").is_some(),
            )
        })
        .collect();
    assert_eq!(
        external,
        [
            ("Issue", true),
            ("Local", false),
            ("Mixed", false),
            ("Passed", true)
        ],
        "{r}"
    );
}

#[test]
fn candidates_record_the_channels_through_which_they_flow_as_errors() {
    let dir = project(&[(
        "errors.ts",
        "export interface Offline { kind: 'offline' }
export type Failure = Offline | { kind: 'denied' };
export type Thrown = { kind: 'boom' };
export type Rejected = { kind: 'late' };
export type Shape = { kind: 'circle' } | { kind: 'square' };
export function load(): { ok: true; value: number } | { ok: false; error: Failure } { return { ok: true, value: 1 }; }
export function fail(e: Thrown): never { throw e; }
export function later(e: Rejected): Promise<void> { return Promise.reject(e); }
export function area(s: Shape): number { return s.kind === 'circle' ? 1 : 2; }",
    )]);
    let (status, r) = scan(&dir, &["--discriminant", "kind"]);
    assert_eq!(status, 0, "{r}");
    let flows: Vec<_> = r["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["source"]["export"].as_str().unwrap(), c["flows"].clone()))
        .collect();
    assert_eq!(
        flows,
        [
            ("Failure", serde_json::json!(["result"])),
            ("Offline", serde_json::json!(["result"])),
            ("Rejected", serde_json::json!(["rejection"])),
            ("Shape", serde_json::json!([])),
            ("Thrown", serde_json::json!(["throw"])),
        ],
        "{r}"
    );
}
