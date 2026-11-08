//! Syntax-only logging heuristics. No runtime purity or equivalence is claimed.
use super::syntax::Node;
use errspec::mutation::exclusions::{ARID_CALLEES, logging_path, valid_callee};
use std::collections::HashMap;
use tsrs_ast::Kind;

pub(super) struct Arid<'a> {
    file: Node<'a>,
    patterns: Vec<String>,
    statements: HashMap<Node<'a>, bool>,
}
impl<'a> Arid<'a> {
    pub(super) fn new(
        file: Node<'a>,
        enabled: bool,
        patterns: Option<Vec<String>>,
    ) -> Result<Self, String> {
        let patterns =
            patterns.unwrap_or_else(|| ARID_CALLEES.iter().map(|name| (*name).into()).collect());
        if patterns.iter().any(|pattern| !valid_callee(pattern)) {
            return Err("Invalid arid callee pattern".into());
        }
        let mut result = Self {
            file,
            patterns: if enabled { patterns } else { vec![] },
            statements: HashMap::new(),
        };
        // Cache each statement once; ancestors are then constant-time lookups.
        if enabled {
            file.walk(&mut |node| {
                result.statement(node);
            });
        }
        Ok(result)
    }
    fn unwrap(mut node: Node<'a>, await_call: bool) -> Node<'a> {
        while matches!(
            node.kind(),
            Kind::ParenthesizedExpression | Kind::NonNullExpression
        ) || (await_call && node.kind() == Kind::AwaitExpression)
        {
            let Some(inner) = node.expression() else {
                break;
            };
            node = inner;
        }
        node
    }
    fn path(&self, node: Node<'a>) -> Option<String> {
        let node = Self::unwrap(node, false);
        match node.kind() {
            Kind::Identifier | Kind::ThisKeyword => {
                let start = node.start();
                Some(self.file.text().get(start..node.end_offset())?.into())
            }
            Kind::PropertyAccessExpression => {
                let name = self.path(node.name()?)?;
                let parent = self.path(node.expression()?)?;
                Some(if parent == "this" {
                    name
                } else {
                    format!("{parent}.{name}")
                })
            }
            _ => None,
        }
    }
    fn logging_call(&self, node: Node<'a>) -> bool {
        let node = Self::unwrap(node, true);
        node.kind() == Kind::CallExpression
            && node
                .expression()
                .and_then(|callee| self.path(callee))
                .is_some_and(|path| logging_path(&path, &self.patterns))
    }
    fn statement(&mut self, node: Node<'a>) -> bool {
        if let Some(result) = self.statements.get(&node) {
            return *result;
        }
        let result = match node.kind() {
            Kind::ExpressionStatement => node
                .expression()
                .is_some_and(|expression| self.logging_call(expression)),
            Kind::EmptyStatement => true,
            Kind::Block => {
                !node.statements().is_empty()
                    && node.statements().iter().all(|child| self.statement(*child))
            }
            Kind::IfStatement => {
                self.statement(node.then_statement())
                    && node
                        .else_statement()
                        .is_none_or(|child| self.statement(child))
            }
            _ => false,
        };
        self.statements.insert(node, result);
        result
    }
    pub(super) fn ignores(&self, mut node: Node<'a>) -> bool {
        if self.patterns.is_empty() {
            return false;
        }
        loop {
            if self.logging_call(node) || self.statements.get(&node) == Some(&true) {
                return true;
            }
            if node.kind() == Kind::SourceFile
                || node.is_function()
                || (node.is_statement()
                    && !matches!(
                        node.kind(),
                        Kind::ExpressionStatement | Kind::Block | Kind::IfStatement
                    ))
            {
                return false;
            }
            let Some(parent) = node.parent() else {
                return false;
            };
            node = parent;
        }
    }
}
