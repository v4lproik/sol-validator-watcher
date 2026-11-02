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

`[[connectors]]` declares a stable provider name, the provider features available through that connector, and reusable transport settings. Each mode then assigns an exact fallback order to every command it owns:

```toml
[[connectors]]
name = "validator"
provider = "solana-validator"
features = []
rpc_url = "http://validator:8899"

[[connectors]]
name = "helius-devnet"
provider = "helius"
features = ["historical-data"]
rpc_url = "https://devnet.helius-rpc.com/?api-key=${HELIUS_DEVNET_API_KEY}"
ws_url = "wss://devnet.helius-rpc.com/?api-key=${HELIUS_DEVNET_API_KEY}"

[modes.health.commands]
rpc_health = ["validator", "helius-devnet"]
ws_health = ["helius-devnet"]

[modes.consensus.commands]
epoch_info = ["validator", "helius-devnet"]
vote_accounts = ["validator", "helius-devnet"]

[modes.leadership.commands]
current_block_production = ["helius-devnet", "validator"]
current_leader_schedule = ["validator", "helius-devnet"]
next_leader_schedule = ["validator", "helius-devnet"]
```

The Helius Devnet profile maps modes to these source requirements:

| Mode | Provider | Transport | Required provider feature |
| --- | --- | --- | --- |
| Health | `helius` | RPC | None |
| Health | `helius` | WebSocket | None |
| Consensus | `helius` | RPC | None |
| Leadership | `helius` | RPC | None |
| Economics | `helius` | RPC | `historical-data` |

`rpc_url` and `ws_url` define the available `rpc` and `websocket` transports; they are not provider features. The current watcher-specific provider feature is `historical-data`. Economics commands require that feature because they query prior-epoch rewards and block production. Standard health, consensus, and leadership commands require only the matching transport. Provider names identify the service supplying each command result and are exported with connector, route, and selected-source metrics.

Array order is the command's sole priority and fallback policy. A connector may be first for one command and last for another. Startup rejects missing providers or feature arrays, duplicate features, unknown names, duplicate route entries, missing required routes, routes without the command's transport, and routes lacking an optional feature required by that command. An optional WebSocket route may name a connector whose environment-expanded `ws_url` is empty; the route remains visible as unavailable and is skipped at execution.

## Contributing

Read [`CONTRIBUTING.md`](CONTRIBUTING.md) and the [module changelog index](CHANGELOG.md). Every change updates the changelog of each affected module and passes:

```bash
just ci verify
```

## License

MIT. See [`LICENSE`](LICENSE).
