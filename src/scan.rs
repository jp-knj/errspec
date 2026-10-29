use crate::tsrs_adapter::{TsrsAdapter, collect_tags};
use errspec::{
    discovery::*,
    model::{Finding, Source},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use tsrs_ast::{
    FunctionFlags, Kind, Node, NodeFlags, SourceFile, SymbolFlags, get_function_flags,
    get_source_file_of_node, is_function_like_declaration,
};
use tsrs_checker::{Checker, Type, TypeFlags};
use tsrs_core::P;

pub(crate) fn location(root: &Path, node: P<Node>) -> Location {
    let file = get_source_file_of_node(node).expect("source node");
    let offset = tsrs_scanner::get_token_pos_of_node(node, file, false) as usize;
    Location {
        file: Path::new(file.file_name())
            .strip_prefix(root)
            .unwrap_or(Path::new(file.file_name()))
            .to_string_lossy()
            .replace('\\', "/"),
        byte_offset: offset as u32,
        line: line_at(file.text(), offset),
    }
}

pub(crate) fn line_at(text: &str, offset: usize) -> usize {
    text.as_bytes()[..offset.min(text.len())]
        .iter()
        .filter(|&&c| c == b'\n')
        .count()
        + 1
}
fn key(root: &Path, node: P<Node>) -> String {
    let loc = location(root, node);
    format!("{}:{}:{:?}", loc.file, loc.byte_offset, node.kind())
}
pub(crate) fn text(node: P<Node>) -> String {
    if matches!(
        node.kind(),
        Kind::Identifier | Kind::PrivateIdentifier | Kind::StringLiteral
    ) {
        return node.text().into();
    }
    get_source_file_of_node(node)
        .and_then(|f| {
            f.text()
                .get(node.pos().source_offset()? as usize..node.end().source_offset()? as usize)
        })
        .unwrap_or("")
        .trim()
        .to_owned()
}
pub(crate) fn walk(node: P<Node>, callback: &mut impl FnMut(P<Node>)) {
    callback(node);
    node.for_each_child(&mut |child| {
        walk(child, callback);
        false
    });
}
fn walk_body(node: P<Node>, callback: &mut impl FnMut(P<Node>)) {
    if is_function_like_declaration(node) {
        return;
    }
    callback(node);
    node.for_each_child(&mut |child| {
        walk_body(child, callback);
        false
    });
}

fn is_const(decl: P<Node>) -> bool {
    decl.parent().is_some_and(|p| {
        p.kind() == Kind::VariableDeclarationList && p.flags().contains(NodeFlags::Const)
    })
}

fn is_builtin(node: P<Node>) -> bool {
    get_source_file_of_node(node).is_some_and(|f| {
        f.is_declaration_file() && f.file_name().starts_with("bundled:///libs/lib.")
    })
}

/// Resolve value bindings, not their structurally compatible call signatures.
fn static_binding(
    checker: &mut Checker,
    node: P<Node>,
    seen: &mut Vec<P<Node>>,
) -> Option<P<Node>> {
    if seen.contains(&node) {
        return None;
    }
    seen.push(node);
    match node.kind() {
        Kind::ArrowFunction | Kind::FunctionExpression => Some(node),
        Kind::ParenthesizedExpression
        | Kind::AsExpression
        | Kind::SatisfiesExpression
        | Kind::TypeAssertionExpression
        | Kind::NonNullExpression => static_binding(checker, node.expression()?, seen),
        Kind::Identifier => {
            let symbol = checker.get_symbol_at_location_exported(node)?;
            let symbol = if symbol.flags().intersects(SymbolFlags::Alias) {
                checker.get_aliased_symbol(symbol)
            } else {
                symbol
            };
            let declarations = symbol.declarations();
            if let Some(function) = declarations
                .iter()
                .copied()
                .find(|d| d.kind() == Kind::FunctionDeclaration && d.body().is_some())
            {
                return Some(function);
            }
            let decl = declarations
                .iter()
                .copied()
                .find(|d| d.kind() == Kind::VariableDeclaration)?;
            if is_builtin(decl) {
                return Some(decl);
            }
            if is_const(decl) {
                return static_binding(checker, decl.initializer()?, seen);
            }
            None
        }
        _ => None,
    }
}

#[derive(Clone, Copy, Default)]
struct Usage {
    awaited: bool,
    returned: bool,
    catches: bool,
    may_rethrow: bool,
}

fn catches(node: P<Node>) -> (bool, bool) {
    let mut current = node;
    while let Some(parent) = current.parent() {
        if is_function_like_declaration(parent) {
            break;
        }
        if parent.kind() == Kind::TryStatement {
            let t = parent.as_try_statement();
            if t.try_block == current
                && let Some(catch) = t.catch_clause
            {
                let mut rethrows = false;
                walk_body(catch.as_catch_clause().block, &mut |n| {
                    if n.kind() == Kind::ThrowStatement {
                        rethrows = true
                    }
                });
                return (true, rethrows);
            }
        }
        current = parent;
    }
    (false, false)
}

fn direct_usage(node: P<Node>) -> Usage {
    let (catches, may_rethrow) = catches(node);
    let mut usage = Usage {
        catches,
        may_rethrow,
        ..Usage::default()
    };
    let mut current = node;
    while let Some(parent) = current.parent() {
        match parent.kind() {
            Kind::AwaitExpression => usage.awaited = true,
            Kind::ReturnStatement => {
                usage.returned = true;
                break;
            }
            Kind::ArrowFunction if parent.body() == Some(current) => {
                usage.returned = true;
                break;
            }
            Kind::ParenthesizedExpression
            | Kind::AsExpression
            | Kind::SatisfiesExpression
            | Kind::TypeAssertionExpression
            | Kind::NonNullExpression => (),
            _ => break,
        }
        current = parent;
    }
    usage
}

/// Track immutable local aliases to await/return uses with checker-resolved symbols.
/// Assignments and escaping closures remain explicitly unresolved.
fn usages(checker: &mut Checker, expression: P<Node>, body: P<Node>) -> Vec<Usage> {
    let mut result = vec![direct_usage(expression)];
    let mut queue = vec![expression];
    let mut seen = BTreeSet::new();
    while let Some(value) = queue.pop() {
        let Some(decl) = value.parent().filter(|n| {
            n.kind() == Kind::VariableDeclaration && is_const(*n) && n.initializer() == Some(value)
        }) else {
            continue;
        };
        if !seen.insert(decl.pos()) {
            continue;
        }
        let Some(symbol) = decl.symbol() else {
            continue;
        };
        let mut uses = vec![];
        walk_body(body, &mut |n| {
            if n.kind() == Kind::Identifier
                && checker.get_symbol_at_location_exported(n) == Some(symbol)
                && decl.name() != Some(n)
            {
                uses.push(n);
            }
        });
        for use_node in uses {
            result.push(direct_usage(use_node));
            queue.push(use_node);
        }
    }
    result
}

pub(crate) fn run(project: &Path, discriminants: &[String]) -> ScanReport {
    let project = match project.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            return ScanReport {
                diagnostics: vec![Finding::new("EC_CONFIG", e.to_string())],
                ..ScanReport::default()
            };
        }
    };
    let root = project.parent().expect("canonical project parent");
    let adapter = match TsrsAdapter::open(root, &project.to_string_lossy()) {
        Ok(a) => a,
        Err(errors) => {
            return ScanReport {
                diagnostics: errors,
                ..ScanReport::default()
            };
        }
    };
    let files: Vec<P<SourceFile>> = adapter
        .program
        .get_source_files()
        .iter()
        .copied()
        .filter(|f| {
            let path = f.file_name().replace('\\', "/");
            !path.contains("/node_modules/") && Path::new(f.file_name()).starts_with(root)
        })
        .collect();
    let mut facts = ScanFacts::default();
    let mut bodies = BTreeMap::new();
    for file in &files {
        let mut checker = adapter
            .program
            .get_type_checker_for_file(&adapter.context, *file);
        facts.files.push(location(root, file.as_node()).file);
        if let Some(module) = file.symbol() {
            for export in checker.get_exports_of_module_exported(module) {
                let name = export.name().to_owned();
                let symbol = if export.flags().intersects(SymbolFlags::Alias) {
                    checker.get_aliased_symbol(export)
                } else {
                    export
                };
                if !symbol.flags().intersects(
                    SymbolFlags::TypeAlias | SymbolFlags::Interface | SymbolFlags::Class,
                ) {
                    continue;
                }
                let ty = checker.get_declared_type_of_symbol_exported(symbol);
                for discriminant in discriminants {
                    let mut tags = BTreeSet::new();
                    if collect_tags(&mut checker, ty, discriminant, &mut tags).is_ok()
                        && !tags.is_empty()
                    {
                        facts.candidates.push(Candidate {
                            source: Source {
                                file: location(root, file.as_node()).file,
                                export: name.clone(),
                                discriminant: discriminant.clone(),
                            },
                            tags: tags.into_iter().collect(),
                        });
                    }
                }
            }
        }
        if file.is_declaration_file() {
            continue;
        }
        walk(file.as_node(), &mut |node| {
            if node.kind() == Kind::SourceFile
                || (is_function_like_declaration(node) && node.body().is_some())
            {
                let id = key(root, node);
                let name = if node.kind() == Kind::SourceFile {
                    "<module>".into()
                } else {
                    node.name()
                        .or_else(|| node.parent().and_then(|p| p.name()))
                        .map(text)
                        .unwrap_or_else(|| "<anonymous>".into())
                };
                facts.functions.push(FunctionFacts {
                    id: id.clone(),
                    name,
                    location: location(root, node),
                    is_async: get_function_flags(Some(node)).contains(FunctionFlags::Async),
                });
                bodies.insert(id, node);
            }
        });
    }
    let mut inspector = Inspector {
        root,
        adapter: &adapter,
        bodies: &bodies,
        facts: &mut facts,
    };
    for (id, node) in &bodies {
        inspector.function(id, *node);
    }
    facts.into_report()
}

struct Inspector<'a> {
    root: &'a Path,
    adapter: &'a TsrsAdapter,
    bodies: &'a BTreeMap<String, P<Node>>,
    facts: &'a mut ScanFacts,
}
impl Inspector<'_> {
    fn unknown(&mut self, owner: &str, node: P<Node>, reason: &str) {
        self.facts.unknowns.push(Unknown {
            function: owner.into(),
            location: location(self.root, node),
            expression: text(node),
            reason: reason.into(),
        });
    }
    fn origin(
        &mut self,
        owner: &str,
        node: P<Node>,
        channel: Channel,
        ty: String,
        caught: bool,
        escapes: bool,
    ) {
        self.facts.origins.push(Origin {
            id: format!("{}:{channel:?}", key(self.root, node)),
            function: owner.into(),
            location: location(self.root, node),
            channel,
            error_type: ty,
            caught,
            escapes,
        });
    }
    fn function(&mut self, owner: &str, node: P<Node>) {
        let file = get_source_file_of_node(node).unwrap();
        let mut checker = self
            .adapter
            .program
            .get_type_checker_for_file(&self.adapter.context, file);
        let is_async = get_function_flags(Some(node)).contains(FunctionFlags::Async);
        let body = node.body().unwrap_or(node);
        if node.kind() != Kind::SourceFile {
            if get_function_flags(Some(node)).contains(FunctionFlags::Generator) {
                self.unknown(owner, node, "generator_execution_requires_iteration");
            }
            let sig = checker.get_signature_from_declaration_exported(node);
            let result = checker.get_return_type_of_signature_exported(sig);
            let result = checker.get_awaited_type_exported(result).unwrap_or(result);
            let mut errors = BTreeSet::new();
            result_errors(&mut checker, result, &mut errors);
            if !errors.is_empty() {
                self.origin(
                    owner,
                    node,
                    Channel::Result,
                    errors.into_iter().collect::<Vec<_>>().join(" | "),
                    false,
                    true,
                );
            }
        }
        walk_body(body, &mut |n| match n.kind() {
            Kind::ThrowStatement => {
                let ty = checker.get_type_at_location(n.as_throw_statement().expression());
                let ty = checker.type_to_string_exported(ty);
                let (caught, rethrows) = catches(n);
                self.origin(
                    owner,
                    n,
                    if is_async {
                        Channel::Rejection
                    } else {
                        Channel::Throw
                    },
                    ty,
                    caught,
                    !caught || rethrows,
                );
            }
            Kind::CallExpression | Kind::NewExpression => self.call(owner, n, body, &mut checker),
            _ => (),
        });
    }
    fn call(&mut self, owner: &str, node: P<Node>, body: P<Node>, checker: &mut Checker) {
        let sig = checker.get_resolved_signature_exported(node);
        let declaration = sig.declaration();
        let uses = usages(checker, node, body);
        let callee = node.expression().expect("call expression");
        let binding = static_binding(checker, callee, &mut vec![]);
        if is_native_promise_reject(declaration)
            && node.kind() == Kind::CallExpression
            && callee.kind() == Kind::PropertyAccessExpression
            && callee
                .expression()
                .and_then(|receiver| static_binding(checker, receiver, &mut vec![]))
                .is_some_and(|receiver| {
                    is_builtin(receiver) && receiver.name().is_some_and(|n| text(n) == "Promise")
                })
        {
            let argument = node.as_call_expression().arguments.nodes().first().copied();
            let ty = argument
                .map(|n| {
                    let ty = checker.get_type_at_location(n);
                    checker.type_to_string_exported(ty)
                })
                .unwrap_or_else(|| "undefined".into());
            let escapes = uses
                .iter()
                .any(|u| u.returned && !u.awaited || u.awaited && (!u.catches || u.may_rethrow));
            let caught = uses.iter().any(|u| u.awaited && u.catches);
            self.origin(owner, node, Channel::Rejection, ty, caught, escapes);
            if !uses.iter().any(|u| u.returned || u.awaited) {
                self.unknown(owner, node, "unobserved_promise_rejection");
            }
            return;
        }
        // Built-in Error construction is the payload of a throw, not an additional call edge.
        if node.kind() == Kind::NewExpression
            && binding.is_some_and(is_builtin)
            && declaration.is_some_and(|d| {
                get_source_file_of_node(d)
                    .is_some_and(|f| f.is_declaration_file() && f.file_name().contains("/lib."))
                    && d.parent()
                        .and_then(|p| p.name())
                        .is_some_and(|n| text(n).ends_with("ErrorConstructor"))
            })
        {
            return;
        }
        let member_call = matches!(
            callee.kind(),
            Kind::PropertyAccessExpression | Kind::ElementAccessExpression
        );
        let target = binding
            .or_else(|| member_call.then_some(declaration).flatten())
            .and_then(|decl| {
                if self.bodies.contains_key(&key(self.root, decl)) {
                    return Some(decl);
                }
                decl.symbol().and_then(|symbol| {
                    symbol
                        .declarations()
                        .iter()
                        .copied()
                        .find(|d| self.bodies.contains_key(&key(self.root, *d)))
                })
            });
        if let Some(target) = target {
            let target_key = key(self.root, target);
            if member_call
                || matches!(
                    target.kind(),
                    Kind::MethodDeclaration | Kind::GetAccessor | Kind::SetAccessor
                )
            {
                self.unknown(owner, node, "member_dispatch_may_be_overridden");
            }
            let target_flags = get_function_flags(Some(target));
            if target_flags.contains(FunctionFlags::Generator) {
                self.unknown(owner, node, "generator_body_is_not_executed_by_call");
                return;
            }
            let return_type = checker.get_return_type_of_signature_exported(sig);
            let promise = checker.get_promised_type_of_promise(return_type).is_some();
            if promise && !uses.iter().any(|u| u.awaited || u.returned) {
                self.unknown(owner, node, "promise_use_not_resolved_to_await_or_return");
            }
            for usage in uses {
                self.facts.edges.push(Edge {
                    caller: owner.into(),
                    callee: target_key.clone(),
                    location: location(self.root, node),
                    awaited: usage.awaited,
                    returned: usage.returned,
                    catches: usage.catches,
                    may_rethrow: usage.may_rethrow,
                });
            }
        } else {
            self.unknown(
                owner,
                node,
                if declaration.is_some() {
                    "implementation_outside_scan_or_dynamic_signature"
                } else {
                    "unresolved_call"
                },
            );
        }
    }
}

fn result_errors(checker: &mut Checker, ty: P<Type>, out: &mut BTreeSet<String>) {
    if ty.flags().contains(TypeFlags::Union) {
        for member in ty.types() {
            result_errors(checker, *member, out);
        }
    } else if !ty
        .flags()
        .intersects(TypeFlags::Any | TypeFlags::Unknown | TypeFlags::Never)
        && let Some(error) = checker.get_type_of_property_of_type_exported(ty, "error")
    {
        out.insert(checker.type_to_string_exported(error));
    }
}
fn is_native_promise_reject(decl: Option<P<Node>>) -> bool {
    decl.is_some_and(|d| {
        d.name().is_some_and(|n| text(n) == "reject")
            && d.parent()
                .and_then(|p| p.name())
                .is_some_and(|n| text(n) == "PromiseConstructor")
            && is_builtin(d)
    })
}
