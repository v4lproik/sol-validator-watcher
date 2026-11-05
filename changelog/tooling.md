# Tooling changelog

## Unreleased

- Documented bounded recent block-fee collection, identity validation, connector fallback, Harmonic attribution limits, and the required RPC boundary proofs.
- Documented nested connector transport ownership, sensitive-header and parse-error redaction, protocol-owned header validation, secure TLS defaults, bounded connect/request/handshake deadlines, and the required local boundary proofs.
- Documented connector-local client endpoint and timeout ownership and kept gRPC configuration absent until a typed, verified gRPC command exists.
- Documented typed provider-API transport ownership, bounded Jito reward collection and caching, optional polling semantics, metric absence, and local HTTP boundary proof.
- Documented the node-local dashboard's unconfigured, unavailable, missing-distance, and multi-network display invariants.
- Clarified that complete required-route error aggregation includes configured WebSocket health fallback.
- Documented complete required-route failure aggregation and the prohibition against discarding prerequisite RPC failures.
- Documented routed node-local status collection, structured Solana behind-state handling, snapshot-owned node metric semantics, and the required dashboard and alert boundaries.
- Documented connector-attributed WebSocket probe failure reset, fallback, health-mode, and readiness semantics.
- Gated main-branch, version-tag, and dispatched container publication on the same revision's complete repository CI workflow.
- Documented loopback HTTP defaults, private-network container binding, non-root/read-only Compose invariants, and the required container health smoke boundary.
- Documented the fatal startup boundary, recoverable whole-poll failure policy, bounded backoff sequence, retained-snapshot semantics, shutdown-time state-transition handling, and required failure/recovery smoke proof.
- Added `just stack check` and a required CI step to validate Prometheus configuration and execute alert rule behavior tests.
- Updated the public module index and repository engineering guidance for the domain-oriented Rust source tree.
- Added `just rust doc` to build warning-free documentation for private production modules and APIs, and required the same documentation build in CI.
- Updated `just rust run`, the Pages container example, and repository guidance to use the self-contained TOML profiles.
- Added modular `just <module> <verb>` workflows for Rust, coverage, CI, containers, stacks, Surfpool, and the Pages site.
- Centralized the Rust toolchain pin in `mise.toml` for local, Compose, and container builds.
- Added repository-scoped skills, advisor routing, and per-module changelog ownership.
- Replaced the README with a minimal project and module index that routes detailed history to module-owned changelogs.
- Added an end-to-end configuration-loading benchmark for one- and 48-connector profiles, including command-route parsing and validation.
- Updated runtime configuration guidance to keep transport ownership inside connectors.
- Documented mode source requirements and secret-safe verification for complete credential-bearing provider URLs.
- Documented command implementation ownership, core/extended standard Solana RPC compatibility, independent provider API surfaces, valid method-specific probes, and transactional reload verification in the repository engineering guidance.
- Extended the opt-in Surfpool smoke to verify the connector's declared standard Solana RPC compatibility against the live local endpoint before collecting a snapshot.
- Documented five-round readiness admission, process-liveness separation, bounded RPC failure and HTTP response status telemetry, and the required runtime smoke boundary.
