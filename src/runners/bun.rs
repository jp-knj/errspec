//! Bun's built-in JUnit reporter, exercised with Bun 1.4.4.
//! Describe order comes from nested suites, not classname (which Bun emits inside-out).
use crate::evidence::{RunEvidence, TestEvidence};
use roxmltree::{Document, Node};

pub fn parse_junit(xml: &str) -> Result<RunEvidence, String> {
    let document = Document::parse(xml).map_err(|e| format!("Invalid Bun JUnit XML: {e}"))?;
    let root = document.root_element();
    if !root.has_tag_name("testsuites") {
        return Err("Expected Bun JUnit <testsuites> root".into());
    }
    let declared = count(root, "tests", true)?;
    let mut success = count(root, "failures", true)? == 0 && count(root, "errors", false)? == 0;
    let mut tests = Vec::new();
    for suite in root.children().filter(|n| n.is_element()) {
        match suite.tag_name().name() {
            "testsuite" => {
                let file = suite
                    .attribute("file")
                    .filter(|s| !s.is_empty())
                    .ok_or("Bun file suite is missing its file attribute")?;
                walk_suite(suite, file, &mut Vec::new(), &mut tests, &mut success)?;
            }
            "properties" | "system-out" | "system-err" => (),
            other => return Err(format!("Unsupported Bun JUnit root element: {other}")),
        }
    }
    if tests.len() != declared {
        return Err(format!(
            "Incomplete Bun report: declares {declared} tests, contains {}",
            tests.len()
        ));
    }
    Ok(RunEvidence {
        runner: "bun",
        success,
        tests,
    })
}

fn count(node: Node<'_, '_>, key: &str, required: bool) -> Result<usize, String> {
    match node.attribute(key) {
        Some(value) => value
            .parse()
            .map_err(|_| format!("Invalid JUnit {key} count: {value}")),
        None if required => Err(format!("Missing JUnit {key} count")),
        None => Ok(0),
    }
}

fn walk_suite(
    suite: Node<'_, '_>,
    file: &str,
    ancestors: &mut Vec<String>,
    tests: &mut Vec<TestEvidence>,
    success: &mut bool,
) -> Result<(), String> {
    if count(suite, "failures", false)? > 0 || count(suite, "errors", false)? > 0 {
        *success = false;
    }
    for child in suite.children().filter(|n| n.is_element()) {
        match child.tag_name().name() {
            "testsuite" => {
                let name = child
                    .attribute("name")
                    .ok_or("Nested Bun suite has no name")?;
                ancestors.push(name.to_owned());
                walk_suite(
                    child,
                    child.attribute("file").unwrap_or(file),
                    ancestors,
                    tests,
                    success,
                )?;
                ancestors.pop();
            }
            "testcase" => {
                let leaf = child.attribute("name").ok_or("Bun testcase has no name")?;
                let mut names = ancestors.clone();
                names.push(leaf.to_owned());
                let mut status = "passed";
                for detail in child.children().filter(|n| n.is_element()) {
                    match detail.tag_name().name() {
                        "failure" | "error" => {
                            status = "failed";
                            *success = false;
                        }
                        "skipped" if status != "failed" => {
                            status = if detail.attribute("message") == Some("TODO") {
                                "todo"
                            } else {
                                "skipped"
                            };
                        }
                        "skipped" | "system-out" | "system-err" | "properties" => (),
                        other => return Err(format!("Unsupported Bun testcase result: {other}")),
                    }
                }
                // A future non-passing status must not silently become a passing test.
                if let Some(value) = child.attribute("status") {
                    return Err(format!(
                        "Unsupported Bun testcase status attribute: {value}"
                    ));
                }
                let file = child.attribute("file").unwrap_or(file);
                if file.is_empty() {
                    return Err("Bun testcase has an empty file".into());
                }
                tests.push(TestEvidence {
                    file: file.into(),
                    name: names.join(" "),
                    status: status.into(),
                });
            }
            "properties" | "system-out" | "system-err" => (),
            "error" | "failure" => *success = false,
            other => return Err(format!("Unsupported Bun suite element: {other}")),
        }
    }
    Ok(())
}
