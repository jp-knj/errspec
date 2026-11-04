//! Owned syntax facts shared by selection, reuse, plugins and mutation reports.
use super::*;
use errspec::mutation::syntax::{Metadata, Scope, SyntaxNode};
use std::collections::BTreeMap;
type Entries<'a> = BTreeMap<(usize, usize), Vec<&'a Candidate>>;

fn pure(node: P<Node>) -> bool {
    match node.kind() {
        Kind::FunctionDeclaration | Kind::FunctionExpression | Kind::ArrowFunction => true,
        Kind::VariableDeclaration => {
            node.name().is_some_and(|n| n.kind() == Kind::Identifier)
                && node.initializer().is_some_and(|n| {
                    matches!(
                        n.kind(),
                        Kind::FunctionExpression
                            | Kind::ArrowFunction
                            | Kind::NumericLiteral
                            | Kind::BigIntLiteral
                            | Kind::StringLiteral
                            | Kind::TrueKeyword
                            | Kind::FalseKeyword
                            | Kind::NullKeyword
                    )
                })
        }
        Kind::VariableStatement => {
            let mut declarations = Vec::new();
            node.for_each_child(&mut |list| {
                if list.kind() == Kind::VariableDeclarationList {
                    list.for_each_child(&mut |declaration| {
                        declarations.push(declaration);
                        false
                    });
                }
                false
            });
            !declarations.is_empty() && declarations.into_iter().all(pure)
        }
        _ => false,
    }
}

fn fingerprint(rules: &Rules<'_>, node: P<Node>, effects_only: bool, hash: &mut Sha256) {
    if node.kind() == Kind::JSDoc {
        return;
    }
    hash.update(format!("{:?}:{};", node.kind(), node.flags().bits()));
    if matches!(
        node.kind(),
        Kind::Identifier
            | Kind::PrivateIdentifier
            | Kind::StringLiteral
            | Kind::NumericLiteral
            | Kind::BigIntLiteral
            | Kind::NoSubstitutionTemplateLiteral
            | Kind::TemplateHead
            | Kind::TemplateMiddle
            | Kind::TemplateTail
            | Kind::RegularExpressionLiteral
            | Kind::JsxText
    ) {
        let text = rules.raw(node);
        hash.update(text.len().to_le_bytes());
        hash.update(text);
    }
    node.for_each_child(&mut |child| {
        if effects_only && node.kind() == Kind::SourceFile && pure(child) {
            skeleton(rules, child, hash);
        } else {
            fingerprint(rules, child, false, hash);
        }
        false
    });
}

// Definitions keep their public binding/signature but do not execute their function bodies.
fn skeleton(rules: &Rules<'_>, node: P<Node>, hash: &mut Sha256) {
    hash.update(format!("{:?}:{};", node.kind(), node.flags().bits()));
    node.for_each_child(&mut |child| {
        if node.body() == Some(child) {
            return false;
        }
        if matches!(
            child.kind(),
            Kind::VariableDeclarationList
                | Kind::VariableDeclaration
                | Kind::FunctionExpression
                | Kind::ArrowFunction
        ) {
            skeleton(rules, child, hash);
        } else {
            fingerprint(rules, child, false, hash);
        }
        false
    });
}

fn references(node: P<Node>, root: P<Node>, out: &mut BTreeSet<String>, unknown: &mut bool) {
    if root.kind() == Kind::SourceFile && node != root && node.parent() == Some(root) && pure(node)
    {
        return;
    }
    let binding = node.parent().is_some_and(|parent| {
        parent.name() == Some(node)
            && matches!(
                parent.kind(),
                Kind::FunctionDeclaration
                    | Kind::VariableDeclaration
                    | Kind::Parameter
                    | Kind::ClassDeclaration
                    | Kind::MethodDeclaration
                    | Kind::PropertyDeclaration
                    | Kind::ImportSpecifier
                    | Kind::ImportClause
                    | Kind::NamespaceImport
                    | Kind::BindingElement
            )
    });
    if node.kind() == Kind::Identifier && !binding {
        out.insert(crate::tsrs_adapter::text(node));
    }
    if node.kind() == Kind::CallExpression
        && node.expression().is_none_or(|callee| {
            callee.kind() != Kind::Identifier
                || matches!(
                    crate::tsrs_adapter::text(callee).as_str(),
                    "eval" | "Function" | "require"
                )
        })
    {
        *unknown = true;
    }
    node.for_each_child(&mut |child| {
        references(child, root, out, unknown);
        false
    });
}

fn syntax(
    rules: &Rules<'_>,
    node: P<Node>,
    parent: Option<usize>,
    nodes: &mut Vec<SyntaxNode>,
) -> usize {
    let id = nodes.len();
    nodes.push(SyntaxNode {
        kind: format!("{:?}", node.kind()),
        start: rules.start(node),
        end: rules.end(node),
        parent,
        name: node.name().map(|name| rules.raw(name).into()),
        children: vec![],
    });
    let mut children = vec![];
    node.for_each_child(&mut |child| {
        children.push(syntax(rules, child, Some(id), nodes));
        false
    });
    nodes[id].children = children;
    id
}

fn entry(rules: &Rules<'_>, entries: &Entries<'_>, owner: &str, node: P<Node>) -> Option<String> {
    let function = if tsrs_ast::is_function_like_declaration(node) {
        Some(node)
    } else if matches!(
        node.kind(),
        Kind::VariableDeclaration | Kind::PropertyDeclaration
    ) {
        node.initializer()
            .filter(|node| matches!(node.kind(), Kind::ArrowFunction | Kind::FunctionExpression))
    } else {
        None
    }?;
    let body = function.body()?;
    entries
        .get(&(rules.start(body), rules.end(body)))?
        .iter()
        .find(|candidate| {
            candidate.ignored.is_none()
                && rules
                    .owners
                    .get(&candidate.key)
                    .is_some_and(|name| name == owner)
                && (body.kind() == Kind::Block
                    && candidate.mutator == "BlockStatement"
                    && candidate.placement == Placement::Body
                    || body.kind() != Kind::Block
                        && candidate.mutator == "ArrowFunction"
                        && candidate.placement == Placement::Expression)
        })
        .map(|candidate| candidate.key.clone())
}

pub(super) fn collect(rules: &Rules<'_>) -> Metadata {
    let mut entries = Entries::new();
    for candidate in &rules.candidates {
        if candidate.ignored.is_none()
            && (candidate.mutator == "BlockStatement" && candidate.placement == Placement::Body
                || candidate.mutator == "ArrowFunction"
                    && candidate.placement == Placement::Expression)
        {
            entries
                .entry((
                    candidate.location.byte_offset as usize,
                    candidate.end as usize,
                ))
                .or_default()
                .push(candidate);
        }
    }
    let scopes = rules
        .scopes
        .iter()
        .map(|(name, node)| {
            let mut full = Sha256::new();
            fingerprint(rules, *node, false, &mut full);
            let mut effects = Sha256::new();
            fingerprint(rules, *node, name == "module", &mut effects);
            // Compiler pragmas can alter behavior without changing syntax. Until
            // their attachments are acquired, commented files require raw identity.
            if name == "module" && !comments::acquire(rules.file).is_empty() {
                effects.update(b"comment-sensitive-source-v1\0");
                effects.update(rules.file.text().as_bytes());
            }
            let mut refs = BTreeSet::new();
            let mut unknown = false;
            references(*node, *node, &mut refs, &mut unknown);
            let start = rules.start(*node);
            let end = rules.end(*node);
            (
                name.clone(),
                Scope {
                    hash: format!("{:x}", full.finalize()),
                    effect_hash: format!("{:x}", effects.finalize()),
                    start,
                    end,
                    line: line_at(rules.file.text(), start),
                    end_line: line_at(rules.file.text(), end),
                    parent: rules.parents.get(name).cloned(),
                    name: node.name().map(|n| rules.raw(n).into()),
                    references: refs,
                    pure: pure(*node),
                    function: matches!(
                        node.kind(),
                        Kind::FunctionDeclaration
                            | Kind::FunctionExpression
                            | Kind::ArrowFunction
                            | Kind::MethodDeclaration
                    ) || matches!(
                        node.kind(),
                        Kind::VariableDeclaration | Kind::PropertyDeclaration
                    ) && node.initializer().is_some_and(|n| {
                        matches!(n.kind(), Kind::FunctionExpression | Kind::ArrowFunction)
                    }),
                    unknown_calls: unknown,
                    entry_key: entry(rules, &entries, name, *node),
                },
            )
        })
        .collect();
    let mut nodes = vec![];
    syntax(rules, rules.file.as_node(), None, &mut nodes);
    Metadata {
        version: 2,
        source_hash: format!("{:x}", Sha256::digest(rules.file.text().as_bytes())),
        scopes,
        owners: rules
            .owners
            .iter()
            .filter(|(key, _)| rules.candidates.iter().any(|c| &c.key == *key))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
        nodes,
    }
}
