//! Syntax-driven mutation rules. File acquisition is owned by the caller.
use crate::tsrs_adapter::line_at;
mod syntax;
use errspec::model::Location;
use errspec::mutation::{Candidate, MUTATORS, Placement};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use syntax::{Graph, Node, graph_token};
use tsrs_ast::{Kind, is_type_node_kind};
mod arid;
mod callgraph;
mod comments;
mod dependencies;
mod facts;
mod plugins;

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SourceRequest {
    pub file: String,
    pub source: String,
    #[serde(default)]
    pub validate_only: bool,
    #[serde(default)]
    pub exclude_mutators: Vec<String>,
    #[serde(default)]
    pub weak_mutation: bool,
    #[serde(default)]
    pub facts: bool,
    #[serde(default)]
    pub arid: bool,
    #[serde(default)]
    pub arid_callees: Option<Vec<String>>,
    #[serde(default)]
    pub custom_mutators: Vec<String>,
    #[serde(default)]
    plugin_edits: Vec<plugins::Edit>,
    #[serde(default)]
    plugin_ignores: Vec<plugins::Ignore>,
    #[serde(default)]
    pub plugin_arid_callees: Vec<String>,
}
impl SourceRequest {
    pub(crate) fn for_facts(file: String, source: String) -> Self {
        Self {
            file,
            source,
            facts: true,
            ..Self::default()
        }
    }
}
#[derive(Serialize)]
pub(crate) struct SourceResult {
    pub candidates: Vec<Candidate>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub critical: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) metadata: Option<errspec::mutation::syntax::Metadata>,
}

pub(crate) fn generate(request: SourceRequest) -> Result<SourceResult, String> {
    for name in &request.exclude_mutators {
        if !MUTATORS.contains(&name.as_str()) && !request.custom_mutators.contains(name) {
            return Err(format!("Unknown mutator: {name}"));
        }
    }
    let graph = Graph::parse(
        &request.file,
        &request.source,
        request.facts || !request.plugin_edits.is_empty() || !request.plugin_ignores.is_empty(),
    )?;
    let file = graph.root();
    plugins::validate(
        file,
        &request.custom_mutators,
        &request.plugin_edits,
        &request.plugin_ignores,
    )?;
    let arid_callees = if request.plugin_arid_callees.is_empty() {
        request.arid_callees
    } else {
        let mut patterns = request.arid_callees.unwrap_or_else(|| {
            errspec::mutation::exclusions::ARID_CALLEES
                .iter()
                .map(|name| (*name).into())
                .collect()
        });
        patterns.extend(request.plugin_arid_callees);
        Some(patterns)
    };
    let mut rules = Rules {
        file,
        name: &request.file,
        excluded: &request.exclude_mutators,
        candidates: vec![],
        owner_counts: std::collections::BTreeMap::new(),
        critical: BTreeSet::new(),
        in_critical: false,
        disabled: errspec::mutation::exclusions::Disabled::new(
            file.text(),
            comments::acquire(file),
        ),
        arid: arid::Arid::new(file, request.arid, arid_callees)?,
        plugin_edits: &request.plugin_edits,
        applied_plugin_edits: BTreeSet::new(),
        weak_mutation: request.weak_mutation,
        facts: request.facts,
        owners: std::collections::BTreeMap::new(),
        scopes: std::collections::BTreeMap::from([("module".into(), file.as_node())]),
        parents: std::collections::BTreeMap::new(),
        calls: vec![],
    };
    if !request.validate_only {
        rules.visit(file.as_node(), "module", &[]);
        for (index, edit) in request.plugin_edits.iter().enumerate() {
            if !rules.applied_plugin_edits.contains(&index) {
                return Err(format!(
                    "Plugin mutator {} anchor is not runtime syntax",
                    edit.mutator
                ));
            }
        }
    }
    rules.candidates.sort_by_key(|c| {
        (
            c.location.byte_offset,
            MUTATORS
                .iter()
                .position(|m| *m == c.mutator)
                .unwrap_or(MUTATORS.len()),
            (!MUTATORS.contains(&c.mutator.as_str()))
                .then(|| (c.mutator.clone(), c.replacement.clone())),
        )
    });
    let mut seen = BTreeSet::new();
    rules
        .candidates
        .retain(|c| seen.insert((c.location.byte_offset, c.end, c.replacement.clone())));
    for candidate in &mut rules.candidates {
        if candidate.ignored.is_none() {
            candidate.ignored = request
                .plugin_ignores
                .iter()
                .find(|ignore| {
                    ignore.start <= candidate.location.byte_offset as usize
                        && candidate.end as usize <= ignore.end
                })
                .map(|ignore| ignore.reason.clone());
        }
    }
    let metadata = request.facts.then(|| facts::collect(&rules));
    Ok(SourceResult {
        metadata,
        critical: rules
            .candidates
            .iter()
            .filter(|c| rules.critical.contains(&c.key))
            .map(|c| c.key.clone())
            .collect(),
        candidates: rules.candidates,
    })
}

struct Rules<'a> {
    file: Node<'a>,
    name: &'a str,
    excluded: &'a [String],
    candidates: Vec<Candidate>,
    owner_counts: std::collections::BTreeMap<String, usize>,
    critical: BTreeSet<String>,
    in_critical: bool,
    disabled: errspec::mutation::exclusions::Disabled,
    arid: arid::Arid<'a>,
    weak_mutation: bool,
    facts: bool,
    owners: std::collections::BTreeMap<String, String>,
    scopes: std::collections::BTreeMap<String, Node<'a>>,
    parents: std::collections::BTreeMap<String, String>,
    calls: Vec<errspec::mutation::syntax::Call>,
    plugin_edits: &'a [plugins::Edit],
    applied_plugin_edits: BTreeSet<usize>,
}
impl<'a> Rules<'a> {
    fn start(&self, node: Node<'a>) -> usize {
        node.start()
    }
    fn end(&self, node: Node<'a>) -> usize {
        node.end_offset()
    }
    fn raw(&self, node: Node<'a>) -> &str {
        self.file
            .text()
            .get(self.start(node)..self.end(node))
            .unwrap_or("")
    }
    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        node: Node<'a>,
        family: &str,
        replacement: String,
        placement: Placement,
        owner: &str,
        path: &[usize],
        ignored: Option<String>,
        regex: Option<(String, String)>,
    ) {
        if self.excluded.iter().any(|s| s == family) || self.raw(node) == replacement {
            return;
        }
        let start = self.start(node) as u32;
        let end = self.end(node) as u32;
        let identity =
            serde_json::to_vec(&("oxc-v1", self.name, owner, path, family, &replacement)).unwrap();
        let key = format!("{:x}", Sha256::digest(identity));
        if self.facts {
            self.owners.insert(key.clone(), owner.into());
        }
        if self.in_critical {
            self.critical.insert(key.clone());
        }
        let guard_offset = if placement == Placement::Body {
            let mut offset = start + 1;
            for statement in node.statements() {
                if statement.kind() == Kind::ExpressionStatement
                    && statement
                        .expression()
                        .is_some_and(|n| n.kind() == Kind::StringLiteral)
                {
                    offset = self.end(statement) as u32;
                } else {
                    break;
                }
            }
            Some(offset)
        } else {
            None
        };
        self.candidates.push(Candidate {
            key,
            location: Location {
                file: self.name.into(),
                byte_offset: start,
                line: line_at(self.file.text(), start as usize),
            },
            end,
            mutator: family.into(),
            original: self.raw(node).into(),
            replacement,
            placement,
            ignored: ignored
                .or_else(|| {
                    self.disabled
                        .reason(self.file.text(), family, start as usize)
                })
                .or_else(|| {
                    self.arid
                        .ignores(node)
                        .then(|| "arid: logging-only code".into())
                }),
            regex,
            guard_offset,
            weak: None,
        });
    }
    fn expression(
        &mut self,
        node: Node<'a>,
        family: &str,
        value: String,
        owner: &str,
        path: &[usize],
    ) {
        // Choosing an optional member before a call would discard the receiver reference.
        let mut target = node;
        let mut value = value;
        if family == "OptionalChaining" {
            while let Some(parent) = target.parent() {
                if (matches!(
                    parent.kind(),
                    Kind::CallExpression
                        | Kind::PropertyAccessExpression
                        | Kind::ElementAccessExpression
                ) && parent.expression() == Some(target))
                    || parent.kind() == Kind::NonNullExpression
                {
                    value = self.splice(parent, target, &value);
                    target = parent;
                } else {
                    break;
                }
            }
        }
        let mut unsafe_reason = None;
        if let Some(parent) = target.parent() {
            let lhs = parent.kind() == Kind::BinaryExpression
                && parent.as_binary_expression().left() == target
                && self
                    .raw(parent.as_binary_expression().operator_token())
                    .ends_with('=')
                && !matches!(
                    self.raw(parent.as_binary_expression().operator_token()),
                    "==" | "===" | "!=" | "!==" | "<=" | ">="
                );
            if lhs
                || matches!(
                    parent.kind(),
                    Kind::DeleteExpression
                        | Kind::PrefixUnaryExpression
                        | Kind::PostfixUnaryExpression
                ) && family == "OptionalChaining"
            {
                unsafe_reason =
                    Some("Mutation would change an assignment or delete reference".into());
            }
        }
        let weak = if self.weak_mutation
            && target.kind() == Kind::BinaryExpression
            && matches!(family, "EqualityOperator" | "ArithmeticOperator")
        {
            let binary = target.as_binary_expression();
            let operator = binary.operator_token();
            let prefix = self.start(operator) - self.start(target);
            let suffix = self.end(target) - self.end(operator);
            value
                .len()
                .checked_sub(suffix)
                .and_then(|end| value.get(prefix..end))
                .map(|replacement| errspec::mutation::WeakBinary {
                    operator: self.raw(operator).into(),
                    replacement_operator: replacement.into(),
                    left_end: self.end(binary.left()) as u32,
                    right_start: self.start(binary.right()) as u32,
                })
        } else {
            None
        };
        let count = self.candidates.len();
        self.add(
            target,
            family,
            value,
            Placement::Expression,
            owner,
            path,
            unsafe_reason,
            None,
        );
        if self.candidates.len() > count {
            self.candidates.last_mut().unwrap().weak = weak;
        }
    }
    fn splice(&self, outer: Node<'a>, inner: Node<'a>, value: &str) -> String {
        let a = self.start(outer);
        let b = self.start(inner);
        let c = self.end(inner);
        let d = self.end(outer);
        format!(
            "{}{}{}",
            &self.file.text()[a..b],
            value,
            &self.file.text()[c..d]
        )
    }
    fn visit(&mut self, node: Node<'a>, owner: &str, path: &[usize]) {
        let kind = node.kind();
        let runtime_heritage = kind == Kind::ExpressionWithTypeArguments
            && node.parent().is_some_and(|parent| {
                parent.kind() == Kind::HeritageClause
                    && parent.as_heritage_clause().token() == Kind::ExtendsKeyword
                    && parent.parent().is_some_and(|class| {
                        matches!(class.kind(), Kind::ClassDeclaration | Kind::ClassExpression)
                    })
            });
        if is_type_node_kind(kind) && !runtime_heritage
            || matches!(
                kind,
                Kind::InterfaceDeclaration
                    | Kind::TypeAliasDeclaration
                    | Kind::ImportDeclaration
                    | Kind::ImportEqualsDeclaration
                    | Kind::ExportDeclaration
                    | Kind::Decorator
                    | Kind::JSDoc
            )
        {
            return;
        }
        if node.ambient() {
            return;
        }
        let named_owner = matches!(
            kind,
            Kind::FunctionDeclaration
                | Kind::MethodDeclaration
                | Kind::VariableDeclaration
                | Kind::PropertyDeclaration
                | Kind::ClassDeclaration
        );
        let mut owned = String::new();
        if named_owner && let Some(name) = node.name() {
            let base = format!("{owner}/{kind:?}:{}", self.raw(name));
            let count = self.owner_counts.entry(base.clone()).or_default();
            owned = format!("{base}#{count}");
            *count += 1;
            if self.facts {
                self.scopes.insert(owned.clone(), node);
                self.parents.insert(owned.clone(), owner.into());
            }
        }
        let (owner, path) = if owned.is_empty() {
            (owner, path)
        } else {
            (owned.as_str(), &[][..])
        };
        let was_critical = self.in_critical;
        if self.facts && matches!(kind, Kind::CallExpression | Kind::NewExpression) {
            self.calls.push(errspec::mutation::syntax::Call {
                scope: owner.into(),
                start: self.start(node),
                end: self.end(node),
                callee: node
                    .expression()
                    .and_then(|callee| callgraph::path(self, callee)),
            });
        }
        if matches!(
            kind,
            Kind::FunctionDeclaration
                | Kind::VariableStatement
                | Kind::ClassDeclaration
                | Kind::ClassExpression
                | Kind::MethodDeclaration
                | Kind::PropertyDeclaration
        ) {
            let full = node.leading_start();
            let leading = self.file.text().get(full..self.start(node)).unwrap_or("");
            self.in_critical |= leading
                .lines()
                .filter_map(|line| line.trim().strip_prefix("//"))
                .any(|body| {
                    matches!(
                        body.split_whitespace().next(),
                        Some("mutator-critical" | "errspec-critical")
                    )
                });
        }
        self.apply(node, owner, path);
        plugins::apply(self, node, owner, path);
        let mut index = 0;
        node.for_each_child(&mut |child| {
            let mut child_path = path.to_vec();
            child_path.push(index);
            index += 1;
            self.visit(child, owner, &child_path);
            false
        });
        self.in_critical = was_critical;
    }
    fn apply(&mut self, node: Node<'a>, owner: &str, path: &[usize]) {
        let raw = self.raw(node).to_string();
        match node.kind() {
            Kind::BinaryExpression => {
                let binary = node.as_binary_expression();
                let op = self.raw(binary.operator_token()).to_string();
                let string = matches!(
                    binary.left().kind(),
                    Kind::StringLiteral
                        | Kind::NoSubstitutionTemplateLiteral
                        | Kind::TemplateExpression
                ) || matches!(
                    binary.right().kind(),
                    Kind::StringLiteral
                        | Kind::NoSubstitutionTemplateLiteral
                        | Kind::TemplateExpression
                );
                let arithmetic = match op.as_str() {
                    "+" => Some("-"),
                    "-" => Some("+"),
                    "*" => Some("/"),
                    "/" => Some("*"),
                    "%" => Some("*"),
                    _ => None,
                };
                if !(string && op == "+")
                    && let Some(op) = arithmetic
                {
                    self.expression(
                        node,
                        "ArithmeticOperator",
                        self.splice(node, binary.operator_token(), op),
                        owner,
                        path,
                    );
                }
                let assignments = match op.as_str() {
                    "+=" => Some("-="),
                    "-=" => Some("+="),
                    "*=" => Some("/="),
                    "/=" => Some("*="),
                    "%=" => Some("*="),
                    "<<=" => Some(">>="),
                    ">>=" => Some("<<="),
                    "&=" => Some("|="),
                    "|=" => Some("&="),
                    "&&=" => Some("||="),
                    "||=" => Some("&&="),
                    "??=" => Some("&&="),
                    _ => None,
                };
                if let Some(value) = assignments
                    && !(string && matches!(op.as_str(), "+=" | "-=" | "*=" | "/=" | "%="))
                {
                    self.expression(
                        node,
                        "AssignmentOperator",
                        self.splice(node, binary.operator_token(), value),
                        owner,
                        path,
                    );
                }
                let equal: &[&str] = match op.as_str() {
                    "<" => &["<=", ">="],
                    "<=" => &["<", ">"],
                    ">" => &[">=", "<="],
                    ">=" => &[">", "<"],
                    "==" => &["!="],
                    "!=" => &["=="],
                    "===" => &["!=="],
                    "!==" => &["==="],
                    _ => &[],
                };
                for op in equal {
                    self.expression(
                        node,
                        "EqualityOperator",
                        self.splice(node, binary.operator_token(), op),
                        owner,
                        path,
                    );
                }
                if matches!(op.as_str(), "&&" | "||" | "??") {
                    let replacement = format!(
                        "({}) {} ({})",
                        self.raw(binary.left()),
                        if op == "&&" { "||" } else { "&&" },
                        self.raw(binary.right())
                    );
                    self.expression(node, "LogicalOperator", replacement, owner, path);
                }
                if !equal.is_empty() || matches!(op.as_str(), "&&" | "||" | "??") {
                    self.condition(node, false, owner, path);
                }
            }
            Kind::ArrayLiteralExpression => {
                let value = if node.elements().is_empty() {
                    "[\"errspec\"]"
                } else {
                    "[]"
                };
                self.expression(node, "ArrayDeclaration", value.into(), owner, path);
            }
            Kind::ArrowFunction => {
                if let Some(body) = node.body()
                    && body.kind() != Kind::Block
                    && self.raw(body) != "undefined"
                {
                    self.expression(body, "ArrowFunction", "undefined".into(), owner, path);
                }
                self.fn_value(node, owner, path);
            }
            Kind::FunctionDeclaration | Kind::FunctionExpression | Kind::MethodDeclaration => {
                self.fn_value(node, owner, path)
            }
            Kind::Block => {
                if !node.statements().is_empty() {
                    let parent = node.parent();
                    let function = parent.is_some_and(Node::is_function);
                    let unsafe_body = parent.is_some_and(|n| {
                        matches!(
                            n.kind(),
                            Kind::Constructor | Kind::GetAccessor | Kind::SetAccessor
                        ) || n.generator()
                    });
                    let unsafe_declaration = node
                        .statements()
                        .iter()
                        .any(|n| n.kind() == Kind::FunctionDeclaration);
                    self.add(node,"BlockStatement","{}".into(),if function {Placement::Body}else{Placement::Block},owner,path,
                        (unsafe_body||unsafe_declaration).then(||"Block has constructor, generator, accessor, or hoisted declaration semantics".into()),None);
                }
            }
            Kind::TrueKeyword | Kind::FalseKeyword => self.expression(
                node,
                "BooleanLiteral",
                if raw == "true" { "false" } else { "true" }.into(),
                owner,
                path,
            ),
            Kind::PrefixUnaryExpression => {
                let unary = node.as_prefix_unary_expression();
                let arg = self.raw(unary.operand());
                let (family, value) = match unary.operator() {
                    Kind::ExclamationToken => ("BooleanLiteral", format!("({arg})")),
                    Kind::PlusToken => ("UnaryOperator", format!("- ({arg})")),
                    Kind::MinusToken => ("UnaryOperator", format!("+ ({arg})")),
                    Kind::TildeToken => ("UnaryOperator", format!("({arg})")),
                    Kind::PlusPlusToken => ("UpdateOperator", format!("--{arg}")),
                    Kind::MinusMinusToken => ("UpdateOperator", format!("++{arg}")),
                    _ => return,
                };
                self.expression(node, family, value, owner, path);
            }
            Kind::PostfixUnaryExpression => {
                let unary = node.as_postfix_unary_expression();
                self.expression(
                    node,
                    "UpdateOperator",
                    format!(
                        "{}{}",
                        self.raw(unary.operand()),
                        if unary.operator() == Kind::PlusPlusToken {
                            "--"
                        } else {
                            "++"
                        }
                    ),
                    owner,
                    path,
                );
            }
            Kind::ExpressionStatement => {
                if let Some(expression) = node.expression()
                    && expression.kind() == Kind::CallExpression
                    && expression.expression().is_some_and(|n| {
                        n.kind() != Kind::SuperKeyword && n.kind() != Kind::ImportKeyword
                    })
                {
                    self.add(
                        node,
                        "CallExpression",
                        ";".into(),
                        Placement::Statement,
                        owner,
                        path,
                        None,
                        None,
                    );
                }
            }
            Kind::IfStatement => self.condition(
                node.as_if_statement().expression().unwrap(),
                false,
                owner,
                path,
            ),
            Kind::ConditionalExpression => self.condition(
                node.as_conditional_expression().condition(),
                false,
                owner,
                path,
            ),
            Kind::WhileStatement | Kind::DoStatement => {
                if let Some(test) = node.expression() {
                    self.condition(test, true, owner, path);
                }
            }
            Kind::ForStatement => {
                if let Some(test) = node.as_for_statement().condition() {
                    self.condition(test, true, owner, path);
                } else {
                    let start = node
                        .as_for_statement()
                        .initializer()
                        .map(|n| self.end(n))
                        .unwrap_or(self.start(node));
                    if let Some(offset) = graph_token(
                        self.file.text(),
                        start,
                        self.start(node.as_for_statement().statement()),
                        ";",
                    )
                    .map(|s| s.end as usize)
                    {
                        let count = self.candidates.len();
                        self.add(
                            node,
                            "ConditionalExpression",
                            "false".into(),
                            Placement::EmptyCondition,
                            owner,
                            path,
                            None,
                            None,
                        );
                        if self.candidates.len() > count {
                            let c = self.candidates.last_mut().unwrap();
                            c.location.byte_offset = offset as u32;
                            c.end = offset as u32;
                            c.original.clear();
                            c.location.line = line_at(self.file.text(), offset);
                        }
                    }
                }
            }
            Kind::CallExpression | Kind::NewExpression => self.call(node, owner, path),
            Kind::ObjectLiteralExpression => {
                if !node.properties().is_empty() {
                    self.expression(node, "ObjectLiteral", "{}".into(), owner, path);
                }
            }
            Kind::PropertyAccessExpression | Kind::ElementAccessExpression => {
                if let Some(token) = node.question_dot_token() {
                    self.expression(
                        node,
                        "OptionalChaining",
                        self.splice(
                            node,
                            token,
                            if node.kind() == Kind::PropertyAccessExpression {
                                "."
                            } else {
                                ""
                            },
                        ),
                        owner,
                        path,
                    );
                }
            }
            Kind::StringLiteral
            | Kind::NoSubstitutionTemplateLiteral
            | Kind::TemplateExpression => {
                if !self.skip_string(node) {
                    self.expression(
                        node,
                        "StringLiteral",
                        if node.kind() != Kind::TemplateExpression && node.text().is_empty() {
                            "\"errspec\""
                        } else {
                            "\"\""
                        }
                        .into(),
                        owner,
                        path,
                    );
                }
            }
            Kind::RegularExpressionLiteral => {
                if let Some(end) = raw.rfind('/')
                    && end > 0
                {
                    let pattern = &raw[1..end];
                    let flags = &raw[end + 1..];
                    for next in regex_mutations(pattern) {
                        self.add(
                            node,
                            "Regex",
                            format!("/{next}/{flags}"),
                            Placement::Expression,
                            owner,
                            path,
                            None,
                            Some((next, flags.into())),
                        );
                    }
                }
            }
            _ => (),
        }
    }
    fn condition(&mut self, node: Node<'a>, loop_test: bool, owner: &str, path: &[usize]) {
        let mut current = node;
        let mut parent = node.parent();
        let mut looping = loop_test;
        while let Some(p) = parent {
            if matches!(p.kind(), Kind::WhileStatement | Kind::DoStatement) {
                looping |= p.expression() == Some(current);
                break;
            }
            if p.kind() == Kind::ForStatement {
                looping |= p.as_for_statement().condition() == Some(current);
                break;
            }
            if !matches!(
                p.kind(),
                Kind::ParenthesizedExpression
                    | Kind::BinaryExpression
                    | Kind::PrefixUnaryExpression
                    | Kind::ConditionalExpression
            ) {
                break;
            }
            current = p;
            parent = p.parent();
        }
        if !looping {
            self.expression(node, "ConditionalExpression", "true".into(), owner, path);
        }
        self.expression(node, "ConditionalExpression", "false".into(), owner, path);
    }
    fn skip_string(&self, node: Node<'a>) -> bool {
        node.parent().is_some_and(|p| {
            matches!(
                p.kind(),
                Kind::ImportDeclaration
                    | Kind::ExportDeclaration
                    | Kind::ExternalModuleReference
                    | Kind::JsxAttribute
                    | Kind::TaggedTemplateExpression
            ) || p.name() == Some(node)
                || p.kind() == Kind::ExpressionStatement && self.directive(p)
                || matches!(p.kind(), Kind::CallExpression | Kind::NewExpression)
                    && p.expression().is_some_and(|n| {
                        matches!(self.raw(n), "require" | "Symbol" | "Symbol.for" | "RegExp")
                    })
        })
    }
    fn directive(&self, statement: Node<'a>) -> bool {
        let Some(parent) = statement.parent() else {
            return false;
        };
        if !matches!(parent.kind(), Kind::SourceFile | Kind::Block) {
            return false;
        }
        for item in parent.statements() {
            if item.kind() != Kind::ExpressionStatement
                || !item
                    .expression()
                    .is_some_and(|n| n.kind() == Kind::StringLiteral)
            {
                return false;
            }
            if item == statement {
                return true;
            }
        }
        false
    }
    fn call(&mut self, node: Node<'a>, owner: &str, path: &[usize]) {
        let Some(callee) = node.expression() else {
            return;
        };
        if node.kind() == Kind::CallExpression
            && let Some(token) = node.question_dot_token()
        {
            self.expression(
                node,
                "OptionalChaining",
                self.splice(node, token, ""),
                owner,
                path,
            );
        }
        if self.raw(callee) == "Array" {
            let value = if node.arguments().is_empty() {
                "Array([])"
            } else {
                "Array()"
            };
            self.expression(node, "ArrayDeclaration", value.into(), owner, path);
        }
        if self.raw(callee) == "RegExp" {
            let args = node.arguments();
            if !args.is_empty()
                && matches!(
                    args[0].kind(),
                    Kind::StringLiteral | Kind::NoSubstitutionTemplateLiteral
                )
                && (args.len() == 1
                    || args.len() == 2
                        && matches!(
                            args[1].kind(),
                            Kind::StringLiteral | Kind::NoSubstitutionTemplateLiteral
                        ))
            {
                let flags = if args.len() == 2 { args[1].text() } else { "" };
                for next in regex_mutations(args[0].text()) {
                    self.add(
                        node,
                        "Regex",
                        self.splice(node, args[0], &serde_json::to_string(&next).unwrap()),
                        Placement::Expression,
                        owner,
                        path,
                        None,
                        Some((next, flags.into())),
                    );
                }
            }
        }
        if callee.kind() != Kind::PropertyAccessExpression || node.kind() != Kind::CallExpression {
            return;
        }
        let Some(name) = callee.name() else { return };
        let method = self.raw(name).to_string();
        if matches!(
            method.as_str(),
            "charAt" | "filter" | "reverse" | "slice" | "sort" | "substr" | "substring" | "trim"
        ) && let Some(receiver) = callee.expression()
        {
            self.expression(
                node,
                "MethodExpression",
                self.raw(receiver).into(),
                owner,
                path,
            );
        }
        let swapped = match method.as_str() {
            "endsWith" => Some("startsWith"),
            "startsWith" => Some("endsWith"),
            "every" => Some("some"),
            "some" => Some("every"),
            "toLowerCase" => Some("toUpperCase"),
            "toUpperCase" => Some("toLowerCase"),
            "toLocaleLowerCase" => Some("toLocaleUpperCase"),
            "toLocaleUpperCase" => Some("toLocaleLowerCase"),
            "trimEnd" => Some("trimStart"),
            "trimStart" => Some("trimEnd"),
            "min" => Some("max"),
            "max" => Some("min"),
            _ => None,
        };
        if let Some(value) = swapped {
            self.expression(
                node,
                "MethodExpression",
                self.splice(node, name, value),
                owner,
                path,
            );
        }
    }
    fn fn_value(&mut self, node: Node<'a>, owner: &str, path: &[usize]) {
        let Some(body) = node.body() else { return };
        let Some(mut typ) = node.type_node() else {
            return;
        };
        while typ.kind() == Kind::ParenthesizedType {
            typ = typ.type_node().unwrap();
        }
        if node.generator() {
            return;
        }
        let is_async = node.is_async();
        let promise = if typ.kind() == Kind::TypeReference
            && self.raw(typ.as_type_reference_node().type_name()) == "Promise"
            && typ.type_arguments().len() == 1
        {
            Some(typ.type_arguments()[0])
        } else {
            None
        };
        if is_async && promise.is_none() {
            return;
        }
        let values = self.type_values(promise.unwrap_or(typ));
        for value in values {
            let value = if promise.is_some() && !is_async {
                format!("Promise.resolve({value})")
            } else {
                value
            };
            if body.kind() == Kind::Block {
                if body.statements().len() == 1
                    && body.statements()[0].kind() == Kind::ReturnStatement
                    && body.statements()[0]
                        .expression()
                        .is_some_and(|n| self.same_value(n, &value))
                {
                    continue;
                }
                let replacement = if value == "undefined" {
                    "{}".into()
                } else {
                    format!("{{ return {value}; }}")
                };
                // Empty-body mutation already represents the undefined return.
                if value != "undefined" {
                    self.add(
                        body,
                        "FnValue",
                        replacement,
                        Placement::Body,
                        owner,
                        path,
                        None,
                        None,
                    );
                }
            } else {
                if !self.same_value(body, &value) {
                    self.expression(body, "FnValue", value, owner, path);
                }
            }
        }
    }
    fn same_value(&self, mut node: Node<'a>, value: &str) -> bool {
        while node.kind() == Kind::ParenthesizedExpression {
            node = node.expression().unwrap();
        }
        self.raw(node) == value
            || matches!(
                node.kind(),
                Kind::StringLiteral | Kind::NoSubstitutionTemplateLiteral
            ) && serde_json::from_str::<String>(value).is_ok_and(|v| v == node.text())
    }
    fn type_values(&self, typ: Node<'a>) -> Vec<String> {
        let values: &[&str] = match typ.kind() {
            Kind::NullKeyword => &["null"],
            Kind::BooleanKeyword => &["true", "false"],
            Kind::NumberKeyword => &["0", "1", "-1"],
            Kind::StringKeyword => &["\"\"", "\"xyzzy\""],
            Kind::BigIntKeyword => &["0n", "1n"],
            Kind::UndefinedKeyword | Kind::VoidKeyword => &["undefined"],
            Kind::ArrayType => &["[]"],
            Kind::ParenthesizedType => return self.type_values(typ.type_node().unwrap()),
            Kind::UnionType => {
                let mut result = vec![];
                for child in typ.children().iter() {
                    for value in self.type_values(*child) {
                        if !result.contains(&value) {
                            result.push(value);
                        }
                    }
                }
                return result;
            }
            Kind::LiteralType => {
                let literal = typ.as_literal_type_node().literal();
                return vec![if literal.kind() == Kind::StringLiteral {
                    serde_json::to_string(literal.text()).unwrap()
                } else {
                    self.raw(literal).into()
                }];
            }
            Kind::TypeReference => match self.raw(typ.as_type_reference_node().type_name()) {
                "Array" | "ReadonlyArray" => &["[]"],
                "undefined" => &["undefined"],
                _ => &[],
            },
            Kind::TypeOperator
                if typ.as_type_operator_node().operator() == Kind::ReadonlyKeyword =>
            {
                return typ
                    .type_node()
                    .map(|n| self.type_values(n))
                    .unwrap_or_default();
            }
            Kind::TypePredicate if typ.as_type_predicate_node().asserts_modifier().is_none() => {
                &["true", "false"]
            }
            _ => &[],
        };
        values.iter().map(|v| (*v).into()).collect()
    }
}

/// Conservative regex syntax edits. The Node driver validates every replacement with RegExp.
fn regex_mutations(pattern: &str) -> Vec<String> {
    let bytes = pattern.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    let mut class = false;
    while i < bytes.len() {
        let b = bytes[i];
        let mut edit = None;
        if b == b'\\' {
            if i + 1 < bytes.len() {
                let c = bytes[i + 1];
                let swap = match c {
                    b'd' => Some('D'),
                    b'D' => Some('d'),
                    b's' => Some('S'),
                    b'S' => Some('s'),
                    b'w' => Some('W'),
                    b'W' => Some('w'),
                    _ => None,
                };
                if let Some(c) = swap {
                    edit = Some((i + 2, format!("\\{c}")));
                }
                i += 1;
            }
        } else if b == b'[' {
            class = true;
            if bytes.get(i + 1) == Some(&b'^') {
                edit = Some((i + 2, "[".into()))
            } else {
                edit = Some((i + 1, "[^".into()))
            }
        } else if b == b']' {
            class = false;
        } else if !class {
            if b == b'^'
                || b == b'$'
                || b == b'*'
                || b == b'+'
                || b == b'?' && i > 0 && bytes[i - 1] != b'('
            {
                edit = Some((i + 1, String::new()));
            } else if b == b'{'
                && let Some(end) = pattern[i..].find('}')
            {
                let end = i + end + 1;
                let range = &pattern[i + 1..end - 1];
                if range.chars().all(|c| c.is_ascii_digit() || c == ',') {
                    edit = Some((end, String::new()));
                    if let Some((lower, _)) = range.split_once(',')
                        && !lower.is_empty()
                    {
                        out.push(format!("{}{{{lower}}}{}", &pattern[..i], &pattern[end..]));
                    }
                }
            }
        }
        if let Some((end, replacement)) = edit {
            let start = if b == b'\\' { i - 1 } else { i };
            out.push(format!(
                "{}{}{}",
                &pattern[..start],
                replacement,
                &pattern[end..]
            ));
        }
        i += 1;
    }
    let mut seen = BTreeSet::new();
    out.retain(|s| s != pattern && seen.insert(s.clone()));
    out
}
