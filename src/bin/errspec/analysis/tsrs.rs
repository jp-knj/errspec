//! Isolate unpublished tsrs APIs. No native handles escape into reports.
//! Compiler arena allocations live until this one-shot CLI process exits.
use errspec::model::{Finding, Location, Source};
use std::{collections::BTreeSet, path::Path, sync::Arc};
use tsrs_ast::{Kind, Node, SymbolFlags, get_source_file_of_node};
use tsrs_checker::{Checker, LiteralValue, Type, TypeFlags};
use tsrs_compiler::{Context, Program, ProgramOptions, new_compiler_host, new_program};
use tsrs_core::{ModuleResolutionKind, P, Tristate};
use tsrs_tsoptions::ParseConfigHost;
use tsrs_vfs::{FS, bundled, osvfs};

/// Parser-only entry for mutation generation. Contract analysis still uses `open`.
pub(crate) fn parse_source(file: &str, source: String) -> Result<P<tsrs_ast::SourceFile>, String> {
    let kind = match Path::new(file).extension().and_then(|s| s.to_str()) {
        Some("jsx") => tsrs_core::ScriptKind::JSX,
        Some("js" | "mjs" | "cjs") => tsrs_core::ScriptKind::JS,
        Some("tsx") => tsrs_core::ScriptKind::TSX,
        _ => tsrs_core::ScriptKind::TS,
    };
    let file_name = if Path::new(file).is_absolute() {
        file.into()
    } else {
        format!("/__errspec_mutation__/{file}")
    };
    let parsed = tsrs_parser::parse_source_file_owned(
        tsrs_ast::SourceFileParseOptions {
            file_name,
            ..Default::default()
        },
        source,
        kind,
    );
    if !parsed.diagnostics().is_empty() {
        return Err(parsed
            .diagnostics()
            .iter()
            .map(|d| d.localize())
            .collect::<Vec<_>>()
            .join("; "));
    }
    walk(parsed.as_node(), &mut |n| {
        tsrs_ast::set_parent_in_children(n)
    });
    Ok(parsed)
}

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

struct ConfigHost {
    fs: Arc<dyn FS>,
    cwd: String,
}
impl ParseConfigHost for ConfigHost {
    fn fs(&self) -> &dyn FS {
        &*self.fs
    }
    fn get_current_directory(&self) -> &str {
        &self.cwd
    }
}

pub struct TsrsAdapter {
    pub(crate) program: &'static Program,
    pub(crate) context: Context,
    /// Analysis choices that differ from the project's own tsconfig; reported, never blocking.
    pub(crate) assumptions: Vec<&'static str>,
}

pub(crate) const NODE10_AS_BUNDLER: &str = "moduleResolution node/node10 was analysed as bundler: TypeScript 7 removed node10 and resolves unset CommonJS projects the same way";

fn diagnostic(d: P<tsrs_ast::Diagnostic>) -> Finding {
    let mut f = Finding::new(&format!("TS{}", d.code()), d.localize());
    f.file = d.file().map(|file| file.file_name().into());
    f.byte_offset = d.pos();
    f.line = d
        .file()
        .zip(d.pos())
        .map(|(file, pos)| line_at(file.text(), pos as usize) as u32);
    f
}

impl TsrsAdapter {
    pub fn open(root: &Path, project: &str) -> Result<Self, Vec<Finding>> {
        let project = root.join(project).canonicalize().map_err(|e| {
            vec![Finding::new(
                "EC_CONFIG",
                format!("Cannot open project {project}: {e}"),
            )]
        })?;
        let cwd = root.to_string_lossy().into_owned();
        let fs: Arc<dyn FS> = Arc::new(bundled::wrap_fs(osvfs::fs()));
        let parse_host = Box::leak(Box::new(ConfigHost {
            fs: Arc::clone(&fs),
            cwd: cwd.clone(),
        }));
        let (parsed, errors) = tsrs_tsoptions::get_parsed_command_line_of_config_file(
            &project.to_string_lossy(),
            None,
            None,
            parse_host,
            None,
        );
        if !errors.is_empty() {
            return Err(errors.into_iter().map(diagnostic).collect());
        }
        let mut parsed = parsed
            .ok_or_else(|| vec![Finding::new("EC_CONFIG", "tsrs could not parse tsconfig")])?;
        let mut assumptions = vec![];
        // TypeScript 7 rejects node10 (TS5108) yet resolves an unset moduleResolution with a
        // CommonJS module as bundler. Unset it so projects that tsc 5.x accepts can be analysed.
        if let Some(compiler) = parsed.parsed_config.compiler_options
            && compiler.module_resolution == ModuleResolutionKind::NODE10
        {
            let mut compiler = (*compiler).clone();
            compiler.module_resolution = ModuleResolutionKind::UNKNOWN;
            parsed.parsed_config.compiler_options = Some(P::new(compiler));
            assumptions.push(NODE10_AS_BUNDLER);
        }
        let host = new_compiler_host(&cwd, fs, &bundled::lib_path(), None, None, None);
        let mut options = ProgramOptions::new(P::new(parsed), host);
        options.single_threaded = Tristate::True;
        let program = new_program(options);
        if program.options().no_check.is_true() {
            return Err(vec![Finding::new(
                "EC_CONFIG",
                "noCheck must be false for errspec analysis",
            )]);
        }
        let context = Context::default();
        let mut errors = program.get_config_file_parsing_diagnostics();
        errors.extend(program.get_program_diagnostics());
        errors.extend(program.get_syntactic_diagnostics(&context, None));
        errors.extend(program.get_global_diagnostics(&context));
        errors.extend(program.get_semantic_diagnostics(&context, None));
        // TS6059 only constrains emitted paths, and errspec never emits.
        errors.retain(|d| d.code() != 6059);
        if !errors.is_empty() {
            return Err(errors.into_iter().map(diagnostic).collect());
        }
        Ok(Self {
            program,
            context,
            assumptions,
        })
    }

    pub fn tags(&self, root: &Path, source: &Source) -> Result<Vec<String>, String> {
        let path = root
            .join(&source.file)
            .canonicalize()
            .map_err(|e| format!("{}: {e}", source.file))?;
        let file = self
            .program
            .get_source_file(&path.to_string_lossy())
            .ok_or_else(|| format!("{} is not in the TypeScript project", source.file))?;
        let mut checker = self.program.get_type_checker_for_file(&self.context, file);
        let module = file
            .symbol()
            .ok_or("Source must be a module with an exported error type")?;
        let exported = checker.get_exports_of_module_exported(module);
        let mut symbol = exported
            .into_iter()
            .find(|s| s.name() == source.export)
            .ok_or_else(|| format!("Export {} not found in {}", source.export, source.file))?;
        if symbol.flags().intersects(SymbolFlags::Alias) {
            symbol = checker.get_aliased_symbol(symbol);
        }
        if !symbol
            .flags()
            .intersects(SymbolFlags::TypeAlias | SymbolFlags::Interface | SymbolFlags::Class)
        {
            return Err("Export must be a type alias, interface, or class".into());
        }
        let ty = checker.get_declared_type_of_symbol_exported(symbol);
        let mut tags = BTreeSet::new();
        collect_tags(&mut checker, ty, &source.discriminant, &mut tags)?;
        Ok(tags.into_iter().collect())
    }
}

pub(crate) fn collect_tags(
    checker: &mut Checker,
    ty: P<Type>,
    key: &str,
    tags: &mut BTreeSet<String>,
) -> Result<(), String> {
    if ty.flags().intersects(TypeFlags::Never) {
        return Ok(());
    }
    if ty.flags().intersects(TypeFlags::Union) {
        for member in ty.types() {
            collect_tags(checker, *member, key, tags)?;
        }
        return Ok(());
    }
    if ty
        .flags()
        .intersects(TypeFlags::Any | TypeFlags::Unknown | TypeFlags::TypeParameter)
    {
        return Err(format!(
            "Cannot enumerate {}",
            checker.type_to_string_exported(ty)
        ));
    }
    let prop = checker
        .get_property_of_type_exported(ty, key)
        .ok_or_else(|| {
            format!(
                "Missing discriminant {key:?} in {}",
                checker.type_to_string_exported(ty)
            )
        })?;
    if prop.flags().intersects(SymbolFlags::Optional) {
        return Err(format!("Discriminant {key:?} is optional"));
    }
    let prop_type = checker.get_type_of_symbol_exported(prop);
    collect_literals(checker, prop_type, tags)
}

fn collect_literals(
    checker: &mut Checker,
    ty: P<Type>,
    tags: &mut BTreeSet<String>,
) -> Result<(), String> {
    if ty.flags().intersects(TypeFlags::Union) {
        for member in ty.types() {
            collect_literals(checker, *member, tags)?;
        }
        return Ok(());
    }
    if ty.flags().intersects(TypeFlags::StringLiteral)
        && let Some(LiteralValue::String(value)) = ty.as_literal_type().value()
    {
        tags.insert(value.to_owned());
        return Ok(());
    }
    Err(format!(
        "Discriminant must be a finite union of string literals; found {}",
        checker.type_to_string_exported(ty)
    ))
}
