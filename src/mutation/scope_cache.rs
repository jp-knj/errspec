//! Conservative reuse over acquired, complete scope-entry and test-file cohorts.
//! The acquiring adapter must establish envelope hashes and entry-probe ownership.
use super::{Candidate, Execution, Mutant, cache::Record, syntax::Metadata};
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
pub struct Source<'a> {
    pub text: &'a str,
    pub metadata: &'a Metadata,
    pub candidates: &'a [Candidate],
}
fn owner_id(file: &str, owner: &str) -> String {
    serde_json::to_string(&(file, owner)).unwrap()
}

/// Build a cohort from the current full probe. Module effects and all other
/// inputs belong in the caller-acquired envelope. No old observations stand in
/// for missing current entry evidence.
pub fn from_probe(
    envelope: String,
    sources: &BTreeMap<String, Source<'_>>,
    probe: &Execution,
    eligible: bool,
) -> Result<Context, String> {
    super::validate_execution(probe, &[])?;
    super::validate_baseline(probe, "Current scope probe")?;
    let tests = probe
        .tests
        .iter()
        .map(|test| (test.id.clone(), test.file.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut scopes = BTreeMap::new();
    let mut candidates = BTreeMap::new();
    for (file, source) in sources {
        super::selection::validate(source.metadata, source.text, source.candidates)?;
        if !source.metadata.scopes.contains_key("module") {
            return Err("Missing module scope".into());
        }
        for (name, scope) in &source.metadata.scopes {
            if name != "module" && !scope.function {
                continue;
            }
            let observation = scope.entry_key.as_ref().and_then(|key| {
                let mut files = probe
                    .tests
                    .iter()
                    .filter(|test| {
                        test.started
                            && test.status == "passed"
                            && test.hits.get(key).is_some_and(|count| *count > 0)
                    })
                    .map(|test| test.file.clone())
                    .collect::<BTreeSet<_>>();
                files.extend(probe.static_hits.get(key).into_iter().flatten().cloned());
                if files.is_empty() && probe.hits.get(key).is_some_and(|count| *count > 0) {
                    return None;
                }
                Some(Observation {
                    entry_key: key.clone(),
                    files,
                })
            });
            scopes.insert(
                owner_id(file, name),
                Scope {
                    file: file.clone(),
                    hash: if name == "module" {
                        scope.effect_hash.clone()
                    } else {
                        scope.hash.clone()
                    },
                    opaque: scope.unknown_calls,
                    observation,
                },
            );
        }
        for candidate in source.candidates {
            if &candidate.location.file != file {
                return Err("Scope candidate file mismatch".into());
            }
            let mut owner = source
                .metadata
                .owners
                .get(&candidate.key)
                .ok_or("Missing candidate owner")?
                .as_str();
            while owner != "module" && !source.metadata.scopes[owner].function {
                owner = source.metadata.scopes[owner]
                    .parent
                    .as_deref()
                    .ok_or("Missing enclosing function or module")?;
            }
            if candidates
                .insert(candidate.key.clone(), owner_id(file, owner))
                .is_some()
            {
                return Err("Duplicate scope candidate key".into());
            }
        }
    }
    if probe
        .hits
        .keys()
        .chain(probe.static_hits.keys())
        .chain(probe.tests.iter().flat_map(|test| test.hits.keys()))
        .chain(probe.tests.iter().flat_map(|test| test.infected.iter()))
        .any(|key| !candidates.contains_key(key))
    {
        return Err("Unknown scope probe candidate".into());
    }
    let context = Context {
        eligible,
        envelope,
        scopes,
        candidates,
        tests,
    };
    if !context.valid() {
        return Err("Incomplete or invalid scope cache context".into());
    }
    Ok(context)
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
