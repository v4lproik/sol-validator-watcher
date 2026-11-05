# Watcher changelog

## Unreleased

- Added an optional routed recent-block-fee command that selects at most 32 completed current-epoch leader slots, batches confirmed `getBlock` reward reads without transaction bodies, validates target identity fee rewards, and preserves ordered connector fallback and source provenance.
- Applied each connector's custom headers and bounded connect/request/handshake deadlines to reusable RPC, provider-API, and WebSocket clients, with HTTP redirects disabled, Rustls certificate and hostname verification, HTTP TLS 1.2 minimums, and no insecure override.
- Added local RPC, Jito, and WebSocket boundary coverage proving credential header delivery and redaction, redirect rejection, and enforced HTTP request and WebSocket handshake deadlines.
- Added an optional routed Jito provider-API command that publishes one economics target's settled prior-epoch MEV reward and commission, uses ordered connector fallback, bounds response size, and caches settled or briefly absent epoch results.
- Made RPC, WebSocket, and Jito API client construction reject zero deadlines, protocol-owned header overrides, and malformed or unsupported connector-local URLs before polling while retaining redacted transport failures.
- Rejected ambiguous `scheme:///path` authorities consistently in direct RPC, WebSocket, and Jito API client construction without exposing endpoint query parameters.
- Expanded the routed recent-performance command with total, vote, and non-vote transaction rates plus slot throughput, preserving omitted transaction splits as absent and falling back when a sample is internally inconsistent.
- Simplified route-exhaustion diagnostics with composable typed formatting and deferred failure-buffer allocation until a connector actually fails.
- Preserved every connector and causal error when a configured WebSocket health route is exhausted instead of returning only the final probe failure.
- Preserved every connector and causal error when a required command exhausts its fallback route, and propagated `minimumLedgerSlot` failures instead of treating failed reward-history prerequisites as absent.
- Added the optional routed `node-status` command for node identity, validator version and feature set, block height, structured `getHealth` catch-up distance, and current/delinquent vote-set classification.
- Counted every failed WebSocket reachability probe by connector, including fallback attempts before a later connector succeeds, without exposing endpoint URLs.
- Added bounded per-connector, per-method RPC request duration and failure telemetry without exposing endpoint URLs.
- Split collector ownership into command, RPC transport, domain model, aggregation, snapshot orchestration, and domain-focused test modules without changing collection behavior.
- Added Rust documentation for command routing, RPC transport and wire models, snapshot provenance, aggregation, labels, and validator metric semantics.
- Added typed health, consensus, leadership, and economics commands with explicit per-command connector fallback routes and selected-source provenance.
- Added validator economics collection with explicit capability checks for required reward methods.
- Migrated collector and Surfpool integration paths to command-specific routes while preserving RPC collection and aggregation behavior.
- Bound each command to an explicit transport and feature-gated retained-RPC economics commands with `historical-data` plus Jito provider-API rewards with `jito-mev-rewards`, so configuration validation rejects ineligible connectors before polling while preserving declared fallback order.
- Made each mode own its valid transport and provider-feature combinations, then resolve the configured provider through each command route.
- Redacted credential-bearing RPC, WebSocket, and provider-API URLs from transport errors.
- Added bounded, valid startup probes for every standard Solana method promised by each connector's compatibility profile plus retained prior-epoch reads promised by `historical-data`, and checked configured Jito routes against the economics target before startup or reload succeeds.
- Counted every failed RPC attempt and each received HTTP status before payload decoding, with bounded failure totals, response classes, and exact readiness-critical status codes.
- Collected recent performance samples, economics-target account balances and rent exemption, VAT runway, epoch timing, and per-validator next-leader estimates with explicit source routes and absent-data semantics; account balances remain available when a provider omits the account size needed for rent and VAT calculations.
- Ignored vote-credit samples beyond the current epoch and preserved unmatched vote credits and efficiency as unavailable instead of zero.
