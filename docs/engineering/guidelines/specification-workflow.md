# Maintaining specifications

These documents describe errspec itself. The failure scenarios a consumer writes
in `errspec.json` describe the application being verified; they have a different
role.

## Read the relevant contract

Start from [the Product](../../products/errspec/product.md), the feature's
Requirements and its corresponding Design. [The documentation index](../../README.md)
links the feature pairs. English is the maintained specification language.
Use the explicit request and existing approved scope to determine the task; do
not ask again about decisions already made.

Product explains users, problems, value and limits. Requirements define observable
behavior, scope and acceptance conditions. Design explains responsibilities,
interfaces, data and the reasoning that implements those conditions. Keep each
responsibility in its own document and use links instead of copying the same rule
into every feature.

## Keep decisions and evidence separate

The user decides purpose, scope and promised behavior. Agents provide research,
alternatives, implementation and verification evidence. Distinguish an approved
decision from a proposal or an unresolved issue. Do not infer an approval date,
approver or new agreement from a file's existence, a review tool or a passing test.

The full implementation plan and feature scope have been authorized. That does
not turn unimplemented work, a failed CI run or an incomplete practical trial into
completed validation. [Implementation status](../../products/errspec/implementation-status.md)
records those distinctions.

## Update the existing documents

Change Product when purpose or users change. Change Requirements and Design when
observable behavior changes. For an implementation improvement that preserves the
requirement, update Design when its responsibility boundaries or reasoning change.
Do not weaken an expected failure, required comparison or acceptance condition
merely to make tests pass.

Keep specifications, implementation and relevant evidence reviewable in the same
change. Typo fixes and execution of already approved procedures do not require a
new approval stage. New behavior outside the authorized scope needs an explicit
decision; existing code alone does not make it an accepted requirement.

## Review and verify

Check Markdown links and read the changed Requirements against Design. Review
inputs, successful results, failures, incomplete states and stated limitations.
Keep executable acceptance links in the capability/status documents. Run the checks
appropriate to a code change using the engineering guides and CI definitions.
Document-only translation does not require rerunning unrelated runner suites.

The temporary specification bootstrap and language configuration have been
retired under the approved plan. Reading, editing, building and testing errspec
requires no specification service, external AI review configuration or additional
specification CLI. Git and PRs retain the earlier documents and decision history.
