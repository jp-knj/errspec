# Node integration test support

Keep scenario inputs, expected results and feature-specific orchestration in the
test that owns them. Share preparation and command mechanics only when their
input conditions, result meaning and failure conditions agree.

## Temporary projects

[`tests/support/project.mjs`](../../../tests/support/project.mjs) owns temporary
project cleanup and the standard Vitest directory/dependency skeleton.
`temporaryProject` registers cleanup with the calling test context. Preserve each
caller's choice of canonical or original temporary paths: path spelling is part
of several acquisition, ownership and cache tests.

`prepareVitestProject` creates only the shared directories and dependency link.
The calling fixture writes its own source, test bodies, package/configuration
files and any alternate runner or compiler setup. Keep those inputs visible next
to the assertions that explain their purpose.

## Native command execution

[`tests/support/cli.mjs`](../../../tests/support/cli.mjs) owns synchronous process
execution, launch-error/exit-status assertions and JSON response parsing.
Callers supply the binary, ordered arguments and spawn options explicitly.

Preserve the existing command, default flags, working directory, environment
merging, timeout, kill signal and expected exit code at each call site. A helper
must not silently add session reuse, caching or runner options. Tests that inspect
a raw unsuccessful command can retain that result and share only its validation
and JSON parsing.

Async cancellation tests retain their lifecycle monitors, event handling and
signal assertions locally. Their process ownership contract differs from a
synchronous command that returns a JSON report. The production
[mutation execution boundaries](../mutation-flow.md) remain unchanged.

## Verification

Run every affected integration suite with the same installed compiler/runner
dependencies and native binary as before extraction. Retain actual failure,
interruption and cleanup cases. Expected reports, source fixtures and snapshots
remain the existing behavior oracle; changing them is not part of moving shared
test mechanics.
