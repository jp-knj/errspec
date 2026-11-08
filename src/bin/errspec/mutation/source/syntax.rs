//! Oxc syntax projected onto the existing mutation/plugin vocabulary.
//! This compatibility projection owns no arena references and never invokes tsrs parsing.
use oxc_allocator::Allocator;
use oxc_ast::{AstKind, ast::*};
use oxc_ast_visit::Visit;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::{GetSpan, SourceType, Span};
use oxc_syntax::GetNodeId;
use tsrs_ast::Kind;
mod compat;
mod modules;

#[derive(Clone)]
struct Entry {
    kind: Kind,
    span: Span,
    parent: Option<usize>,
    children: Vec<usize>,
    expression: Option<usize>,
    name: Option<usize>,
    body: Option<usize>,
    typ: Option<usize>,
    init: Option<usize>,
    left: Option<usize>,
    right: Option<usize>,
    condition: Option<usize>,
    statement: Option<usize>,
    otherwise: Option<usize>,
    operator: Option<usize>,
    optional: Option<usize>,
    args: Vec<usize>,
    op: Kind,
    value: String,
    flags: u32,
    ambient: bool,
    runtime: bool,
    asynchronous: bool,
    generator: bool,
}
impl Entry {
    fn new(kind: Kind, span: Span) -> Self {
        Self {
            kind,
            span,
            parent: None,
            children: vec![],
            expression: None,
            name: None,
            body: None,
            typ: None,
            init: None,
            left: None,
            right: None,
            condition: None,
            statement: None,
            otherwise: None,
            operator: None,
            optional: None,
            args: vec![],
            op: Kind::Unknown,
            value: String::new(),
            flags: 0,
            ambient: false,
            runtime: false,
            asynchronous: false,
            generator: false,
        }
    }
}
pub(super) struct Graph {
    file: String,
    modules: modules::Facts,
    source: String,
    entries: Vec<Entry>,
    comments: Vec<(u32, u32)>,
    redirects: Vec<(usize, usize)>,
    plugin_nodes: Vec<errspec::mutation::syntax::SyntaxNode>,
}
impl Graph {
    pub fn parse(file: &str, source: &str, facts: bool) -> Result<Self, String> {
        let allocator = Allocator::default();
        let parsed = Parser::new(
            &allocator,
            source,
            SourceType::from_path(file).map_err(|e| e.to_string())?,
        )
        .parse();
        if !parsed.diagnostics.is_empty() {
            return Err(format!("Syntax error: {:?}", parsed.diagnostics));
        }
        let semantic = SemanticBuilder::new()
            .with_build_nodes(true)
            .build(&parsed.program);
        let nodes = semantic.semantic.nodes();
        let mut graph = Self {
            file: file.into(),
            modules: modules::acquire(&parsed.program),
            source: source.into(),
            redirects: vec![],
            plugin_nodes: vec![],
            entries: nodes
                .iter()
                .map(|n| Entry::new(Kind::Unknown, n.kind().span()))
                .collect(),
            comments: parsed
                .program
                .comments
                .iter()
                .map(|c| (c.span.start, c.span.end))
                .collect(),
        };
        for n in nodes.iter().skip(1) {
            graph.entries[n.id().index()].parent = Some(nodes.parent_id(n.id()).index());
        }
        Builder { graph: &mut graph }.visit_program(&parsed.program);
        for (child, parent) in std::mem::take(&mut graph.redirects) {
            graph.entries[child].parent = Some(parent);
        }
        // Transparent ESTree wrappers have no counterpart in the public vocabulary.
        for i in 1..graph.entries.len() {
            let mut p = graph.entries[i].parent;
            while let Some(id) = p {
                if graph.entries[id].kind != Kind::Unknown {
                    break;
                }
                p = graph.entries[id].parent;
            }
            graph.entries[i].parent = p;
            if graph.entries[i].kind != Kind::Unknown
                && let Some(p) = p
            {
                graph.entries[p].children.push(i);
            }
        }
        for i in 0..graph.entries.len() {
            let mut children = std::mem::take(&mut graph.entries[i].children);
            children.sort_by_key(|&n| graph.entries[n].span.start);
            graph.entries[i].children = children;
        }
        if facts {
            graph.plugin_nodes = compat::nodes(&graph, &parsed.program);
        }
        Ok(graph)
    }
    pub fn root(&self) -> Node<'_> {
        Node { graph: self, id: 0 }
    }
    fn token(&mut self, parent: usize, span: Span, kind: Kind) -> usize {
        let id = self.entries.len();
        let mut e = Entry::new(kind, span);
        e.parent = Some(parent);
        e.value = self.source[span.start as usize..span.end as usize].into();
        self.entries.push(e);
        id
    }
}
struct Builder<'b> {
    graph: &'b mut Graph,
}
fn id(n: &impl GetNodeId) -> usize {
    n.node_id().index()
}
fn ty(n: &Option<oxc_allocator::Box<'_, TSTypeAnnotation<'_>>>) -> Option<usize> {
    n.as_ref().map(|v| id(&v.type_annotation))
}
impl Builder<'_> {
    fn binary(
        &mut self,
        e: &mut Entry,
        parent: usize,
        left: usize,
        right: usize,
        op: &'static str,
    ) {
        e.kind = Kind::BinaryExpression;
        e.left = Some(left);
        e.right = Some(right);
        let a = self.graph.entries[left].span.end as usize;
        let b = self.graph.entries[right].span.start as usize;
        if let Some(span) = graph_token(&self.graph.source, a, b, op) {
            e.operator = Some(self.graph.token(parent, span, token_kind(op)));
        }
    }
    fn optional(&mut self, e: &mut Entry, parent: usize, start: u32, end: u32) {
        if let Some(span) = graph_token(&self.graph.source, start as usize, end as usize, "?.") {
            e.optional = Some(self.graph.token(parent, span, Kind::QuestionDotToken));
        }
    }
    fn computed(&mut self, e: &mut Entry, parent: usize, computed: bool) {
        if !computed {
            return;
        }
        let Some(key) = e.name else {
            return;
        };
        let key_span = self.graph.entries[key].span;
        let mut start = e.span.start as usize;
        let mut bracket = None;
        while let Some(span) = graph_token(&self.graph.source, start, key_span.start as usize, "[")
        {
            bracket = Some(span.start);
            start = span.end as usize;
        }
        if let (Some(start), Some(end)) = (
            bracket,
            graph_token(
                &self.graph.source,
                key_span.end as usize,
                e.span.end as usize,
                "]",
            ),
        ) {
            let wrapper = self.graph.token(
                parent,
                Span::new(start, end.end),
                Kind::ComputedPropertyName,
            );
            self.graph.entries[wrapper].expression = Some(key);
            self.graph.redirects.push((key, wrapper));
            e.name = Some(wrapper);
        }
    }
    fn function(e: &mut Entry, n: &Function<'_>) {
        e.name = n.id.as_ref().map(id);
        e.body = n.body.as_ref().map(|n| id(n.as_ref()));
        e.typ = ty(&n.return_type);
        e.ambient = n.declare;
        e.asynchronous = n.r#async;
        e.generator = n.generator;
    }
}
impl<'a> Visit<'a> for Builder<'_> {
    fn visit_expression(&mut self, expression: &Expression<'a>) {
        self.graph.entries[id(expression)].runtime = true;
        oxc_ast_visit::walk::walk_expression(self, expression);
    }

    fn enter_node(&mut self, k: AstKind<'a>) {
        use AstKind as A;
        let index = k.node_id().index();
        let mut e = self.graph.entries[index].clone();
        e.kind = kind(k);
        match k {
            A::ImportDeclaration(n) => {
                e.flags = u32::from(n.import_kind == ImportOrExportKind::Type)
                    | n.phase.map_or(0, |phase| ((phase as u32) + 1) << 1);
            }
            A::ImportSpecifier(n) => e.flags = u32::from(n.import_kind == ImportOrExportKind::Type),
            A::ExportNamedDeclaration(n) => {
                e.flags = u32::from(n.export_kind == ImportOrExportKind::Type)
            }
            A::ExportFromDeclaration(n) => {
                e.flags = u32::from(n.export_kind == ImportOrExportKind::Type)
            }
            A::ExportAllDeclaration(n) => {
                e.flags = u32::from(n.export_kind == ImportOrExportKind::Type)
            }
            A::ExportSpecifier(n) => e.flags = u32::from(n.export_kind == ImportOrExportKind::Type),
            A::IdentifierName(n) => e.value = n.name.to_string(),
            A::IdentifierReference(n) => e.value = n.name.to_string(),
            A::BindingIdentifier(n) => e.value = n.name.to_string(),
            A::StringLiteral(n) => e.value = n.value.display().to_string(),
            A::TemplateLiteral(n) => {
                e.kind = if n.expressions.is_empty() {
                    Kind::NoSubstitutionTemplateLiteral
                } else {
                    Kind::TemplateExpression
                };
                if n.expressions.is_empty() {
                    e.value = n
                        .quasis
                        .first()
                        .and_then(|q| q.value.cooked.as_ref())
                        .map(|v| v.display().to_string())
                        .unwrap_or_default();
                }
            }
            A::BooleanLiteral(n) => {
                e.kind = if n.value {
                    Kind::TrueKeyword
                } else {
                    Kind::FalseKeyword
                }
            }
            A::BinaryExpression(n) => self.binary(
                &mut e,
                index,
                id(&n.left),
                id(&n.right),
                n.operator.as_str(),
            ),
            A::LogicalExpression(n) => self.binary(
                &mut e,
                index,
                id(&n.left),
                id(&n.right),
                n.operator.as_str(),
            ),
            A::AssignmentExpression(n) => self.binary(
                &mut e,
                index,
                id(&n.left),
                id(&n.right),
                n.operator.as_str(),
            ),
            A::UnaryExpression(n) => {
                e.expression = Some(id(&n.argument));
                e.op = operator(n.operator.as_str());
                e.kind = match n.operator.as_str() {
                    "delete" => Kind::DeleteExpression,
                    "typeof" => Kind::TypeOfExpression,
                    "void" => Kind::VoidExpression,
                    _ => Kind::PrefixUnaryExpression,
                };
            }
            A::UpdateExpression(n) => {
                e.expression = Some(id(&n.argument));
                e.op = operator(n.operator.as_str());
                e.kind = if n.prefix {
                    Kind::PrefixUnaryExpression
                } else {
                    Kind::PostfixUnaryExpression
                };
            }
            A::ImportExpression(n) => {
                e.kind = Kind::CallExpression;
                e.expression = Some(self.graph.token(
                    index,
                    Span::new(n.span.start, n.span.start + 6),
                    Kind::ImportKeyword,
                ));
                e.args = vec![id(&n.source)];
                if let Some(options) = &n.options {
                    e.args.push(id(options));
                }
            }
            A::ExpressionStatement(n) => e.expression = Some(id(&n.expression)),
            A::Directive(n) => e.expression = Some(id(&n.expression)),
            A::ParenthesizedExpression(n) => e.expression = Some(id(&n.expression)),
            A::AwaitExpression(n) => e.expression = Some(id(&n.argument)),
            A::TSNonNullExpression(n) => e.expression = Some(id(&n.expression)),
            A::TSAsExpression(n) => e.expression = Some(id(&n.expression)),
            A::TSSatisfiesExpression(n) => e.expression = Some(id(&n.expression)),
            A::TSTypeAssertion(n) => e.expression = Some(id(&n.expression)),
            A::ChainExpression(n) => e.expression = Some(id(&n.expression)),
            A::ReturnStatement(n) => e.expression = n.argument.as_ref().map(id),
            A::ThrowStatement(n) => e.expression = Some(id(&n.argument)),
            A::IfStatement(n) => {
                e.expression = Some(id(&n.test));
                e.statement = Some(id(&n.consequent));
                e.otherwise = n.alternate.as_ref().map(id);
            }
            A::ConditionalExpression(n) => e.condition = Some(id(&n.test)),
            A::WhileStatement(n) => e.expression = Some(id(&n.test)),
            A::DoWhileStatement(n) => e.expression = Some(id(&n.test)),
            A::ForStatement(n) => {
                e.condition = n.test.as_ref().map(id);
                e.init = n.init.as_ref().map(id);
                e.statement = Some(id(&n.body));
            }
            A::VariableDeclaration(n) => {
                e.ambient = n.declare;
                e.flags |= n.kind as u32;
            }
            A::VariableDeclarator(n) => {
                e.name = Some(id(&n.id));
                e.init = n.init.as_ref().map(id);
                e.typ = ty(&n.type_annotation);
            }
            A::FormalParameter(n) => {
                e.name = Some(id(&n.pattern));
                e.typ = ty(&n.type_annotation);
                e.init = n.initializer.as_ref().map(|n| id(n.as_ref()));
            }
            A::Function(n) => {
                Self::function(&mut e, n);
                e.kind = if matches!(
                    n.r#type,
                    FunctionType::FunctionDeclaration | FunctionType::TSDeclareFunction
                ) {
                    Kind::FunctionDeclaration
                } else {
                    Kind::FunctionExpression
                };
                if e.parent.is_some_and(|p| {
                    matches!(
                        self.graph.entries[p].kind,
                        Kind::MethodDeclaration
                            | Kind::Constructor
                            | Kind::GetAccessor
                            | Kind::SetAccessor
                    )
                }) {
                    e.kind = Kind::Unknown;
                }
            }
            A::ArrowFunctionExpression(n) => {
                e.body = Some(id(&n.body));
                e.typ = ty(&n.return_type);
                e.asynchronous = n.r#async;
            }
            A::TSNamespaceDeclaration(n) => {
                e.ambient = n.declare;
                e.name = Some(id(&n.id));
            }
            A::TSExternalModuleDeclaration(n) => {
                e.ambient = n.declare;
                e.name = Some(id(&n.id));
            }
            A::TSGlobalDeclaration(n) => {
                e.kind = Kind::ModuleDeclaration;
                e.ambient = n.declare;
            }
            A::StaticBlock(n) => {
                e.kind = Kind::ClassStaticBlockDeclaration;
                if let Some(open) = graph_token(
                    &self.graph.source,
                    n.span.start as usize,
                    n.span.end as usize,
                    "{",
                ) {
                    let block =
                        self.graph
                            .token(index, Span::new(open.start, n.span.end), Kind::Block);
                    e.body = Some(block);
                    for statement in &n.body {
                        self.graph.redirects.push((id(statement), block));
                    }
                }
            }
            A::AssignmentTargetPropertyProperty(n) => {
                e.name = Some(id(&n.name));
                self.computed(&mut e, index, n.computed);
            }
            A::AccessorProperty(n) => {
                e.kind = Kind::PropertyDeclaration;
                e.name = Some(id(&n.key));
                e.init = n.value.as_ref().map(id);
                e.typ = ty(&n.type_annotation);
                e.flags |= u32::from(n.r#static) << 18;
                self.computed(&mut e, index, n.computed);
            }
            A::MethodDefinition(n) => {
                e.flags |= u32::from(n.r#static) << 18;
                Self::function(&mut e, &n.value);
                e.name = Some(id(&n.key));
                e.kind = match n.kind {
                    MethodDefinitionKind::Constructor => Kind::Constructor,
                    MethodDefinitionKind::Get => Kind::GetAccessor,
                    MethodDefinitionKind::Set => Kind::SetAccessor,
                    _ => Kind::MethodDeclaration,
                };
                self.computed(&mut e, index, n.computed);
            }
            A::ObjectProperty(n) => {
                e.name = Some(id(&n.key));
                e.init = Some(id(&n.value));
                if (n.method || n.kind != PropertyKind::Init)
                    && let Expression::FunctionExpression(f) = &n.value
                {
                    Self::function(&mut e, f);
                    e.name = Some(id(&n.key));
                    e.kind = match n.kind {
                        PropertyKind::Get => Kind::GetAccessor,
                        PropertyKind::Set => Kind::SetAccessor,
                        _ => Kind::MethodDeclaration,
                    };
                }
                self.computed(&mut e, index, n.computed);
            }
            A::PropertyDefinition(n) => {
                e.flags |= u32::from(n.r#static) << 18;
                e.name = Some(id(&n.key));
                e.init = n.value.as_ref().map(id);
                e.typ = ty(&n.type_annotation);
                e.ambient = n.declare;
                self.computed(&mut e, index, n.computed);
            }
            A::Class(n) => {
                e.name = n.id.as_ref().map(id);
                e.ambient = n.declare;
                e.kind = if n.r#type == ClassType::ClassDeclaration {
                    Kind::ClassDeclaration
                } else {
                    Kind::ClassExpression
                };
            }
            A::CallExpression(n) => {
                e.expression = Some(id(&n.callee));
                e.args = n.arguments.iter().map(id).collect();
                if n.optional {
                    self.optional(&mut e, index, n.callee.span().end, n.span.end);
                }
            }
            A::NewExpression(n) => {
                e.expression = Some(id(&n.callee));
                e.args = n.arguments.iter().map(id).collect();
            }
            A::StaticMemberExpression(n) => {
                e.expression = Some(id(&n.object));
                e.name = Some(id(&n.property));
                if n.optional {
                    self.optional(&mut e, index, n.object.span().end, n.property.span.start);
                }
            }
            A::ComputedMemberExpression(n) => {
                e.expression = Some(id(&n.object));
                if n.optional {
                    self.optional(
                        &mut e,
                        index,
                        n.object.span().end,
                        n.expression.span().start,
                    );
                }
            }
            A::TSTypeReference(n) => {
                e.name = Some(id(&n.type_name));
                e.args = n
                    .type_arguments
                    .as_ref()
                    .map(|a| a.params.iter().map(id).collect())
                    .unwrap_or_default();
            }
            A::TSTypeAnnotation(n) => e.typ = Some(id(&n.type_annotation)),
            A::TSParenthesizedType(n) => e.typ = Some(id(&n.type_annotation)),
            A::TSTypeOperator(n) => {
                e.typ = Some(id(&n.type_annotation));
                e.op = if n.operator == TSTypeOperatorOperator::Readonly {
                    Kind::ReadonlyKeyword
                } else {
                    Kind::Unknown
                };
            }
            A::TSLiteralType(n) => e.expression = Some(id(&n.literal)),
            A::TSTypePredicate(n) => e.flags = u32::from(n.asserts),
            _ => {}
        }
        if matches!(
            e.kind,
            Kind::FunctionDeclaration
                | Kind::FunctionExpression
                | Kind::VariableStatement
                | Kind::ClassDeclaration
                | Kind::ClassExpression
                | Kind::EnumDeclaration
                | Kind::ModuleDeclaration
        ) && let Some(p) = e.parent
            && matches!(self.graph.entries[p].kind, Kind::Unknown)
            && self.graph.source[self.graph.entries[p].span.start as usize..e.span.start as usize]
                .trim_start()
                .starts_with("export")
        {
            let prefix = &self.graph.source
                [self.graph.entries[p].span.start as usize..e.span.start as usize];
            e.flags |= 1 << 16;
            if prefix.contains("default") {
                e.flags |= 1 << 17;
            }
            e.span.start = self.graph.entries[p].span.start;
        }
        self.graph.entries[index] = e;
    }
}

/// Locate punctuation between AST operands, skipping actual comment text and trivia.
pub(super) fn graph_token(source: &str, mut start: usize, end: usize, token: &str) -> Option<Span> {
    while start < end {
        let s = &source[start..end];
        if s.starts_with("//") {
            start += s.find('\n').unwrap_or(s.len());
            continue;
        }
        if s.starts_with("/*") {
            start += s.find("*/").map(|i| i + 2).unwrap_or(s.len());
            continue;
        }
        if s.starts_with(token) {
            return Some(Span::new(start as u32, (start + token.len()) as u32));
        }
        start += s.chars().next()?.len_utf8();
    }
    None
}
fn operator(s: &str) -> Kind {
    match s {
        "!" => Kind::ExclamationToken,
        "+" => Kind::PlusToken,
        "-" => Kind::MinusToken,
        "~" => Kind::TildeToken,
        "++" => Kind::PlusPlusToken,
        "--" => Kind::MinusMinusToken,
        _ => Kind::Unknown,
    }
}

#[derive(Clone, Copy)]
pub(super) struct Node<'a> {
    graph: &'a Graph,
    id: usize,
}
impl PartialEq for Node<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && std::ptr::eq(self.graph, other.graph)
    }
}
impl Eq for Node<'_> {}
impl std::hash::Hash for Node<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::ptr::hash(self.graph, state);
        std::hash::Hash::hash(&self.id, state);
    }
}

impl<'a> Node<'a> {
    pub fn file_name(self) -> &'a str {
        &self.graph.file
    }
    pub fn imports(self) -> &'a [errspec::mutation::syntax::Import] {
        &self.graph.modules.imports
    }
    pub fn dependencies(self) -> &'a [String] {
        &self.graph.modules.dependencies
    }
    pub fn walk(self, visitor: &mut impl FnMut(Self)) {
        visitor(self);
        for child in self.children() {
            child.walk(visitor);
        }
    }
    pub fn is_expression(self) -> bool {
        self.e().runtime
    }
    pub fn is_runtime_statement(self) -> bool {
        matches!(
            self.kind(),
            Kind::EmptyStatement
                | Kind::VariableStatement
                | Kind::ExpressionStatement
                | Kind::IfStatement
                | Kind::DoStatement
                | Kind::WhileStatement
                | Kind::ForStatement
                | Kind::ForInStatement
                | Kind::ForOfStatement
                | Kind::ContinueStatement
                | Kind::BreakStatement
                | Kind::ReturnStatement
                | Kind::WithStatement
                | Kind::SwitchStatement
                | Kind::LabeledStatement
                | Kind::ThrowStatement
                | Kind::TryStatement
                | Kind::DebuggerStatement
                | Kind::NotEmittedStatement
        )
    }
    pub fn is_statement(self) -> bool {
        self.is_runtime_statement()
            || matches!(
                self.kind(),
                Kind::VariableStatement
                    | Kind::FunctionDeclaration
                    | Kind::ClassDeclaration
                    | Kind::InterfaceDeclaration
                    | Kind::TypeAliasDeclaration
                    | Kind::EnumDeclaration
                    | Kind::ModuleDeclaration
                    | Kind::ImportDeclaration
                    | Kind::ImportEqualsDeclaration
                    | Kind::ExportDeclaration
                    | Kind::ExportAssignment
                    | Kind::NamespaceExportDeclaration
            )
            || self.kind() == Kind::Block
                && self.parent().is_none_or(|p| {
                    !p.is_function() && !matches!(p.kind(), Kind::TryStatement | Kind::CatchClause)
                })
    }

    pub fn plugin_nodes(self) -> Vec<errspec::mutation::syntax::SyntaxNode> {
        self.graph.plugin_nodes.clone()
    }
    fn e(self) -> &'a Entry {
        &self.graph.entries[self.id]
    }
    fn get(self, id: usize) -> Self {
        let n = Self {
            graph: self.graph,
            id,
        };
        if n.kind() == Kind::Unknown
            && let Some(i) = n.e().expression.or(n.e().typ)
        {
            return n.get(i);
        }
        n
    }
    fn field(self, id: Option<usize>) -> Option<Self> {
        id.map(|id| self.get(id))
    }
    pub fn kind(self) -> Kind {
        self.e().kind
    }
    pub fn start(self) -> usize {
        self.e().span.start as usize
    }
    pub fn end_offset(self) -> usize {
        self.e().span.end as usize
    }
    pub fn leading_start(self) -> usize {
        let mut start = self.start();
        for &(a, b) in self.graph.comments.iter().rev() {
            if b as usize <= start && self.graph.source[b as usize..start].trim().is_empty() {
                start = a as usize;
            }
        }
        start
    }
    pub fn as_node(self) -> Self {
        self
    }
    pub fn text(self) -> &'a str {
        if self.id == 0 {
            &self.graph.source
        } else {
            &self.e().value
        }
    }
    pub fn comments(self) -> &'a [(u32, u32)] {
        &self.graph.comments
    }
    pub fn parent(self) -> Option<Self> {
        self.field(self.e().parent)
    }
    pub fn children(self) -> Vec<Self> {
        self.e().children.iter().map(|&id| self.get(id)).collect()
    }
    pub fn for_each_child(self, f: &mut impl FnMut(Self) -> bool) {
        for n in self.children() {
            if f(n) {
                break;
            }
        }
    }
    pub fn statements(self) -> Vec<Self> {
        self.children()
    }
    pub fn properties(self) -> Vec<Self> {
        self.children()
    }
    pub fn elements(self) -> Vec<Self> {
        self.children()
    }
    pub fn arguments(self) -> Vec<Self> {
        self.e().args.iter().map(|&id| self.get(id)).collect()
    }
    pub fn type_arguments(self) -> Vec<Self> {
        self.arguments()
    }
    pub fn expression(self) -> Option<Self> {
        self.field(self.e().expression)
    }
    pub fn name(self) -> Option<Self> {
        self.field(self.e().name)
    }
    pub fn body(self) -> Option<Self> {
        self.field(self.e().body)
    }
    pub fn initializer(self) -> Option<Self> {
        self.field(self.e().init)
    }
    pub fn type_node(self) -> Option<Self> {
        self.field(self.e().typ)
    }
    pub fn left(self) -> Self {
        self.get(self.e().left.unwrap())
    }
    pub fn right(self) -> Self {
        self.get(self.e().right.unwrap())
    }
    pub fn operator_token(self) -> Self {
        self.get(self.e().operator.unwrap())
    }
    pub fn question_dot_token(self) -> Option<Self> {
        self.field(self.e().optional)
    }
    pub fn operand(self) -> Self {
        self.expression().unwrap()
    }
    pub fn operator(self) -> Kind {
        self.e().op
    }
    pub fn token(self) -> Kind {
        self.e().op
    }
    pub fn condition(self) -> Option<Self> {
        self.field(self.e().condition)
    }
    pub fn statement(self) -> Self {
        self.get(self.e().statement.unwrap())
    }
    pub fn then_statement(self) -> Self {
        self.statement()
    }
    pub fn else_statement(self) -> Option<Self> {
        self.field(self.e().otherwise)
    }
    pub fn literal(self) -> Self {
        self.expression().unwrap()
    }
    pub fn type_name(self) -> Self {
        self.name().unwrap()
    }
    pub fn asserts_modifier(self) -> Option<Self> {
        (self.e().flags != 0).then_some(self)
    }
    pub fn ambient(self) -> bool {
        self.e().ambient
    }
    pub fn flags(self) -> u32 {
        self.e().flags | u32::from(self.e().asynchronous) << 8 | u32::from(self.e().generator) << 9
    }
    pub fn generator(self) -> bool {
        self.e().generator
    }
    pub fn is_async(self) -> bool {
        self.e().asynchronous
    }
    pub fn is_function(self) -> bool {
        matches!(
            self.kind(),
            Kind::FunctionDeclaration
                | Kind::FunctionExpression
                | Kind::MethodDeclaration
                | Kind::ArrowFunction
                | Kind::Constructor
                | Kind::GetAccessor
                | Kind::SetAccessor
        )
    }
}
pub(super) struct Conditional<'a>(Node<'a>);
impl<'a> Node<'a> {
    pub fn as_conditional_expression(self) -> Conditional<'a> {
        Conditional(self)
    }
}
impl<'a> Conditional<'a> {
    pub fn condition(self) -> Node<'a> {
        self.0.condition().unwrap()
    }
}

fn kind(k: AstKind<'_>) -> Kind {
    match k {
        AstKind::Program(..) => Kind::SourceFile,
        AstKind::IdentifierName(..) => Kind::Identifier,
        AstKind::IdentifierReference(..) => Kind::Identifier,
        AstKind::BindingIdentifier(..) => Kind::Identifier,
        AstKind::LabelIdentifier(..) => Kind::Identifier,
        AstKind::ThisExpression(..) => Kind::ThisKeyword,
        AstKind::ArrayExpression(..) => Kind::ArrayLiteralExpression,
        AstKind::Elision(..) => Kind::OmittedExpression,
        AstKind::ObjectExpression(..) => Kind::ObjectLiteralExpression,
        AstKind::ObjectProperty(..) => Kind::PropertyAssignment,
        AstKind::TemplateLiteral(..) => Kind::Unknown,
        AstKind::TaggedTemplateExpression(..) => Kind::TaggedTemplateExpression,
        AstKind::TemplateElement(..) => Kind::Unknown,
        AstKind::ComputedMemberExpression(..) => Kind::ElementAccessExpression,
        AstKind::StaticMemberExpression(..) => Kind::PropertyAccessExpression,
        AstKind::PrivateFieldExpression(..) => Kind::PropertyAccessExpression,
        AstKind::CallExpression(..) => Kind::CallExpression,
        AstKind::NewExpression(..) => Kind::NewExpression,
        AstKind::ImportMeta(..) => Kind::MetaProperty,
        AstKind::NewTarget(..) => Kind::MetaProperty,
        AstKind::SpreadElement(..) => Kind::SpreadElement,
        AstKind::UpdateExpression(..) => Kind::Unknown,
        AstKind::UnaryExpression(..) => Kind::Unknown,
        AstKind::BinaryExpression(..) => Kind::BinaryExpression,
        AstKind::PrivateInExpression(..) => Kind::Unknown,
        AstKind::LogicalExpression(..) => Kind::Unknown,
        AstKind::ConditionalExpression(..) => Kind::ConditionalExpression,
        AstKind::AssignmentExpression(..) => Kind::Unknown,
        AstKind::ArrayAssignmentTarget(..) => Kind::ArrayLiteralExpression,
        AstKind::ObjectAssignmentTarget(..) => Kind::ObjectLiteralExpression,
        AstKind::AssignmentTargetRest(..) => Kind::Unknown,
        AstKind::AssignmentTargetWithDefault(..) => Kind::Unknown,
        AstKind::AssignmentTargetPropertyIdentifier(..) => Kind::ShorthandPropertyAssignment,
        AstKind::AssignmentTargetPropertyProperty(..) => Kind::PropertyAssignment,
        AstKind::SequenceExpression(..) => Kind::Unknown,
        AstKind::Super(..) => Kind::SuperKeyword,
        AstKind::AwaitExpression(..) => Kind::AwaitExpression,
        AstKind::ChainExpression(..) => Kind::Unknown,
        AstKind::ParenthesizedExpression(..) => Kind::ParenthesizedExpression,
        AstKind::Directive(..) => Kind::ExpressionStatement,
        AstKind::Hashbang(..) => Kind::Unknown,
        AstKind::BlockStatement(..) => Kind::Block,
        AstKind::VariableDeclaration(..) => Kind::VariableStatement,
        AstKind::VariableDeclarator(..) => Kind::VariableDeclaration,
        AstKind::EmptyStatement(..) => Kind::EmptyStatement,
        AstKind::ExpressionStatement(..) => Kind::ExpressionStatement,
        AstKind::IfStatement(..) => Kind::IfStatement,
        AstKind::DoWhileStatement(..) => Kind::DoStatement,
        AstKind::WhileStatement(..) => Kind::WhileStatement,
        AstKind::ForStatement(..) => Kind::ForStatement,
        AstKind::ForInStatement(..) => Kind::ForInStatement,
        AstKind::ForOfStatement(..) => Kind::ForOfStatement,
        AstKind::ContinueStatement(..) => Kind::ContinueStatement,
        AstKind::BreakStatement(..) => Kind::BreakStatement,
        AstKind::ReturnStatement(..) => Kind::ReturnStatement,
        AstKind::WithStatement(..) => Kind::WithStatement,
        AstKind::SwitchStatement(..) => Kind::SwitchStatement,
        AstKind::SwitchCase(..) => Kind::Unknown,
        AstKind::LabeledStatement(..) => Kind::LabeledStatement,
        AstKind::ThrowStatement(..) => Kind::ThrowStatement,
        AstKind::TryStatement(..) => Kind::TryStatement,
        AstKind::CatchClause(..) => Kind::CatchClause,
        AstKind::CatchParameter(..) => Kind::Unknown,
        AstKind::DebuggerStatement(..) => Kind::DebuggerStatement,
        AstKind::AssignmentPattern(..) => Kind::BindingElement,
        AstKind::ObjectPattern(..) => Kind::ObjectBindingPattern,
        AstKind::BindingProperty(..) => Kind::BindingElement,
        AstKind::ArrayPattern(..) => Kind::ArrayBindingPattern,
        AstKind::BindingRestElement(..) => Kind::BindingElement,
        AstKind::Function(..) => Kind::Unknown,
        AstKind::FormalParameters(..) => Kind::Unknown,
        AstKind::FormalParameter(..) => Kind::Parameter,
        AstKind::FormalParameterRest(..) => Kind::Parameter,
        AstKind::FunctionBody(..) => Kind::Block,
        AstKind::ArrowFunctionExpression(..) => Kind::ArrowFunction,
        AstKind::YieldExpression(..) => Kind::YieldExpression,
        AstKind::Class(..) => Kind::Unknown,
        AstKind::ClassBody(..) => Kind::Unknown,
        AstKind::MethodDefinition(..) => Kind::Unknown,
        AstKind::PropertyDefinition(..) => Kind::PropertyDeclaration,
        AstKind::PrivateIdentifier(..) => Kind::PrivateIdentifier,
        AstKind::StaticBlock(..) => Kind::Unknown,
        AstKind::AccessorProperty(..) => Kind::Unknown,
        AstKind::ImportExpression(..) => Kind::Unknown,
        AstKind::ImportDeclaration(..) => Kind::ImportDeclaration,
        AstKind::ImportSpecifier(..) => Kind::ImportSpecifier,
        AstKind::ImportDefaultSpecifier(..) => Kind::ImportClause,
        AstKind::ImportNamespaceSpecifier(..) => Kind::NamespaceImport,
        AstKind::WithClause(..) => Kind::Unknown,
        AstKind::ImportAttribute(..) => Kind::ImportAttribute,
        AstKind::ExportDeclaration(..) => Kind::Unknown,
        AstKind::ExportNamedDeclaration(..) => Kind::ExportDeclaration,
        AstKind::ExportFromDeclaration(..) => Kind::ExportDeclaration,
        AstKind::ExportDefaultDeclaration(..) => Kind::Unknown,
        AstKind::ExportAllDeclaration(..) => Kind::ExportDeclaration,
        AstKind::ExportSpecifier(..) => Kind::ExportSpecifier,
        AstKind::V8IntrinsicExpression(..) => Kind::Unknown,
        AstKind::BooleanLiteral(..) => Kind::Unknown,
        AstKind::NullLiteral(..) => Kind::NullKeyword,
        AstKind::NumericLiteral(..) => Kind::NumericLiteral,
        AstKind::StringLiteral(..) => Kind::StringLiteral,
        AstKind::BigIntLiteral(..) => Kind::BigIntLiteral,
        AstKind::RegExpLiteral(..) => Kind::RegularExpressionLiteral,
        AstKind::JSXElement(..) => Kind::JsxElement,
        AstKind::JSXOpeningElement(..) => Kind::JsxOpeningElement,
        AstKind::JSXClosingElement(..) => Kind::JsxClosingElement,
        AstKind::JSXFragment(..) => Kind::JsxFragment,
        AstKind::JSXOpeningFragment(..) => Kind::JsxOpeningFragment,
        AstKind::JSXClosingFragment(..) => Kind::JsxClosingFragment,
        AstKind::JSXNamespacedName(..) => Kind::JsxNamespacedName,
        AstKind::JSXMemberExpression(..) => Kind::Unknown,
        AstKind::JSXExpressionContainer(..) => Kind::JsxExpression,
        AstKind::JSXEmptyExpression(..) => Kind::Unknown,
        AstKind::JSXAttribute(..) => Kind::JsxAttribute,
        AstKind::JSXSpreadAttribute(..) => Kind::JsxSpreadAttribute,
        AstKind::JSXIdentifier(..) => Kind::Identifier,
        AstKind::JSXSpreadChild(..) => Kind::JsxExpression,
        AstKind::JSXText(..) => Kind::JsxText,
        AstKind::TSThisParameter(..) => Kind::Unknown,
        AstKind::TSEnumDeclaration(..) => Kind::EnumDeclaration,
        AstKind::TSEnumBody(..) => Kind::Unknown,
        AstKind::TSEnumMember(..) => Kind::EnumMember,
        AstKind::TSTypeAnnotation(..) => Kind::Unknown,
        AstKind::TSLiteralType(..) => Kind::LiteralType,
        AstKind::TSConditionalType(..) => Kind::ConditionalType,
        AstKind::TSUnionType(..) => Kind::UnionType,
        AstKind::TSIntersectionType(..) => Kind::IntersectionType,
        AstKind::TSParenthesizedType(..) => Kind::ParenthesizedType,
        AstKind::TSTypeOperator(..) => Kind::TypeOperator,
        AstKind::TSArrayType(..) => Kind::ArrayType,
        AstKind::TSIndexedAccessType(..) => Kind::IndexedAccessType,
        AstKind::TSTupleType(..) => Kind::TupleType,
        AstKind::TSNamedTupleMember(..) => Kind::NamedTupleMember,
        AstKind::TSOptionalType(..) => Kind::OptionalType,
        AstKind::TSRestType(..) => Kind::RestType,
        AstKind::TSAnyKeyword(..) => Kind::AnyKeyword,
        AstKind::TSStringKeyword(..) => Kind::StringKeyword,
        AstKind::TSBooleanKeyword(..) => Kind::BooleanKeyword,
        AstKind::TSNumberKeyword(..) => Kind::NumberKeyword,
        AstKind::TSNeverKeyword(..) => Kind::NeverKeyword,
        AstKind::TSIntrinsicKeyword(..) => Kind::IntrinsicKeyword,
        AstKind::TSUnknownKeyword(..) => Kind::UnknownKeyword,
        AstKind::TSNullKeyword(..) => Kind::NullKeyword,
        AstKind::TSUndefinedKeyword(..) => Kind::UndefinedKeyword,
        AstKind::TSVoidKeyword(..) => Kind::VoidKeyword,
        AstKind::TSSymbolKeyword(..) => Kind::SymbolKeyword,
        AstKind::TSThisType(..) => Kind::ThisType,
        AstKind::TSObjectKeyword(..) => Kind::ObjectKeyword,
        AstKind::TSBigIntKeyword(..) => Kind::BigIntKeyword,
        AstKind::TSTypeReference(..) => Kind::TypeReference,
        AstKind::TSQualifiedName(..) => Kind::QualifiedName,
        AstKind::TSTypeParameterInstantiation(..) => Kind::Unknown,
        AstKind::TSTypeParameter(..) => Kind::TypeParameter,
        AstKind::TSTypeParameterDeclaration(..) => Kind::Unknown,
        AstKind::TSTypeAliasDeclaration(..) => Kind::TypeAliasDeclaration,
        AstKind::TSClassImplements(..) => Kind::HeritageClause,
        AstKind::TSInterfaceDeclaration(..) => Kind::InterfaceDeclaration,
        AstKind::TSInterfaceBody(..) => Kind::Unknown,
        AstKind::TSPropertySignature(..) => Kind::PropertySignature,
        AstKind::TSIndexSignature(..) => Kind::IndexSignature,
        AstKind::TSCallSignatureDeclaration(..) => Kind::CallSignature,
        AstKind::TSMethodSignature(..) => Kind::MethodSignature,
        AstKind::TSConstructSignatureDeclaration(..) => Kind::ConstructSignature,
        AstKind::TSIndexSignatureName(..) => Kind::Parameter,
        AstKind::TSInterfaceHeritage(..) => Kind::Unknown,
        AstKind::TSTypePredicate(..) => Kind::TypePredicate,
        AstKind::TSExternalModuleDeclaration(..) => Kind::ModuleDeclaration,
        AstKind::TSNamespaceDeclaration(..) => Kind::ModuleDeclaration,
        AstKind::TSGlobalDeclaration(..) => Kind::Unknown,
        AstKind::TSModuleBlock(..) => Kind::ModuleBlock,
        AstKind::TSTypeLiteral(..) => Kind::TypeLiteral,
        AstKind::TSInferType(..) => Kind::InferType,
        AstKind::TSTypeQuery(..) => Kind::TypeQuery,
        AstKind::TSImportType(..) => Kind::ImportType,
        AstKind::TSImportTypeQualifiedName(..) => Kind::Unknown,
        AstKind::TSFunctionType(..) => Kind::FunctionType,
        AstKind::TSConstructorType(..) => Kind::ConstructorType,
        AstKind::TSMappedType(..) => Kind::MappedType,
        AstKind::TSTemplateLiteralType(..) => Kind::TemplateLiteralType,
        AstKind::TSAsExpression(..) => Kind::AsExpression,
        AstKind::TSSatisfiesExpression(..) => Kind::SatisfiesExpression,
        AstKind::TSTypeAssertion(..) => Kind::TypeAssertionExpression,
        AstKind::TSImportEqualsDeclaration(..) => Kind::ImportEqualsDeclaration,
        AstKind::TSExternalModuleReference(..) => Kind::ExternalModuleReference,
        AstKind::TSNonNullExpression(..) => Kind::NonNullExpression,
        AstKind::Decorator(..) => Kind::Decorator,
        AstKind::TSExportAssignment(..) => Kind::ExportAssignment,
        AstKind::TSNamespaceExportDeclaration(..) => Kind::NamespaceExportDeclaration,
        AstKind::TSInstantiationExpression(..) => Kind::ExpressionWithTypeArguments,
        AstKind::JSDocNullableType(..) => Kind::JSDocNullableType,
        AstKind::JSDocNonNullableType(..) => Kind::JSDocNonNullableType,
        AstKind::JSDocUnknownType(..) => Kind::Unknown,
    }
}

fn token_kind(text: &'static str) -> Kind {
    let mut scanner = tsrs_scanner::Scanner::new();
    scanner.set_text(text);
    scanner.scan()
}
