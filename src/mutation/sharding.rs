//! Deterministic partitions of the requested candidate inventory.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, str::FromStr};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Shard {
    pub index: u32,
    pub total: u32,
}
impl Shard {
    pub fn validate(&self) -> Result<(), String> {
        if self.index == 0 || self.total == 0 || self.index > self.total {
            return Err("shard requires 1 <= index <= total".into());
        }
        Ok(())
    }
    pub fn select<'a>(
        &self,
        keys: impl IntoIterator<Item = &'a str>,
    ) -> Result<(BTreeSet<String>, Info), String> {
        self.validate()?;
        let mut seen = BTreeSet::new();
        let mut selected = BTreeSet::new();
        for key in keys {
            if key.len() != 64 || !key.bytes().all(|c| c.is_ascii_hexdigit()) || !seen.insert(key) {
                return Err("Invalid or duplicate sharding candidate identity".into());
            }
            let prefix = u64::from_str_radix(&key[..16], 16).map_err(|error| error.to_string())?;
            if prefix % u64::from(self.total) == u64::from(self.index - 1) {
                selected.insert(key.to_owned());
            }
        }
        let info = Info {
            index: self.index,
            total: self.total,
            algorithm: "sha256_prefix_v1",
            eligible: seen.len(),
            selected: selected.len(),
            omitted: seen.len() - selected.len(),
        };
        Ok((selected, info))
    }
}
impl FromStr for Shard {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, String> {
        let (index, total) = value.split_once('/').ok_or("shard must be INDEX/TOTAL")?;
        let shard = Self {
            index: index
                .parse()
                .map_err(|_| "shard index must be a positive integer")?,
            total: total
                .parse()
                .map_err(|_| "shard total must be a positive integer")?,
        };
        shard.validate()?;
        Ok(shard)
    }
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub index: u32,
    pub total: u32,
    pub algorithm: &'static str,
    pub eligible: usize,
    pub selected: usize,
    pub omitted: usize,
}
