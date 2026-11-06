# CI, Publishing, and Repository Workflows

## Scope

Use this resource for `Justfile`, GitHub Actions, coverage artifacts, Pages,
container publishing, scripts, public command changes, permissions, and release
checks.

## Command Ownership

- The root `Justfile` routes to domain modules under `just/`. Use `just` or
  `just --list --list-submodules` to discover commands and
  `just --show <module>::<verb>` to inspect one without copying recipe bodies.
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

- `.github/workflows/ci.yml` runs `just rust doctor`, `just ci verify`,
  `just rust doc`, the coverage gate, LCOV/HTML generation, artifact uploads,
  and optional Codecov upload. Local verification must exercise every changed
  step that can run locally.
- Prometheus configuration or rule changes run `just stack check` locally and
  in the required CI job.
- Keep the required verification job name stable unless repository protection
  is updated in the same operational change.
- Workflow permissions remain least-privilege. Do not grant write scopes to a
  read-only build or test job.
- Concurrency cancellation is acceptable for stale pull-request revisions. A
  completed run must match the exact commit being reviewed.
- Actions must be pinned to an intentional release reference consistent with
  repository policy. Review deprecation annotations rather than suppressing
  them.
- Repository security workflows use pinned scanner versions, deny warnings, and
  fail on actionable dependency, image, secret, or configuration findings.
  Dependabot covers Cargo, Docker, and GitHub Actions dependencies.
- Never print secrets, entire environments, authenticated URLs, or token-bearing
  command arguments.

## Coverage and Pages

- Coverage thresholds and ignore patterns have one owner in `just/coverage.just`.
  Update local, CI, and documentation semantics together.
- Pages generation is two-stage: build HTML coverage, then assemble `site/`,
  `assets/`, and coverage into ignored `public/` output.
- `scripts/build-pages-site.sh` must fail when the expected coverage root is
  absent. Do not publish an empty or stale coverage directory.
- Pages enablement is fail-closed: deploy only when the build job reports that
  Pages should run.
- Generated `coverage/`, `lcov.info`, and `public/` remain uncommitted.

## Container Publishing

- Container publication must call the repository CI workflow from the same
  revision and make publishing depend on its success. Keep caller and reusable
  workflow concurrency groups distinct so they cannot cancel each other.
- Published image indexes contain `linux/amd64` and `linux/arm64` manifests,
  explicit BuildKit provenance and SBOM attestations, and a GitHub artifact
  attestation bound to the pushed digest.
- Grant `packages: write`, `attestations: write`, and `id-token: write` only to
  the publish job. Verification remains read-only.
- Pull requests build one exact `linux/amd64` verification image, run it against
  controlled RPC responses, fetch readiness/build identity, stop it cleanly,
  and scan that same local image before any registry publication.
- After publication, pull and smoke the pushed multi-architecture digest on
  `linux/amd64`, then scan both `linux/amd64` and `linux/arm64` manifests. A
  pre-publication build does not prove the registry digest.
- `.github/workflows/container.yml` publishes to GHCR only from `main`, version
  tags, or explicit dispatch according to its trigger contract.
- Preserve branch/tag metadata, full-commit SHA tags, and default-branch
  `latest` behavior.
- Build context, Dockerfile path, cache, labels, and tags must describe the same
  source revision.
- `mise.toml` owns the Rust version. `Justfile` and container CI resolve that
  pin with `mise config get tools.rust.version` and pass `RUST_VERSION` into
  Docker; Dockerfiles and Compose overlays must not duplicate the version.
- A local `just container build` proves the image builds; it does not authorize
  a registry push.
- Version tags create release notes only after the pushed image digest has a
  verified repository-bound artifact attestation.

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

- `Justfile` or module files: `just --list --list-submodules`,
  `just --show <module>::<verb>`, then execute the harmless changed path.
- Rust documentation: run `just rust doc` to render private production modules with warnings denied.
- GitHub Actions: `actionlint -color` plus every referenced local `just` recipe.
- Prometheus configuration and alerts: `just stack check`.
- Coverage logic: `just coverage check` and applicable artifact generation.
- Pages: `just site build` and inspect the generated root/coverage links.
- Container: `just container build <tag>`, `just container smoke <tag>
  <revision>`, and `just security image <tag>`.
- Security: `just security dependencies` and `just security repository`.

## Prohibited Shortcuts

- Do not run raw `cargo`, coverage, Docker Compose, or site assembly commands in
  CI when a repository recipe owns them.
- Do not use CI as the first run of a locally reproducible formatter, linter,
  test, build, coverage, or site check.
- Do not upload a missing artifact with warning-only behavior.
- Do not broaden token permissions to fix an unrelated action failure.
- Do not commit generated coverage or Pages output.
