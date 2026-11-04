//! Acquire lexical comments using parsed trivia, never directive-looking literals.
use crate::tsrs_adapter::walk;
use errspec::mutation::exclusions::Comment;
use std::collections::BTreeSet;
use tsrs_ast::{Kind, SourceFile};
use tsrs_core::P;

pub(super) fn acquire(file: P<SourceFile>) -> Vec<Comment> {
    let mut ranges = BTreeSet::new();
    let mut literals = vec![];
    walk(file.as_node(), &mut |node| {
        let start = tsrs_scanner::get_token_pos_of_node(node, file, false);
        let end = node.end().source_offset().unwrap_or(start);
        if matches!(
            node.kind(),
            Kind::StringLiteral
                | Kind::RegularExpressionLiteral
                | Kind::NoSubstitutionTemplateLiteral
                | Kind::TemplateHead
                | Kind::TemplateMiddle
                | Kind::TemplateTail
                | Kind::JsxText
        ) {
            literals.push((start, end));
        }
        for comment in tsrs_scanner::get_leading_comment_ranges(
            file.text(),
            node.pos().source_offset().unwrap_or(start),
        )
        .chain(tsrs_scanner::get_trailing_comment_ranges(file.text(), end))
        {
            if let (Some(start), Some(end)) = (
                comment.text_range.pos().source_offset(),
                comment.text_range.end().source_offset(),
            ) {
                ranges.insert((start, end));
            }
        }
    });
    ranges
        .into_iter()
        .filter(|(start, end)| {
            !literals
                .iter()
                .any(|(left, right)| start < right && left < end)
        })
        .filter_map(|(start, end)| {
            let raw = file.text().get(start as usize..end as usize)?;
            let text = raw
                .strip_prefix("//")
                .or_else(|| raw.strip_prefix("/*")?.strip_suffix("*/"))?
                .to_owned();
            Some(Comment {
                start: start as usize,
                end: end as usize,
                text,
            })
        })
        .collect()
}
