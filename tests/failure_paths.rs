//! Failure paths from a contract's entry function: inventory and EC018 in `check`,
//! reachability (EC019) from a separate Bun coverage run in `verify`.
use serde_json::{Value, json};
use std::{fs, process::Command};
use tempfile::TempDir;

const SUBMIT: &str = r#"export type Result = { status: 'invalid' } | { status: 'blocked' } | { status: 'sent' };
function parse(raw: string): number {
  if (!raw) throw new RangeError('empty');
  return Number(raw);
}
function guarded(raw: string): number {
  try {
    return parse(raw);
  } catch {
    return 0;
  }
}
export async function submit(input: string, bot: boolean): Promise<Result> {
  if (input === '') return { status: 'invalid' };
  if (bot) {
    return { status: 'blocked' };
  }
  const size = guarded(input);
  const later = () => {
    return { status: 'sent' as const };
  };
  if (size > 10) {
    return { status: 'blocked' };
  }
  void later;
  return { status: 'sent' };
}
"#;

struct Fixture {
    dir: TempDir,
}

impl Fixture {
    fn new(source: &str) -> Self {
        Self::with(source, json!({"file":"submit.ts","export":"submit"}))
    }
    fn with(source: &str, entry: Value) -> Self {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("submit.ts"), source).unwrap();
        fs::write(dir.path().join("case.test.ts"), "export {};").unwrap();
        fs::write(
            dir.path().join("tsconfig.json"),
            r#"{"compilerOptions":{"strict":true,"skipLibCheck":true,"target":"ES2022","module":"ESNext","moduleResolution":"Bundler"},"files":["submit.ts"]}"#,
        )
        .unwrap();
        let case = |id: &str, tag: &str| json!({"id":id,"tag":tag,"expect":"x","test":{"file":"case.test.ts"},"observe":{"faults":[],"assertions":["r"]}});
        let mut contract = json!({"id":"submit","source":{"file":"submit.ts","export":"Result","discriminant":"status"},
            "cases":[case("i","invalid"),case("b","blocked"),case("s","sent")]});
        if !entry.is_null() {
            contract["entry"] = entry;
        }
        fs::write(
            dir.path().join("errspec.json"),
            json!({"schemaVersion":2,"project":"tsconfig.json","contracts":[contract]}).to_string(),
        )
        .unwrap();
        Self { dir }
    }
    fn run(&self, args: &[&str]) -> (i32, Value) {
        let output = Command::new(env!("CARGO_BIN_EXE_errspec"))
            .current_dir(self.dir.path())
            .args(args)
            .args(["--format", "json"])
            .output()
            .unwrap();
        let report = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&output.stderr)));
        (output.status.code().unwrap(), report)
    }
    fn check(&self) -> (i32, Value) {
        self.run(&["check"])
    }
}

fn lines_and_tags(report: &Value) -> Vec<(u64, Vec<String>)> {
    report["contracts"][0]["paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["line"].as_u64().unwrap(),
                p["tags"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|t| t.as_str().unwrap().to_owned())
                    .collect(),
            )
        })
        .collect()
}

fn codes(report: &Value) -> Vec<String> {
    report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap().to_owned())
        .collect()
}

fn tags(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| v.to_string()).collect()
}

// [正常系] path inventory

#[test]
fn returns_of_the_entry_are_listed_with_their_tags() {
    let (status, report) = Fixture::new(SUBMIT).check();
    assert_eq!(status, 0, "{report}");
    assert_eq!(
        lines_and_tags(&report),
        vec![
            (14, tags(&["invalid"])),
            (16, tags(&["blocked"])),
            (23, tags(&["blocked"])),
            (26, tags(&["sent"])),
        ]
    );
    let first = &report["contracts"][0]["paths"][0];
    assert_eq!(first["file"], "submit.ts");
    assert!(
        first.get("reached").is_none(),
        "check records no reach: {first}"
    );
    assert_eq!(report["contracts"][0]["entry"]["export"], "submit");
}

#[test]
fn a_return_of_a_helper_union_is_one_path_with_every_tag() {
    let source = SUBMIT.replace(
        "  return { status: 'sent' };\n}",
        "  return decide(size);\n}\nfunction decide(n: number): Result {\n  return n > 0 ? { status: 'sent' } : { status: 'invalid' };\n}",
    );
    let (_, report) = Fixture::new(&source).check();
    // The helper's declared return type is the whole union, so the path may yield every tag.
    assert_eq!(
        lines_and_tags(&report).last().unwrap().1,
        tags(&["blocked", "invalid", "sent"])
    );
}

#[test]
fn const_arrow_and_expression_bodied_entries_are_resolved() {
    let source = "export type Result = { status: 'invalid' } | { status: 'sent' };\n\
        export const submit = (ok: boolean): Result => ok ? { status: 'sent' } : { status: 'invalid' };\n";
    let (status, report) = Fixture::new(source).check();
    assert_eq!(status, 1, "{report}"); // 'blocked' is not in this type, so EC004 for its case
    assert_eq!(
        lines_and_tags(&report),
        vec![(2, tags(&["invalid", "sent"]))]
    );
    let source = "export type Result = { status: 'invalid' } | { status: 'blocked' } | { status: 'sent' };\n\
        export const submit = function (ok: boolean): Result {\n  if (ok) {\n    return { status: 'sent' };\n  }\n  return { status: 'blocked' };\n};\n";
    let (_, report) = Fixture::new(source).check();
    assert_eq!(
        lines_and_tags(&report),
        vec![(4, tags(&["sent"])), (6, tags(&["blocked"]))]
    );
}

#[test]
fn reexported_entries_resolve_to_their_declaration() {
    let fixture = Fixture::with(SUBMIT, json!({"file":"index.ts","export":"send"}));
    fs::write(
        fixture.dir.path().join("index.ts"),
        "export { submit as send } from './submit';\n",
    )
    .unwrap();
    fs::write(
        fixture.dir.path().join("tsconfig.json"),
        r#"{"compilerOptions":{"strict":true,"skipLibCheck":true,"target":"ES2022","module":"ESNext","moduleResolution":"Bundler"},"files":["submit.ts","index.ts"]}"#,
    )
    .unwrap();
    let (status, report) = fixture.check();
    assert_eq!(status, 0, "{report}");
    assert_eq!(lines_and_tags(&report).len(), 4);
}

#[test]
fn values_that_are_not_contract_tags_are_not_paths() {
    let source = "export type Result = { status: 'invalid' } | { status: 'blocked' } | { status: 'sent' };\n\
        export function submit(n: number): Result | number | { status: 'other' } {\n  if (n === 0) {\n    return 0;\n  }\n  if (n === 1) {\n    return { status: 'other' };\n  }\n  return { status: 'sent' };\n}\n";
    let (_, report) = Fixture::new(source).check();
    assert_eq!(lines_and_tags(&report), vec![(9, tags(&["sent"]))]);
}

#[test]
fn contracts_without_an_entry_have_no_paths() {
    let (status, report) = Fixture::with(SUBMIT, Value::Null).check();
    assert_eq!(status, 0, "{report}");
    assert!(report["contracts"][0].get("paths").is_none());
    assert!(report["contracts"][0].get("entry").is_none());
}

// [異常系] EC018

#[test]
fn an_uncaught_throw_escaping_the_entry_is_reported() {
    let source = SUBMIT.replace("const size = guarded(input);", "const size = parse(input);");
    let (status, report) = Fixture::new(&source).check();
    assert_eq!(status, 1, "{report}");
    let finding = &report["diagnostics"][0];
    assert_eq!(finding["code"], "EC018");
    let message = finding["message"].as_str().unwrap();
    assert!(message.contains("RangeError"), "{message}");
    assert!(
        message.contains("rejection via submit -> parse"),
        "{message}"
    );
    assert!(
        message.contains("Result does not represent it"),
        "{message}"
    );
    assert_eq!(finding["contractId"], "submit");
    assert_eq!(finding["file"], "submit.ts");
}

#[test]
fn throws_and_rejections_in_the_entry_itself_are_reported() {
    for (line, channel) in [
        ("throw new TypeError('bad');", "rejection"),
        ("await Promise.reject(new SyntaxError('bad'));", "rejection"),
    ] {
        let source = SUBMIT.replace(
            "  const size = guarded(input);",
            &format!("  if (input === 'x') {line}\n  const size = guarded(input);"),
        );
        let (_, report) = Fixture::new(&source).check();
        assert!(codes(&report).contains(&"EC018".into()), "{line}: {report}");
        assert!(report["diagnostics"].to_string().contains(channel));
    }
    let sync = "export type Result = { status: 'invalid' } | { status: 'blocked' } | { status: 'sent' };\n\
        export function submit(n: number): Result {\n  if (n < 0) throw new RangeError('negative');\n  return { status: 'sent' };\n}\n";
    let (_, report) = Fixture::new(sync).check();
    assert!(
        report["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("as a throw"),
        "{report}"
    );
}

// [エッジケース] not definite

#[test]
fn caught_failures_are_not_reported_even_if_the_catch_may_rethrow() {
    for source in [
        SUBMIT.to_owned(),
        SUBMIT.replace(
            "const size = guarded(input);",
            "let size = 0;\n  try { size = parse(input); } catch { size = -1; }",
        ),
    ] {
        let (_, report) = Fixture::new(&source).check();
        assert!(!codes(&report).contains(&"EC018".into()), "{report}");
    }
    // The original RangeError may be rethrown, so it is not definite. The rethrow itself is
    // an explicit throw outside any try, so it is reported as a new origin of type unknown.
    let rethrow = SUBMIT.replace(
        "const size = guarded(input);",
        "let size = 0;\n  try { size = parse(input); } catch (e) { if (input) throw e; }",
    );
    let (_, report) = Fixture::new(&rethrow).check();
    let messages = report["diagnostics"].to_string();
    assert!(!messages.contains("RangeError"), "{report}");
    assert!(messages.contains("lets unknown escape"), "{report}");
}

#[test]
fn unresolved_calls_and_uncalled_nested_throws_are_not_reported() {
    let source = SUBMIT
        .replace(
            "const size = guarded(input);",
            "const size = Number(JSON.parse(input));",
        )
        .replace(
            "    return { status: 'sent' as const };",
            "    throw new Error('never called');",
        );
    let (_, report) = Fixture::new(&source).check();
    assert!(!codes(&report).contains(&"EC018".into()), "{report}");
}

#[test]
fn entries_that_cannot_be_resolved_stop_the_analysis() {
    for (entry, message) in [
        (json!({"file":"submit.ts","export":"missing"}), "not found"),
        (
            json!({"file":"submit.ts","export":"Result"}),
            "must be a function",
        ),
        (json!({"file":"nowhere.ts","export":"submit"}), "nowhere.ts"),
    ] {
        let (status, report) = Fixture::with(SUBMIT, entry).check();
        assert_eq!(status, 2, "{report}");
        let finding = &report["diagnostics"][0];
        assert_eq!(finding["code"], "EC002");
        assert!(
            finding["message"].as_str().unwrap().contains(message),
            "{report}"
        );
        assert_eq!(report["analysisComplete"], false);
    }
    let fixture = Fixture::new(SUBMIT);
    fs::write(fixture.dir.path().join("outside.ts"), SUBMIT).unwrap();
    let (status, report) =
        Fixture::with(SUBMIT, json!({"file":"case.test.ts","export":"submit"})).check();
    assert_eq!(status, 2, "{report}");
    assert!(
        report["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("not in the TypeScript project")
            || report["diagnostics"][0]["message"]
                .as_str()
                .unwrap()
                .contains("not found")
    );
}

#[test]
fn an_entry_requires_version_two_and_nonempty_fields() {
    let fixture = Fixture::new(SUBMIT);
    let path = fixture.dir.path().join("errspec.json");
    let base: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    for change in [
        json!({"file":"","export":"submit"}),
        json!({"file":"submit.ts","export":" "}),
        json!({"file":"submit.ts","export":"submit","extra":1}),
    ] {
        let mut config = base.clone();
        config["contracts"][0]["entry"] = change.clone();
        fs::write(&path, config.to_string()).unwrap();
        let (status, report) = fixture.check();
        assert_eq!(status, 2, "{change}: {report}");
        assert_eq!(report["diagnostics"][0]["code"], "EC_CONFIG");
    }
    let mut legacy = json!({"schemaVersion":1,"project":"tsconfig.json","contracts":[{
        "id":"submit","source":{"file":"submit.ts","export":"Result","discriminant":"status"},
        "entry":{"file":"submit.ts","export":"submit"},
        "cases":[{"tag":"invalid","expect":"x","test":{"file":"case.test.ts","name":"i"}}]}]});
    fs::write(&path, legacy.to_string()).unwrap();
    let (status, report) = fixture.check();
    assert_eq!(status, 2, "{report}");
    assert!(
        report["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("schemaVersion: 2")
    );
    legacy["contracts"][0]
        .as_object_mut()
        .unwrap()
        .remove("entry");
    fs::write(&path, legacy.to_string()).unwrap();
    assert_ne!(fixture.check().0, 2);
}

// verify: reachability from Bun coverage

#[cfg(unix)]
fn fake_bun(fixture: &Fixture, lcov: Option<&str>) -> String {
    use std::os::unix::fs::PermissionsExt;
    let lcov_step = match lcov {
        Some(text) => format!("printf '%s' '{text}' > \"$5/lcov.info\""),
        None => "true".into(),
    };
    let script = format!(
        r#"#!/bin/sh
if [ "$1" = --version ]; then echo 1.4.4; exit 0; fi
if [ "$2" = --coverage ]; then
  printf '%s\n' "$@" > coverage-args
  env | grep '^ERRSPEC_' > coverage-env || true
  {lcov_step}
  exit 0
fi
printf '%s' '<testsuites tests="0" failures="0"/>' > "$4"
"#
    );
    let path = fixture.dir.path().join("fake-bun");
    fs::write(&path, script).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path.to_string_lossy().into_owned()
}

#[cfg(unix)]
fn verify_bun(fixture: &Fixture, bun: &str) -> (i32, Value) {
    fixture.run(&["verify", "--runner", "bun", "--bun", bun])
}

fn reached(report: &Value) -> Vec<Option<bool>> {
    report["contracts"][0]["paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.get("reached").and_then(Value::as_bool))
        .collect()
}

#[cfg(unix)]
#[test]
fn verify_marks_paths_from_a_scenario_only_coverage_run() {
    let fixture = Fixture::new(SUBMIT);
    let lcov = "SF:submit.ts\nDA:14,1\nDA:16,2\nDA:23,0\nDA:26,1\nend_of_record\n";
    let bun = fake_bun(&fixture, Some(lcov));
    let (status, report) = verify_bun(&fixture, &bun);
    assert_eq!(status, 1, "{report}");
    assert_eq!(
        reached(&report),
        vec![Some(true), Some(true), Some(false), Some(true)]
    );
    let unreached: Vec<_> = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["code"] == "EC019")
        .collect();
    assert_eq!(unreached.len(), 1, "{report}");
    assert!(
        unreached[0]["message"]
            .as_str()
            .unwrap()
            .contains("blocked (line 23)")
    );
    // Only SDK scenarios of registered files run, and without observation variables.
    let args = fs::read_to_string(fixture.dir.path().join("coverage-args")).unwrap();
    let args: Vec<_> = args.lines().collect();
    assert_eq!(
        &args[..4],
        [
            "test",
            "--coverage",
            "--coverage-reporter=lcov",
            "--coverage-dir"
        ]
    );
    assert_eq!(&args[5..7], ["-t", r"errspec \["]);
    assert!(args[7].ends_with("case.test.ts"), "{args:?}");
    assert_eq!(
        fs::read_to_string(fixture.dir.path().join("coverage-env")).unwrap(),
        ""
    );
}

#[cfg(unix)]
#[test]
fn all_paths_reached_add_no_failure() {
    let fixture = Fixture::new(SUBMIT);
    let lcov = "SF:submit.ts\nDA:14,1\nDA:16,1\nDA:23,1\nDA:26,1\nend_of_record\n";
    let (_, report) = verify_bun(&fixture, &fake_bun(&fixture, Some(lcov)));
    assert_eq!(reached(&report), vec![Some(true); 4]);
    assert!(!codes(&report).contains(&"EC019".into()), "{report}");
}

#[cfg(unix)]
#[test]
fn a_source_never_loaded_by_scenarios_leaves_every_path_unreached() {
    let fixture = Fixture::new(SUBMIT);
    let (_, report) = verify_bun(
        &fixture,
        &fake_bun(&fixture, Some("SF:other.ts\nDA:1,1\nend_of_record\n")),
    );
    assert_eq!(reached(&report), vec![Some(false); 4]);
    assert_eq!(codes(&report).iter().filter(|c| *c == "EC019").count(), 4);
}

#[cfg(unix)]
#[test]
fn a_missing_coverage_report_is_a_runner_error() {
    let fixture = Fixture::new(SUBMIT);
    let (status, report) = verify_bun(&fixture, &fake_bun(&fixture, None));
    assert_eq!(status, 2, "{report}");
    assert!(
        report["diagnostics"]
            .to_string()
            .contains("Bun coverage report"),
        "{report}"
    );
}

#[cfg(unix)]
#[test]
fn contracts_without_entries_skip_the_coverage_run() {
    let fixture = Fixture::with(SUBMIT, Value::Null);
    let _ = verify_bun(
        &fixture,
        &fake_bun(&fixture, Some("SF:submit.ts\nend_of_record\n")),
    );
    assert!(!fixture.dir.path().join("coverage-args").exists());
}

#[cfg(unix)]
#[test]
fn vitest_lists_paths_without_claiming_reach() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new(SUBMIT);
    let vitest = fixture.dir.path().join("fake-vitest");
    fs::write(
        &vitest,
        "#!/bin/sh\nprintf '%s' '{\"success\":true,\"testResults\":[]}' > \"$4\"\n",
    )
    .unwrap();
    fs::set_permissions(&vitest, fs::Permissions::from_mode(0o755)).unwrap();
    let (_, report) = fixture.run(&["verify", "--vitest", vitest.to_str().unwrap()]);
    assert_eq!(reached(&report), vec![None; 4]);
    assert!(!codes(&report).contains(&"EC019".into()), "{report}");
}

#[cfg(unix)]
#[test]
fn text_output_lists_each_path_with_its_reach_and_the_unreached_location() {
    let fixture = Fixture::new(SUBMIT);
    let text = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_errspec"))
            .current_dir(fixture.dir.path())
            .args(args)
            .output()
            .unwrap();
        String::from_utf8(output.stdout).unwrap()
    };
    let checked = text(&["check"]);
    assert!(
        checked.contains("  path submit.ts:14 invalid: not recorded"),
        "{checked}"
    );
    let lcov = "SF:submit.ts\nDA:14,1\nDA:16,1\nDA:23,0\nDA:26,1\nend_of_record\n";
    let bun = fake_bun(&fixture, Some(lcov));
    let verified = text(&["verify", "--runner", "bun", "--bun", &bun]);
    assert!(
        verified.contains("  path submit.ts:16 blocked: reached"),
        "{verified}"
    );
    assert!(
        verified.contains("  path submit.ts:23 blocked: not reached"),
        "{verified}"
    );
    assert!(verified.contains("EC019 submit submit.ts:23"), "{verified}");
}

#[test]
fn findings_point_at_the_line_of_the_origin() {
    let source = SUBMIT.replace("const size = guarded(input);", "const size = parse(input);");
    let (_, report) = Fixture::new(&source).check();
    assert_eq!(report["diagnostics"][0]["line"], 3, "{report}");
}
