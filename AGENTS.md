# AGENTS.md

Repository guidance is organized by engineering category. Start with the
repository skill at `.omp/skills/sol-validator-watcher/SKILL.md`, then read only
the matching resource files. Read every matching category when a change crosses
boundaries; do not bulk-load unrelated guidance.

## Workflow

1. Identify the behavior being changed and its caller, module, external
   boundary, configuration, metric, test, and operational surface.
2. Read the repository module/changelog resource and every matching category
   resource from the index below.
3. Trace the current success and failure paths before editing.
4. Reuse the existing module owner and `just` workflow. Do not add a second
   client, configuration path, fallback policy, or command convention.
5. Update the `Unreleased` section of every affected module changelog.
6. Verify the changed behavior at its real boundary.
7. Run `just ci verify` on the final revision before every commit or push.

## Category Index

| Category | Read when changing | Skill resource |
| --- | --- | --- |
| Repository modules and changelog | Every change; identify all affected modules and required changelog files | `.omp/skills/sol-validator-watcher/resources/changelog-and-modules.md` |
| Rust implementation | Rust APIs, ownership, lifetimes, enums, serde models, async code, allocations, or module visibility | `.omp/skills/rust-best-practices/SKILL.md` and `.omp/skills/sol-validator-watcher/resources/rust-code-quality.md` |
| Rust review | Reviewing a Rust file, module, crate, diff, or pull request | `.omp/skills/rust-review/SKILL.md` and `.omp/skills/sol-validator-watcher/resources/rust-review.md` |
| Rust porting | Translating or rewriting C, C++, Python, Go, JavaScript, or TypeScript behavior in Rust | `.omp/skills/rust-port/SKILL.md` and `.omp/skills/sol-validator-watcher/resources/rust-porting.md` |
| RPC collection and errors | Solana JSON-RPC methods, connector precedence, fallback, batching, provenance, provider failures, or economics capability checks | `.omp/skills/sol-validator-watcher/resources/rpc-collection-and-errors.md` |
| Runtime and configuration | Startup, config loading/reload, task supervision, shutdown, HTTP health, websocket probes, liveness, or environment expansion | `.omp/skills/sol-validator-watcher/resources/runtime-configuration-and-shutdown.md` |
| Metrics and dashboards | Prometheus descriptors or labels, snapshot resets, Grafana JSON/provisioning, alerts, or operator semantics | `.omp/skills/sol-validator-watcher/resources/metrics-and-dashboards.md` |
| Tests, coverage, and performance | Unit/integration/E2E scope, mock RPC fixtures, Surfpool, coverage thresholds, benchmarks, or smoke proof | `.omp/skills/sol-validator-watcher/resources/testing-and-verification.md` |
| Local stacks and operations | Dockerfiles, Compose profiles, validator/Surfpool wiring, env templates, Prometheus provisioning, or teardown | `.omp/skills/sol-validator-watcher/resources/local-stacks-and-surfpool.md` |
| CI, publishing, and repository workflows | `Justfile`, GitHub Actions, container publishing, coverage Pages, scripts, public commands, or release checks | `.omp/skills/sol-validator-watcher/resources/ci-publishing-and-workflows.md` |

Every catch, fallback, retry, ignored result, and default-on-error path also
requires `.omp/skills/sol-validator-watcher/resources/error-handling.md`.

## Commands

Run commands from the repository root. Use the repository's `just` recipes
instead of invoking `cargo`, Docker Compose file combinations, coverage tools,
or site scripts directly.

### Rust development

| Command | Purpose |
| --- | --- |
| `just rust doctor` | Verify the pinned Rust, `just`, formatting, lint, and coverage tools |
| `just rust check` | Fast Rust compilation check |
| `just rust fmt` | Format Rust sources |
| `just rust clippy` | Lint all targets and features with warnings denied |
| `just rust test` | Run the default Rust test suite |
| `just rust build [debug\|release]` | Build the selected profile; defaults to debug |
| `just rust run [debug\|release] [config]` | Run with `.env`, a profile, and a config; defaults to debug and `config/local/validator/watcher.yaml` |
| `just rust clean` | Remove Cargo build artifacts |

### Verification and coverage

| Command | Purpose |
| --- | --- |
| `just ci verify` | Baseline pre-commit/pre-push gate: format, Clippy, build, and tests |
| `just ci full` | Full local CI including coverage and the focused Surfpool smoke |
| `just coverage summary` | Generate a local coverage summary |
| `just coverage check [threshold]` | Enforce line coverage; default threshold is 80 |
| `just coverage html` | Generate the local HTML coverage report |
| `just coverage lcov` | Generate `lcov.info` |
| `just surfpool doctor` | Verify the optional Surfpool CLI |
| `just surfpool test [smoke\|all]` | Run focused or complete opt-in Surfpool tests |

### Containers and local stacks

| Command | Purpose |
| --- | --- |
| `just container doctor` | Verify Docker and Docker Compose |
| `just container build [tag]` | Build the watcher image with the Rust version pinned in `mise.toml` |
| `just stack config <target>` | Render `surfpool`, `validator`, `devnet`, `mainnet`, or `all` without starting it |
| `just stack up <target>` | Build with the pinned Rust version and start the selected stack |
| `just stack logs <target>` | Follow logs for the selected stack |
| `just stack ps <target>` | Inspect selected stack containers |
| `just stack reload prometheus` | Reload local Prometheus configuration |
| `just stack down` | Stop all repository Compose profiles without deleting volumes |

### Coverage site

| Command | Purpose |
| --- | --- |
| `just site build` | Rebuild the Pages site and coverage report |

Additional gates:

- GitHub Actions changes: run `actionlint -color`.
- Pages site or coverage publishing changes: run `just site build`.
- Coverage logic or threshold changes: run `just coverage check`.
- Compose changes: run `just stack config <target>` for every affected target
  before any opt-in stack startup.

## Maintaining This Guidance

- The root `Justfile` routes commands to the modules under `just/`; together
  they are authoritative for recipe behavior and accepted arguments.
- Update the owning skill resource in the same change that modifies a durable
  repository invariant.
- Keep the README module index, changelog ownership resource, and module
  changelogs aligned when ownership changes.
- Keep one rule under one category and link to it instead of duplicating it.
- Never commit credentials, generated coverage output, `.env`, or endpoint
  secrets.
