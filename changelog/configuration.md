# Configuration changelog

## Unreleased

- Replaced paired YAML watcher and TOML connection files with one ordered TOML file per runnable profile.
- Removed split-config compatibility fields, relative companion-file loading, YAML parsing, and the obsolete transport-mode environment setting.
- Replaced the multi-provider parser dependency with direct TOML deserialization and current Serde support.
- Grouped the self-contained profiles under `config/local/`, `config/compose/`, and `config/remote/`.
- Preserved environment expansion, connector declaration-order priority, validation, and profile behavior.
- Rejected unknown top-level and nested fields so misspellings and removed split-config keys fail before runtime construction.
- Removed the redundant root validator mode and WebSocket toggle; connector URLs now define available transports.
