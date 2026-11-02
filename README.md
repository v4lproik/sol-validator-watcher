<p align="center">
  <img src="assets/logo.svg" alt="Solana Validator Watcher logo" width="112">
</p>

<h1 align="center">Solana Validator Watcher</h1>

[![codecov](https://codecov.io/gh/v4lproik/sol-validator-watcher/graph/badge.svg?token=V3PM3PQJ49)](https://codecov.io/gh/v4lproik/sol-validator-watcher)

<p align="center">
  Operator-first Prometheus monitoring for Solana validators, built in Rust.
</p>

<p align="center">
  Build a single Grafana-oriented operator surface for Solana: validator health, watched-validator comparisons, network context, and eventually validator economics like Jito revenue, Harmonic revenue, and base revenue per block.
</p>

It follows the same core model as [`kilnfi/eth-validator-watcher`](https://github.com/kilnfi/eth-validator-watcher): poll the network, group validators by labels, and compare:

- watched validators
- the rest of the network
- the full network
- any custom `category:value` scopes you define

## Why operators use it

- operator-first metrics instead of raw RPC dumps
- watched-validator views alongside network and custom-scope baselines
- prebuilt Grafana dashboards for overview and breakdown workflows
- a clear path toward unifying revenue and economics metrics in the same Grafana surface
- local devnet, Surfpool, and external-validator profiles out of the box
- split validator connection config via `figment` and TOML, so secrets stay out of the main YAML
- graceful degradation when optional RPC methods are unavailable on a provider

## What it exports

- network health: epoch, slot, watcher/RPC/websocket status
- connector inventory and metric-source provenance
- validator counts and activated stake
- delinquent validators and stake
- vote efficiency, commission, vote lag, root lag
- blocks produced, missed, skip rate inputs
- future leader slots
- per-watched-validator metrics

## What you get in the box

- a Prometheus exporter for Solana validator fleets
- a label model for watched validators, the rest of the network, and custom slices
- Docker Compose profiles for devnet, Surfpool, and external validator targets
- provisioned Grafana dashboards for fleet health and scope drill-downs
- GitHub Actions for CI, coverage, container publishing, and a Pages-hosted project site with coverage under `/coverage/`

## Product direction

- unify validator operations and validator economics in one Grafana interface
- add Jito revenue, Harmonic revenue, and base revenue per block
- normalize revenue by leader slots, produced blocks, and stake where useful
- reduce operator context-switching across separate dashboards and vendor tools

## Quick start

Install tools:

```bash
mise install
```

Create a local env file:

```bash
cp .env.example .env
```

The files under `etc/` are committed templates. Keep real RPC credentials in `.env` or exported environment variables instead of writing secrets into YAML.
The watcher config can point at one or more companion connector TOML files, which are loaded through `figment` and still support `${VAR}` / `${VAR:-default}` expansion.

Pick a config profile:

- `etc/config.devnet.yaml`
- `etc/config.mainnet.yaml`
- `etc/config.compose.validator.yaml`
- `etc/config.compose.surfpool.yaml`

Set at least:

- `connectors`
- `rpc_url` inside at least one referenced connector TOML file
- `ws_url` inside a connector TOML file if you want websocket probing
- `watched_validators` if you want watched-validator and custom-scope dashboards
- `economics.mode: single-validator` if you want a dedicated economics target

Minimal example:

```yaml
network: devnet
validator_mode: rpc
validator_websocket_enabled: true

connectors:
  - name: local-validator
    kind: standard-validator+rpc
    connection_file: validator.devnet.toml
  - name: helius-fallback
    kind: helius+enhanced-rpc
    rpc_url: ${SOL_DEVNET_FALLBACK_RPC_URL}
  - name: helius-ws
    kind: helius+enhanced-wss
    ws_url: ${SOL_DEVNET_WS_URL}

watched_validators:
  - vote_account: FwR3PbjS5iyqzLiLugrBqKSa5EKZ4vK9SKs7eQXtT59f
    labels: ["operator:kiln", "validator:kiln-devnet"]
```

```toml
rpc_url = "${SOL_DEVNET_RPC_URL}"
ws_url = "${SOL_DEVNET_WS_URL}"
```

Connectors are ordered. The first connector that can serve a metric wins, so you can declare a local validator first, then a hosted RPC fallback, then a websocket-only connector for probes. The exporter exposes both:

- `sol_connector_info` for the declared connector stack and precedence
- `sol_metric_source_info` for the connector that actually supplied each metric family in the latest snapshot

Single-validator economics target:

```yaml
economics:
  mode: single-validator
  validator:
    vote_account: FwR3PbjS5iyqzLiLugrBqKSa5EKZ4vK9SKs7eQXtT59f
    labels: ["operator:kiln", "validator:kiln-devnet"]
```

That target is merged into `watched_validators` automatically and tagged with `focus:economics`.

When `economics.mode: single-validator` is enabled, the watcher runs a startup capability check and treats `getInflationReward` and `getBlockProduction` as required RPC methods. If the endpoint does not support them, the watcher fails before entering the polling loop instead of exporting partial economics.

Start the stack:

```bash
just stack up devnet
```

For a hosted mainnet RPC profile:

```bash
just rust run debug etc/config.mainnet.yaml
```

Or with the local Compose stack:

```bash
just stack up mainnet
```

If Prometheus was already running before you added or changed a watcher profile, reload it so the
new scrape target is picked up:

```bash
just stack reload prometheus
```

The committed `etc/config.mainnet.yaml` uses
sample watched validators and keeps `economics.mode: disabled` by default. Many hosted RPC
endpoints do not retain enough reward-epoch block production history for strict economics mode. If
you enable economics on mainnet, use a vote account you care about and an RPC endpoint that can
serve both `getInflationReward` and previous-epoch `getBlockProduction`.

Then verify:

- `just stack ps devnet`
- `just stack logs devnet`
- `just stack ps mainnet`
- `just stack logs mainnet`
- `http://127.0.0.1:9090/targets`
- `http://127.0.0.1:3001`

If you run more than one watcher profile, select a concrete Grafana `Network` value such as `devnet` instead of `All`.

## Commands

Run everything through `just`:

```bash
just rust doctor
just rust fmt
just rust check
just rust clippy
just rust test
just ci verify
just coverage check
just site build
just rust run debug etc/config.devnet.yaml
just rust run debug etc/config.mainnet.yaml
just stack up devnet
just stack up mainnet
just stack reload prometheus
just stack up validator
just stack up surfpool
just stack down
```

`just rust run` loads `.env` automatically when it exists, and Docker Compose uses the same `.env` file for the template values.

## Compose stack

The local stack includes:

- Prometheus on `http://127.0.0.1:9090`
- Grafana on `http://127.0.0.1:3001`
- watcher metrics on:
  - `:8000` for Surfpool
  - `:8001` for external validator mode
  - `:8002` for devnet mode
  - `:8003` for mainnet mode

Provisioned dashboards:

- `Solana Validator Watcher Overview`
- `Solana Validator Watcher Breakdown`

The overview dashboard works immediately on `scope:all-network`. The breakdown dashboard becomes useful once you add watched validators with custom labels.

## CI

GitHub Actions runs:

- formatting, Clippy, build, and tests through `just ci verify`
- coverage through `just coverage check`
- coverage artifacts upload
- optional Codecov upload
- container publishing to GHCR
- a GitHub Pages site that serves the landing page at `/` and the HTML coverage report at `/coverage/`

For a public repo, set GitHub Pages to `GitHub Actions`. If Pages is not enabled yet and your plan supports it, set `PAGES_AUTO_ENABLE=true` and provide `PAGES_ENABLEMENT_TOKEN` with repository admin or Pages-write access so the workflow can enable it automatically; otherwise the Pages publish job skips cleanly. GitHub documents that Pages is available for public repositories on GitHub Free, and for public and private repositories on GitHub Pro, Team, Enterprise Cloud, and Enterprise Server. `CODECOV_TOKEN` is only needed for private repositories.

## Notes

- The watcher exits if the configured RPC endpoint cannot be reached or fails `getHealth`.
- In websocket mode, it also exits if the websocket endpoint cannot be contacted.
- Some providers reject optional enrichment methods like `getBlockProduction`; the watcher logs that degradation and keeps exporting the core network metrics.

## License

MIT. See [LICENSE](LICENSE).
