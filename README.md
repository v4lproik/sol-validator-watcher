<p align="center">
  <img src="assets/logo.svg" alt="Solana Validator Watcher logo" width="112">
</p>

<h1 align="center">Solana Validator Watcher</h1>

[![codecov](https://codecov.io/gh/v4lproik/sol-validator-watcher/graph/badge.svg?token=V3PM3PQJ49)](https://codecov.io/gh/v4lproik/sol-validator-watcher)

<p align="center">
  Prometheus monitoring and Grafana dashboards for Solana validator operators.
</p>

Solana Validator Watcher polls ordered RPC and WebSocket connectors, aggregates network and watched-validator health, and exports the result as Prometheus metrics. The repository includes local Surfpool, validator, Devnet, and Mainnet profiles with provisioned Grafana dashboards.

## Modules

| Module | Owns | Start here | Changelog |
| --- | --- | --- | --- |
| Watcher | Solana RPC collection, connector fallback, labels, and validator aggregation | [`src/collector.rs`](src/collector.rs), [`src/labels.rs`](src/labels.rs), [`src/websocket.rs`](src/websocket.rs) | [`changelog/watcher.md`](changelog/watcher.md) |
| Runtime | Process startup, polling, HTTP serving, reload, task supervision, and shutdown | [`src/main.rs`](src/main.rs), [`src/tasks.rs`](src/tasks.rs) | [`changelog/runtime.md`](changelog/runtime.md) |
| Configuration | YAML/TOML loading, environment expansion, validation, and committed profiles | [`src/config.rs`](src/config.rs), [`config/`](config/) | [`changelog/configuration.md`](changelog/configuration.md) |
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

Run `just` to discover all module commands. Configuration templates live in [`config/`](config/); keep credentials in the ignored root `.env`.

## Contributing

Read [`CONTRIBUTING.md`](CONTRIBUTING.md) and the [module changelog index](CHANGELOG.md). Every change updates the changelog of each affected module and passes:

```bash
just ci verify
```

## License

MIT. See [`LICENSE`](LICENSE).
