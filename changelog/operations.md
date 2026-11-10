# Operations changelog

## Unreleased

- Updated the container build context for the Cargo workspace so the same root binary is built from the extracted domain, configuration, source, and Prometheus libraries without changing the runtime image contract.
- Added a supported semantically versioned and digest-pinned Mainnet production stack with secret-file injection, loopback-only ingress, resource/PID/file/log/retention limits, deterministic persistent volumes, health-ordered startup, and an operator runbook covering verification, alert response, backup, restore, and shutdown.
- Added exact-image canary, guarded rollout, and rollback commands that require a semantic image version bound to an immutable digest, verify five-round readiness and immutable build identity before replacing production, and retain the previous image reference for recovery.
- Updated the external-validator Compose environment contract for connector-local RPC request and connection deadlines, and removed dormant WebSocket inputs from its WebSocket-free default profile.
- Published `linux/amd64` and `linux/arm64` image indexes with full-commit SHA tags, BuildKit provenance, an SBOM, and GitHub artifact attestations.
- Hardened the watcher image and Compose profiles with a `mise.toml`-owned Rust builder version, a versioned and digest-pinned runtime base, a non-root runtime user, dropped capabilities, no-new-privileges, read-only root filesystems, bounded liveness tmpfs mounts, graceful container stop signaling, endpoint health checks, and loopback-only published ports for watchers, Prometheus, Grafana, and Surfpool.
- Provisioned tested Prometheus alert rules and selected-profile target discovery with the local observability stack while leaving notification routing to external Alertmanager deployments.
- Updated container defaults and Compose mounts to consume one self-contained TOML file per profile.
- Added container and Compose profiles for Surfpool, an external validator, Devnet, and Mainnet.
- Consolidated Docker, Compose, Prometheus, and Grafana assets under `docker/` while preserving repository-root environment loading.
- Removed redundant validator mode and WebSocket toggle environment inputs from the external-validator Compose profile.
- Changed the Devnet Compose profile to forward complete Helius RPC and WebSocket URLs without reconstructing them from an API token.
- Added the external-validator Compose standard Solana RPC compatibility declaration and environment default required by connector validation.
