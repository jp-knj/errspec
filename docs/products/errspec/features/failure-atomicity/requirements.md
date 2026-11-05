# Check operation state under injected faults

## About this document

Under the [Product](../../product.md), inject developer-specified faults into
dependency calls in turn, checking whether an operation that returns failure has
left business state partially updated.

### Scope

Inject one fault at each dependency call observed during the successful run.
Compare before/after snapshots using an independent fixture for every case.
The consumer calls this from its existing test runner.

### Outside scope

Do not synthesize arbitrary error values or business expectations from types.
Do not promise combinations of faults, every concurrent ordering or rollback in
external systems.

## User story: reproduce partial updates with one operation definition

**Scenario: a failed operation that changes state reports a counterexample.**

- Given the normal run succeeds but a dependency failure leaves a partial update.
- When state is checked under injected faults.
- Then fail with fault ID, dependency, call ordinal and before/after state.

**Scenario: rollback is judged by final state rather than intermediate writes.**

- Given the operation writes state and restores it on failure.
- When a fault is injected.
- Then report preserved state when the final snapshot matches the initial one.

**Scenario: repeated calls are checked independently.**

- Given the normal run calls the same dependency twice.
- When one fault is registered.
- Then return two cases, injecting once at each ordinal in a fresh fixture.

**Scenario: recovery success differs from failure-state preservation.**

- Given a retry succeeds after the injected fault.
- When state is checked.
- Then report recovery, without claiming that failure-state preservation was
  verified.

**Scenario: an unreached dependency or call is not an empty success.**

- Given the normal run never calls the named dependency, or a fault run does not
  reach the requested ordinal.
- When checked.
- Then fail and identify the unexecuted fault.

**Scenario: an unexpected exception is not a pass merely because state matches.**

- Given the operation should return a failure value but throws.
- When checked.
- Then report the exception and reproduction conditions and clean up all fixtures.

## Success measure

Effectiveness on real projects is not yet measured. The acceptance target is a
counterexample for an operation that leaves a partial update and success for the
repaired operation under the same inputs/faults. Compare faulty and repaired
operations during acceptance; superiority over other tools requires separate
measurement.

## Functional requirements

### Normal execution and single-fault cases

**Input:** fixture creation, operation, state acquisition, failure-value predicate,
named executable faults, and optional selection of one case.

**Output:** normal-run call observations and each fault's preserved state,
recovery or violation. Violations include reproduction conditions and snapshots.

The user chooses the protected business state and valid fault values. Initialize
the fixture per run and await the operation and dependency calls. Run optional
cleanup after success and failure. Store an unchanged copy of acquired state.

A failed normal run, empty/duplicate fault IDs, unobserved dependency, incomplete
call or unreachable replay condition is a failure. Never report uncheckable state
as preserved.

## Data

- Reproduction: fault ID, dependency and call ordinal.
- State: the user-selected business state before and after the operation.
- Result: operation outcome, violation reasons and cleanup failures.

One successful run defines several single-fault cases. Cases share neither fixture
nor state. User-selected values can appear in diagnostics; they are not sent to an
external service.

## Dependencies and safety

No additional external service is required; use the consumer's runner. Operations
and dependencies are supplied by the user. This does not isolate real side effects:
the fixture must supply that isolation.

## Unresolved decisions

None for this feature's accepted scope.
