# Rust code ownership and snapshot tests

Place Rust functions and types according to their responsibility and input/output
contract.

## Functions and types

- A function used by one feature belongs to that feature's module.
- Share a function in a clearly named common module only when its input conditions,
  result meaning and failure conditions are the same for every caller.
- Do not split files by line count alone or combine functions with different
  purposes/failure conditions merely because their code looks similar.
- Separate file acquisition from judgment. Judgment functions receive acquired
  values and file-check results and return results and diagnostics.

`tsrs_adapter` acquires positions/text from the tsrs AST and supplies recursive
traversal. `scan` and `scenario_source` consume it; the adapter does not depend on
individual features. Body-only traversal and propagation rules stay with the
feature that owns their meaning.

`analysis_backend` chooses analysis providers and manages external API requests
and responses. Official-API adapters return owned facts; Rust shares propagation
and static-matching rules. Do not embed compiler-specific type acquisition in
judgment functions. Compare backends on the same input after changing declaration
or location interpretation.

Compiler-independent common data belongs in `model`. `Location` represents source
positions; the existing `discovery::Location` re-export remains available. Do not
place compiler handles in shared models just to facilitate reuse.

## Snapshot analysis and judgments

Use the development dependency [insta](https://insta.rs/docs/quickstart/) to compare
representative whole results. Commit expectations under `tests/snapshots/`. Use
JSON for serializable results and Debug snapshots for non-serialized internal
judgments, retaining existing assertions.

Cover type candidates, propagation paths, static-match diagnostics, contract
registration, test/observation matching, tag summaries and baseline differences.
When an assertion previously compared only a fragment, also fix the combination
of the whole result and its diagnostics.

Create expectations before moving shared functions. Normalize only the absolute
temporary-directory prefix created by that test and its canonical spelling to
`<project>/`. Relative paths, IDs, byte offsets, line numbers, diagnostics and array
order remain observable. Do not add filters that erase all paths or sort arrays
before comparison.

The CLI `snapshot` command stores consumer scenario history. Rust `.snap` files
check changes in errspec's own behavior. Neither replaces the other.

## Review expected changes

Refactors must match existing expectations without updating them. Use a second
temporary directory to verify that the expectation does not depend on its location.
For intentional behavior changes, inspect Requirements, Design and existing
assertions first, then generate a candidate with the relevant test:

```sh
INSTA_UPDATE=new cargo test --locked --test discovery
```

Compare `.snap` with `.snap.new` for changes in inputs, results, diagnostics and
order. Use `cargo insta review` when cargo-insta is installed, or
`git diff --no-index <existing.snap> <candidate.snap.new>`. Adopt only reviewed
candidates. `INSTA_UPDATE=unseen` can create a first expectation without replacing
an existing one.

Review `git diff -- tests/snapshots` after adoption and explain the behavior and
reason in the PR or discussion. A passing test is not a reason for bulk updates.

```sh
INSTA_UPDATE=no cargo test --locked
```

CI uses `INSTA_UPDATE=no`. Also run the appropriate fmt, clippy, testkit and
Bun/Vitest acceptance checks described by the engineering guides and workflows.
