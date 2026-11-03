# Operations changelog

## Unreleased

- Provisioned tested Prometheus alert rules and selected-profile target discovery with the local observability stack while leaving notification routing to external Alertmanager deployments.
- Updated container defaults and Compose mounts to consume one self-contained TOML file per profile.
- Added container and Compose profiles for Surfpool, an external validator, Devnet, and Mainnet.
- Consolidated Docker, Compose, Prometheus, and Grafana assets under `docker/` while preserving repository-root environment loading.
- Removed redundant validator mode and WebSocket toggle environment inputs from the external-validator Compose profile.
- Changed the Devnet Compose profile to forward complete Helius RPC and WebSocket URLs without reconstructing them from an API token.
- Added the external-validator Compose standard Solana RPC compatibility declaration and environment default required by connector validation.
