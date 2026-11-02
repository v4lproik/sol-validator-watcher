# Operations changelog

## Unreleased

- Updated container defaults and Compose mounts to consume one self-contained TOML file per profile.
- Added container and Compose profiles for Surfpool, an external validator, Devnet, and Mainnet.
- Consolidated Docker, Compose, Prometheus, and Grafana assets under `docker/` while preserving repository-root environment loading.
- Removed redundant validator mode and WebSocket toggle environment inputs from the external-validator Compose profile.
- Changed the Devnet Compose profile to inject `HELIUS_DEVNET_API_KEY` while the watcher constructs the documented Helius RPC and WebSocket endpoints.
