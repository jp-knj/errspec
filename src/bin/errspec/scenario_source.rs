//! Static matching of registered test files against the configuration, before any test runs.
//! Only definite contradictions are reported: a value whose type is not a finite set of string
//! literals, or SDK use routed through helpers, is left to the runtime observation checks.
use crate::tsrs_adapter::{TsrsAdapter, location, text, walk};
use errspec::{
    config::{CaseKind, ValidatedConfig},
    model::{Finding, Location, ObservationRequirements},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
use tsrs_ast::{Kind, Node, get_source_file_of_node};
use tsrs_checker::{Checker, LiteralValue, Type, TypeFlags};
use tsrs_core::P;

struct Registered<'a> {
    tag: &'a str,
    file: PathBuf,
    observe: &'a ObservationRequirements,
}

/// Values a property can take, read from its initializer so contextual widening does not hide literals.
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "kind", content = "values", rename_all = "camelCase")]
pub(crate) enum Value {
    Absent,
    Known(BTreeSet<String>),
    Unknown,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ScenarioCall {
    pub location: Location,
    pub contract: Value,
    pub id: Value,
    pub tags: Value,
    pub without_faults: Value,
    pub uses: Vec<(String, String)>,
}

pub(crate) fn files(root: &Path, config: &ValidatedConfig) -> Vec<PathBuf> {
    config
        .contracts()
        .iter()
        .flat_map(|c| c.cases())
        .filter(|c| matches!(c.kind(), CaseKind::Scenario { .. }))
        .filter_map(|c| root.join(&c.test().file).canonicalize().ok())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn inspect(
    adapter: &TsrsAdapter,
    root: &Path,
    config: &ValidatedConfig,
) -> Vec<Finding> {
    let mut calls = Vec::new();
    for path in files(root, config) {
        let Some(file) = adapter.program.get_source_file(&path.to_string_lossy()) else {
            continue;
        };
        let mut checker = adapter
            .program
            .get_type_checker_for_file(&adapter.context, file);
        let mut nodes = Vec::new();
        walk(file.as_node(), &mut |node| {
            if node.kind() == Kind::CallExpression
                && is_sdk(&mut checker, node, "scenarioTest", None)
            {
                nodes.push(node);
            }
        });
        for call in nodes {
            let args = call.arguments();
            let definition = args
                .get(2)
                .filter(|d| d.kind() == Kind::ObjectLiteralExpression);
            let mut value = |name: &str| {
                definition.map_or(Value::Unknown, |d| property(&mut checker, *d, name))
            };
            let contract = value("contract");
            let id = value("id");
            let tags = value("tag");
            let without_faults = value("withoutFaults");
            calls.push(ScenarioCall {
                location: location(root, call),
                contract,
                id,
                tags,
                without_faults,
                uses: args
                    .get(3)
                    .map(|body| observation_ids(&mut checker, *body))
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(kind, id)| (kind.into(), id))
                    .collect(),
            });
        }
    }
    inspect_facts(root, config, &calls)
}

/// Resolve file identities, then compare acquired facts without compiler handles.
pub(crate) fn inspect_facts(
    root: &Path,
    config: &ValidatedConfig,
    calls: &[ScenarioCall],
) -> Vec<Finding> {
    let mut registered = BTreeMap::new();
    for contract in config.contracts() {
        for case in contract.cases() {
            let CaseKind::Scenario { id, observe } = case.kind() else {
                continue;
            };
            let Ok(file) = root.join(&case.test().file).canonicalize() else {
                continue;
            };
            registered.insert(
                (contract.id(), id.as_str()),
                Registered {
                    tag: case.tag(),
                    file,
                    observe,
                },
            );
        }
    }
    let mut findings = Vec::new();
    for call in calls {
        let path = root.join(&call.location.file);
        let path = path.canonicalize().unwrap_or(path);
        inspect_call(&path, call, &registered, &mut findings);
    }
    findings
}

fn inspect_call(
    path: &Path,
    call: &ScenarioCall,
    registered: &BTreeMap<(&str, &str), Registered<'_>>,
    findings: &mut Vec<Finding>,
) {
    let at = |code: &str, message: String| {
        let location = call.location.clone();
        let mut finding = Finding::new(code, message);
        finding.file = Some(location.file.into());
        finding.byte_offset = Some(location.byte_offset);
        finding.line = Some(location.line as u32);
        finding
    };
    let (Value::Known(contracts), Value::Known(ids)) = (&call.contract, &call.id) else {
        return;
    };
    let tags = &call.tags;
    let without_faults = &call.without_faults;
    let uses = &call.uses;
    for contract in contracts {
        for id in ids {
            let case = registered.get(&(contract.as_str(), id.as_str()));
            let scenario = |finding: Finding| {
                let mut finding = match case {
                    Some(case) => finding.for_case(contract, case.tag),
                    None => finding,
                };
                finding.contract_id = Some(contract.as_str().into());
                finding.for_scenario(Some(id))
            };
            let Some(case) = case else {
                findings.push(scenario(at(
                    "EC015",
                    format!("Test registers a scenario that is not in the configuration: {contract} / {id}"),
                )));
                continue;
            };
            let mut mismatch = |message: String| findings.push(scenario(at("EC015", message)));
            if case.file != path {
                mismatch("Scenario is registered for a different test file".into());
            }
            if let Value::Known(tags) = &tags
                && !tags.contains(case.tag)
            {
                let tags = tags.iter().cloned().collect::<Vec<_>>().join(" | ");
                mismatch(format!(
                    "Test tag {tags} differs from the registered tag {}",
                    case.tag
                ));
            }
            let declared = match &without_faults {
                Value::Absent => Some(false),
                Value::Known(values) if values.len() == 1 => values.first().map(|v| v == "true"),
                _ => None,
            };
            if let Some(declared) = declared
                && declared != case.observe.faults.is_empty()
            {
                mismatch(if declared {
                    "Test declares withoutFaults but the configuration requires faults".into()
                } else {
                    "Configuration declares \"faults\": [] but the test does not declare withoutFaults".into()
                });
            }
            for (kind, value, list) in uses.iter().map(|(kind, value)| {
                let list = if kind == "fault" {
                    &case.observe.faults
                } else {
                    &case.observe.assertions
                };
                (kind, value, list)
            }) {
                if !list.contains(value) {
                    findings.push(scenario(at(
                        "EC016",
                        format!("Scenario uses {kind} id {value:?} that the configuration does not declare"),
                    )));
                }
            }
        }
    }
}

/// A call resolves to the testkit when its declaration sits beside `ScenarioScope`, not by name alone.
fn is_sdk(checker: &mut Checker, call: P<Node>, name: &str, owner: Option<&str>) -> bool {
    let Some(declaration) = checker.get_resolved_signature_exported(call).declaration() else {
        return false;
    };
    if declaration.name().is_none_or(|n| text(n) != name) {
        return false;
    }
    match owner {
        Some(owner) => declaration.parent().is_some_and(|p| {
            p.kind() == Kind::InterfaceDeclaration && p.name().is_some_and(|n| text(n) == owner)
        }),
        None => get_source_file_of_node(declaration).is_some_and(|file| {
            file.symbol().is_some_and(|module| {
                checker
                    .get_exports_of_module_exported(module)
                    .iter()
                    .any(|s| s.name() == "ScenarioScope")
            })
        }),
    }
}

fn property(checker: &mut Checker, object: P<Node>, key: &str) -> Value {
    for property in object.properties() {
        let name = property.name().map(text);
        if name.as_deref() != Some(key) {
            continue;
        }
        let expression = match property.kind() {
            Kind::PropertyAssignment => property.initializer(),
            Kind::ShorthandPropertyAssignment => property.name(),
            _ => None,
        };
        let Some(expression) = expression else {
            return Value::Unknown;
        };
        let ty = checker.get_type_at_location(expression);
        return literals(checker, ty).map_or(Value::Unknown, Value::Known);
    }
    // A spread may still supply the property.
    if object
        .properties()
        .iter()
        .any(|p| p.kind() == Kind::SpreadAssignment)
    {
        Value::Unknown
    } else {
        Value::Absent
    }
}

fn literals(checker: &mut Checker, ty: P<Type>) -> Option<BTreeSet<String>> {
    let members: Vec<_> = if ty.flags().intersects(TypeFlags::Union) {
        ty.types().to_vec()
    } else {
        vec![ty]
    };
    let mut values = BTreeSet::new();
    for member in members {
        if member.flags().intersects(TypeFlags::StringLiteral)
            && let Some(LiteralValue::String(value)) = member.as_literal_type().value()
        {
            values.insert(value.to_owned());
        } else if member.flags().intersects(TypeFlags::BooleanLiteral) {
            values.insert(checker.type_to_string_exported(member));
        } else {
            return None;
        }
    }
    (!values.is_empty()).then_some(values)
}

/// Literal ids passed directly to `ScenarioScope.fault` / `check` within the scenario body.
fn observation_ids(checker: &mut Checker, body: P<Node>) -> Vec<(&'static str, String)> {
    let mut calls = Vec::new();
    walk(body, &mut |node| {
        if node.kind() == Kind::CallExpression {
            calls.push(node);
        }
    });
    let mut ids = Vec::new();
    for call in calls {
        for kind in ["fault", "check"] {
            if !is_sdk(checker, call, kind, Some("ScenarioScope")) {
                continue;
            }
            if let Some(first) = call.arguments().first() {
                let ty = checker.get_type_at_location(*first);
                if let Some(values) = literals(checker, ty) {
                    ids.extend(values.into_iter().map(|value| (kind, value)));
                }
            }
        }
    }
    ids
}
