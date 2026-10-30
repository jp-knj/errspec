# @errspec/analysis-driver (private)

The Rust CLI embeds these ESM sources and writes them to a temporary directory for each analysis request. No build step or installed driver package is required by consumers. The compiler is loaded from the user's installed package; it is neither bundled nor downloaded.

TypeScript 6 uses its Compiler API. TypeScript 7 uses `unstable/sync` and `unstable/ast`; those interfaces may change. The development aliases pin 6.0.3 and 7.0.2 for comparisons against the same native fixtures, plus 5.9.3 for mutant typechecking tests. Use Node.js 24 for development; the analysis driver requires Node.js 22 or later.

```sh
npm ci --ignore-scripts --no-audit --no-fund
# From the repository root, after cargo build and installing examples/checkout:
node --test tests/analysis-driver.test.mjs
```

Protocol 1 accepts one JSON request on stdin: absolute `package`, `project`, and `root` paths, ordered `sources`, optional `discriminants`, and `scenarioFiles`. Stdout contains one JSON response with the actual compiler version, compiler diagnostics, indexed tag results, optional scan facts, and scenario call facts. Logs use stderr. Compiler errors stop acquisition; failures to access an API or decode a type fail the process. A tag that is not a finite discriminated type is a per-source error, distinct from a bridge failure and from the empty variant set of `never`.

Compiler handles stay inside this process. Rust validates response identity and references, computes propagation and static scenario judgments, and owns process-group cleanup, interruption, a 300-second deadline, and output limits. API snapshots and servers close in `finally`.

Locations use relative paths and UTF-8 byte offsets. To preserve the native scanner's coordinate convention, an initial UTF-8 BOM is excluded. Source contents are checked against acquired compiler text and checked again before output. Array order and IDs are preserved in comparisons.

This package performs semantic analysis for `scan`, `init`, `check`, `verify`, `snapshot`, and `coverage`. It does not typecheck generated mutants.
