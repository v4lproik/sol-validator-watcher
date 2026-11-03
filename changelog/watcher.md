# Watcher changelog

## Unreleased

- Split collector ownership into command, RPC transport, domain model, aggregation, snapshot orchestration, and domain-focused test modules without changing collection behavior.
- Added Rust documentation for command routing, RPC transport and wire models, snapshot provenance, aggregation, labels, and validator metric semantics.
- Added typed health, consensus, leadership, and economics commands with explicit per-command connector fallback routes and selected-source provenance.
- Added validator economics collection with explicit capability checks for required reward methods.
- Migrated collector and Surfpool integration paths to command-specific routes while preserving RPC collection and aggregation behavior.
- Bound each command to an explicit transport and only feature-gated prior-epoch economics commands with `historical-data`, so configuration validation rejects ineligible connectors before polling while preserving declared fallback order.
- Made each mode own its valid transport and provider-feature combinations, then resolve the configured provider through each command route.
- Redacted credential-bearing RPC and WebSocket URLs from transport errors.
- Added bounded, valid startup probes for every standard Solana method promised by each connector's compatibility profile plus retained prior-epoch reads promised by `historical-data`; diagnostics name the connector, provider, endpoint kind, capability, and method without exposing endpoint URLs, and changed reload candidates are fully verified before replacing the live runtime.
- Counted every failed RPC attempt and each received HTTP status before payload decoding, with bounded failure totals, response classes, and exact readiness-critical status codes.
- Collected recent performance samples, economics-target account balances and rent exemption, VAT runway, epoch timing, and per-validator next-leader estimates with explicit source routes and absent-data semantics; account balances remain available when a provider omits the account size needed for rent and VAT calculations.
- Ignored vote-credit samples beyond the current epoch and preserved unmatched vote credits and efficiency as unavailable instead of zero.
