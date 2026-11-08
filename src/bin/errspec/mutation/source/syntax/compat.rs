//! Preserve the public plugin tree independently of the mutation-rule traversal.
use super::*;
use errspec::mutation::syntax::SyntaxNode;

pub(super) fn nodes(graph: &Graph, program: &Program<'_>) -> Vec<SyntaxNode> {
    let mut copy = Graph {
        file: graph.file.clone(),
        modules: modules::Facts::default(),
        source: graph.source.clone(),
        entries: graph.entries.clone(),
        comments: graph.comments.clone(),
        redirects: vec![],
        plugin_nodes: vec![],
    };
    Projection { graph: &mut copy }.visit_program(program);
    let end = copy.source.len() as u32;
    copy.token(0, Span::new(end, end), Kind::EndOfFile);
    copy.entries[0].span.start = trivia_end(&copy.source, &copy.comments, 0) as u32;
    for entry in &mut copy.entries {
        entry.children.clear();
    }
    for i in 1..copy.entries.len() {
        if copy.entries[i].kind == Kind::Unknown {
            continue;
        }
        let mut parent = copy.entries[i].parent;
        while let Some(p) = parent {
            if copy.entries[p].kind != Kind::Unknown {
                break;
            }
            parent = copy.entries[p].parent;
        }
        copy.entries[i].parent = parent;
        if let Some(p) = parent {
            copy.entries[p].children.push(i);
        }
    }
    for i in 0..copy.entries.len() {
        let mut children = std::mem::take(&mut copy.entries[i].children);
        children.sort_by_key(|&n| copy.entries[n].span.start);
        copy.entries[i].children = children;
    }
    fn emit(graph: &Graph, id: usize, parent: Option<usize>, out: &mut Vec<SyntaxNode>) -> usize {
        let index = out.len();
        let entry = &graph.entries[id];
        out.push(SyntaxNode {
            kind: format!("{:?}", entry.kind),
            start: entry.span.start as usize,
            end: entry.span.end as usize,
            parent,
            name: entry.name.map(|n| {
                let span = graph.entries[n].span;
                graph.source[span.start as usize..span.end as usize].into()
            }),
            children: vec![],
        });
        let children = entry
            .children
            .iter()
            .map(|&n| emit(graph, n, Some(index), out))
            .collect();
        out[index].children = children;
        index
    }
    let mut output = vec![];
    emit(&copy, 0, None, &mut output);
    output
}

fn trivia_end(source: &str, comments: &[(u32, u32)], mut start: usize) -> usize {
    loop {
        let next = start + source[start..].len()
            - source[start..]
                .trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
                .len();
        start = next;
        if source[start..].starts_with("//") {
            start += source[start..].find('\n').unwrap_or(source.len() - start);
            continue;
        }
        if source[start..].starts_with("/*") {
            start += source[start..]
                .find("*/")
                .map_or(source.len() - start, |n| n + 2);
            continue;
        }
        if let Some(&(_, end)) = comments.iter().find(|&&(begin, _)| begin as usize == start) {
            start = end as usize;
        } else {
            return start;
        }
    }
}
struct Projection<'g> {
    graph: &'g mut Graph,
}
impl Projection<'_> {
    fn token(&mut self, parent: usize, start: u32, end: u32, text: &str, kind: Kind) {
        if let Some(span) = graph_token(&self.graph.source, start as usize, end as usize, text) {
            self.graph.token(parent, span, kind);
        }
    }
    fn wrap(&mut self, parent: usize, span: Span, kind: Kind, children: &[usize]) -> usize {
        let wrapper = self.graph.token(parent, span, kind);
        for &child in children {
            self.graph.entries[child].parent = Some(wrapper);
        }
        wrapper
    }
    fn modifiers(&mut self, index: usize, end: u32, words: &[(&str, Kind)]) {
        let start = self.graph.entries[index].span.start;
        for &(word, kind) in words {
            self.token(index, start, end, word, kind);
        }
    }
    fn declaration_modifiers(&mut self, index: usize, end: u32, declare: bool, asynchronous: bool) {
        let flags = self.graph.entries[index].flags;
        let mut words = vec![];
        if flags & (1 << 16) != 0 {
            words.push(("export", Kind::ExportKeyword));
        }
        if flags & (1 << 17) != 0 {
            words.push(("default", Kind::DefaultKeyword));
        }
        if declare {
            words.push(("declare", Kind::DeclareKeyword));
        }
        if asynchronous {
            words.push(("async", Kind::AsyncKeyword));
        }
        self.modifiers(index, end, &words);
    }
}
impl<'a> Visit<'a> for Projection<'_> {
    fn enter_node(&mut self, k: AstKind<'a>) {
        use AstKind as A;
        let index = k.node_id().index();
        match k {
            A::VariableDeclaration(n) => {
                let old = self.graph.entries[index].clone();
                self.graph.entries[index].kind = Kind::VariableDeclarationList;
                let start = if n.declare {
                    graph_token(
                        &self.graph.source,
                        n.span.start as usize,
                        n.declarations[0].span.start as usize,
                        "declare",
                    )
                    .map_or(n.span.start, |s| {
                        trivia_end(&self.graph.source, &self.graph.comments, s.end as usize) as u32
                    })
                } else {
                    n.span.start
                };
                self.graph.entries[index].span =
                    Span::new(start, n.declarations.last().unwrap().span.end);
                let in_loop = old.parent.is_some_and(|p| {
                    matches!(
                        self.graph.entries[p].kind,
                        Kind::ForStatement | Kind::ForInStatement | Kind::ForOfStatement
                    )
                });
                if !in_loop {
                    let wrapper = self.wrap(
                        old.parent.unwrap_or(0),
                        old.span,
                        Kind::VariableStatement,
                        &[index],
                    );
                    self.graph.entries[wrapper].flags = old.flags;
                    self.declaration_modifiers(
                        wrapper,
                        n.declarations[0].span.start,
                        n.declare,
                        false,
                    );
                }
            }
            A::Function(n) => {
                if self.graph.entries[index].kind != Kind::Unknown {
                    let end = n.id.as_ref().map_or(n.params.span.start, |n| n.span.start);
                    self.declaration_modifiers(index, end, n.declare, n.r#async);
                    if n.generator {
                        self.token(index, n.span.start, end, "*", Kind::AsteriskToken);
                    }
                }
            }
            A::ArrowFunctionExpression(n) => {
                self.declaration_modifiers(index, n.params.span.start, false, n.r#async);
                self.token(
                    index,
                    n.params.span.end,
                    n.body.span().start,
                    "=>",
                    Kind::EqualsGreaterThanToken,
                );
            }
            A::ConditionalExpression(n) => {
                self.token(
                    index,
                    n.test.span().end,
                    n.consequent.span().start,
                    "?",
                    Kind::QuestionToken,
                );
                self.token(
                    index,
                    n.consequent.span().end,
                    n.alternate.span().start,
                    ":",
                    Kind::ColonToken,
                );
            }
            A::BinaryExpression(n) => {
                if let Some(op) = self.graph.entries[index].operator {
                    self.graph.entries[op].kind = match n.operator.as_str() {
                        ">=" => Kind::GreaterThanEqualsToken,
                        ">>" => Kind::GreaterThanGreaterThanToken,
                        ">>>" => Kind::GreaterThanGreaterThanGreaterThanToken,
                        _ => self.graph.entries[op].kind,
                    };
                }
            }
            A::AssignmentExpression(n) => {
                if let Some(op) = self.graph.entries[index].operator {
                    self.graph.entries[op].kind = match n.operator.as_str() {
                        ">>=" => Kind::GreaterThanGreaterThanEqualsToken,
                        ">>>=" => Kind::GreaterThanGreaterThanGreaterThanEqualsToken,
                        _ => self.graph.entries[op].kind,
                    };
                }
            }
            A::TSNamespaceDeclaration(n) => {
                self.declaration_modifiers(index, n.id.span().start, n.declare, false)
            }
            A::TSExternalModuleDeclaration(n) => {
                self.declaration_modifiers(index, n.id.span.start, n.declare, false)
            }
            A::AccessorProperty(n) => self.token(
                index,
                n.span.start,
                n.key.span().start,
                "accessor",
                Kind::AccessorKeyword,
            ),
            A::AssignmentPattern(n) => {
                self.graph.entries[index].name = Some(id(&n.left));
                if self.graph.entries[index].parent.is_some_and(|p| {
                    self.graph.entries[p].kind == Kind::BindingElement
                        && self.graph.entries[p].span == n.span
                }) {
                    self.graph.entries[index].kind = Kind::Unknown;
                }
            }
            A::AssignmentTargetPropertyIdentifier(n) => {
                self.graph.entries[index].name = Some(id(&n.binding));
                if let Some(init) = &n.init {
                    self.token(
                        index,
                        n.binding.span.end,
                        init.span().start,
                        "=",
                        Kind::EqualsToken,
                    );
                }
            }
            A::ExportDefaultDeclaration(n) => {
                let child = id(&n.declaration);
                if !matches!(
                    self.graph.entries[child].kind,
                    Kind::FunctionDeclaration | Kind::ClassDeclaration
                ) {
                    self.graph.entries[index].kind = Kind::ExportAssignment;
                    self.graph.entries[child].parent = Some(index);
                }
            }
            A::TSEnumDeclaration(n) => self.graph.entries[index].name = Some(id(&n.id)),
            A::TSEnumMember(n) => self.graph.entries[index].name = Some(id(&n.id)),
            A::SequenceExpression(n) => {
                self.graph.entries[index].kind = Kind::BinaryExpression;
                let mut left = id(&n.expressions[0]);
                for (i, right) in n.expressions.iter().enumerate().skip(1) {
                    let parent = if i + 1 == n.expressions.len() {
                        index
                    } else {
                        self.wrap(
                            index,
                            Span::new(n.span.start, right.span().end),
                            Kind::BinaryExpression,
                            &[],
                        )
                    };
                    let left_end = self.graph.entries[left].span.end;
                    self.graph.entries[left].parent = Some(parent);
                    self.graph.entries[id(right)].parent = Some(parent);
                    self.token(parent, left_end, right.span().start, ",", Kind::CommaToken);
                    left = parent;
                }
            }
            A::TSTypeReference(_) => self.graph.entries[index].name = None,
            A::TSTypeAliasDeclaration(n) => self.graph.entries[index].name = Some(id(&n.id)),
            A::TSTypeParameter(n) => self.graph.entries[index].name = Some(id(&n.name)),
            A::TSPropertySignature(n) => {
                self.graph.entries[index].name = Some(id(&n.key));
                if n.readonly {
                    self.token(
                        index,
                        n.span.start,
                        n.key.span().start,
                        "readonly",
                        Kind::ReadonlyKeyword,
                    );
                }
            }
            A::BindingProperty(n) => {
                let binding = match &n.value {
                    BindingPattern::AssignmentPattern(p) => id(&p.left),
                    _ => id(&n.value),
                };
                self.graph.entries[index].name = Some(binding);
                if n.key.span() == self.graph.entries[binding].span {
                    self.graph.entries[id(&n.key)].kind = Kind::Unknown;
                }
            }
            A::ImportSpecifier(n) => {
                self.graph.entries[index].name = Some(id(&n.local));
                if n.imported.span() == n.local.span {
                    self.graph.entries[id(&n.imported)].kind = Kind::Unknown;
                }
            }
            A::JSXAttribute(n) => self.graph.entries[index].name = Some(id(&n.name)),
            A::MethodDefinition(n) if n.kind == MethodDefinitionKind::Constructor => {
                self.graph.entries[index].name = None;
                self.graph.entries[id(&n.key)].kind = Kind::Unknown;
            }
            A::TSNullKeyword(n) => {
                self.graph.entries[index].kind = Kind::LiteralType;
                self.graph.token(index, n.span, Kind::NullKeyword);
            }
            A::TSTypePredicate(n) if n.asserts => self.token(
                index,
                n.span.start,
                n.span.end,
                "asserts",
                Kind::AssertsKeyword,
            ),
            A::Class(n) => {
                self.declaration_modifiers(
                    index,
                    n.id.as_ref().map_or(n.body.span.start, |n| n.span.start),
                    n.declare,
                    false,
                );
                if let Some(heritage) = &n.heritage {
                    let expr = &heritage.expression;
                    let end = heritage
                        .type_arguments
                        .as_ref()
                        .map_or(expr.span().end, |n| n.span.end);
                    if let Some(keyword) = graph_token(
                        &self.graph.source,
                        n.span.start as usize,
                        expr.span().start as usize,
                        "extends",
                    ) {
                        let clause = self.wrap(
                            index,
                            Span::new(keyword.start, end),
                            Kind::HeritageClause,
                            &[],
                        );
                        let mut children = vec![id(expr)];
                        if let Some(args) = &heritage.type_arguments {
                            children.extend(args.params.iter().map(id));
                        }
                        self.wrap(
                            clause,
                            Span::new(expr.span().start, end),
                            Kind::ExpressionWithTypeArguments,
                            &children,
                        );
                    }
                }
                if let (Some(first), Some(last)) = (n.implements.first(), n.implements.last())
                    && let Some(keyword) = graph_token(
                        &self.graph.source,
                        n.span.start as usize,
                        first.span.start as usize,
                        "implements",
                    )
                {
                    self.wrap(
                        index,
                        Span::new(keyword.start, last.span.end),
                        Kind::HeritageClause,
                        &n.implements.iter().map(id).collect::<Vec<_>>(),
                    );
                }
            }
            A::TSClassImplements(_) => self.graph.entries[index].kind = Kind::TypeReference,
            A::ImportDeclaration(n) => {
                if let Some(specifiers) = &n.specifiers
                    && let (Some(first), Some(last)) = (specifiers.first(), specifiers.last())
                {
                    let start = graph_token(
                        &self.graph.source,
                        n.span.start as usize,
                        first.span().start as usize,
                        "{",
                    )
                    .map_or(first.span().start, |s| s.start);
                    let end = graph_token(
                        &self.graph.source,
                        last.span().end as usize,
                        n.source.span.start as usize,
                        "}",
                    )
                    .map_or(last.span().end, |s| s.end);
                    let clause = self.wrap(
                        index,
                        Span::new(start, end),
                        Kind::ImportClause,
                        &specifiers.iter().map(id).collect::<Vec<_>>(),
                    );
                    let named = specifiers
                        .iter()
                        .filter(|s| matches!(s, ImportDeclarationSpecifier::ImportSpecifier(_)))
                        .map(id)
                        .collect::<Vec<_>>();
                    if !named.is_empty() {
                        self.wrap(clause, Span::new(start, end), Kind::NamedImports, &named);
                    }
                }
            }
            A::JSXOpeningElement(n) => {
                let start = n
                    .attributes
                    .first()
                    .map_or(n.name.span().end, |a| a.span().start);
                let end = n.attributes.last().map_or(start, |a| a.span().end);
                self.wrap(
                    index,
                    Span::new(start, end),
                    Kind::JsxAttributes,
                    &n.attributes.iter().map(id).collect::<Vec<_>>(),
                );
            }
            A::TemplateLiteral(n) if !n.expressions.is_empty() => {
                let first = &n.expressions[0];
                let head_end = graph_token(
                    &self.graph.source,
                    n.span.start as usize,
                    first.span().start as usize,
                    "${",
                )
                .unwrap()
                .end;
                self.graph
                    .token(index, Span::new(n.span.start, head_end), Kind::TemplateHead);
                for (i, expr) in n.expressions.iter().enumerate() {
                    let tail_start = graph_token(
                        &self.graph.source,
                        expr.span().end as usize,
                        n.span.end as usize,
                        "}",
                    )
                    .unwrap()
                    .start;
                    let tail_end = n.expressions.get(i + 1).map_or(n.span.end, |next| {
                        graph_token(
                            &self.graph.source,
                            tail_start as usize,
                            next.span().start as usize,
                            "${",
                        )
                        .unwrap()
                        .end
                    });
                    let wrapper = self.wrap(
                        index,
                        Span::new(expr.span().start, tail_end),
                        Kind::TemplateSpan,
                        &[id(expr)],
                    );
                    self.graph.token(
                        wrapper,
                        Span::new(tail_start, tail_end),
                        if i + 1 == n.expressions.len() {
                            Kind::TemplateTail
                        } else {
                            Kind::TemplateMiddle
                        },
                    );
                }
            }
            A::PropertyDefinition(n) => {
                let mut modifiers = vec![];
                if n.readonly {
                    modifiers.push(("readonly", Kind::ReadonlyKeyword));
                }
                if n.declare {
                    modifiers.push(("declare", Kind::DeclareKeyword));
                }
                if n.r#static {
                    modifiers.push(("static", Kind::StaticKeyword));
                }
                self.modifiers(index, n.key.span().start, &modifiers);
            }
            _ => {}
        }
    }
}
