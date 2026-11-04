//! Ownership protocol judgments. No process lookup or signalling is performed here.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Record {
    pub version: u8,
    pub run_id: String,
    pub token: String,
    pub owner: u32,
    pub supervisor: u32,
    pub socket: PathBuf,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Response {
    pub version: u8,
    pub run_id: String,
    pub owner: u32,
    pub supervisor: u32,
    pub group_owned: bool,
    pub orphaned: bool,
    pub terminating: bool,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Active,
    Recovered,
    Unverified,
}
impl Record {
    pub fn valid(&self) -> bool {
        self.version == 1
            && !self.run_id.is_empty()
            && self.run_id.len() <= 128
            && self.run_id.bytes().all(|c| c.is_ascii_alphanumeric())
            && self.token.len() == 64
            && self.token.bytes().all(|c| c.is_ascii_hexdigit())
            && self.owner > 0
            && self.supervisor > 1
            && self.owner != self.supervisor
    }
    pub fn judge(&self, response: &Response) -> Decision {
        if !self.valid()
            || response.version != 1
            || response.run_id != self.run_id
            || response.owner != self.owner
            || response.supervisor != self.supervisor
            || !response.group_owned
        {
            return Decision::Unverified;
        }
        match (response.orphaned, response.terminating) {
            (false, false) => Decision::Active,
            (true, true) => Decision::Recovered,
            _ => Decision::Unverified,
        }
    }
}
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub active: usize,
    pub recovered: usize,
    pub unverified: usize,
    #[serde(skip_serializing_if = "is_zero")]
    pub endpoint_unavailable: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}
impl Info {
    pub fn is_empty(&self) -> bool {
        self.active == 0
            && self.recovered == 0
            && self.unverified == 0
            && self.endpoint_unavailable == 0
            && self.warnings.is_empty()
    }
}
fn is_zero(value: &usize) -> bool {
    *value == 0
}
