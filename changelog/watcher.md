# Watcher changelog

## Unreleased

- Added typed health, consensus, leadership, and economics commands with explicit per-command connector fallback routes and selected-source provenance.
- Added validator economics collection with explicit capability checks for required reward methods.
- Migrated collector and Surfpool integration paths to command-specific routes while preserving RPC collection and aggregation behavior.
- Bound each command to an explicit transport and only feature-gated prior-epoch economics commands with `historical-data`, so configuration validation rejects ineligible connectors before polling while preserving declared fallback order.
