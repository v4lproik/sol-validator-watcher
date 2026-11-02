# Runtime changelog

## Unreleased

- Changed the default CLI profile path to `config/local/validator.toml` while preserving local-validator behavior.
- Added supervised polling, metrics HTTP serving, configuration reload, liveness updates, signal handling, and cooperative task shutdown.
- Built WebSocket probes directly from connectors that declare `ws_url`.
