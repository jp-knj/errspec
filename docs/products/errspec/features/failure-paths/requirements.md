# Follow failure paths from code to tests

## About this document

Following the [Product](../../product.md), this extends
[failure-scenario verification](../failure-verification/requirements.md) to the
code of functions that return an error type. One report connects code failure,
error variant and scenario, showing where the connection is missing.

### Scope

For an optional contract entry function, detect two omissions:

- **Error-type omission:** the entry allows a throw or Promise rejection to escape
  although the declared error type does not represent it.
- **Test-path omission:** a direct `return` of a tagged value is reached by no
  scenario execution, even when another path returns the same tag.

Tag registration remains part of the failure-verification requirements.

### Outside scope and limits

Reachability is line-based. If a condition and `return` occupy one line,
evaluating the condition may count the line as reached and conceal the missed
return. Reaching a line does not prove that the scenario asserted the right value.
Only explicit project-local throws and `Promise.reject` paths that escape catches
are reported as error-type omissions; library behavior and implicit runtime
exceptions are outside scope. Reachability acquisition is supported with Bun.

## User stories

### Find failures absent from the error type

**Scenario: report an explicit uncaught throw before executing tests.**

- Given an entry or a project-local function it calls has an uncaught throw.
- When registration is checked without launching tests.
- Then report its source and the functions it traversed, and fail the check.

**Scenario: do not report a potentially caught failure as a confirmed omission.**

- Given a throw is enclosed by try/catch on its path to the entry.
- When registration is checked.
- Then do not treat the original throw as a confirmed escape, even if the catch
  might rethrow. An explicit rethrow inside the catch is a new source in its own
  right.

### Find untested paths for the same tag

**Scenario: fail verification when no scenario reaches a tagged return.**

- Given the entry has several returns for one tag and one is never reached.
- When scenarios are verified with Bun.
- Then report that return's position and tag, and fail verification.

**Scenario: do not fail for path omissions when all returns are reached.**

- Given every tagged return is reached by at least one scenario execution.
- When scenarios are verified.
- Then show the paths and their reachability without a path-omission failure.

**Scenario: unsupported runners do not report paths as reached.**

- Given a contract with an entry is verified by a runner without this acquisition.
- When verification runs.
- Then list the paths and show reachability as unverified.

## Functional requirements

### Entry function

A contract may identify the error-returning function by file and export name.
Contracts without an entry do not receive these checks. An entry that cannot be
resolved is a configuration failure.

### Path inventory

A path is a `return` written directly in the entry body whose returned type reveals
a discriminator value. Report its location and tags. Nested functions' returns
are not returns of the entry and are excluded.

### Reachability

After the ordinary test execution, run only errspec scenarios once with coverage.
Report paths reached by none of those executions. The success/failure of this
additional acquisition does not replace the ordinary execution verdict.
