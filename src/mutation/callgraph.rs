//! Experimental source-call relationships. Test-code value flow is not acquired.
use super::syntax::{Metadata, function_node, scope_key};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Node {
    file: String,
    module: bool,
    opaque: bool,
    edges: BTreeSet<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Graph {
    nodes: BTreeMap<String, Node>,
}
fn promote<'a>(source: &'a Metadata, mut owner: &'a str) -> Option<&'a str> {
    let mut seen = BTreeSet::new();
    loop {
        let scope = source.scopes.get(owner)?;
        if !seen.insert(owner) {
            return None;
        }
        if owner == "module" || scope.function {
            return Some(owner);
        }
        owner = scope.parent.as_deref()?;
    }
}
struct Names<'a> {
    sources: &'a BTreeMap<String, &'a Metadata>,
}
impl Names<'_> {
    fn member(&self, file: &str, parent: &str, names: &[&str]) -> Option<String> {
        let source = self.sources.get(file)?;
        if names.is_empty() {
            return source
                .scopes
                .get(parent)?
                .function
                .then(|| scope_key(file, parent));
        }
        let mut matches = source.scopes.iter().filter(|(_, scope)| {
            scope.parent.as_deref() == Some(parent) && scope.name.as_deref() == Some(names[0])
        });
        let (name, _) = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        self.member(file, name, &names[1..])
    }
    fn shadowed_parameter(source: &Metadata, owner: &str, name: &str) -> bool {
        let Some(function) = source
            .scopes
            .get(owner)
            .and_then(|scope| function_node(source, scope))
        else {
            return false;
        };
        function
            .children
            .iter()
            .filter_map(|index| source.nodes.get(*index))
            .filter(|node| node.kind == "Parameter")
            .any(|parameter| {
                parameter.name.as_ref().is_none_or(|binding| {
                    binding == name
                        || binding.chars().any(|character| {
                            !character.is_alphanumeric() && !matches!(character, '_' | '$')
                        })
                })
            })
    }
    fn imported(&self, file: &str, names: &[&str]) -> Option<String> {
        let source = self.sources.get(file)?;
        let mut bindings = source
            .imports
            .iter()
            .filter(|binding| binding.local == names[0]);
        let binding = bindings.next()?;
        if bindings.next().is_some() {
            return None;
        }
        let target = binding.resolved.as_ref()?;
        let target_source = self.sources.get(target)?;
        if binding.imported == "*" {
            return self.member(target, "module", &names[1..]);
        }
        if binding.imported == "default" {
            // Only the parser's actual default declaration identifies a binding.
            let mut defaults = target_source.scopes.iter().filter(|(_, scope)| {
                scope.parent.as_deref() == Some("module")
                    && target_source.nodes.iter().any(|node| {
                        node.start == scope.start
                            && node.end == scope.end
                            && node
                                .children
                                .iter()
                                .filter_map(|index| target_source.nodes.get(*index))
                                .any(|child| child.kind == "DefaultKeyword")
                    })
            });
            let (owner, _) = defaults.next()?;
            if defaults.next().is_some() {
                return None;
            }
            return self.member(target, owner, &names[1..]);
        }
        let mut imported = vec![binding.imported.as_str()];
        imported.extend_from_slice(&names[1..]);
        self.member(target, "module", &imported)
    }
    fn target<'a>(&'a self, file: &str, mut owner: &'a str, callee: &str) -> Option<String> {
        let source = self.sources.get(file)?;
        let names = callee.split('.').collect::<Vec<_>>();
        let mut seen = BTreeSet::new();
        if names[0] == "this" {
            loop {
                if !seen.insert(owner) {
                    return None;
                }
                let scope = source.scopes.get(owner)?;
                if let Some(function) = function_node(source, scope) {
                    if function.kind == "MethodDeclaration" {
                        return self.member(file, scope.parent.as_deref()?, &names[1..]);
                    }
                    // Only arrows retain their enclosing method's receiver.
                    if function.kind != "ArrowFunction" {
                        return None;
                    }
                }
                owner = scope.parent.as_deref()?;
            }
        }
        loop {
            if !seen.insert(owner) || Self::shadowed_parameter(source, owner, names[0]) {
                return None;
            }
            let scope = source.scopes.get(owner)?;
            let children = source
                .scopes
                .values()
                .filter(|scope| {
                    scope.parent.as_deref() == Some(owner)
                        && scope.name.as_deref() == Some(names[0])
                })
                .count();
            if children > 0 {
                return self.member(file, owner, &names);
            }
            if scope.name.as_deref() == Some(names[0]) {
                return self.member(file, owner, &names[1..]);
            }
            let Some(parent) = &scope.parent else {
                break;
            };
            owner = parent;
        }
        self.imported(file, &names)
    }
}
impl Graph {
    pub fn build(sources: &BTreeMap<String, &Metadata>) -> Option<Self> {
        let names = Names { sources };
        let mut nodes = BTreeMap::new();
        for (file, source) in sources {
            for (owner, scope) in &source.scopes {
                if owner == "module" || scope.function {
                    nodes.insert(
                        scope_key(file, owner),
                        Node {
                            file: file.clone(),
                            module: owner == "module",
                            opaque: false,
                            edges: BTreeSet::new(),
                        },
                    );
                }
            }
        }
        for (file, source) in sources {
            for call in &source.calls {
                let owner = promote(source, &call.scope)?;
                let target = call
                    .callee
                    .as_ref()
                    .and_then(|callee| names.target(file, &call.scope, callee));
                let node = nodes.get_mut(&scope_key(file, owner))?;
                if let Some(target) = target {
                    node.edges.insert(target);
                } else {
                    node.opaque = true;
                }
            }
        }
        let graph = Self { nodes };
        graph.valid().then_some(graph)
    }
    pub fn valid(&self) -> bool {
        !self.nodes.is_empty()
            && self.nodes.iter().all(|(key, node)| {
                serde_json::from_str::<(String, String)>(key).is_ok_and(|(file, owner)| {
                    file == node.file
                        && !file.is_empty()
                        && !owner.is_empty()
                        && node.module == (owner == "module")
                }) && node
                    .edges
                    .iter()
                    .all(|target| self.nodes.contains_key(target))
            })
    }
    pub fn matches_scopes<'a>(&self, scopes: impl Iterator<Item = (&'a str, &'a str)>) -> bool {
        let scopes = scopes.collect::<BTreeMap<_, _>>();
        scopes.len() == self.nodes.len()
            && scopes
                .iter()
                .all(|(key, file)| self.nodes.get(*key).is_some_and(|node| node.file == *file))
    }
    fn reaches(&self, from: &str, to: &str) -> bool {
        let Some(target) = self.nodes.get(to) else {
            return true;
        };
        let mut seen = BTreeSet::new();
        let mut pending = vec![from];
        while let Some(key) = pending.pop() {
            if !seen.insert(key) {
                continue;
            }
            let Some(node) = self.nodes.get(key) else {
                return true;
            };
            if key == to || node.opaque || node.module && node.file == target.file {
                return true;
            }
            pending.extend(node.edges.iter().map(String::as_str));
        }
        false
    }
    /// Unknown acquisitions stay related. A false result is still experimental:
    /// functions composed only in tests have no source call connecting them.
    pub fn related(&self, a: &str, b: &str) -> bool {
        !self.valid() || self.reaches(a, b) || self.reaches(b, a)
    }
}
