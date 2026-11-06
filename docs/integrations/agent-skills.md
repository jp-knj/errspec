# Agent Skills

The CLI's `skills/` directory follows the skills-npm layout, with a `SKILL.md`
entrypoint in each named folder. It is part of local npm tarballs; no registry
publication or automatic skill-sync installation is assumed.

| Skill | Intended work |
| --- | --- |
| `errspec` | Install from available artifacts, select a runner/config, run and interpret mutation evidence |
| `errspec-test-loop` | Add observable behavior checks, recheck specific IDs, and report unresolved or equivalent cases |
| `errspec-playwright` | Use the Playwright fixture and Vite browser adapter with projects and per-worker servers |

Discover the installed package's `node_modules/@errspec/cli/skills/` with a
compatible skill-sync tool, or copy the bundled instructions explicitly:

```sh
errspec init-skill
errspec init-skill --dir .claude/skills
errspec init-skill --format json
```

The native executable embeds the same files as the npm package, so standalone
artifact users need no checkout or npm source files to initialize skills. The
default is `.agents/skills`; choose another directory for a specific agent tool.
No package installation, test execution, network request or source edit is
performed by initialization.

All file conflicts are checked before writes. Identical copies are left alone.
Edited copies require `--force`, which replaces the shipped files and preserves
unrelated project notes. Existing targets must be regular files; symlinked skill
targets and directories, including relative destination traversal through a
symlink, are refused. Existing individual files are bounded to 1 MiB for comparison
and rollback. Changed files are written atomically, and a later failure restores
this operation's previous bytes/permissions and removes its newly written files
when no concurrent change prevents safe restoration. Empty created directories
may remain after a failure. Concurrent edits are reported instead of overwritten
by rollback. Errors use exit 2 and `EC_SKILL_INIT`; success is exit 0.

The instructions preserve native distinctions: a quality failure is exit 1,
baseline/configuration/incomplete execution is exit 2, and Pending/unknown scores
remain outstanding. Agent reports contain bounded reaching-test lists plus their
total count. Restored caches and selected-run success do not establish full-project
verification. Equivalence is recorded with a specific reason only when supported
by the relevant behavior and the requested scope.

Validation includes YAML/frontmatter/link checks, real initialization, idempotent
copies, conflict preflight, explicit force, symlink rejection, rollback under an
actual write failure, a real gap-to-kill mutation loop, and offline-installed CLI
copies compared byte for byte with the packaged skills. Playwright instructions
are checked against the adapter's public contract; full browser adapter acceptance
is maintained separately.
