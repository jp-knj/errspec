# Error discovery and propagation design

## About this document

This implements the [Requirements](requirements.md) under the [Product](../../product.md)
with resolved compiler types/symbols and a finite graph analysis. Maintainers need
to distinguish types from runtime values and understand conservative analysis.
Test execution/observation matching remains a separate feature.

## Acquisition and ownership

Share AST positions, text and recursive traversal through the tsrs adapter.
Function-body traversal and propagation rules belong to discovery. Define
`Location` in the common model and retain `discovery::Location` as a re-export,
following [Rust ownership and snapshot standards](../../../../engineering/guidelines/rust-code.md).

Open tsconfig with the existing adapter and traverse compiler source files/ASTs.
Resolve actual function declarations and const initializers through symbols,
not regexes or matching names. Matching signatures do not imply the same
implementation: parameters and mutable aliases remain unresolved. Link member
calls to the signature's implementation candidate while also recording dynamic-
dispatch uncertainty.

Extract finite string discriminator values from exported type aliases, interfaces
and classes. Group public names by resolved declaration position and discriminator.
Prefer an export from the declaring file; otherwise choose the first ordered path.
List the remaining names in `reexports`. Inspect each union member's discriminator
symbol declaration. If every declaration is in node_modules or outside tsconfig's
directory, mark `external: true`; this includes adapted dependency types such as
`Omit<SdkIssue, 'at'>`. Members without identified declarations count as local.

At failure sources, collect error aliases/symbols and union-member declaration
positions. Match candidate declarations by position and deduplicate their channels
in `flows`; equal names in different files do not link. Intermediate union aliases
may disappear during type expansion and therefore be absent from `flows`.

Recognize `Promise.reject` only after checking the standard-library declaration
and the receiver's value binding. A parameter typed PromiseConstructor is not the
built-in Promise value. Report the assumption that declarations and built-in
globals are not replaced at runtime. Track Promise aliases through await/return
only for const bindings.

## Propagation

Acquire direct throws, rejections and error-valued returns per function. Propagate
possibilities over call edges to a fixed point on the finite set of origins, so
recursion terminates. Distinguish synchronous throw, async rejection and Result
values, recording await, return and catch conditions on edges. Keep unresolved
calls separately: an empty error set cannot prove completeness.

Separate a caught original failure from failures created inside the catch.
Conservatively retain the original when rethrow is possible. Always state the
scope and unresolved reasons for dynamic dispatch and callback execution.

AST acquisition returns owned function identities, origins, call edges and unknown
sites in `ScanFacts`; it does not store computed `effects` or `hasUnknown` in its
collection records. Pure propagation borrows those facts and returns summaries in
input function order without compiler/filesystem access or input/report mutation.

After a successful scan, move acquired facts and computed summaries into the
report. Sort/deduplicate file lists and sort type candidates at this assembly
boundary. Retain function, edge, origin and unresolved-site order, fixed-point
iteration order and the first discovered propagation path. Do not silently replace
it with a shortest path or a new analysis rule. Configuration/type failures still
return incomplete reports.

## Flow and data

A single CLI request crosses the shared analysis-thread boundary, then performs
type analysis, AST acquisition, callee resolution, fixed point and report assembly.
The output boundary chooses presentation and exit status; no separate sequence
diagram is needed. Do not share a whole mutable report as work state or pass
compiler handles into the fixed-point calculation.

File/position identifies functions and origins. An edge carries caller, callee,
position and handling conditions. Function summaries reference origin IDs. Position
changes alter these IDs, so scan output is not a persistent baseline.

## Interfaces and compatibility

`scan --project tsconfig.json --format json` produces `errspec/scan`.
`--discriminant` defaults to `_tag`, `kind`, `type`, `code`. Normal completion exits
0; `--fail-on-unknown` exits 1 when unresolved sites remain; configuration/analysis
failure exits 2. Completion does not establish runtime exhaustiveness.

Retain Rust `propagate` as a compatibility entry that calls the same calculation
and updates only function summaries in a report. CLI acquisition assembles reports
from facts. Preserve JSON shape/field order, diagnostics and exit codes.

## Integration and data handling

Use fixed tsrs or installed official TS 6/7 APIs under the
[TypeScript compatibility requirements](../typescript-compatibility/requirements.md).
Every backend returns owned facts and shares Rust propagation. Read source into
memory and return stdout results without caching or changing code. Use local file
permissions. Do not execute external config JS or source; convert compiler panics
to diagnostics at the CLI boundary. This is an additive independent command with
no special deployment procedure.

## Alternative and future decisions

A name-only call graph would wrongly connect shadowed locals or import aliases;
use compiler symbols/signatures instead. Additional library semantics must define
both recognition and unresolved conditions.
