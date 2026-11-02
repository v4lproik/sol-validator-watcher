# AGENTS.md

Repository guidance is organized by engineering category. Start with the
repository skill at `.omp/skills/sol-validator-watcher/SKILL.md`, then read only
the matching resource files. Read every matching category when a change crosses
boundaries; do not bulk-load unrelated guidance.

## Workflow

1. Identify the behavior being changed and its caller, module, external
   boundary, configuration, metric, test, and operational surface.
2. Read the matching category resources from the index below.
3. Trace the current success and failure paths before editing.
4. Reuse the existing module owner and `just` workflow. Do not add a second
   client, configuration path, fallback policy, or command convention.
5. Verify the changed behavior at its real boundary.
6. Run `just verify` on the final revision before every commit or push.

## Category Index

| Category | Read when changing | Skill resource |
| --- | --- | --- |
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
| `just doctor` | Verify the pinned Rust, `just`, formatting, lint, and coverage tools |
| `just check` | Fast Rust compilation check |
| `just fmt` | Format Rust sources |
| `just clippy` | Lint all targets and features with warnings denied |
| `just test` | Run the default Rust test suite |
| `just build` | Build the debug binary |
| `just build-release` | Build the release binary |
| `just run [config]` | Run with `.env` and a selected config; defaults to `etc/config.local.yaml` |
| `just run-release [config]` | Run the release binary with a selected config |

### Verification

| Command | Purpose |
| --- | --- |
| `just verify` | Baseline pre-commit/pre-push gate: format, Clippy, build, and tests |
| `just coverage` | Local coverage summary |
| `just coverage-ci [threshold]` | Enforce line coverage; default threshold is 80 |
| `just test-surfpool` | Run the focused opt-in Surfpool smoke test |
| `just test-surfpool-all` | Run all tests with Surfpool E2E enabled |
| `just ci` | Alias for the baseline verification gate |
| `just ci-full` | Full local CI including coverage and Surfpool |

### Containers and local stacks

| Command family | Purpose |
| --- | --- |
| `just doctor-docker` / `just doctor-surfpool` | Verify optional local dependencies |
| `just docker-build [tag]` | Build the watcher image |
| `just stack-config-{surfpool,validator,devnet,mainnet,all}` | Render the selected Compose configuration without starting it |
| `just stack-up-{surfpool,validator,devnet,mainnet,all}` | Build and start the selected stack |
| `just stack-logs-{surfpool,validator,devnet,mainnet,all}` | Follow logs for the selected stack |
| `just stack-ps-{surfpool,validator,devnet,mainnet,all}` | Inspect selected stack containers |
| `just stack-reload-prometheus` | Reload local Prometheus configuration |
| `just stack-down` | Stop all repository Compose profiles without deleting volumes |

### Coverage site and CI

| Command | Purpose |
| --- | --- |
| `just coverage-html` | Generate the local HTML coverage report |
| `just coverage-lcov` | Generate `lcov.info` |
| `just site-build` | Rebuild the Pages site and coverage report |

Additional gates:

- GitHub Actions changes: run `actionlint -color`.
- Pages site or coverage publishing changes: run `just site-build`.
- Coverage logic or threshold changes: run `just coverage-ci`.
- Compose changes: run every affected `just stack-config-*` recipe before any
  opt-in stack startup.

## Maintaining This Guidance

- The live `Justfile` is authoritative for recipe behavior and accepted
  arguments. Update this command catalog when its public surface changes.
- Update the owning skill resource in the same change that modifies a durable
  repository invariant.
- Keep one rule under one category and link to it instead of duplicating it.
- Never commit credentials, generated coverage output, `.env`, or endpoint
  secrets.
