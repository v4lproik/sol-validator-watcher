# Configuration changelog

## Unreleased

- Added the `block-fee-rewards` provider feature and optional economics `recent_block_fee_rewards` route, requiring an extended RPC endpoint with confirmed or finalized commitment and explicit connector capability.
- Replaced flat connector endpoint fields with nested `rpc`, `websocket`, and `provider_api` transport tables that own URLs, sensitive headers, connect/request/handshake deadlines, and RPC commitment without compatibility aliases.
- Added case-normalized header validation, protocol-owned header protection, redirect denial, contradictory-timeout rejection, secure TLS-only configuration, redacted transport debug output, and source-free TOML parse diagnostics that retain safe line/column locations for expanded credentials.
- Rejected empty, malformed, hostless, or unsupported-scheme connector RPC, WebSocket, and provider-API URLs before client construction while keeping every endpoint and timeout connector-local.
- Rejected command routes whose connector lacks the required RPC, WebSocket, or provider-API transport.
- Added validated provider-API connector URLs and timeouts, the `jito-mev-rewards` provider feature, and the optional economics `jito_mev_rewards` command route without inferring either from provider names or standard Solana RPC compatibility.
- Added an optional ordered `health.commands.node_status` route to local and Compose validator profiles for operator-owned node diagnostics.
- Added a typed `metrics_host` setting that defaults omitted profiles to loopback while container-loaded profiles explicitly bind their private-network listener.
- Moved configuration ownership into a dedicated module tree with separate load, validation, and environment-expansion tests.
- Added Rust documentation for TOML profile fields, connector declarations, mode enablement, and ordered command-route semantics.
- Replaced paired YAML watcher and TOML connection files with one ordered TOML file per runnable profile.
- Removed split-config compatibility fields, relative companion-file loading, YAML parsing, and the obsolete transport-mode environment setting.
- Replaced the multi-provider parser dependency with direct TOML deserialization and current Serde support.
- Grouped the self-contained profiles under `config/local/`, `config/compose/`, and `config/remote/`.
- Preserved environment expansion, normalization, validation, and profile behavior while moving execution priority from connector declaration order to explicit command routes.
- Rejected unknown top-level and nested fields so misspellings and removed split-config keys fail before runtime construction.
- Removed the redundant root validator mode and WebSocket toggle; connector URLs now define available transports.
- Added strict built-in mode command routes whose connector arrays define the complete priority and fallback order for each command.
- Rejected missing required routes, unknown or duplicate route names, disabled economics routes, and RPC commands assigned to connectors without RPC transport.
- Replaced provider-specific connector kinds with explicit `provider` names and watcher-relevant `features` arrays.
- Separated transport from provider capabilities: nested `rpc`, `websocket`, and `provider_api` tables determine available transports, feature arrays may be empty, retained RPC history requires `historical-data`, and Jito rewards require `jito-mev-rewards`.
- Added required `solana_rpc_compatibility = "core" | "extended"` declarations for RPC connectors, validated every command route against the standard Solana method profile independently of provider-specific APIs, reserved `historical-data` for prior-epoch retention on extended endpoints, and allowed extended-only optional routes to remain empty in core-only profiles.
- Added a Helius Devnet profile that consumes complete authenticated RPC and WebSocket URLs and enables the economics mode on a declared `historical-data` source.
- Added required `recent_performance_samples` consensus routes and economics `account_balances` routes; balance collection uses standard RPC while prior-epoch reward routes retain the `historical-data` requirement.
