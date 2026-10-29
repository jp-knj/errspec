//! Follow a contract's entry function: failures that escape outside the error type (EC018),
//! and the `return`s that yield each tag, which execution later marks as reached or not.
use crate::{scan, tsrs_adapter::TsrsAdapter};
use errspec::{
    config::ValidatedContract,
    discovery::{Channel, ScanReport},
    model::{EntryRef, FailurePath, Finding},
};
use std::{collections::BTreeSet, path::Path};
use tsrs_ast::{FunctionFlags, Kind, Node, SymbolFlags, get_function_flags};
use tsrs_checker::{Checker, LiteralValue, Type, TypeFlags};
use tsrs_core::P;

pub(crate) struct EntryAnalysis {
    pub paths: Vec<FailurePath>,
    pub findings: Vec<Finding>,
}

pub(crate) fn analyze(
    adapter: &TsrsAdapter,
    root: &Path,
    contract: &ValidatedContract,
    tags: &[String],
    scan: &ScanReport,
) -> Result<EntryAnalysis, String> {
    let entry = contract.entry().expect("contract with entry");
    let function = resolve(adapter, root, entry)?;
    let file = tsrs_ast::get_source_file_of_node(function).expect("entry source file");
    let mut checker = adapter
        .program
        .get_type_checker_for_file(&adapter.context, file);
    let paths = return_paths(
        &mut checker,
        root,
        function,
        &contract.source().discriminant,
        tags,
    );
    let findings = escaping_failures(root, function, entry, contract, scan);
    Ok(EntryAnalysis { paths, findings })
}

fn resolve(adapter: &TsrsAdapter, root: &Path, entry: &EntryRef) -> Result<P<Node>, String> {
    let path = root
        .join(&entry.file)
        .canonicalize()
        .map_err(|e| format!("entry {}: {e}", entry.file))?;
    let file = adapter
        .program
        .get_source_file(&path.to_string_lossy())
        .ok_or_else(|| format!("entry {} is not in the TypeScript project", entry.file))?;
    let mut checker = adapter
        .program
        .get_type_checker_for_file(&adapter.context, file);
    let module = file.symbol().ok_or("entry file must be a module")?;
    let mut symbol = checker
        .get_exports_of_module_exported(module)
        .into_iter()
        .find(|s| s.name() == entry.export)
        .ok_or_else(|| format!("entry export {} not found in {}", entry.export, entry.file))?;
    if symbol.flags().intersects(SymbolFlags::Alias) {
        symbol = checker.get_aliased_symbol(symbol);
    }
    let function = symbol
        .declarations()
        .iter()
        .copied()
        .find_map(|declaration| match declaration.kind() {
            Kind::FunctionDeclaration => declaration.body().map(|_| declaration),
            Kind::VariableDeclaration => declaration.initializer().filter(|init| {
                matches!(init.kind(), Kind::ArrowFunction | Kind::FunctionExpression)
            }),
            _ => None,
        });
    let function = function.ok_or_else(|| {
        format!(
            "entry {} must be a function declaration or a const function",
            entry.export
        )
    })?;
    Ok(function)
}

fn return_paths(
    checker: &mut Checker,
    root: &Path,
    function: P<Node>,
    discriminant: &str,
    tags: &[String],
) -> Vec<FailurePath> {
    let is_async = get_function_flags(Some(function)).contains(FunctionFlags::Async);
    let Some(body) = function.body() else {
        return Vec::new();
    };
    let mut returns = Vec::new();
    if body.kind() == Kind::Block {
        scan::walk_body(body, &mut |node| {
            if node.kind() == Kind::ReturnStatement
                && let Some(expression) = node.expression()
            {
                returns.push((node, expression));
            }
        });
    } else {
        // An expression-bodied arrow function returns its body.
        returns.push((body, body));
    }
    let mut paths = Vec::new();
    for (statement, expression) in returns {
        let mut ty = checker.get_type_at_location(expression);
        if is_async {
            ty = checker.get_awaited_type_exported(ty).unwrap_or(ty);
        }
        let mut found = BTreeSet::new();
        collect_tags(checker, ty, discriminant, &mut found);
        let found: Vec<_> = found.into_iter().filter(|tag| tags.contains(tag)).collect();
        if found.is_empty() {
            continue;
        }
        let location = scan::location(root, statement);
        paths.push(FailurePath {
            file: location.file,
            line: location.line,
            byte_offset: location.byte_offset,
            tags: found,
            reached: None,
        });
    }
    paths
}

fn collect_tags(
    checker: &mut Checker,
    ty: P<Type>,
    discriminant: &str,
    out: &mut BTreeSet<String>,
) {
    if ty.flags().intersects(TypeFlags::Union) {
        for member in ty.types() {
            collect_tags(checker, *member, discriminant, out);
        }
        return;
    }
    if ty
        .flags()
        .intersects(TypeFlags::Any | TypeFlags::Unknown | TypeFlags::Never)
    {
        return;
    }
    let Some(property) = checker.get_type_of_property_of_type_exported(ty, discriminant) else {
        return;
    };
    let members = if property.flags().intersects(TypeFlags::Union) {
        property.types().to_vec()
    } else {
        vec![property]
    };
    for member in members {
        if member.flags().intersects(TypeFlags::StringLiteral)
            && let Some(LiteralValue::String(value)) = member.as_literal_type().value()
        {
            out.insert(value.to_owned());
        }
    }
}

/// Throws and rejections that reach the entry with no catch on the way. A catch that may
/// rethrow is not a definite escape, and unresolved calls have no origin, so neither is reported.
fn escaping_failures(
    root: &Path,
    function: P<Node>,
    entry: &EntryRef,
    contract: &ValidatedContract,
    scan: &ScanReport,
) -> Vec<Finding> {
    let id = scan::key(root, function);
    let Some(summary) = scan.functions.iter().find(|f| f.id == id) else {
        return Vec::new();
    };
    let mut findings = Vec::new();
    for effect in &summary.effects {
        if !matches!(effect.channel, Channel::Throw | Channel::Rejection) {
            continue;
        }
        let Some(origin) = scan.origins.iter().find(|o| o.id == effect.origin) else {
            continue;
        };
        let uncaught_hops = effect.via.windows(2).all(|hop| {
            scan.edges
                .iter()
                .any(|e| e.caller == hop[0] && e.callee == hop[1] && !e.catches)
        });
        if origin.caught || !uncaught_hops {
            continue;
        }
        let via: Vec<_> = effect
            .via
            .iter()
            .filter_map(|id| {
                scan.functions
                    .iter()
                    .find(|f| &f.id == id)
                    .map(|f| f.name.as_str())
            })
            .collect();
        let channel = if effect.channel == Channel::Throw {
            "throw"
        } else {
            "rejection"
        };
        let mut finding = Finding::new(
            "EC018",
            format!(
                "{} lets {} escape as a {channel} via {}; {} does not represent it",
                entry.export,
                origin.error_type,
                via.join(" -> "),
                contract.source().export,
            ),
        );
        finding.contract_id = Some(contract.id().into());
        finding.file = Some(origin.location.file.as_str().into());
        finding.byte_offset = Some(origin.location.byte_offset);
        finding.line = Some(origin.location.line as u32);
        findings.push(finding);
    }
    findings
}
