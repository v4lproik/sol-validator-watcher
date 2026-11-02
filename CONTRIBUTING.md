# Contributing

## Setup

Install the pinned toolchain and repo tools:

```bash
mise install
```

All project commands should go through `just`:

```bash
just rust doctor
just rust fmt
just rust clippy
just rust test
just coverage check
```

## Development flow

1. Make the change.
2. Run `just ci verify`.
3. Run `just coverage check` if the change affects Rust code.
4. If you changed dashboards, validate them with:

```bash
jq empty deploy/grafana/dashboards/sol-validator-watcher-overview.json deploy/grafana/dashboards/sol-validator-watcher-breakdown.json
```

## Planning and tracking

- Use a GitHub issue as the unit of work before starting non-trivial changes.
- Pick the closest issue template: bug, feature, or task.
- Add the issue to the GitHub Project and set the core fields before starting implementation.
- Keep commits scoped to one task when practical.
- Link the PR back to the issue with `Closes #...` or `Refs #...`.

Recommended GitHub Project fields:

- `Status`: `Backlog`, `Next`, `In Progress`, `Review`, `Done`
- `Priority`: `p0` to `p3`
- `Area`: `watcher`, `dashboard`, `config`, `docs`, `ci`, `ops`
- `Size`: `xs`, `s`, `m`, `l`
- `Blocked`: `yes` or `no`
- `Target`: optional milestone or delivery window

## Configuration and stacks

- Keep example configs free of secrets.
- Route local workflows through `just`, not ad-hoc shell commands.
- When testing Docker profiles, prefer module commands such as `just stack up devnet` and `just stack logs devnet`.

## Commit style

Use conventional commits where practical:

- `feat(...)`
- `fix(...)`
- `chore(...)`
- `docs(...)`
- `refactor(...)`
- `test(...)`
- `ci(...)`

Examples:

- `feat(watcher): add devnet profile`
- `fix(ci): restore coverage gate`
- `docs(readme): shorten public setup guide`

## Pull requests

- Keep changes scoped.
- Explain user-visible behavior changes.
- Call out config, dashboard, and operational changes explicitly.
- Include verification notes in the PR description.
- Move the linked GitHub Project item to `Review` when the PR is ready.
