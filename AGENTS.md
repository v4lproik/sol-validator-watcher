# AGENTS.md

Use the repo's `just` recipes instead of invoking `cargo` directly.

Before committing or pushing, run:

```bash
just verify
```

`just verify` is the baseline gate and must stay green before a push. It runs:

- `just fmt`
- `just clippy`
- `just build`
- `just test`

Additional repo-specific checks:

- If you change GitHub Actions workflows, also run `actionlint -color`.
- If you change the Pages site or coverage publishing flow, run `just site-build`.
- If you change coverage logic or thresholds, run `just coverage-ci`.

Prefer `mise`-backed recipes from `Justfile` over ad hoc commands so the pinned toolchain and components are used consistently.
