# Runtime changelog

## Unreleased

- Derived dependency provenance and health-mode outcomes before snapshot publication, narrowing mutable runtime state without changing polling or readiness behavior.
- Published the compiled build revision, SHA-256 fingerprint of the active expanded configuration, accepted/rejected reload outcomes, and per-command fallback state without exposing endpoint or credential values.
- Bound the metrics listener before supervised tasks start and exercised the production health, readiness, metrics, and cooperative-shutdown boundaries.
- Preserved Solana `getHealth` error `-32005` as a completed degraded snapshot so node catch-up diagnostics remain observable while RPC health and readiness stay down; other dependency failures retain whole-poll recovery behavior.
- Routed WebSocket probe failures through the watcher-state actor so counters remain ordered with polling transitions while preserving RPC-based readiness admission.
- Defaulted the metrics HTTP listener to loopback and added a bounded `healthcheck` command that probes process liveness without conflating recoverable collection failures with container health.
- Made watcher-state publication loss during an already-requested cooperative shutdown complete normally, while preserving actor loss as fatal outside shutdown.
- Made post-startup RPC and collection failures recoverable at the whole-poll boundary: affected modes go down, readiness resets, the last successful snapshot and liveness timestamp remain unchanged, and polling resumes after exponential 1, 2, 4, 8, 16, then 30-second delays. Startup and configuration contract failures remain fatal.
- Published configured poll intervals, successful snapshot timestamps, success/failure durations, and failed-round totals.
- Split the binary entrypoint into runtime application, HTTP, polling, reload state, and single-owner watcher-state modules while preserving process behavior.
- Added Rust documentation for runtime wiring, transactional reloads, polling commits, HTTP readiness, watcher-state transitions, RPC telemetry, and task supervision.
- Changed the default CLI profile path to `config/local/validator.toml` while preserving local-validator behavior.
- Added supervised polling, metrics HTTP serving, configuration reload, liveness updates, signal handling, and cooperative task shutdown.
- Built connector clients and compiled command routes once in a reusable source chain.
- Rebuilt the complete source chain transactionally when connector definitions or mode command routes change on reload.
- Verified declared connector RPC capabilities before the first poll and before committing a changed reload candidate; startup failures terminate through task supervision, while failed reload probes preserve the last-known-good runtime.
- Added a supervised single-owner watcher-state actor with 30-second reports and a `/readyz` admission gate that requires five consecutive required polling rounds without a failed RPC attempt or non-2xx response; transport, protocol, and non-success HTTP failures plus accepted reloads reset admission while `/healthz` retains process-liveness semantics.
