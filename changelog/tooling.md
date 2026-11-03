# Tooling changelog

## Unreleased

- Updated `just rust run`, the Pages container example, and repository guidance to use the self-contained TOML profiles.
- Added modular `just <module> <verb>` workflows for Rust, coverage, CI, containers, stacks, Surfpool, and the Pages site.
- Centralized the Rust toolchain pin in `mise.toml` for local, Compose, and container builds.
- Added repository-scoped skills, advisor routing, and per-module changelog ownership.
- Replaced the README with a minimal project and module index that routes detailed history to module-owned changelogs.
- Added an end-to-end configuration-loading benchmark for one- and 48-connector profiles, including command-route parsing and validation.
- Updated runtime configuration guidance to keep transport ownership inside connectors.
- Documented mode source requirements and secret-safe verification for complete credential-bearing provider URLs.
- Documented minimal/full Agave RPC method boundaries, valid method-specific capability probes, and transactional reload verification in the repository engineering guidance.
- Extended the opt-in Surfpool smoke to verify the connector's declared RPC API against the live local endpoint before collecting a snapshot.
