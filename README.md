# Solana Validator Watcher

Rust implementation of the same core idea as [`kilnfi/eth-validator-watcher`](https://github.com/kilnfi/eth-validator-watcher), adapted to Solana.

The watcher polls Solana RPC, groups validators with the same label model as the Ethereum watcher, and exports Prometheus metrics that compare:

- your watched validators
- the rest of the network
- the whole network
- any custom `category:value` grouping you define

## What It Tracks

- current epoch and slot
- watched vs network validator counts
- watched vs network activated stake
- delinquent validators and delinquent stake
- vote-credit performance
- average last-vote lag and root-slot lag
- average commission
- produced blocks vs missed leader slots
- future leader slots in the current and next epoch
- per-watched-validator raw metrics and scope membership

## Configuration

Example config:

```yaml
network: mainnet-beta
rpc_url: https://api.mainnet-beta.solana.com
rpc_timeout_sec: 30
transport_mode: poll-http-and-ws
ws_url: wss://api.mainnet-beta.solana.com
ws_timeout_sec: 10
validator_mode: consensus-and-rpc
validator_websocket_enabled: true
metrics_port: 8000
poll_interval_sec: 15
commitment: confirmed
delinquent_slot_distance: 128
liveness_file: /tmp/sol-validator-watcher.liveness

watched_validators:
  - identity: 11111111111111111111111111111111
    labels: ["vc:validator-1", "region:sbg"]
  - vote_account: Vote111111111111111111111111111111111111111
    labels: ["vc:validator-2", "region:rbx"]
```

Each watched entry can match by `identity`, `vote_account`, or both.

Transport and validator role options:

- `transport_mode: poll-http` for RPC polling only
- `transport_mode: poll-http-and-ws` to keep polling over HTTP and also probe a configured websocket transport
- `validator_mode: consensus`
- `validator_mode: rpc`
- `validator_mode: consensus-and-rpc`
- `validator_mode: non-voting`
- `validator_mode: observer`

As in the Ethereum watcher, labels should follow the `category:value` format. The watcher always adds these built-in scopes:

- `scope:watched`
- `scope:network`
- `scope:all-network`

So a watched validator with `["region:sbg", "vc:validator-1"]` contributes to all of these groups:

- `scope:all-network`
- `scope:watched`
- `region:sbg`
- `vc:validator-1`

Non-watched validators contribute to:

- `scope:all-network`
- `scope:network`

## Metrics

Primary exported series:

- `sol_slot{network}`
- `sol_epoch{network}`
- `sol_watcher_up{network}`
- `sol_rpc_up{network}`
- `sol_ws_up{network}`
- `sol_watcher_config_info{network,transport_mode,validator_mode,validator_websocket_enabled,ws_configured}`
- `sol_validator_status_count{scope,status,network}`
- `sol_validator_status_stake_sol{scope,status,network}`
- `sol_activated_stake_sol{scope,network}`
- `sol_delinquent_validator_count{scope,network}`
- `sol_delinquent_stake_sol{scope,network}`
- `sol_vote_credits_sample_epoch{network}`
- `sol_vote_credits_latest_epoch{scope,network}`
- `sol_vote_efficiency_pct{scope,network}`
- `sol_last_vote_slot_lag{scope,network}`
- `sol_root_slot_lag{scope,network}`
- `sol_average_commission_pct{scope,network}`
- `sol_blocks_produced{scope,network}`
- `sol_blocks_missed{scope,network}`
- `sol_block_production_rate{scope,network}`
- `sol_future_leader_slots{scope,network}`
- `sol_watched_validator_info{network,identity,vote_account,status}`
- `sol_watched_validator_scope_info{network,identity,vote_account,scope}`
- `sol_watched_validator_activated_stake_sol{network,identity,vote_account,status}`
- `sol_watched_validator_vote_credits_latest_epoch{network,identity,vote_account,status}`
- `sol_watched_validator_vote_efficiency_pct{network,identity,vote_account,status}`
- `sol_watched_validator_last_vote_slot_lag{network,identity,vote_account,status}`
- `sol_watched_validator_root_slot_lag{network,identity,vote_account,status}`
- `sol_watched_validator_commission_pct{network,identity,vote_account,status}`
- `sol_watched_validator_blocks_produced{network,identity,vote_account,status}`
- `sol_watched_validator_blocks_missed{network,identity,vote_account,status}`
- `sol_watched_validator_block_production_rate{network,identity,vote_account,status}`
- `sol_watched_validator_future_leader_slots{network,identity,vote_account,status}`

## Run

```bash
just run-release
```

Metrics are exposed on:

```text
http://127.0.0.1:8000/metrics
http://127.0.0.1:8000/healthz
```

## Commands

Bootstrap the repo tools with `mise` first:

```bash
mise install
```

Everything should be run through `just`:

```bash
just
just doctor
just fmt
just check
just clippy
just test
just test-surfpool
just coverage
just coverage-html
just coverage-lcov
just coverage-ci
just build-release
just run
just run-release
just docker-build
just doctor-docker
just stack-config-surfpool
just stack-config-validator
just stack-up-surfpool
just stack-up-validator
just stack-up-all
just stack-logs-surfpool
just stack-logs-validator
just stack-ps-surfpool
just stack-ps-validator
just stack-down
just ci
just ci-full
```

`mise.toml` installs the pinned `rust` toolchain, `just`, and `cargo-llvm-cov`. `surfpool` is still only needed for `just test-surfpool` / `just ci-full`.

## Hosted Automation

GitHub Actions is wired for three jobs:

- `ci`: runs `just ci`, enforces `just coverage-ci`, uploads `lcov.info` and HTML coverage artifacts, and sends coverage to Codecov.
- `container`: publishes OCI images to `ghcr.io/<owner>/sol-validator-watcher` on pushes to `main`, tags that match `v*`, and manual workflow runs.
- `coverage-pages`: publishes the HTML report generated by `just coverage-html` to GitHub Pages on `main` and manual workflow runs.

Repo settings needed on GitHub:

- Pages source: `GitHub Actions`
- Codecov: no extra secret for public repos; add `CODECOV_TOKEN` for private repos if you want uploads to succeed consistently
- GHCR: no extra secret needed; the workflow uses the repository `GITHUB_TOKEN`

Artifacts and endpoints:

- CI artifacts: `coverage-lcov` and `coverage-html`
- GHCR image: `ghcr.io/<owner>/sol-validator-watcher:latest` on `main`, plus semver tags on `v*`
- GitHub Pages coverage site: exposed by the `coverage-pages` workflow deployment URL

## Compose Stack

The repo now ships a full local observability stack:

- `Prometheus` on `http://127.0.0.1:9090`
- `Grafana` on `http://127.0.0.1:3001`
- `Surfpool Studio` on `http://127.0.0.1:13000` by default when the Surfpool profile is enabled
- `Surfpool RPC` on `http://127.0.0.1:18899` by default when the Surfpool profile is enabled
- `Surfpool WS` on `ws://127.0.0.1:18900` by default when the Surfpool profile is enabled
- `watcher-surfpool` metrics on `http://127.0.0.1:8000/metrics`
- `watcher-validator` metrics on `http://127.0.0.1:8001/metrics`

Profiles exposed through `just`:

- `just stack-up-surfpool` starts `surfpool`, `watcher-surfpool`, `prometheus`, and `grafana`
- `just stack-up-validator` starts `watcher-validator`, `prometheus`, and `grafana`
- `just stack-up-all` starts both watcher targets plus the shared Prometheus and Grafana services
- `just stack-down` tears the stack down

Useful inspection commands:

- `just doctor-docker`
- `just stack-config-surfpool`
- `just stack-config-validator`
- `just stack-ps-surfpool`
- `just stack-ps-validator`
- `just stack-logs-surfpool`
- `just stack-logs-validator`

`Grafana` provisions a `Solana / Solana Validator Watcher Overview` dashboard automatically, backed by the provisioned Prometheus datasource. The dashboard defaults to `scope:all-network` so it shows data even before you add watched validators to the compose watcher configs.

External validator mode:

- `compose.validator.yaml` points the watcher at `host.docker.internal` by default so it can reach a validator or RPC endpoint running on the host machine
- the watcher config now expands `${VAR}` and `${VAR:-default}` placeholders, so `etc/config.compose.validator.yaml` can be driven by environment variables
- `.env.example` lists the variables used by the validator profile and the Surfpool host port mappings; the compose stack reads them when `just stack-up-validator`, `just stack-up-surfpool`, or `just stack-up-all` is used

Compose config files:

- `compose.yaml` defines the shared Prometheus and Grafana services
- `compose.surfpool.yaml` adds Surfpool and a watcher pointed at Surfpool
- `compose.validator.yaml` adds a watcher pointed at an external validator or RPC endpoint
- `deploy/prometheus/prometheus.yml` scrapes both watcher services
- `deploy/grafana/provisioning` and `deploy/grafana/dashboards` provision the Prometheus datasource and the default dashboard

## Tests

Fast tests:

```bash
just test
```

Surfpool-backed smoke test:

```bash
just test-surfpool
```

The Surfpool test launches a real local Surfpool process on ephemeral ports, waits for RPC readiness, runs the watcher snapshot collection path against it, and renders metrics. It is opt-in so the default test suite stays fast and does not depend on local validator startup.

Coverage:

```bash
just coverage
just coverage-html
just coverage-lcov
just coverage-ci
```

`just coverage-ci` enforces a minimum line coverage threshold of `80%`.
The coverage gate excludes `src/main.rs` and `src/surfpool_tests.rs`, so the threshold tracks core watcher modules rather than binary bootstrap and the opt-in e2e harness.

## Notes

- Config reload is automatic when the config file changes.
- The watcher fails fast and exits if the configured RPC endpoint cannot be contacted, reports unhealthy via `getHealth`, or if an enabled websocket endpoint cannot be reached.
- If `rpc_url`, `rpc_timeout_sec`, or `commitment` changes, the RPC client is recreated.
- If `transport_mode`, `ws_url`, or `ws_timeout_sec` changes, the websocket probe client is recreated.
- If `metrics_port` changes, restart the process. The listener address is fixed at startup.
- `sol_ws_up` is a websocket reachability probe. The watcher still uses HTTP RPC for data collection in this version.
- `sol_vote_efficiency_pct` is a stake-weighted comparison against the best validator in the sampled vote-credit epoch. It is intended as a Solana equivalent of the Ethereum watcher’s performance view, not a protocol-native official score.
