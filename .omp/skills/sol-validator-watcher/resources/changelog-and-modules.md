# Repository Modules and Changelog

## Scope

Read this resource for every repository change. It assigns durable ownership and the changelog file that records each module's changes.

## Module Ownership

| Module | Owns | Changelog |
| --- | --- | --- |
| Watcher | RPC and WebSocket collection, connector fallback, labels, validator records, aggregation, and economics collection | `changelog/watcher.md` |
| Runtime | CLI startup, polling, HTTP serving, reload, task supervision, liveness, signals, and shutdown | `changelog/runtime.md` |
| Configuration | Config parsing, environment expansion, validation, connector TOML, and committed profiles | `changelog/configuration.md` |
| Observability | Prometheus metrics and scraping, Grafana provisioning and dashboards, and the Pages surface | `changelog/observability.md` |
| Operations | Docker image, Compose profiles, Surfpool, local services, and stack lifecycle | `changelog/operations.md` |
| Tooling | Just recipes, tests and coverage workflows, CI, publishing, scripts, contributor docs, and repository guidance | `changelog/tooling.md` |

## Changelog Contract

- Every change updates the `Unreleased` section of each affected module's changelog before verification. Cross-module changes update multiple files.
- Record the observable result or durable contract in one concise bullet. Do not record commit hashes, verification output, implementation narration, or future work.
- A refactor still needs an entry when it changes module ownership, public paths, configuration, commands, operations, or contributor workflow.
- Documentation-only corrections update `changelog/tooling.md` when they change repository guidance or the public module map. Typographical fixes that change no meaning do not need an entry.
- At release time, move each module's `Unreleased` entries under the same version and date. Keep module histories separate.
- Adding, splitting, merging, or renaming a module requires the same change to update the README module index, this ownership table, `CHANGELOG.md`, and the module changelog files.

## Review Gate

Map every changed file to the table above. Reject the change when an affected module lacks a changelog update or when an entry is filed only under Tooling to hide a cross-module change.
