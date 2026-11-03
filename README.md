<p align="center">
  <img src="assets/logo.svg" alt="Solana Validator Watcher logo" width="112">
</p>

<h1 align="center">Solana Validator Watcher</h1>

[![codecov](https://codecov.io/gh/v4lproik/sol-validator-watcher/graph/badge.svg?token=V3PM3PQJ49)](https://codecov.io/gh/v4lproik/sol-validator-watcher)

<p align="center">
  Prometheus monitoring and Grafana dashboards for Solana validator operators.
</p>

Solana Validator Watcher executes typed health, consensus, leadership, and economics command chains, aggregates network and watched-validator health, and exports the result as Prometheus metrics. Each command declares its own ordered connector fallback route, so provider choice is explicit rather than inferred from global connector order.

## Modules

| Module | Owns | Start here | Changelog |
| --- | --- | --- | --- |
| Watcher | Typed mode commands, Solana RPC collection, connector fallback, provenance, labels, and validator aggregation | [`src/mode.rs`](src/mode.rs), [`src/source.rs`](src/source.rs), [`src/collector/`](src/collector/), [`src/collector.rs`](src/collector.rs) | [`changelog/watcher.md`](changelog/watcher.md) |
| Runtime | Process startup, polling, HTTP serving, reload, task supervision, and shutdown | [`src/main.rs`](src/main.rs), [`src/tasks.rs`](src/tasks.rs) | [`changelog/runtime.md`](changelog/runtime.md) |
| Configuration | TOML loading, environment expansion, validation, and committed profiles | [`src/config.rs`](src/config.rs), [`config/`](config/) | [`changelog/configuration.md`](changelog/configuration.md) |
| Observability | Prometheus metrics, scraping, Grafana provisioning, dashboards, and the Pages site | [`src/metrics.rs`](src/metrics.rs), [`docker/prometheus/`](docker/prometheus/), [`docker/grafana/`](docker/grafana/), [`site/`](site/) | [`changelog/observability.md`](changelog/observability.md) |
| Operations | Container image, Compose profiles, Surfpool, and local stack lifecycle | [`docker/`](docker/) | [`changelog/operations.md`](changelog/operations.md) |
| Tooling | Just commands, CI, coverage, publishing, scripts, and repository guidance | [`Justfile`](Justfile), [`just/`](just/), [`.github/workflows/`](.github/workflows/) | [`changelog/tooling.md`](changelog/tooling.md) |

## Quick start

Requires Docker and the tools pinned by [`mise.toml`](mise.toml).

```bash
mise install
cp .env.example .env
just stack up surfpool
```

Open Prometheus at <http://127.0.0.1:9090> and Grafana at <http://127.0.0.1:3001>. Stop the stack with:

```bash
just stack down
```

Run `just` to discover all module commands. Configuration templates live in [`config/`](config/), grouped under `local/`, `compose/`, and `remote/`; each runnable profile is one self-contained TOML file. Keep credentials in the ignored root `.env`.

## Command routes

`[[connectors]]` declares a stable provider name, its compatibility with the standard Solana JSON-RPC methods used by this watcher, optional provider features, and reusable transport settings. Each mode then assigns an exact fallback order to every command it owns:

```toml
[[connectors]]
name = "validator"
provider = "solana-validator"
features = []
solana_rpc_compatibility = "core"
rpc_url = "http://validator:8899"

[[connectors]]
name = "helius-devnet"
provider = "helius"
features = ["historical-data"]
solana_rpc_compatibility = "extended"
rpc_url = "${SOL_DEVNET_RPC_URL}"
ws_url = "${SOL_DEVNET_WS_URL}"

[modes.health.commands]
rpc_health = ["validator", "helius-devnet"]
ws_health = ["helius-devnet"]

[modes.consensus.commands]
epoch_info = ["validator", "helius-devnet"]
vote_accounts = ["validator", "helius-devnet"]
recent_performance_samples = ["helius-devnet"]

[modes.leadership.commands]
current_block_production = ["helius-devnet"]
current_leader_schedule = ["validator", "helius-devnet"]
next_leader_schedule = ["validator", "helius-devnet"]

[modes.economics]
enabled = true

[modes.economics.commands]
account_balances = ["helius-devnet"]
inflation_rewards = ["helius-devnet"]
reward_epoch_block_production = ["helius-devnet"]

[modes.economics.validator]
vote_account = "<VOTE_ACCOUNT_PUBKEY>"
```

Command requirements are independent of the provider name:

| Commands | Transport | Required Solana RPC compatibility | Required provider feature |
| --- | --- | --- | --- |
| `rpc-health`, `epoch-info`, `vote-accounts`, `current-leader-schedule`, `next-leader-schedule` | RPC | `core` | None |
| `ws-health` | WebSocket | None | None |
| `recent-performance-samples`, `current-block-production`, `account-balances` | RPC | `extended` | None |
| `inflation-rewards`, `reward-epoch-block-production` | RPC | `extended` | `historical-data` |

`core` and `extended` are watcher-defined compatibility profiles for standard Solana JSON-RPC methods, not provider product tiers. `core` covers health, epoch, vote-account, and leader-schedule reads. `extended` adds performance samples, block production, account reads, rent exemption, inflation rewards, and minimum-ledger-slot reads; it corresponds to Agave's `--full-rpc-api` surface, while a hosted provider may expose equivalent methods differently. An `extended` connector can serve `core` commands. `historical-data` is an independent promise that prior-epoch data required by economics collection remains available.

A Helius connector may be `extended` for the standard Solana methods while also offering DAS, Enhanced Transactions, Sender, WebSocket extensions, or LaserStream. Those are independent provider APIs and transports; `solana_rpc_compatibility = "extended"` does not claim or verify them. A provider-specific API belongs in the capability and endpoint model only when a typed watcher command consumes it.

Route and source metrics expose the connector declaration as `solana_rpc_compatibility` and the command requirement as `required_solana_rpc_compatibility`; Grafana labels both as standard Solana RPC compatibility.

The URL variables contain the complete provider endpoints, including any provider-specific host, path, and authentication query. The watcher does not reconstruct them from an API token. `rpc_url` requires an explicit `solana_rpc_compatibility`; `ws_url` does not. Provider names are provenance labels, not capability inference.

Array order is the command's sole priority and fallback policy. A connector may be first for one command and last for another. Static validation rejects routes whose connector lacks the required transport, standard Solana RPC compatibility, or provider feature. Before the first poll, the watcher sends bounded valid requests for every method implied by each connector's compatibility declaration. A transport, HTTP, payload, JSON-RPC, or missing-method failure exits startup with connector, provider, endpoint kind, capability, and method context while redacting the URL. Changed connectors are probed before a reload replaces the last-known-good runtime.

## Contributing

Read [`CONTRIBUTING.md`](CONTRIBUTING.md) and the [module changelog index](CHANGELOG.md). Every change updates the changelog of each affected module and passes:

```bash
just ci verify
```

## License

MIT. See [`LICENSE`](LICENSE).
