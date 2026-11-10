<p align="center">
  <img src="assets/logo.svg" alt="Solana Validator Watcher logo" width="112">
</p>

<h1 align="center">Solana Validator Watcher</h1>

<p align="center">
  <a href="https://github.com/v4lproik/sol-validator-watcher/actions/workflows/ci.yml"><img src="https://github.com/v4lproik/sol-validator-watcher/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI status"></a>
  <a href="https://github.com/v4lproik/sol-validator-watcher/actions/workflows/security.yml"><img src="https://github.com/v4lproik/sol-validator-watcher/actions/workflows/security.yml/badge.svg?branch=main" alt="Security status"></a>
  <a href="https://github.com/v4lproik/sol-validator-watcher/actions/workflows/coverage-pages.yml"><img src="https://github.com/v4lproik/sol-validator-watcher/actions/workflows/coverage-pages.yml/badge.svg?branch=main" alt="Pages deployment status"></a>
  <a href="https://codecov.io/gh/v4lproik/sol-validator-watcher"><img src="https://codecov.io/gh/v4lproik/sol-validator-watcher/graph/badge.svg?token=V3PM3PQJ49" alt="Code coverage"></a>
  <a href="#published-images"><img src="https://img.shields.io/badge/Docker-multi--arch-2496ED?logo=docker&amp;logoColor=white" alt="Docker images for AMD64 and ARM64"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/v4lproik/sol-validator-watcher" alt="MIT license"></a>
</p>

<p align="center">
  Prometheus monitoring and Grafana dashboards for any Solana validator by identity or vote-account public key.
</p>

Configure validators you operate, delegate to, compare, or research. Solana Validator Watcher polls ordered RPC, WebSocket, and provider APIs for health, consensus, leadership, and validator economics, exports Prometheus metrics, and preserves explicit per-command source provenance.

## Quick start

Requires Docker and the tools pinned by [`mise.toml`](mise.toml).

```bash
mise install
cp .env.example .env
just stack up surfpool
```

Open Prometheus at <http://127.0.0.1:9090> and Grafana at <http://127.0.0.1:3001>.

```bash
just stack down
```

Run `just` to list all supported commands.

## Configuration

Each watcher process loads one self-contained TOML profile. Start from the committed examples:

- [`config/local/validator.toml`](config/local/validator.toml) — direct local validator
- [`config/compose/surfpool.toml`](config/compose/surfpool.toml) — local Surfpool stack
- [`config/compose/validator.toml`](config/compose/validator.toml) — validator Compose stack
- [`config/remote/devnet.toml`](config/remote/devnet.toml) — hosted Devnet endpoints
- [`config/remote/mainnet.toml`](config/remote/mainnet.toml) — hosted Mainnet monitoring with Puffin economics and Trillium MEV capture enabled

Keep credentials in the ignored root `.env`. Command routes name their primary
connectors; type-scoped connector backups extend those routes in deterministic
order. The first successful connector wins.

| Declaration | Purpose |
| --- | --- |
| `solana_rpc_compatibility = "core"` | Standard health, epoch, vote-account, and leader-schedule RPC methods |
| `solana_rpc_compatibility = "extended"` | Additional performance, block-production, account, and reward RPC methods |
| `historical-data` | Retained prior-epoch rewards and block production |
| `block-fee-rewards` | Confirmed validator block-fee reward reads |
| `jito-mev-rewards` | Settled Jito validator rewards through its provider API |
| `trillium-mev-capture` | Completed-epoch MEV, block rewards, priority and signature fees, transactions, compute use, stake, Jito tips, leader slots, and block production through the public Trillium API |

A connector may declare ordered backups for only the source types they can
replace:

```toml
[[connectors]]
name = "helius-primary"
provider = "helius"
features = ["historical-data"]
solana_rpc_compatibility = "extended"
rpc = { url = "${SOL_MAINNET_RPC_URL}" }

[connectors.backups]
rpc-core = ["helius-backup"]
rpc-extended = ["helius-backup"]
rpc-historical-data = ["helius-backup"]

[[connectors]]
name = "helius-backup"
provider = "helius"
features = ["historical-data"]
solana_rpc_compatibility = "extended"
rpc = { url = "${SOL_MAINNET_BACKUP_RPC_URL}" }

[modes.consensus.commands]
epoch_info = ["helius-primary"]
vote_accounts = ["helius-primary"]
recent_performance_samples = ["helius-primary"]
```

Configuration loading expands the example routes to primary-then-backup order.
Backup keys are exact: `rpc-core`, `rpc-extended`, `rpc-historical-data`,
`rpc-block-fee-rewards`, `websocket`, `jito-mev-rewards`, and
`trillium-mev-capture`. Lists are not recursively expanded. Every named backup
must exist and satisfy the key's transport, RPC compatibility, and provider
feature contract.

At runtime, a failed command attempt immediately opens failover for that primary
connector and exact backup key. Subsequent commands sharing that pair skip the
primary for 60 seconds. After the cooldown, only the command that caused the
failover may make one real primary attempt: success restores the primary for the
whole key, while failure restarts the cooldown. Inserted backup entries remain
ungated, and other source types remain independent. Reloading a changed source
configuration installs a newly verified source chain with fresh failover state;
a rejected reload preserves the active chain and its state.

Run a profile directly with:

```bash
just rust run release config/local/validator.toml
```

## Local stacks

| Target | Command | Purpose |
| --- | --- | --- |
| Surfpool | `just stack up surfpool` | Local simulated validator and watcher |
| Validator | `just stack up validator` | Local validator endpoint |
| Devnet | `just stack up devnet` | Hosted Devnet RPC and WebSocket endpoints |
| Mainnet | `just stack up mainnet` | Hosted Mainnet monitoring |

Use `just stack ps <target>` and `just stack logs <target>` to inspect a stack. Validate Compose rendering with `just stack config <target>`.

## Production

The supported single-host Mainnet deployment uses explicit image versions
bound to immutable manifest digests, keeps every published port on loopback,
applies service resource and log limits, and includes Prometheus, Grafana, and Alertmanager.

```bash
cp .env.production.example /etc/sol-validator-watcher/production.env
cp .env.production.watcher.example /etc/sol-validator-watcher/watcher.env
just production check /etc/sol-validator-watcher/production.env
just production canary /etc/sol-validator-watcher/production.env \
  ghcr.io/v4lproik/sol-validator-watcher:vMAJOR.MINOR.PATCH@sha256:<digest>
just production rollout /etc/sol-validator-watcher/production.env \
  ghcr.io/v4lproik/sol-validator-watcher:vMAJOR.MINOR.PATCH@sha256:<digest>
```

Read the [production runbook](docs/production-runbook.md) before deployment. It
defines the secret-file contract, readiness and alert checks, canary rollout,
automatic rollback, backup/restore procedure, and release gate.

## Observability

| Surface | Purpose |
| --- | --- |
| `/metrics` | Prometheus metrics |
| `/healthz` | HTTP process health |
| `/readyz` | Collection readiness |
| <http://127.0.0.1:9090> | Local Prometheus |
| <http://127.0.0.1:3001> | Local Grafana dashboards |

Readiness requires five consecutive clean polling rounds. Recoverable collection failures retain the last successful snapshot while marking affected modes and readiness down.

Grafana includes overview, consensus, leadership, economics, and MEV Capture dashboards. The MEV dashboard compares Trillium's latest completed-epoch capture, allocations, block rewards, priority and signature fees, transactions, compute use, Jito tips, stake, leader slots, produced blocks, and efficiency while keeping independent published fields separate. The economics dashboard compares selected watched validators by inflation reward, reward-epoch blocks, reward per produced block, account balances, VAT reserve, and VAT runway, then visualizes network epoch progress, recent slot duration, time to epoch end, and each validator's next leader ETA. Missing provider or account samples render as no data instead of zero. Prometheus and Grafana are published on loopback by default. The watcher container runs unprivileged with a read-only root filesystem and dropped Linux capabilities.

## Published images

Pull requests build, smoke, and vulnerability-scan the exact `linux/amd64`
container after repository CI passes. Main-branch and version-tag workflows then
publish multi-architecture GHCR images for the same revision, smoke the pushed
digest, scan both published architectures, and attach provenance, an SBOM, and a
GitHub artifact attestation. Production references retain the release version
for review while binding it to the immutable image digest.

```bash
gh attestation verify \
  oci://ghcr.io/v4lproik/sol-validator-watcher:vMAJOR.MINOR.PATCH@sha256:<digest> \
  --repo v4lproik/sol-validator-watcher \
  --signer-workflow v4lproik/sol-validator-watcher/.github/workflows/container.yml
```

## Modules

| Module | Owns | Start here | Changelog |
| --- | --- | --- | --- |
| Watcher | Commands, collection, routing, provenance, and aggregation | [`src/mode.rs`](src/mode.rs), [`src/source.rs`](src/source.rs), [`src/collector/`](src/collector/) | [`changelog/watcher.md`](changelog/watcher.md) |
| Runtime | Startup, polling, HTTP, reload, state, and shutdown | [`src/main.rs`](src/main.rs), [`src/runtime/`](src/runtime/) | [`changelog/runtime.md`](changelog/runtime.md) |
| Configuration | TOML loading, validation, and profiles | [`src/config/`](src/config/), [`config/`](config/) | [`changelog/configuration.md`](changelog/configuration.md) |
| Observability | Metrics, Prometheus, Grafana, and Pages | [`src/metrics/`](src/metrics/), [`docker/prometheus/`](docker/prometheus/), [`docker/grafana/`](docker/grafana/) | [`changelog/observability.md`](changelog/observability.md) |
| Operations | Containers, Compose, and local stacks | [`docker/`](docker/) | [`changelog/operations.md`](changelog/operations.md) |
| Tooling | Just, CI, coverage, and publishing | [`Justfile`](Justfile), [`just/`](just/), [`.github/workflows/`](.github/workflows/) | [`changelog/tooling.md`](changelog/tooling.md) |

## Development

Read [`CONTRIBUTING.md`](CONTRIBUTING.md) and the [module changelog index](CHANGELOG.md).

```bash
just rust doctor
just ci verify
just ci full
```

Every change updates the `Unreleased` section of each affected module changelog.

## License

MIT. See [`LICENSE`](LICENSE).
