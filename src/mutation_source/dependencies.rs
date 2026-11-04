//! Acquire literal runtime module specifiers for child-process warning attribution.
use super::*;

pub(super) fn collect(file: P<SourceFile>) -> Vec<String> {
    fn visit(node: P<Node>, result: &mut BTreeSet<String>) {
        if is_type_node_kind(node.kind()) || node.kind() == Kind::JSDoc {
            return;
        }
        let specifier = match node.kind() {
            Kind::ImportDeclaration => {
                let declaration = node.as_import_declaration();
                if declaration.import_clause().is_some_and(|clause| {
                    clause.as_import_clause().phase_modifier() == Kind::TypeKeyword
                }) {
                    return;
                }
                Some(declaration.module_specifier())
            }
            Kind::ExportDeclaration => {
                let declaration = node.as_export_declaration();
                if declaration.is_type_only() {
                    return;
                }
                declaration.module_specifier()
            }
            Kind::CallExpression => {
                let callee = node.expression();
                if callee.is_some_and(|callee| {
                    callee.kind() == Kind::ImportKeyword
                        || callee.kind() == Kind::Identifier
                            && callee.as_identifier().text() == "require"
                }) {
                    node.as_call_expression()
                        .arguments()
                        .nodes()
                        .first()
                        .copied()
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(specifier) = specifier
            && specifier.kind() == Kind::StringLiteral
        {
            result.insert(specifier.as_string_literal().text().into());
        }
        node.for_each_child(&mut |child| {
            visit(child, result);
            false
        });
    }
    let mut result = BTreeSet::new();
    visit(file.as_node(), &mut result);
    result.into_iter().collect()
}
