# Watcher changelog

## Unreleased

- Added typed health, consensus, leadership, and economics commands with explicit per-command connector fallback routes and selected-source provenance.
- Added validator economics collection with explicit capability checks for required reward methods.
- Migrated collector and Surfpool integration paths to command-specific routes while preserving RPC collection and aggregation behavior.
- Bound each command to an explicit transport and only feature-gated prior-epoch economics commands with `historical-data`, so configuration validation rejects ineligible connectors before polling while preserving declared fallback order.
- Made each mode own its valid transport and provider-feature combinations, then resolve the configured provider through each command route.
- Redacted credential-bearing RPC and WebSocket URLs from transport errors.
- Collected recent performance samples, economics-target account balances and rent exemption, VAT runway, epoch timing, and per-validator next-leader estimates with explicit source routes and absent-data semantics; account balances remain available when a provider omits the account size needed for rent and VAT calculations.
- Ignored vote-credit samples beyond the current epoch and preserved unmatched vote credits and efficiency as unavailable instead of zero.
