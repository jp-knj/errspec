# Incremental workflows and runner integration design

This design implements the [requirements](requirements.md) and [Product](../../product.md) using the existing generator, inactive switches, and common verdict model. It does not change scenario registration or failure-verification guarantees.

## Responsibilities and compatibility

Rust owns resolved requests, validated compiler/runner facts, selection/reuse planning, bounded trials, verdicts, gates, and reports. JS/TS adapters own compiler/runner acquisition and return owned values; orchestration belongs in the language appropriate to the underlying API. This is not a plan to move every responsibility into Rust.

Additional behavior is enabled through explicit options/configuration. Existing arguments preserve native output/exit contracts. New reasons/statistics are optional where required. Unexecuted work never becomes a kill.

## Compiler checks

The typecheck request identifies tsconfig, installed TypeScript, and candidates. TS 6 uses LanguageService text overlays; TS 7 uses the official API's virtual files. Apply each edit to original text and compare diagnostic multisets against original diagnostics. Only new diagnostics produce `CompileError`.

Restore text between candidates and close compiler state in `finally`. Reject `noCheck` as typechecking. Optional-chain equivalence requires strictNullChecks, noUncheckedIndexedAccess, and a receiver type excluding null/undefined. The complete replacement must match exactly one optional-token removal from the original expression; altered property names, indices, arguments, call shapes, suffixes, or multiple removals require normal diagnostic acquisition. Semantic discovery backend options remain distinct from mutation typecheck options.

## History and probe reuse

Mutation history is a versioned format separate from error-contract baselines. It stores input fingerprints, acquisition mode, reach, and eligible verdicts by location-independent key. Write through temporary-file rename. A key match alone never permits reuse.

`--cache`/`cache` is opt-in. Run a current ordinary baseline every time. Reuse only first-attempt assertion kills or all-selected-test success, with matching key, reach/selection, acquisition, and fingerprints. Mark each reused candidate. Timeout, RuntimeError, retries, and typecheck require fresh validation. Save pre-acceptance verdicts; apply current approvals and gates again.

`coverage-v1.json` separately stores reach/infection acquisition. Reuse requires a successful current ordinary baseline, matching inputs, every test's ID/name/file/state/start fact, and valid candidate references. Rebind current task IDs and use the larger current/previous duration for deadlines. Invalid references/counts/states or deleted tests force a full probe. Report `cache.coverageReused`. `--full-dry-run`/`fullDryRun` forces all-test probe acquisition independently of result cache; its negative flag clears configured true.

Raw input changes intentionally refresh the complete probe, including scope mode. Same-input reuse and changed-input revalidation satisfy the accepted feature contract. Partial probe refresh remains a deferred performance optimization; see the [audit](../../implementation-status.md#probe-reuse-audit).

Fingerprints bind build-fixed Rust/driver/lock/compiler configuration identity, Node/runtime/environment, resolved runner options, project files, acquired runtime files, and resolved packages/content. Preserve graph edges for cyclic/shared packages, including edges into already acquired packages. Exclude own cache output and current temporary directory. Unreadable/opaque inputs disable reuse. Default reuse requires exact inputs in the same checkout, including position-only edits.

The build-fixed identity includes native source paths as well as bytes. Grouping
CLI modules under `src/bin/errspec/mutation/` and library modules by feature
therefore invalidates previously cached mutation evidence; the first run rebuilds
that evidence and subsequent compatible runs can reuse it. Fingerprint coverage
and validation rules remain unchanged, and old cache identities are never
relabeled to bypass validation.

The embedded driver's `source-inventory` module shares raw source registration, per-source plugin generation, and ancestor manifest acquisition. Runner adapters retain enumeration, exclusion rules, and acquisition order. Plugin generation remains once per source before the baseline, and each adapter's final input validation uses the same acquired record.

Even an all-cache-hit run re-enumerates current files before completion without another test run, rechecks fingerprints, and rejects changes during reuse. Corrupt/inconsistent cache falls back to fresh execution. Save only after final validation. Uncontrolled external-state-dependent tests are outside the cache contract.

Acquire resolved compile values from Vite root/environments, including resolved defines when Vitest injects them. Changes to define, resolve, compiler, env, or optimizer invalidate reuse even if config bytes match. Runtime define scripts, opaque asset matchers, unacquired transform/load/resolve plugins, duplicate builtin plugin names, or unserializable compiler values disable reuse. With CSS disabled, use the resolved CSS policy; required CSS processing with opaque callbacks falls back fresh. Only known project paths enter relocation normalization; never rewrite define expressions/runtime values.

### Explicit checkout relocation

`--cache-relocation`/`cacheRelocation` is opt-in. Tests/dependencies must not depend semantically on project absolute location, cwd, PWD, or INIT_CWD. Convert known project-internal paths to tagged relative identities consistently across package edges, runtime/start locations, and known runner setup paths. External paths, other environment values, provide values, arbitrary settings, and file contents must still match. Unresolved paths containing `..` are not normalized.

Only fingerprints change; runtime paths and user code do not. Fingerprint the mode itself and report `cache.relocation`. Preserve current ordinary baseline, registration matching, task rebinding, and final acquisition by real paths. Changed CI metadata can prevent reuse: restoration alone never proves an identical environment.

## Git selection and scope reuse

Resolve the requested Git revision to a commit from project root, then collect staged, unstaged, and untracked changes. Use NUL-delimited names and literal pathspecs rather than quoted/colorized output parsing. Bound acquisition to 30 seconds and 16 MiB per stream; shared owned-process handling supports interruption.

Node mode matches changed lines against mutated expression/statement ranges. Scope mode uses the nearest named function/method/function-value binding, or the module for top-level code. `--only` intersects this selection. Changed/deleted tests and changes outside candidate inventory broaden to all candidates because dependency effects cannot be ruled out. Include candidates whose killing test disappeared and warn for changed tests not collected. Reacquire the diff after execution; changes during the run prevent completion. Selection alone never skips the ordinary baseline.

Owned syntax metadata v2 includes original UTF-8 SHA-256 and entry candidates for direct function bodies. The native parser acquires it; common Rust validation checks text, candidate original ranges, scope hashes/relationships, complete AST tree, and entry ownership/placement. A changed source after acquisition is an input error. Do not treat a nested callback body as its outer entry, or ignored/non-generated guards as entries.

Scope reuse derives current entry-reaching test-file cohorts from the full probe, including static hits. Missing entries never establish unused functions. Unknown changed scopes, missing entries, or opaque calls force revalidation. For known scopes, revalidate when the union of previous/current changed-scope cohorts overlaps a candidate's covering files. File cohorts conservatively include sharing between different test IDs in one file.

Require matching input envelope, candidate owner, function hash, selected tests, and existing eligible first-attempt verdict. Enable through `--cache --cache-scope`/`cacheScope`, with a negative flag to clear it. The contract assumes deterministic isolated test files; source text, positions, and stack-trace dependent tests are excluded. Until compiler comment attachment is acquired, files with comments retain raw text in the module envelope, so location-only edits revalidate. Anonymous default function declarations, including async/generator forms, also retain body text because no independent function owner exists.

Build raw/scope fingerprints from one acquisition. Substitute only validated inventory module effects; test/helpers, dependencies, settings, environment, package edges, and external runtime inputs must match. Changed raw input takes a full probe and rebinds current entry cohorts. Store in separate `scope-results-v1.json`; expose optional `cache.scope`/`scopeReused`. Retain unselected old rows only with matching complete context. Final raw acquisition uses the same projection before atomic save.

`--experimental-callgraph` additionally limits old-survivor revalidation using bidirectional static call relations and actual resolved imports. It remains off by default, mode changes revalidate, and unknown calls/entries broaden. This graph does not model all value flow through tests; report its warning and omission counts instead of claiming soundness.

## Weak mutation and exclusions

Acquire reach separately from changed values. Apply weak mutation only when evaluation count, receiver, and short-circuit semantics are preserved; otherwise execute covering tests. Initial comparison/arithmetic support does not imply support for every expression. Static/concurrent/ambiguous observations broaden conservatively.

Acquire real comment trivia and discard apparent directives overlapping strings, regexes, template/JSX text. Pure Rust interpretation supports errspec-/mutator- disable-next-line, disable-line, disable/enable and Stryker disable/restore. Line directives take priority over regions; regions use the last directive per family. Next-line starts after a multiline comment's ending line; regions start after its ending offset. Preserve key and add disabled reason in `candidate.ignored`. Existing builtin safety reasons win. Excluded candidates do not instrument, execute, or enter scores/gates. Removing a directive changes input and invalidates cache.

Arid is off by default to preserve prior results. `--arid`/`arid` enables it; the negative flag clears it. Default patterns cover console/logger/log receivers, debug/trace properties, and debug functions. `aridCallees`/repeatable `--arid-callee` replaces the pattern set; empty means no matches, null restores defaults. Match acquired dotted callee paths, excluding computed access/casts. Memoize call-only nonempty blocks/ifs and walk candidate ancestors, stopping at mixed statements, declarations, return, or function boundaries. Explicit comments/builtin reasons take priority. Fingerprint enablement/patterns. This naming heuristic does not prove equivalence of side effects in arguments or conditions.

Acceptance stores mutant key plus mandatory human reason separately, reports stale keys, and is reapplied to current evidence rather than cached as a detector verdict.

## Custom mutators and ignorers

`plugins` accepts module strings or `{module, settings}` entries; CLI repeats `--plugin`. Resolve relative modules from project root and package names using project module resolution. Default exports are objects/arrays from typed identity helpers `defineMutator`, `defineIgnorer`, or `definePlugin`. Do not serialize callbacks to JSON or generated executable source.

Node supplies native-acquired syntax node kind, relationships, UTF-8 positions, and text to visitors. Mutators return node-contained edits; ignorers return reasons. Rust validates nodes/ranges, reparses edited files, finds safe runtime expression/statement/body placements, and creates ordinary keys. Lift callee/assignment-reference edits to the complete expression. Type/declaration-token anchors incapable of runtime switching are errors; otherwise unsupported switch contexts become reasoned `Ignored`. Custom/builtin names must be unique and support per-family exclusion.

Reason priority is builtin safety, disable/Stryker comment, arid, then ignorer. Plugin `aridCallees` adds patterns only when arid is enabled.

Node module hooks acquire import/require dependencies and package manifests and detect changes during generation. Explicit plugin `inputs` and context `readInput` join the same input record. `cache: "deterministic"` is the author's contract to depend only on acquired modules/settings/environment/declared inputs. Time, randomness, or undeclared reads require conservative cache disablement. All acquired plugin inputs remain in exact and scope fingerprints; changes force fresh results and reach.

## Test selection and complete kill matrices

`select-tests --since` and `selectTests({since})` use the same native request/configuration/baseline/probe path without CLI argument reconstruction. Current test IDs/reach are authoritative; only matching probe input can be reused. Selection executes no mutation trials and ignores trial budgets/gates/weak narrowing/typechecking/shards. Git base is required.

Rank current coverage of changed function scopes. Historical killing IDs may rank ahead only if still current covering tests; they are hints, not proof of current killing. Remove deleted tests. Changed tests/helpers/settings, outside-inventory changes, deleted lines, module edits, or unobserved functions fall back to all current files. Preserve deletion-only hunks; a removed old scope must not select only its neighbor.

Acquire the Git-base text and syntax for changed inventory files. Compare the old
and current module text after excluding each version's function byte spans. An
import or module statement changed or removed on the same line as a function
therefore cannot restrict recommendations to that function's coverage. Missing
old text or syntax also falls back to all current files; function-only edits keep
their bounded recommendations.

Return `errspec/test-selection` JSON or files with uncollected-test warnings/fallback reasons. Re-enumerate final source/test inventory, including ignored-by-Git files added during acquisition. Failed current baseline/probe acquisition or a final input mismatch returns incomplete exit 2 and no file recommendation.

`--kill-matrix`/`killMatrix` requires complete all-covering-test acquisition, with a negative flag. Adapters declare complete capability and force bail/global retry to zero so late killers execute; fingerprint the forced settings. Do not narrow to weak-infected tests.

A complete row requires every selected ID to have started and returned ordinary pass/fail, without module/runner error, timeout, native retry, or observed test retry/repeat. Unexecuted/sample/budget/unreached/insufficient acquisition does not create complete cells. Failed final input validation clears every row's completeness.

Do not reuse first-detection or scope result cache as matrix cells. Acquire rows afresh; compatible probe reuse remains allowed. Do not write matrix verdict rows into first-detection storage; `cache.reused`/`saved` stay zero. Pure analysis uses complete rows only to find no-kill tests, strict subsumption, and identical nonempty kill sets. Report incomplete counts/reasons. Relations apply only to the selected Git/ID/shard mutant set and do not prove safe test deletion.

Complete acquisition may restart a runner/browser after each failing test, so the successful baseline duration alone cannot estimate cohort wall time. The native total deadline is the larger of the existing estimate and selected-test count multiplied by the existing progress-stall deadline. Per-test/hook deadlines and progress-stall checks remain active; native retries never upgrade a row to complete. Rust recognizes Jest/Playwright test/hook deadline forms while retaining ordinary expect/locator assertion failures as kills.

## Concurrency, shards, and sessions

Concurrency is integer 1–64 through `-j`/`--concurrency`/configuration/API, default 1. Rust queues candidates to independent drivers using the same attempt/confirmation/verdict code. Sampling/cache decisions precede allocation. Time budget uses the request's common start time; started trials finish, unstarted candidates become Pending. Restore inventory result order independent of completion order. `engine.parallel` records requested and started trial workers.

The native coordinator separates configuration/session setup, inventory and probe
acquisition, candidate planning, trial execution, and final input/cache validation.
Each phase receives owned facts or explicit borrows; partial reports remain
available on failure. Serial trials retain the existing per-candidate sampling
and reuse order. Parallel workers return their execution facts before final
validation and persistence. Driver ownership outlives every request, while
temporary resources outlive the driver, including interruption and early errors.
The [execution guide](../../../../engineering/mutation-flow.md) records module
ownership and the boundaries that must survive further extraction.

Each worker owns driver/run ID/temp directory/process group. Extra workers take full probes and match test registration/state/reach/infection against the primary, rebinding task IDs locally. Restarted timeout workers revalidate probes. One worker's failure interrupts peers. SIGINT/SIGTERM or parent SIGKILL stops all owned groups. Before cache save, merge runtime inputs: incompatible settings disable save; input-content conflicts or inconsistent reused evidence make the run incomplete. External service state is not isolated; users need concurrency-safe tests.

`--shard INDEX/TOTAL` partitions candidates after Git/only selection using first 64 key bits modulo total. Configuration/API use one-based `{index,total}`. `report.shard` discloses selected/omitted counts and method; score/gate apply only to that request's shard. Baseline/probe cover the project; typecheck/trials cover the shard. Exclude shard setting from common cache fingerprint and retain all-key records for sequential aggregation. Atomic writes prevent corruption; concurrent lost updates require fresh work rather than assumed evidence. Samples/budgets apply within each shard invocation.

Pending from budgets/samples leaves the overall score null and cannot pass break/fail-on-undetected. Partial execution must not manufacture 100% overall success.

`--reuse-session`/`reuseSession` is opt-in; default fresh. Keep ordinary baseline separate and retain only instrumented context from probe onward. Each attempt refreshes request/output/provided context/test IDs and clears public collection cache before re-enumeration. Keep forks/isolation. Reset timeout from original values each time; triple only confirmation attempts.

Root/selected-project globalSetup forces fresh to preserve setup/teardown per attempt. Missing public collection-cache APIs also forces fresh. `engine.sessionReuse` explains availability/fallback. Out-of-run events or old request metadata make the run incomplete rather than counting as detection. Close retained context and propagate cleanup failures.

## Runner adapters

Options are `runner` (auto/vitest/vite-plus/jest/playwright), `runnerConfig`, `runnerProject`, and `runnerPackage`; legacy Vitest options remain. Auto selection checks Vitest/Vite config, Jest config/package field, Playwright config, then installed packages. Explicitly select Playwright when Vite and Playwright config coexist.

Adapters return common execution facts and use the shared generator/emitter/probe/Rust trial loop. Jest 30 is native ESM: wrap project ESM transforms and apply switches before transformation; otherwise use Node type stripping for TS. Vite+ uses its bundled Vitest 5 public API. These are acquisition adapters, not independent verdict implementations.

Playwright uses Vite and `@errspec/playwright` fixtures, enumerates all projects, and includes project in identity. Each owned worker starts Vite on a different temporary port. Automatic fixtures extend existing base fixtures and collect page/frame reach/infection, sending observations through browser bindings before document destruction and retaining them across navigation. Missing fixture acquisition cannot count as a kill. Browser WebWorkers are outside acquisition. Relative navigation is required for per-worker baseURL. Existing vite/vp dev webServer commands are replaced by adapter-owned Vite; custom servers are rejected. Runner entry points and exact supported versions are documented with [Agent Skills](../../../../integrations/agent-skills.md) and package guides.

Additional adapters leave `configurationHash` null and revalidate results/probes until complete external-input acquisition exists. Support does not imply cache safety for unobserved inputs.

`@errspec/vitest` and `@errspec/jest` fix the runner and delegate to the same core request; they do not reconstruct CLI arguments or implement a second verdict loop. `@errspec/playwright` independently distributes extendable fixtures, while the project supplies Playwright. `@errspec/typecheck` owns compiler and overlay acquisition, which the embedded driver also imports. Its public API verifies original source and UTF-8 spans and returns diagnostic/equivalence facts; Rust assigns CompileError and Ignored. The same acquisition is available in a cancellable worker thread.

Jest and Vite scratch caches live inside each owned worker's private temporary directory. Orphan recovery validates run identity, owner UID, private permissions and canonical directory identity, then removes only known generated namespaces without following links. Unknown project-created root files and directories remain, and external symlink targets are never traversed.

## Process ownership

Use a native supervisor as process-group leader; Node inherits group/stdio. Stop the group before reaping the supervisor. A supervisor detects changed kernel parentage and stops its own group. A private per-UID registry records version, run ID, 256-bit token, owner/supervisor facts, and Unix endpoint.

At startup, query at most 256 records within three seconds. A live supervisor verifies its own parent/group before stopping. Never signal a saved PID directly, or terminate active/mismatched/unverified records. If endpoint creation is prohibited, retain parent monitoring and report `workers.endpointUnavailable`. Match driver/generator engine fingerprints so replacing binaries mid-run causes incompletion. Detailed control flow is in [mutation flow](../../../../engineering/mutation-flow.md).

## Formatting, APIs, Action, and Skills

Render text, MTE JSON/offline HTML, agent/simplify, and matrix data from owned Rust results. Keep formatting separate from gates and stdout separate from saved output. Public native JSON remains the established format; additional reporters do not alter default payloads.

`formatReport` can acquire bounded in-root source or explicit snapshots. Validate original spans/locations before attaching source; matching a slice is not proof of complete historical source identity. Formatting saved reports never refreshes verdicts/gates. Missing source remains missing. No generic child-process warning is emitted without relevant observed facts; public attribution exposes executable and relevant project paths, not raw arguments/environment. Child-loaded static dependency closure is a possible execution risk, not assertion proof or complete dynamic/package/shell resolution. Reporter/schema/boundary tests are linked in [implementation status](../../implementation-status.md).

The Action's Node coordinator forwards JSON options to existing runMutation/formatReport without reimplementing verdicts, fingerprints, or exit status. Default PR base is the event base SHA; fetch missing shallow commits from origin. Separate official cache restore/save with `always()` save, including failed runs. Use a prebuilt native artifact or project-installed CLI, never unpublished-registry fallback or consumer Rust compilation. Save native/agent/MTE JSON, offline HTML, summary, and diff annotations through shared formatters. Native fingerprints decide restored-cache eligibility. See the [Action guide](../../../../integrations/github-action.md).

Skills use one skills-npm-compatible set under the CLI package, embedded byte-for-byte in native init-skill. Acquire every existing target before conflict planning; identical files are unchanged. Only explicit force updates bundled files; preserve unrelated project notes. On a later write failure, roll back owned changes without overwriting concurrent edits. Initialization installs no dependencies, executes no tests, and changes no project source. See the [Skills guide](../../../../integrations/agent-skills.md).

## Configuration and public request protocol

Choose one root-local `errspec.mutation.config.{ts,mts,mjs,js,json}`, or explicit `--config-file`. Do not search parents; multiple discovered files are an error. Validate the entire configuration before applying explicitly supplied CLI/API properties. Precedence is per property, arrays replace, and negative flags override configured true. Explicitly disabling typecheck also clears non-explicit compiler paths.

Configuration root is relative to the configuration file; its paths are relative to effective project root. Explicit CLI/API root/paths resolve from caller cwd. Script config uses Node 24 type stripping and a JSON-compatible default export, with 10-second/1-MiB limits and logs on stderr. Stop its owned process group before reap; never signal a reaped PID.

CLI/API share `mutation::options::Options`. `@errspec/core` runMutation sends protocol-1 JSON directly to native execution, not CLI argument parsing. defineConfig/plugin helpers are typed identity functions; native code validates final input and judges. Configuration/quality failures return report plus exitCode; startup/invalid response rejects the Promise. Ship declarations/schema. Typecheck options are `--typecheck`, `--no-typecheck`, `--typecheck-project`, `--typecheck-package`, using mutation `--node`; semantic-analysis options remain separate.

Core's private `native-transport.mjs` owns native binary resolution, JSON-compatible serialization, process I/O and bounded response acquisition. Each public API constructs its request and validates its response and exit status. Mutation/selection retain the 64-MiB response limit and AbortSignal interruption with SIGINT followed by SIGKILL after five seconds; formatting retains its 64-MiB request and 128-MiB response limits and force-stops overflowing output. Formatting has no cancellation transport option. Package the private module with the public entry without adding another public export.

Validate internal version/run/request IDs, counts, duplicate/missing/mismatched candidates, diagnostics, and executed targets. Typecheck status stays in engine.typecheck plus acquired version facts. History binds fingerprints and acquisition mode and never becomes scenario history.

## Security, distribution, and alternatives

Use installed compilers/runners; do not upload source or auto-publish. Configuration/plugins execute trusted local code. Typecheck overlays never alter originals. History, acceptance, and reports have separate purposes/versions. Keep acquisition effects out of pure judgments and protocol output free of logs.

GitHub permissions come from the Action environment; no new secret store is created. Existing CLI/testkit need no migration. Verify API/Action/package behavior using locally packed artifacts before publication. Publishing, billing changes, visibility changes, and repository migration remain later work.

Bundling the whole fixed mutator was considered: it reuses capability directly but adds its oxc native dependencies and a second verdict model. The chosen implementation shares existing candidates and common judgments, retaining JS/TS acquisition where appropriate. Scope is approved; this design does not invent additional product guarantees. Local acceptance and outstanding CI/practical evidence remain separate in [implementation status](../../implementation-status.md).
