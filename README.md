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
| Watcher | Typed mode commands, Solana RPC collection, connector fallback, provenance, labels, and validator aggregation | [`src/mode.rs`](src/mode.rs), [`src/source.rs`](src/source.rs), [`src/collector/`](src/collector/) | [`changelog/watcher.md`](changelog/watcher.md) |
| Runtime | Process startup, polling, HTTP serving, reload, watcher state, task supervision, and shutdown | [`src/main.rs`](src/main.rs), [`src/runtime/`](src/runtime/), [`src/watcher_state/`](src/watcher_state/), [`src/tasks.rs`](src/tasks.rs) | [`changelog/runtime.md`](changelog/runtime.md) |
| Configuration | TOML loading, environment expansion, validation, and committed profiles | [`src/config/`](src/config/), [`config/`](config/) | [`changelog/configuration.md`](changelog/configuration.md) |
| Observability | Prometheus metrics, scraping, Grafana provisioning, dashboards, and the Pages site | [`src/metrics/`](src/metrics/), [`docker/prometheus/`](docker/prometheus/), [`docker/grafana/`](docker/grafana/), [`site/`](site/) | [`changelog/observability.md`](changelog/observability.md) |
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

## HTTP and container security

`metrics_host` defaults to `127.0.0.1`, so profiles that omit it expose
`/metrics`, `/healthz`, and `/readyz` only on the local machine. Profiles loaded
by containers explicitly listen on `0.0.0.0` so Prometheus can scrape them,
while every documented host publication remains bound to `127.0.0.1`. The
committed Devnet and Mainnet profiles are container-oriented; do not use them
for a direct host run without changing `metrics_host`.

The runtime image runs as the unprivileged `watcher` user. Compose drops all
Linux capabilities, prevents privilege escalation, makes the root filesystem
read-only, and mounts a bounded `/tmp` tmpfs for the liveness file. The image
health check runs `sol-validator-watcher healthcheck`, which probes `/healthz`;
collection failures remain a readiness concern and do not make the container
unhealthy.

Grafana and Prometheus are loopback-only local tools. Grafana uses the
credentials from `.env` and defaults to `admin`/`admin`; change that password
before intentionally adapting the Compose files for non-loopback access.

## Published images

The container workflow publishes only after the repository CI workflow passes
for the same commit. Main-branch, version-tag, and explicitly dispatched builds
produce a `linux/amd64` and `linux/arm64` image index with BuildKit provenance,
an SBOM, and a GitHub artifact attestation for the published digest. Branch and
version tags preserve the existing release semantics; `sha-*` tags contain the
full verified commit identifier.

After authenticating to GHCR, verify a published digest or tag against the
repository and its publishing workflow:

```bash
gh attestation verify \
  oci://ghcr.io/v4lproik/sol-validator-watcher:sha-<commit> \
  --repo v4lproik/sol-validator-watcher \
  --signer-workflow v4lproik/sol-validator-watcher/.github/workflows/container.yml
```

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

[[connectors]]
name = "jito-api"
provider = "jito"
features = ["jito-mev-rewards"]
provider_api_url = "${JITO_API_URL:-https://kobe.mainnet.jito.network/api/v1}"
provider_api_timeout_sec = 30

[modes.health.commands]
rpc_health = ["validator", "helius-devnet"]
ws_health = ["helius-devnet"]
node_status = ["validator"]

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
jito_mev_rewards = ["jito-api"]

[modes.economics.validator]
vote_account = "<VOTE_ACCOUNT_PUBKEY>"
```

Command requirements are independent of the provider name:

| Commands | Transport | Required Solana RPC compatibility | Required provider feature |
| --- | --- | --- | --- |
| `rpc-health`, `node-status`, `epoch-info`, `vote-accounts`, `current-leader-schedule`, `next-leader-schedule` | RPC | `core` | None |
| `ws-health` | WebSocket | None | None |
| `recent-performance-samples`, `current-block-production`, `account-balances` | RPC | `extended` | None |
| `inflation-rewards`, `reward-epoch-block-production` | RPC | `extended` | `historical-data` |
| `jito-mev-rewards` | Provider API | None | `jito-mev-rewards` |

`core` and `extended` are watcher-defined compatibility profiles for standard Solana JSON-RPC methods, not provider product tiers. `core` covers health, identity, version, block-height, epoch, vote-account, and leader-schedule reads. `extended` adds performance samples, block production, account reads, rent exemption, inflation rewards, and minimum-ledger-slot reads; it corresponds to Agave's `--full-rpc-api` surface, while a hosted provider may expose equivalent methods differently. An `extended` connector can serve `core` commands. `historical-data` is an independent promise that prior-epoch data required by economics collection remains available.

`jito-mev-rewards` is optional within enabled economics mode. It reads the
configured target vote account from Jito's validator API and exports the latest
settled prior-epoch MEV reward plus validator commission. A configured route is
checked during startup and transactional reload. During polling, route
exhaustion omits only the Jito series; required balance, inflation-reward, and
block-production failures still fail the economics round. Settled rewards are
cached for the active source chain; an absent epoch is retried after Jito's
documented 60-second API cache.

`node_status` is an optional health-mode route intended for an operator-owned validator endpoint. When configured, it collects the active identity, validator client version and feature set, block height, structured catch-up distance, and vote-set classification from that routed connector. Route exhaustion omits the node-local series and marks health mode down without fabricating values.

A Helius connector may be `extended` for the standard Solana methods while also offering DAS, Enhanced Transactions, Sender, WebSocket extensions, or LaserStream. Those are independent provider APIs and transports; `solana_rpc_compatibility = "extended"` does not claim or verify them. A provider-specific API belongs in the capability and endpoint model only when a typed watcher command consumes it.

Route and source metrics expose the connector declaration as `solana_rpc_compatibility` and the command requirement as `required_solana_rpc_compatibility`; Grafana labels both as standard Solana RPC compatibility.

The URL variables contain the complete provider endpoints, including any provider-specific host, path, and authentication query. The watcher does not reconstruct them from an API token. `rpc_url` requires an explicit `solana_rpc_compatibility`; `ws_url` does not. `provider_api_url` is a validated HTTP(S) base URL used only by typed provider-API commands; a route using it must still satisfy the command's declared provider feature. Provider names are provenance labels, not capability inference.

Array order is the command's sole priority and fallback policy. A connector may be first for one command and last for another. Static validation rejects routes whose connector lacks the required transport, standard Solana RPC compatibility, or provider feature. Before the first poll, the watcher sends bounded valid requests for every method implied by each connector's compatibility declaration and checks configured Jito reward routes against the economics target. A transport, HTTP, payload, JSON-RPC, or missing-method failure exits startup with connector, provider, endpoint kind, capability, and method context while redacting the URL. Changed connectors are probed before a reload replaces the last-known-good runtime. When a required RPC command or configured WebSocket health command exhausts its route, the returned error preserves every failed connector, provider, and causal error in fallback order.

## Readiness, freshness, and transport reliability

`/healthz` reports only that the HTTP process is serving. `/readyz` returns
`503 Service Unavailable` until five consecutive required polling rounds
complete without a failed RPC attempt or non-2xx HTTP response. Transport,
response-read, payload, JSON-RPC, and missing-result failures plus any non-2xx
response reset the streak; an accepted configuration reload also restarts
admission for the new runtime.

After startup validation succeeds, a required RPC or collection failure fails
the whole polling round rather than the process. Required prerequisite calls,
including `minimumLedgerSlot` for retained reward history, propagate failure
instead of being converted to absence. The watcher marks affected collection
modes down, resets readiness, retains the last successful snapshot, does not
advance the liveness file, and retries after 1, 2, 4, 8, 16, then at most 30
seconds. A successful round resets the backoff; `/readyz` requires five new
clean rounds before readmission. Startup capability or configuration contract
failures remain fatal, while an invalid reload candidate continues to preserve
the last-known-good runtime.

Every 30 seconds, the watcher logs its readiness, completed and clean polling
rounds, failed RPC attempts, cumulative response count, response-class counts,
and exact counts for HTTP 200, 403, 429, and 500. Prometheus exposes the same
state through `sol_watcher_ready`, `sol_watcher_poll_round_count`,
`sol_watcher_clean_poll_streak`, `sol_watcher_poll_interval_seconds`,
`sol_rpc_request_failure_count`, `sol_rpc_http_response_class_count`, and
`sol_rpc_http_response_status_count`.

`sol_last_successful_snapshot_timestamp_seconds` and
`sol_snapshot_duration_seconds` distinguish fresh data from a running process;
the timestamp remains absent until the first complete snapshot.
`sol_snapshot_failures_total`, `sol_rpc_request_duration_seconds`,
`sol_rpc_request_failures_total`, and `sol_ws_probe_failures_total` expose
failed rounds, per-connector/per-method RPC latency and failures, and every
failed WebSocket reachability probe by connector. The routed
`recent-performance-samples` command publishes
`sol_cluster_transactions_per_second`,
`sol_cluster_vote_transactions_per_second`,
`sol_cluster_non_vote_transactions_per_second`, and
`sol_cluster_slots_per_second`; the vote split remains absent when the selected
endpoint omits it. The consensus dashboard pairs these cluster rates with
delinquent-stake and network-stake-share trends, while the leadership dashboard
compares each selected scope's skip rate with the all-network baseline. When
`node_status` is configured, `sol_node_info`, `sol_node_health`,
`sol_node_catchup_distance_slots`, and `sol_node_block_height` expose the
operator-owned node's identity, software, voting classification, cluster
health, and height. The overview dashboard shows snapshot freshness and these
node-local signals with warning and critical thresholds.
When a Jito reward route succeeds, the economics dashboard publishes
`sol_economics_jito_mev_reward_sol`,
`sol_economics_jito_mev_commission_pct`, and
`sol_economics_jito_mev_reward_epoch`. These series remain absent when
economics mode is disabled, no route is configured, the epoch is not settled,
or every configured provider-API connector fails.

The local Prometheus scrapes only the watcher profiles selected by
`just stack up <target>` and evaluates baseline target-down, readiness,
freshness, RPC- and WebSocket-failure, node-behind, node-not-voting, validator
delinquency/vote-lag, upcoming-leader, and VAT-funding alerts. Snapshot
freshness uses the greater of 120 seconds or four configured poll intervals.
Notification routing requires an external Alertmanager. Run
`just stack check` to validate the Prometheus configuration and execute its
alert rule tests.

## Contributing

Read [`CONTRIBUTING.md`](CONTRIBUTING.md) and the [module changelog index](CHANGELOG.md). Every change updates the changelog of each affected module and passes:

```bash
just ci verify
```

## License

MIT. See [`LICENSE`](LICENSE).
