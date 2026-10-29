//! Isolate unpublished tsrs APIs. No native handles escape into reports.
//! Compiler arena allocations live until this one-shot CLI process exits.
use errspec::model::{Finding, Source};
use std::{collections::BTreeSet, path::Path, sync::Arc};
use tsrs_ast::SymbolFlags;
use tsrs_checker::{Checker, LiteralValue, Type, TypeFlags};
use tsrs_compiler::{Context, Program, ProgramOptions, new_compiler_host, new_program};
use tsrs_core::{P, Tristate};
use tsrs_tsoptions::ParseConfigHost;
use tsrs_vfs::{FS, bundled, osvfs};

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
}

fn diagnostic(d: P<tsrs_ast::Diagnostic>) -> Finding {
    let mut f = Finding::new(&format!("TS{}", d.code()), d.localize());
    f.file = d.file().map(|file| file.file_name().to_owned());
    f.byte_offset = d.pos();
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
        let parsed = parsed
            .ok_or_else(|| vec![Finding::new("EC_CONFIG", "tsrs could not parse tsconfig")])?;
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
        if !errors.is_empty() {
            return Err(errors.into_iter().map(diagnostic).collect());
        }
        Ok(Self { program, context })
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
