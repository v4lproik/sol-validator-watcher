<p align="center">
  <img src="assets/logo.svg" alt="Solana Validator Watcher logo" width="112">
</p>

<h1 align="center">Solana Validator Watcher</h1>

<p align="center">
  Operator-first Prometheus monitoring for Solana validators, built in Rust.
</p>

<p align="center">
  Watch your validators against the rest of the network, ship usable dashboards on day one, and surface the signals that matter during incidents: vote lag, delinquency, skip pressure, stake, and future leader slots.
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
- local devnet, Surfpool, and external-validator profiles out of the box
- split validator connection config via `figment` and TOML, so secrets stay out of the main YAML
- graceful degradation when optional RPC methods are unavailable on a provider

## What it exports

- network health: epoch, slot, watcher/RPC/websocket status
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
The watcher config can point at a companion validator-connection TOML file, which is loaded through `figment` and still supports `${VAR}` / `${VAR:-default}` expansion.

Pick a config profile:

- `etc/config.devnet.yaml`
- `etc/config.compose.validator.yaml`
- `etc/config.compose.surfpool.yaml`

Set at least:

- `validator_connection_file`
- `rpc_url` inside the referenced TOML file
- `ws_url` inside the referenced TOML file if `transport_mode = "poll-http-and-ws"`
- `watched_validators` if you want watched-validator and custom-scope dashboards

Minimal example:

```yaml
network: devnet
validator_connection_file: validator.devnet.toml
validator_mode: rpc
validator_websocket_enabled: true

watched_validators:
  - vote_account: FwR3PbjS5iyqzLiLugrBqKSa5EKZ4vK9SKs7eQXtT59f
    labels: ["operator:kiln", "validator:kiln-devnet"]
```

```toml
rpc_url = "${SOL_DEVNET_RPC_URL}"
transport_mode = "poll-http-and-ws"
ws_url = "${SOL_DEVNET_WS_URL}"
```

Start the stack:

```bash
just stack-up-devnet
```

Then verify:

- `just stack-ps-devnet`
- `just stack-logs-devnet`
- `http://127.0.0.1:9090/targets`
- `http://127.0.0.1:3001`

If you run more than one watcher profile, select a concrete Grafana `Network` value such as `devnet` instead of `All`.

## Commands

Run everything through `just`:

```bash
just doctor
just fmt
just check
just clippy
just test
just coverage-ci
just site-build
just run config=etc/config.devnet.yaml
just stack-up-devnet
just stack-up-validator
just stack-up-surfpool
just stack-down
```

`just run` loads `.env` automatically when it exists, and Docker Compose uses the same `.env` file for the template values.

## Compose stack

The local stack includes:

- Prometheus on `http://127.0.0.1:9090`
- Grafana on `http://127.0.0.1:3001`
- watcher metrics on:
  - `:8000` for Surfpool
  - `:8001` for external validator mode
  - `:8002` for devnet mode

Provisioned dashboards:

- `Solana Validator Watcher Overview`
- `Solana Validator Watcher Breakdown`

The overview dashboard works immediately on `scope:all-network`. The breakdown dashboard becomes useful once you add watched validators with custom labels.

## CI

GitHub Actions runs:

- formatting, clippy, and tests through `just ci`
- coverage through `just coverage-ci`
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
