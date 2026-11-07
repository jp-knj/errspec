//! Acquire source spelling and lexical owners, without inferring reachability.
use super::*;
use errspec::mutation::syntax::Import;

pub(super) fn path(rules: &Rules<'_>, mut node: P<Node>) -> Option<String> {
    while matches!(
        node.kind(),
        Kind::ParenthesizedExpression
            | Kind::NonNullExpression
            | Kind::AsExpression
            | Kind::TypeAssertionExpression
            | Kind::SatisfiesExpression
    ) {
        node = node.expression()?;
    }
    match node.kind() {
        Kind::Identifier | Kind::ThisKeyword => Some(rules.raw(node).into()),
        Kind::PropertyAccessExpression => Some(format!(
            "{}.{}",
            path(rules, node.expression()?)?,
            rules.raw(node.name()?)
        )),
        _ => None,
    }
}
fn name(rules: &Rules<'_>, node: P<Node>) -> String {
    if node.kind() == Kind::StringLiteral {
        node.as_string_literal().text().into()
    } else {
        rules.raw(node).into()
    }
}
pub(super) fn imports(rules: &Rules<'_>) -> Vec<Import> {
    let mut result = vec![];
    rules.file.as_node().for_each_child(&mut |node| {
        if node.kind() != Kind::ImportDeclaration {
            return false;
        }
        let declaration = node.as_import_declaration();
        let Some(clause) = declaration.import_clause() else {
            return false;
        };
        let clause = clause.as_import_clause();
        if clause.phase_modifier() == Kind::TypeKeyword {
            return false;
        }
        let specifier = declaration.module_specifier();
        if specifier.kind() != Kind::StringLiteral {
            return false;
        }
        let source = specifier.as_string_literal().text();
        if let Some(local) = clause.name() {
            result.push(Import {
                source: source.into(),
                imported: "default".into(),
                local: name(rules, local),
                resolved: None,
            });
        }
        if let Some(bindings) = clause.named_bindings() {
            match bindings.kind() {
                Kind::NamespaceImport => result.push(Import {
                    source: source.into(),
                    imported: "*".into(),
                    local: name(rules, bindings.as_namespace_import().name()),
                    resolved: None,
                }),
                Kind::NamedImports => {
                    for element in bindings.as_named_imports().elements().nodes() {
                        let import = element.as_import_specifier();
                        if !import.is_type_only() {
                            result.push(Import {
                                source: source.into(),
                                imported: name(
                                    rules,
                                    import.property_name().unwrap_or(import.name()),
                                ),
                                local: name(rules, import.name()),
                                resolved: None,
                            });
                        }
                    }
                }
                _ => (),
            }
        }
        false
    });
    result
}
