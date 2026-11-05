# Failure-path design

This implements the [Requirements](requirements.md) under the [Product](../../product.md).

## Configuration

Version 2 contracts accept optional `entry: { "file", "export" }`. Resolve the
file from the configuration directory. The export must resolve to a function
declaration or a const initialized with a function expression/arrow function.
Missing or unsupported entries produce `EC002`.

## Error-type omissions: EC018

After type analysis, use the same fact acquisition and propagation as `scan` on
the opened tsrs program. Consider throw and rejection effects of the entry. Emit
`EC018` only when the origin is outside a catch and no traversed call catches the
path. A potentially rethrowing catch does not make the original source a confirmed
escape. Its explicit rethrow is outside the corresponding try body and is treated
as a new source. Unresolved external implementations or dynamic dispatch have no
known origin and do not become `EC018`.

## Path inventory

Traverse the entry body without entering nested functions. Split each return
expression's type into union members, using the awaited type for async functions.
Collect discriminator string literals. A return with at least one contract tag
becomes a path with file, line, byte offset and tags in the contract's `paths`.
`reached` is a boolean only when acquired; otherwise omit it.

## Reachability: EC019

For Bun verification with an entry, run registered test files after the ordinary
run using `bun test --coverage --coverage-reporter=lcov -t "errspec \\["`.
Do not pass observation environment variables: testkit behaves as a normal unit
test in this additional run.

Resolve lcov `SF` paths from the configuration directory and inspect each path's
line count in `DA`. A zero count produces `EC019`. A missing file makes all its
paths unreached. A missing line is not judged and is treated as reached, preserving
the stated line-coverage limitation. Unreadable coverage produces `EC_RUNNER`.
Vitest does not acquire this evidence and omits `reached`.
