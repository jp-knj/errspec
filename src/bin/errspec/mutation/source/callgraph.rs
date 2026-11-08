//! Acquire source spelling and lexical owners, without inferring reachability.
use super::*;
use errspec::mutation::syntax::Import;

pub(super) fn path(rules: &Rules<'_>, mut node: Node<'_>) -> Option<String> {
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
pub(super) fn imports(rules: &Rules<'_>) -> Vec<Import> {
    rules.file.imports().to_vec()
}
