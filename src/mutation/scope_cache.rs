//! Conservative reuse over acquired, complete scope-entry and test-file cohorts.
//! The acquiring adapter must establish envelope hashes and entry-probe ownership.
use super::{Mutant, cache::Record};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Observation {
    pub entry_key: String,
    pub files: BTreeSet<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Scope {
    pub file: String,
    pub hash: String,
    /// Unknown calls/reflection prevent excluding transitive effects after edits.
    pub opaque: bool,
    /// None means entry observation was unavailable, not that the scope was unused.
    pub observation: Option<Observation>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Context {
    pub eligible: bool,
    /// Bound environment/dependencies/configuration/tests and module effects.
    pub envelope: String,
    pub scopes: BTreeMap<String, Scope>,
    pub candidates: BTreeMap<String, String>,
    /// Current stable test ID -> file; fresh probe acquisition is required.
    pub tests: BTreeMap<String, String>,
}
fn hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
impl Context {
    fn valid(&self) -> bool {
        let files = self.tests.values().collect::<BTreeSet<_>>();
        self.eligible
            && hash(&self.envelope)
            && !self.tests.is_empty()
            && self
                .tests
                .iter()
                .all(|(id, file)| !id.is_empty() && !file.is_empty())
            && self
                .candidates
                .iter()
                .all(|(key, owner)| hash(key) && self.scopes.contains_key(owner))
            && self.scopes.iter().all(|(owner, scope)| {
                !owner.is_empty()
                    && !scope.file.is_empty()
                    && hash(&scope.hash)
                    && scope.observation.as_ref().is_none_or(|observation| {
                        self.candidates.get(&observation.entry_key) == Some(owner)
                            && observation.files.iter().all(|file| files.contains(file))
                    })
            })
    }
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ScopedRecord {
    owner: String,
    hash: String,
    outcome: Record,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct File {
    kind: String,
    schema_version: u8,
    acquisition_mode: String,
    context: Context,
    records: BTreeMap<String, ScopedRecord>,
}
enum Affected {
    All,
    Files(BTreeSet<String>),
}
fn affected(before: &Context, current: &Context) -> Affected {
    if before.scopes.keys().ne(current.scopes.keys()) {
        return Affected::All;
    }
    let mut files = BTreeSet::new();
    for (name, old) in &before.scopes {
        let new = &current.scopes[name];
        if old.file != new.file {
            return Affected::All;
        }
        if old.hash == new.hash && old.observation == new.observation {
            continue;
        }
        let (Some(old_entry), Some(new_entry)) = (&old.observation, &new.observation) else {
            return Affected::All;
        };
        if old.opaque || new.opaque {
            return Affected::All;
        }
        files.extend(old_entry.files.iter().chain(&new_entry.files).cloned());
    }
    Affected::Files(files)
}
impl File {
    pub fn new(context: Context) -> Option<Self> {
        context.valid().then_some(Self {
            kind: "errspec/mutation-scope-results".into(),
            schema_version: 1,
            acquisition_mode: "all_tests_scope_entries_v1".into(),
            context,
            records: BTreeMap::new(),
        })
    }
    pub fn record(&mut self, mutant: &Mutant, selected: &[String]) -> bool {
        let Some(owner) = self.context.candidates.get(&mutant.candidate.key) else {
            return false;
        };
        let Some(scope) = self.context.scopes.get(owner) else {
            return false;
        };
        if scope.file != mutant.candidate.location.file
            || mutant
                .covered_by
                .iter()
                .any(|id| !self.context.tests.contains_key(id))
        {
            return false;
        }
        let Some(outcome) = Record::capture(mutant, selected) else {
            return false;
        };
        self.records.insert(
            mutant.candidate.key.clone(),
            ScopedRecord {
                owner: owner.clone(),
                hash: scope.hash.clone(),
                outcome,
            },
        );
        true
    }
    pub fn reuse(&self, current: &Context, mutant: &mut Mutant, selected: &[String]) -> bool {
        if self.kind != "errspec/mutation-scope-results"
            || self.schema_version != 1
            || self.acquisition_mode != "all_tests_scope_entries_v1"
            || !self.context.valid()
            || !current.valid()
            || self.context.envelope != current.envelope
            || self.context.tests != current.tests
        {
            return false;
        }
        let Some(owner) = current.candidates.get(&mutant.candidate.key) else {
            return false;
        };
        let Some(scope) = current.scopes.get(owner) else {
            return false;
        };
        let Some(record) = self.records.get(&mutant.candidate.key) else {
            return false;
        };
        if &record.owner != owner
            || self.context.candidates.get(&mutant.candidate.key) != Some(owner)
            || record.hash != scope.hash
            || self
                .context
                .scopes
                .get(owner)
                .is_none_or(|old| old.hash != record.hash)
            || scope.file != mutant.candidate.location.file
            || mutant
                .covered_by
                .iter()
                .any(|id| !current.tests.contains_key(id))
        {
            return false;
        }
        match affected(&self.context, current) {
            Affected::All => false,
            Affected::Files(files) => {
                let changed = !files.is_empty()
                    || self
                        .context
                        .scopes
                        .iter()
                        .any(|(name, scope)| scope.hash != current.scopes[name].hash);
                if changed && (scope.opaque || self.context.scopes[owner].opaque)
                    || mutant
                        .covered_by
                        .iter()
                        .any(|id| files.contains(&current.tests[id]))
                {
                    return false;
                }
                record.outcome.apply(mutant, selected)
            }
        }
    }
}
