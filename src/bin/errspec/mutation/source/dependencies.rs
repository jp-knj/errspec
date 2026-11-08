//! Literal runtime module specifiers acquired by the Oxc visitor.
use super::syntax::Node;
pub(super) fn collect(file: Node<'_>) -> Vec<String> {
    file.dependencies().to_vec()
}
