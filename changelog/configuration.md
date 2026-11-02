# Configuration changelog

## Unreleased

- Grouped committed watcher and connection templates under `config/local/`, `config/compose/`, and `config/remote/`.
- Added environment-expanded YAML profiles with companion TOML connector configuration and validation.
- Moved committed configuration from `etc/` to `config/` and fixed relative legacy connector paths so they resolve exactly once.
