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

`[[connectors]]` declares reusable transport settings. Each mode then assigns an exact fallback order to every command it owns:

```toml
[[connectors]]
name = "validator"
kind = "standard-validator+rpc"
rpc_url = "http://validator:8899"

[[connectors]]
name = "hosted"
kind = "helius+enhanced-rpc"
rpc_url = "${SOL_MAINNET_RPC_URL}"

[modes.consensus.commands]
epoch_info = ["validator", "hosted"]
vote_accounts = ["validator", "hosted"]

[modes.leadership.commands]
current_block_production = ["hosted", "validator"]
current_leader_schedule = ["validator", "hosted"]
next_leader_schedule = ["validator", "hosted"]
```

Array order is the command's sole priority and fallback policy. A connector may be first for one command and last for another. Startup rejects unknown names, duplicate route entries, missing required routes, and RPC commands assigned to connectors without `rpc_url`. An optional WebSocket route may name a connector whose environment-expanded `ws_url` is empty; the route remains visible as unavailable and is skipped at execution.

## Contributing

Read [`CONTRIBUTING.md`](CONTRIBUTING.md) and the [module changelog index](CHANGELOG.md). Every change updates the changelog of each affected module and passes:

```bash
just ci verify
```

## License

MIT. See [`LICENSE`](LICENSE).
