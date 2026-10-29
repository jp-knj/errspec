use errspec::config::ValidatedConfig;
use errspec::{bun, evidence, model::*};
mod support;
use serde_json::json;

#[test]
fn parses_real_bun_nested_names_entities_skip_and_todo() {
    let result = bun::parse_junit(include_str!("fixtures/bun/nested.xml")).unwrap();
    assert!(!result.success);
    assert_eq!(result.runner, "bun");
    assert_eq!(result.tests.len(), 6);
    assert_eq!(result.tests[0].name, "top-level");
    assert_eq!(
        result.tests[1].name,
        "outer <xml> inner & 日本語 pass \"quoted\""
    );
    assert_eq!(result.tests[1].status, "passed");
    assert_eq!(result.tests[2].status, "skipped");
    assert_eq!(result.tests[3].status, "todo");
    assert_eq!(result.tests[4].status, "failed");
    assert_eq!(result.tests[5].file, "two.test.ts");
}

#[test]
fn hook_failures_do_not_imply_the_named_test_ran() {
    let result = bun::parse_junit(include_str!("fixtures/bun/hook-failure.xml")).unwrap();
    assert!(!result.success);
    assert_eq!(result.tests[0].name, "(unnamed)");
    assert_eq!(result.tests[0].status, "failed");
}

#[test]
fn malformed_incomplete_and_unknown_reports_are_rejected() {
    for xml in [
        "<testsuites>",
        "<unrelated/>",
        "<testsuites tests='1' failures='0'/>",
        "<testsuites tests='oops' failures='0'/>",
        "<testsuites tests='0'/>",
        "<testsuites tests='1' failures='0'><testsuite name='a'><testcase name='x'/></testsuite></testsuites>",
        "<testsuites tests='1' failures='0'><testsuite file='a'><testcase name='x'><cancelled/></testcase></testsuite></testsuites>",
        "<testsuites tests='1' failures='0'><testsuite file='a'><testcase name='x' status='disabled'/></testsuite></testsuites>",
    ] {
        assert!(bun::parse_junit(xml).is_err(), "{xml}");
    }
}

#[test]
fn bun_file_identity_duplicates_and_skipped_cases_are_checked() {
    for (cases, expected) in [
        ("<testcase name='case'/>", "passed"),
        ("<testcase name='case'><skipped/></testcase>", "skipped"),
        (
            "<testcase name='case'><skipped message='TODO'/></testcase>",
            "todo",
        ),
        (
            "<testcase name='case'/><testcase name='case'/>",
            "ambiguous",
        ),
        ("<testcase name='other'/>", "not_found"),
    ] {
        let count = cases.matches("<testcase").count() + 1;
        let xml = format!(
            "<testsuites tests='{count}' failures='0'><testsuite file='a.test.ts' name='a.test.ts'>{cases}</testsuite><testsuite file='b.test.ts' name='b.test.ts'><testcase name='case'/></testsuite></testsuites>"
        );
        let c: Contract = serde_json::from_value(json!({"id":"x", "source":{"file":"e.ts","export":"E","discriminant":"tag"},"cases":[{"tag":"A","expect":"rejects","test":{"file":"a.test.ts","name":"case"}}]})).unwrap();
        let config = ValidatedConfig::try_from(Config {
            schema_version: 1,
            project: "tsconfig.json".into(),
            contracts: vec![c],
        })
        .unwrap();
        let mut data = support::register(&config, &["A"]);
        let (tests, findings) = evidence::judge(
            &data.registrations,
            &support::paths(&["a.test.ts", "b.test.ts"]),
            bun::parse_junit(&xml).unwrap(),
            true,
        );
        data.tests = Some(tests);
        data.diagnostics.extend(findings);
        let report = data.finish("verify");
        assert_eq!(report.test_runner, Some("bun"));
        assert_eq!(report.contracts[0].cases[0].test_status, expected);
        assert_eq!(report.diagnostics.is_empty(), expected == "passed");
    }
}

#[test]
fn summary_errors_and_process_failure_fail_even_with_passing_cases() {
    let result = bun::parse_junit("<testsuites tests='1' failures='0' errors='1'><testsuite file='a.test.ts'><testcase name='case'/></testsuite></testsuites>").unwrap();
    assert!(!result.success);
    let result = bun::parse_junit("<testsuites tests='0' failures='0'/>").unwrap();
    let (_, findings) = evidence::judge(&[], &Default::default(), result, false);
    assert!(findings.iter().any(|f| f.code == "EC006"));
}
