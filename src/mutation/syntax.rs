//! Owned syntax facts; compiler handles stay in the acquiring adapter.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub version: u8,
    pub scopes: BTreeMap<String, Scope>,
    pub owners: BTreeMap<String, String>,
    pub nodes: Vec<SyntaxNode>,
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
