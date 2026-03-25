# GitHub Project Workflow

This repository should use GitHub Issues for units of work and GitHub Projects for visibility.

## Goal

Keep the workflow lightweight while making the next step, current owner, and blockers obvious.

## Unit of work

- Open an issue for any non-trivial bug, feature, refactor, docs task, or operational task.
- Use the issue templates so each item starts with scope and acceptance criteria.
- Keep one issue focused on one deliverable when possible.

## Project setup

Use a single project with these statuses:

- `Backlog`
- `Next`
- `In Progress`
- `Review`
- `Done`

Recommended custom fields:

- `Priority`: `p0`, `p1`, `p2`, `p3`
- `Area`: `watcher`, `dashboard`, `config`, `docs`, `ci`, `ops`
- `Size`: `xs`, `s`, `m`, `l`
- `Blocked`: `yes`, `no`
- `Target`: optional milestone, release, or date window

Recommended views:

- Board by `Status`
- Table sorted by `Priority` then `Target`
- `Next` filtered to `Status = Next`

## Issue conventions

- Bug reports should include a reproduction and acceptance criteria for the fix.
- Feature and task issues should define the expected outcome and done conditions.
- Link related issues, PRs, and roadmap docs in the issue body.
- If a task is blocked, capture the dependency in the issue and set `Blocked = yes`.

## PR conventions

- Every PR should link the issue it advances with `Closes #...` or `Refs #...`.
- When a PR is ready, move the linked project item to `Review`.
- If follow-up work remains, open a new issue instead of hiding it in review comments.

## Labels

Use labels sparingly. The project fields carry most of the workflow.

Recommended base labels:

- `bug`
- `enhancement`
- `task`
- `docs`
- `dashboard`
- `config`
- `ci`
- `ops`

## Weekly hygiene

- Re-rank `Next` at least once a week.
- Close or re-scope stale issues instead of leaving them ambiguous.
- Make blockers explicit; do not leave blocked items in `In Progress` without context.
