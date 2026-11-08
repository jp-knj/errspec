//! Acquire only comments identified by the parser, never directive-looking literals.
use super::syntax::Node;
use errspec::mutation::exclusions::Comment;
pub(super) fn acquire(file: Node<'_>) -> Vec<Comment> {
    file.comments()
        .iter()
        .filter_map(|&(start, end)| {
            let raw = &file.text()[start as usize..end as usize];
            Some(Comment {
                start: start as usize,
                end: end as usize,
                text: raw
                    .strip_prefix("//")
                    .or_else(|| raw.strip_prefix("/*")?.strip_suffix("*/"))?
                    .into(),
            })
        })
        .collect()
}
