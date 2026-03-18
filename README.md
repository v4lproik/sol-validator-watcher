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
cargo run --release -- --config etc/config.local.yaml
```

Metrics are exposed on:

```text
http://127.0.0.1:8000/metrics
http://127.0.0.1:8000/healthz
```

## Tests

Fast tests:

```bash
cargo test
```

Surfpool-backed smoke test:

```bash
SURFPOOL_E2E=1 cargo test
```

Or with the helper script:

```bash
bash scripts/test-with-surfpool.sh
```

The Surfpool test launches a real local Surfpool process on ephemeral ports, waits for RPC readiness, runs the watcher snapshot collection path against it, and renders metrics. It is opt-in so the default test suite stays fast and does not depend on local validator startup.

## Notes

- Config reload is automatic when the config file changes.
- If `rpc_url`, `rpc_timeout_sec`, or `commitment` changes, the RPC client is recreated.
- If `metrics_port` changes, restart the process. The listener address is fixed at startup.
- `sol_vote_efficiency_pct` is a stake-weighted comparison against the best validator in the sampled vote-credit epoch. It is intended as a Solana equivalent of the Ethereum watcher’s performance view, not a protocol-native official score.
