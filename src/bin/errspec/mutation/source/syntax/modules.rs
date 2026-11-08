//! Acquire runtime module spelling directly from the parser, without resolution.
use super::*;
use errspec::mutation::syntax::Import;
use std::collections::BTreeSet;
#[derive(Default)]
pub(super) struct Facts {
    pub imports: Vec<Import>,
    pub dependencies: Vec<String>,
}
struct Acquisition {
    facts: Facts,
    dependencies: BTreeSet<String>,
}
pub(super) fn acquire(program: &Program<'_>) -> Facts {
    let mut visitor = Acquisition {
        facts: Facts::default(),
        dependencies: BTreeSet::new(),
    };
    visitor.visit_program(program);
    visitor.facts.dependencies = visitor.dependencies.into_iter().collect();
    visitor.facts
}
impl<'a> Visit<'a> for Acquisition {
    fn visit_ts_type(&mut self, _: &TSType<'a>) {}
    fn enter_node(&mut self, node: AstKind<'a>) {
        match node {
            AstKind::ImportDeclaration(n) if n.import_kind == ImportOrExportKind::Value => {
                let source = n.source.value.display().to_string();
                self.dependencies.insert(source.clone());
                for specifier in n.specifiers.iter().flatten() {
                    let (imported, local) = match specifier {
                        ImportDeclarationSpecifier::ImportSpecifier(n)
                            if n.import_kind == ImportOrExportKind::Value =>
                        {
                            (n.imported.name().to_string(), n.local.name.to_string())
                        }
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(n) => {
                            ("default".into(), n.local.name.to_string())
                        }
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(n) => {
                            ("*".into(), n.local.name.to_string())
                        }
                        _ => continue,
                    };
                    self.facts.imports.push(Import {
                        source: source.clone(),
                        imported,
                        local,
                        resolved: None,
                    });
                }
            }
            AstKind::ExportFromDeclaration(n) if n.export_kind == ImportOrExportKind::Value => {
                self.dependencies
                    .insert(n.source.value.display().to_string());
            }
            AstKind::ExportAllDeclaration(n) if n.export_kind == ImportOrExportKind::Value => {
                self.dependencies
                    .insert(n.source.value.display().to_string());
            }
            AstKind::ImportExpression(n) => {
                if let Expression::StringLiteral(s) = &n.source {
                    self.dependencies.insert(s.value.display().to_string());
                }
            }
            AstKind::CallExpression(n) if matches!(&n.callee, Expression::Identifier(i) if i.name == "require") => {
                if let Some(Argument::StringLiteral(s)) = n.arguments.first() {
                    self.dependencies.insert(s.value.display().to_string());
                }
            }
            _ => (),
        }
    }
}
