# Repository Modules and Changelog

## Scope

Read this resource for every repository change. It assigns durable ownership and the changelog file that records each module's changes.

## Module Ownership

| Module | Source entry points | Owns | Changelog |
| --- | --- | --- | --- |
| Watcher | `src/mode.rs`, `src/source.rs`, `src/collector/`, `src/labels.rs`, `src/websocket.rs` | Typed mode commands, RPC and WebSocket collection, command fallback, source provenance, labels, validator records, aggregation, and economics collection | `changelog/watcher.md` |
| Runtime | `src/main.rs`, `src/runtime/`, `src/watcher_state/`, `src/tasks.rs` | CLI startup, polling, HTTP serving, transactional source-chain reload, watcher state, task supervision, liveness, signals, and shutdown | `changelog/runtime.md` |
| Configuration | `src/config/`, `config/` | Config parsing, environment expansion, connector and command-route validation, and committed profiles | `changelog/configuration.md` |
| Observability | `src/metrics/`, `docker/prometheus/`, `docker/grafana/`, `site/` | Prometheus metrics and scraping, mode-focused Grafana provisioning and dashboards, and the Pages surface | `changelog/observability.md` |
| Operations | `docker/` | Docker image, Compose profiles, Surfpool, local services, and stack lifecycle | `changelog/operations.md` |
| Tooling | `Justfile`, `just/`, `.github/workflows/`, `.omp/skills/` | Just recipes, tests and coverage workflows, CI, publishing, scripts, contributor docs, and repository guidance | `changelog/tooling.md` |

## Changelog Contract

- Every change updates the `Unreleased` section of each affected module's changelog before verification. Cross-module changes update multiple files.
- Record the observable result or durable contract in one concise bullet. Do not record commit hashes, verification output, implementation narration, or future work.
- A refactor still needs an entry when it changes module ownership, public paths, configuration, commands, operations, or contributor workflow.
- Documentation-only corrections update `changelog/tooling.md` when they change repository guidance or the public module map. Typographical fixes that change no meaning do not need an entry.
- At release time, move each module's `Unreleased` entries under the same version and date. Keep module histories separate.
- Adding, splitting, merging, or renaming a module requires the same change to update the README module index, this ownership table, `CHANGELOG.md`, and the module changelog files.

## Review Gate

Map every changed file to the table above. Reject the change when an affected module lacks a changelog update or when an entry is filed only under Tooling to hide a cross-module change.
