//! Configuration accepted at the input boundary. Wire types remain in `model`.
use crate::model::{Config, Finding, ObservationRequirements, Source, TestRef, scenario_test_name};
use std::collections::BTreeSet;

/// Created only by validating a complete configuration. Access is read-only.
#[derive(Debug)]
pub struct ValidatedConfig {
    schema_version: u32,
    project: String,
    contracts: Vec<ValidatedContract>,
}

#[derive(Debug)]
pub struct ValidatedContract {
    id: String,
    source: Source,
    boundaries: Vec<String>,
    cases: Vec<ValidatedCase>,
}

#[derive(Debug)]
pub struct ValidatedCase {
    tag: String,
    expect: String,
    test: TestRef,
    kind: CaseKind,
}

/// A scenario always has both its identity and its observation requirements.
#[derive(Debug)]
pub enum CaseKind {
    Legacy,
    Scenario {
        id: String,
        observe: ObservationRequirements,
    },
}

impl ValidatedConfig {
    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }
    pub fn project(&self) -> &str {
        &self.project
    }
    pub fn contracts(&self) -> &[ValidatedContract] {
        &self.contracts
    }
}

impl ValidatedContract {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn source(&self) -> &Source {
        &self.source
    }
    pub fn boundaries(&self) -> &[String] {
        &self.boundaries
    }
    pub fn cases(&self) -> &[ValidatedCase] {
        &self.cases
    }
}

impl ValidatedCase {
    pub fn tag(&self) -> &str {
        &self.tag
    }
    pub fn expect(&self) -> &str {
        &self.expect
    }
    pub fn test(&self) -> &TestRef {
        &self.test
    }
    pub fn kind(&self) -> &CaseKind {
        &self.kind
    }
}

impl TryFrom<Config> for ValidatedConfig {
    type Error = Finding;

    fn try_from(config: Config) -> Result<Self, Finding> {
        if !matches!(config.schema_version, 1 | 2)
            || config.project.trim().is_empty()
            || config.contracts.is_empty()
        {
            return Err(Finding::new(
                "EC_CONFIG",
                "Require schemaVersion: 1 or 2, a project, and at least one contract",
            ));
        }
        let mut ids = BTreeSet::new();
        let mut contracts = Vec::with_capacity(config.contracts.len());
        for c in config.contracts {
            if c.id.trim().is_empty() || !ids.insert(c.id.clone()) {
                return Err(Finding::new(
                    "EC_CONFIG",
                    format!("Empty or duplicate contract id: {:?}", c.id),
                ));
            }
            if [&c.source.file, &c.source.export, &c.source.discriminant]
                .iter()
                .any(|s| s.trim().is_empty())
            {
                return Err(Finding::new(
                    "EC_CONFIG",
                    format!("Incomplete source in {}", c.id),
                ));
            }
            let mut identities = BTreeSet::new();
            let mut cases = Vec::with_capacity(c.cases.len());
            for mut case in c.cases {
                let invalid = |message: &str| {
                    Finding::new("EC_CONFIG", message)
                        .for_case(&c.id, &case.tag)
                        .for_scenario(case.id.as_deref())
                };
                if [&case.expect, &case.test.file]
                    .iter()
                    .any(|s| s.trim().is_empty())
                {
                    return Err(invalid("Case requires expect and test.file"));
                }
                let kind = if config.schema_version == 1 {
                    if case.id.is_some()
                        || case.observe.is_some()
                        || case.test.name.trim().is_empty()
                    {
                        return Err(invalid(
                            "Version 1 requires test.name; scenario ids and observe require schemaVersion: 2",
                        ));
                    }
                    if !identities.insert(case.tag.clone()) {
                        return Err(invalid("Duplicate case tag"));
                    }
                    CaseKind::Legacy
                } else {
                    let Some(id) = case.id.as_deref().filter(|id| !id.trim().is_empty()) else {
                        return Err(invalid("Version 2 requires a nonempty scenario id"));
                    };
                    if !identities.insert(id.to_owned()) {
                        return Err(invalid("Duplicate scenario id within contract"));
                    }
                    if !case.test.name.is_empty() && case.test.name != scenario_test_name(&c.id, id)
                    {
                        return Err(invalid(
                            "Version 2 uses SDK-generated test names; omit test.name",
                        ));
                    }
                    let Some(requirements) = &case.observe else {
                        return Err(invalid(
                            "Version 2 requires observe.faults and observe.assertions",
                        ));
                    };
                    for list in [&requirements.faults, &requirements.assertions] {
                        let unique: BTreeSet<_> = list.iter().collect();
                        if list.is_empty()
                            || unique.len() != list.len()
                            || list.iter().any(|id| id.trim().is_empty())
                        {
                            return Err(invalid(
                                "Observation ids must be nonempty, unique, nonempty lists",
                            ));
                        }
                    }
                    case.test.name = scenario_test_name(&c.id, id);
                    CaseKind::Scenario {
                        id: case.id.expect("validated scenario id"),
                        observe: case.observe.expect("validated observation requirements"),
                    }
                };
                cases.push(ValidatedCase {
                    tag: case.tag,
                    expect: case.expect,
                    test: case.test,
                    kind,
                });
            }
            contracts.push(ValidatedContract {
                id: c.id,
                source: c.source,
                boundaries: c.boundaries,
                cases,
            });
        }
        Ok(Self {
            schema_version: config.schema_version,
            project: config.project,
            contracts,
        })
    }
}
