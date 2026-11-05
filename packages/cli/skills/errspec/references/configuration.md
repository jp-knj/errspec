# Configuration and CI

Use the configuration schema shipped as `@errspec/core/schema`. Script configs
must default-export plain JSON-compatible values. File root is relative to the
config file; explicit CLI root is relative to the caller. Do not mix this config
with the error-contract `errspec.json`.

`policy.thresholdBreak` gates the score; `policy.failOnSurvived` gates survivors.
`policy.critical` globs require detection or reasoned acceptance for every selected
critical candidate, including Pending ones. Unknown scores cannot establish a
full-score gate. High/low thresholds describe score bands. Presentation never
changes a verdict. `--only` intersects other selection and gates only that subset.

Use `--typecheck` with the project's TypeScript/tsconfig when compiler exclusions
are useful; `CompileError` differs from runtime detection. `--weak-mutation`
distinguishes reached inputs with unchanged values. Intentional source exclusions
must carry their actual reason; do not disable a mutator solely to hide a gap.

The repository's composite Action uses a reviewed checkout of errspec and an
installed CLI or explicit `native-binary` from verified distribution artifacts.
It fetches a missing PR base, restores mutation history, writes summaries and
annotations, and attempts cache save even after a failed run. It does not fetch an
unpublished npm package. Supply Node 24 and normal project dependencies first.
Action `options` is a JSON mutation-options object; `working-directory`, `since`
and `config-file` have dedicated inputs. Upload the report directory with
`if: always()` if CI artifacts are wanted.

Native fingerprint checks still apply after restore. Changing CI environment
values, dependency installation or configuration may require fresh execution.
Never remove fingerprint inputs to obtain an artificially warm result.
