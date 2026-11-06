//! Validate acquired plugin edits against owned syntax; callbacks never enter Rust.
use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Edit {
    pub mutator: String,
    pub anchor_start: usize,
    pub anchor_end: usize,
    pub anchor_kind: String,
    pub start: usize,
    pub end: usize,
    pub replacement: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Ignore {
    pub start: usize,
    pub end: usize,
    pub reason: String,
}

pub(super) fn validate(
    file: P<SourceFile>,
    names: &[String],
    edits: &[Edit],
    ignores: &[Ignore],
) -> Result<(), String> {
    let mut unique = BTreeSet::new();
    for name in names {
        if name.is_empty()
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
            || MUTATORS.contains(&name.as_str())
            || !unique.insert(name)
        {
            return Err(format!("Invalid or duplicate custom mutator name: {name}"));
        }
    }
    let mut nodes = BTreeSet::new();
    let mut runtime_nodes = BTreeSet::new();
    crate::tsrs_adapter::walk(file.as_node(), &mut |node| {
        let anchor = (
            tsrs_scanner::get_token_pos_of_node(node, file, false) as usize,
            node.end().source_offset().unwrap_or(0) as usize,
            format!("{:?}", node.kind()),
        );
        nodes.insert(anchor.clone());
        if node.parent().is_some()
            && (tsrs_ast::is_expression_node(node)
                || node.kind() == Kind::Block
                || runtime_statement(node))
        {
            runtime_nodes.insert(anchor);
        }
    });
    for edit in edits {
        if !names.contains(&edit.mutator)
            || !nodes.contains(&(edit.anchor_start, edit.anchor_end, edit.anchor_kind.clone()))
            || edit.start < edit.anchor_start
            || edit.end > edit.anchor_end
            || edit.start > edit.end
            || file.text().get(edit.start..edit.end).is_none()
        {
            return Err(format!(
                "Plugin mutator {} returned an invalid anchor or range",
                edit.mutator
            ));
        }
        if !runtime_nodes.contains(&(edit.anchor_start, edit.anchor_end, edit.anchor_kind.clone()))
        {
            return Err(format!(
                "Plugin mutator {} anchor cannot hold a runtime mutation switch",
                edit.mutator
            ));
        }
        let anchor_replacement = format!(
            "{}{}{}",
            &file.text()[edit.anchor_start..edit.start],
            edit.replacement,
            &file.text()[edit.end..edit.anchor_end]
        );
        if edit.anchor_kind == "Block"
            && !(anchor_replacement.trim_start().starts_with('{')
                && anchor_replacement.trim_end().ends_with('}'))
        {
            return Err(format!(
                "Plugin mutator {} must replace a block with a block",
                edit.mutator
            ));
        }
        let modified = format!(
            "{}{}{}",
            &file.text()[..edit.start],
            edit.replacement,
            &file.text()[edit.end..]
        );
        parse_source(file.file_name(), modified).map_err(|error| {
            format!(
                "Plugin mutator {} produced invalid syntax: {error}",
                edit.mutator
            )
        })?;
    }
    for ignore in ignores {
        if ignore.reason.trim().is_empty()
            || !nodes
                .iter()
                .any(|(start, end, _)| *start == ignore.start && *end == ignore.end)
        {
            return Err("Plugin ignorer returned an invalid node or empty reason".into());
        }
    }
    Ok(())
}

pub(super) fn apply(rules: &mut Rules<'_>, node: P<Node>, owner: &str, path: &[usize]) {
    for (index, edit) in rules.plugin_edits.iter().enumerate() {
        if edit.anchor_start != rules.start(node)
            || edit.anchor_end != rules.end(node)
            || edit.anchor_kind != format!("{:?}", node.kind())
        {
            continue;
        }
        rules.applied_plugin_edits.insert(index);
        let replacement = format!(
            "{}{}{}",
            &rules.file.text()[rules.start(node)..edit.start],
            edit.replacement,
            &rules.file.text()[edit.end..rules.end(node)]
        );
        let directive = node.kind() == Kind::ExpressionStatement && rules.directive(node)
            || node.parent().is_some_and(|parent| {
                parent.kind() == Kind::ExpressionStatement && rules.directive(parent)
            });
        if directive {
            rules.add(
                node,
                &edit.mutator,
                replacement,
                Placement::Expression,
                owner,
                path,
                Some("Plugin mutation would change a directive prologue".into()),
                None,
            );
            continue;
        }
        if node.parent().is_some() && tsrs_ast::is_expression_node(node) {
            // Lift callee and reference edits to a complete runtime expression so
            // switches preserve receivers, optional calls and assignment references.
            let mut target = node;
            while let Some(parent) = target.parent() {
                let reference = matches!(
                    parent.kind(),
                    Kind::CallExpression
                        | Kind::NewExpression
                        | Kind::TaggedTemplateExpression
                        | Kind::PropertyAccessExpression
                        | Kind::ElementAccessExpression
                        | Kind::NonNullExpression
                        | Kind::DeleteExpression
                        | Kind::PrefixUnaryExpression
                        | Kind::PostfixUnaryExpression
                ) && (parent.expression() == Some(target)
                    || matches!(
                        parent.kind(),
                        Kind::PrefixUnaryExpression | Kind::PostfixUnaryExpression
                    ));
                let lhs = parent.kind() == Kind::BinaryExpression
                    && parent.as_binary_expression().left() == target
                    && rules
                        .raw(parent.as_binary_expression().operator_token())
                        .ends_with('=')
                    && !matches!(
                        rules.raw(parent.as_binary_expression().operator_token()),
                        "==" | "===" | "!=" | "!==" | "<=" | ">="
                    );
                if reference || lhs {
                    target = parent;
                } else {
                    break;
                }
            }
            let value = rules.splice(target, node, &replacement);
            rules.expression(target, &edit.mutator, value, owner, path);
        } else if node.kind() == Kind::Block {
            let body = node.parent().is_some_and(|parent| {
                tsrs_ast::is_function_like(parent) && parent.body() == Some(node)
            });
            let ignored = if body {
                let parsed = parse_source(
                    "plugin-body.ts",
                    format!("async function* __errspec__(){replacement}"),
                );
                let modified = parsed.ok().and_then(|file| {
                    file.as_node()
                        .statements()
                        .first()
                        .and_then(|function| function.body())
                });
                modified
                    .filter(|modified| directives(node) != directives(*modified))
                    .map(|_| "Plugin body replacement would change a directive prologue".into())
            } else {
                None
            };
            rules.add(
                node,
                &edit.mutator,
                replacement,
                if body {
                    Placement::Body
                } else {
                    Placement::Block
                },
                owner,
                path,
                ignored,
                None,
            );
        } else if runtime_statement(node) {
            let mut target = node;
            while let Some(parent) = target.parent() {
                if parent.kind() == Kind::LabeledStatement {
                    target = parent;
                } else {
                    break;
                }
            }
            let replacement = rules.splice(target, node, &replacement);
            rules.add(
                target,
                &edit.mutator,
                replacement,
                Placement::Statement,
                owner,
                path,
                None,
                None,
            );
        } else {
            // Type/declaration/property-token edits cannot hold a runtime switch.
            rules.add(
                node,
                &edit.mutator,
                replacement,
                Placement::Expression,
                owner,
                path,
                Some("Plugin anchor cannot hold a runtime mutation switch".into()),
                None,
            );
        }
    }
}

fn directives(node: P<Node>) -> Vec<String> {
    node.statements()
        .iter()
        .take_while(|statement| {
            statement.kind() == Kind::ExpressionStatement
                && statement
                    .expression()
                    .is_some_and(|expression| expression.kind() == Kind::StringLiteral)
        })
        .map(|statement| statement.expression().unwrap().text().to_owned())
        .collect()
}

fn runtime_statement(node: P<Node>) -> bool {
    tsrs_ast::is_statement_but_not_declaration(node)
        && !matches!(
            node.kind(),
            Kind::VariableStatement | Kind::NotEmittedStatement
        )
}
