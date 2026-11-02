# CI, Publishing, and Repository Workflows

## Scope

Use this resource for `Justfile`, GitHub Actions, coverage artifacts, Pages,
container publishing, scripts, public command changes, permissions, and release
checks.

## Command Ownership

- The live `Justfile` is the stable human and agent interface. Use `just --list`
  and `just --show <recipe>` instead of inventing flags or copying recipe bodies.
- Recipes use `mise` so the pinned Rust and coverage tools are consistent
  locally and in CI.
- Add a public recipe when a repeated repository workflow needs a stable entry
  point. Keep implementation in the rightful script only when shell complexity
  exceeds a readable recipe.
- A recipe change must update the command catalog in `AGENTS.md` in the same
  change.
- Preserve argument order, defaults, prerequisites, environment loading, and
  exit status when refactoring a recipe.
- Destructive commands remain visibly named (`down`, `clean`) and never hide in
  a validation or read-only recipe.

## CI Contracts

- `.github/workflows/ci.yml` runs `just doctor`, `just ci`, the coverage gate,
  LCOV/HTML generation, artifact uploads, and optional Codecov upload. Local
  verification must exercise every changed step that can run locally.
- Keep the required verification job name stable unless repository protection
  is updated in the same operational change.
- Workflow permissions remain least-privilege. Do not grant write scopes to a
  read-only build or test job.
- Concurrency cancellation is acceptable for stale pull-request revisions. A
  completed run must match the exact commit being reviewed.
- Actions must be pinned to an intentional release reference consistent with
  repository policy. Review deprecation annotations rather than suppressing
  them.
- Never print secrets, entire environments, authenticated URLs, or token-bearing
  command arguments.

## Coverage and Pages

- Coverage thresholds and ignore patterns have one owner in `Justfile` recipes.
  Update local, CI, and documentation semantics together.
- Pages generation is two-stage: build HTML coverage, then assemble `site/`,
  `assets/`, and coverage into ignored `public/` output.
- `scripts/build-pages-site.sh` must fail when the expected coverage root is
  absent. Do not publish an empty or stale coverage directory.
- Pages enablement is fail-closed: deploy only when the build job reports that
  Pages should run.
- Generated `coverage/`, `lcov.info`, and `public/` remain uncommitted.

## Container Publishing

- `.github/workflows/container.yml` publishes to GHCR only from `main`, version
  tags, or explicit dispatch according to its trigger contract.
- Preserve branch/tag/SHA metadata and default-branch `latest` behavior.
- Build context, Dockerfile path, cache, labels, and tags must describe the same
  source revision.
- A local `just docker-build` proves the image builds; it does not authorize a
  registry push.

## Review Gates

1. What public command or CI contract changes?
2. Is the behavior owned by `Justfile`, a script, or a workflow, and is there
   only one implementation?
3. Are local and CI commands using the same pinned toolchain and arguments?
4. Which permissions, secrets, artifacts, caches, and outputs does the job need?
5. Can a skipped, cancelled, missing, or failed required step look green?
6. Do Pages and coverage steps consume the artifact just built by this revision?
7. Do container tags and labels resolve to the reviewed commit?
8. Did `AGENTS.md` and the owning skill resource change with a public workflow?

## Required Verification

- `Justfile`: `just --list`, `just --show <changed-recipe>`, then execute the
  harmless changed path.
- GitHub Actions: `actionlint -color` plus every referenced local `just` recipe.
- Coverage logic: `just coverage-ci` and applicable artifact generation.
- Pages: `just site-build` and inspect the generated root/coverage links.
- Container: `just docker-build` with a local tag.

## Prohibited Shortcuts

- Do not run raw `cargo`, coverage, Docker Compose, or site assembly commands in
  CI when a repository recipe owns them.
- Do not use CI as the first run of a locally reproducible formatter, linter,
  test, build, coverage, or site check.
- Do not upload a missing artifact with warning-only behavior.
- Do not broaden token permissions to fix an unrelated action failure.
- Do not commit generated coverage or Pages output.
