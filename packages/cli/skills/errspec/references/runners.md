# Runner selection

Use `--runner` explicitly when a project has several configurations. Runner
acquisition is JavaScript; native Rust planning and verdicts remain shared.

- **Vitest 5**: `--vitest-config`, `--vitest-project`, `--vitest-package` retain their
  existing meanings. Session reuse is opt-in with `--reuse-session`; setup that
  cannot safely preserve its lifecycle falls back to fresh sessions.
- **Vite+**: use `--runner vite-plus` for the project's installed Vite+ runner.
- **Jest 30**: use `--runner jest --runner-config jest.config.mjs`; native ESM is
  supported. CommonJS is outside this adapter's supported contract.
- **Playwright Test with Vite**: use the `errspec-playwright` skill. Browser targets
  require the errspec fixture and a supported Vite app; ordinary Node-only API
  tests are not browser mutation evidence.

Generic `--runner-config`, `--runner-project` and `--runner-package` select the
configuration, project name or installed runner package. Omitting the Playwright
project runs all configured projects. Keep opaque test IDs intact when using
reachability, kill matrix or selection output; do not infer an ID from a test name.

Parallelism defaults to one. `-j 2` uses independent owned workers; tests still
need separate external resources. A shard score applies to its selected partition.
Never terminate a saved PID or delete a path returned by mutated application code;
fixtures may clean up only processes and files they created and own.
