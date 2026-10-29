//! Fixed-point analysis over borrowed facts; no compiler, I/O, or wire-report mutation.
use super::{Channel, Edge, Effect, Origin, Unknown};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct FunctionEffects {
    pub effects: Vec<Effect>,
    pub has_unknown: bool,
}

/// Monotone fixed point over (function, origin, channel). Each fact is inserted once.
/// Witnesses do not participate in equality, so recursive graphs cannot grow paths forever.
/// Results follow the input function order; edge order preserves the first witness.
pub(super) fn evaluate<'a>(
    functions: impl Iterator<Item = (&'a str, bool)>,
    origins: &[Origin],
    edges: &[Edge],
    unknowns: &[Unknown],
) -> Vec<FunctionEffects> {
    let functions: Vec<_> = functions.collect();
    let indices: BTreeMap<_, _> = functions
        .iter()
        .enumerate()
        .map(|(i, (id, _))| (id.to_string(), i))
        .collect();
    let mut facts: Vec<BTreeMap<(String, Channel), Vec<String>>> =
        vec![BTreeMap::new(); functions.len()];
    let mut unknown: BTreeSet<String> = unknowns.iter().map(|u| u.function.clone()).collect();
    for origin in origins {
        if origin.escapes
            && let Some(&i) = indices.get(&origin.function)
        {
            facts[i].insert(
                (origin.id.clone(), origin.channel),
                vec![origin.function.clone()],
            );
        }
    }
    loop {
        let mut changed = false;
        for edge in edges {
            let (Some(&caller), Some(&callee)) =
                (indices.get(&edge.caller), indices.get(&edge.callee))
            else {
                continue;
            };
            if unknown.contains(&edge.callee) && unknown.insert(edge.caller.clone()) {
                changed = true;
            }
            for ((origin, channel), path) in facts[callee].clone() {
                let channel = match channel {
                    Channel::Throw if edge.catches && !edge.may_rethrow => continue,
                    Channel::Throw if functions[caller].1 => Channel::Rejection,
                    Channel::Rejection if edge.awaited && edge.catches && !edge.may_rethrow => {
                        continue;
                    }
                    Channel::Rejection if !edge.awaited && !edge.returned => continue,
                    Channel::Result if !edge.returned => continue,
                    other => other,
                };
                if let std::collections::btree_map::Entry::Vacant(entry) =
                    facts[caller].entry((origin, channel))
                {
                    let mut witness = vec![edge.caller.clone()];
                    witness.extend(path);
                    entry.insert(witness);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    functions
        .into_iter()
        .zip(facts)
        .map(|((id, _), facts)| FunctionEffects {
            effects: facts
                .into_iter()
                .map(|((origin, channel), via)| Effect {
                    origin,
                    channel,
                    via,
                })
                .collect(),
            has_unknown: unknown.contains(id),
        })
        .collect()
}
