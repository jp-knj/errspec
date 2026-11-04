//! Owned syntax facts; compiler handles stay in the acquiring adapter.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub version: u8,
    pub source_hash: String,
    pub scopes: BTreeMap<String, Scope>,
    pub owners: BTreeMap<String, String>,
    pub nodes: Vec<SyntaxNode>,
}

pub fn source_matches(metadata: &Metadata, source: &str) -> bool {
    use sha2::{Digest, Sha256};
    metadata.source_hash == format!("{:x}", Sha256::digest(source.as_bytes()))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scope {
    pub hash: String,
    pub effect_hash: String,
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub end_line: usize,
    pub parent: Option<String>,
    pub name: Option<String>,
    pub references: BTreeSet<String>,
    pub pure: bool,
    pub function: bool,
    pub unknown_calls: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_key: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyntaxNode {
    pub kind: String,
    pub start: usize,
    pub end: usize,
    pub parent: Option<usize>,
    pub name: Option<String>,
    pub children: Vec<usize>,
}

/// Return the direct runtime body, never a nested callback's body.
pub fn function_body<'a>(metadata: &'a Metadata, scope: &Scope) -> Option<&'a SyntaxNode> {
    if !scope.function {
        return None;
    }
    let mut function = metadata.nodes.iter().find(|node| {
        node.start == scope.start
            && node.end == scope.end
            && matches!(
                node.kind.as_str(),
                "FunctionDeclaration"
                    | "MethodDeclaration"
                    | "FunctionExpression"
                    | "ArrowFunction"
                    | "VariableDeclaration"
                    | "PropertyDeclaration"
            )
    })?;
    if matches!(
        function.kind.as_str(),
        "VariableDeclaration" | "PropertyDeclaration"
    ) {
        function = function
            .children
            .iter()
            .filter_map(|index| metadata.nodes.get(*index))
            .find(|node| matches!(node.kind.as_str(), "ArrowFunction" | "FunctionExpression"))?;
    }
    metadata.nodes.get(*function.children.last()?)
}
