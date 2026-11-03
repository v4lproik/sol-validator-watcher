# Operator Observability Roadmap

The dashboards are organized by execution mode, but several panels can still provide stronger operator decision support.

## Improve now with current metrics

These changes do not require new exporter metrics.

### Overview dashboard

- Keep the top row focused on operator state.
  - watcher health
  - RPC health
  - websocket health
  - metrics freshness
- Show every watched validator directly in the operator-state row.
  - map each identity to an explicit `ACTIVE` or `DELINQUENT` voting state
- Place the detailed watched-validator table immediately below operator state as the first detailed investigation panel.
- Add explicit outlier panels.
  - highest vote lag
  - highest root lag
  - most missed slots
  - lowest vote efficiency
- Prefer short trend panels over large gauges for watched-vs-network comparisons.

### Mode dashboards

- Consensus: keep delinquency, vote efficiency, finalization lag, stake, and
  commission panels together; prefer ranked tables when the goal is action.
- Leadership: keep future slots, produced/missed blocks, and skip rate together;
  surface the worst validator or scope first.
- Economics: keep rewards, rewarded blocks, and reward per produced block
  together; distinguish a disabled mode from a failed or stale command.
- Every mode dashboard must expose its enabled state, configured command routes,
  latest successful command sources, and source health.

### Usage guidance

- Use a concrete `Network` selection for day-to-day operations.
- Use the overview dashboard's `Validators` multi-select to inspect one or more
  watched identities; `All` preserves the fleet-wide watched-validator view.
- Treat `scope:all-network` as the default health view.
- Treat watched validators and custom scopes as investigation views.
- `ACTIVE` means Solana currently lists the vote account in its current set.
  `DELINQUENT` means the vote account is not keeping up with cluster voting.
  These states do not probe a validator's private RPC process; use vote and root
  lag for additional liveness context.

## Metrics the exporter should add

These are the operator signals that would most improve the dashboards.

### Freshness and reliability

- `sol_snapshot_timestamp_seconds`
- `sol_last_successful_snapshot_timestamp_seconds`
- `sol_snapshot_duration_seconds`
- `sol_rpc_request_duration_seconds`
- `sol_rpc_request_failures_total{method}`
- `sol_ws_probe_failures_total`

### Node-local operator health

- node block height
- catch-up distance
- active identity
- voting enabled / disabled
- validator client version
- node health summary

### Epoch and schedule context

- epoch progress percent
- slots remaining in epoch
- average slot time
- estimated time to next epoch
- time to next leader slot per watched validator

### Rewards and balances

- vote account balance
- identity balance
- reserve wallet balance
- epoch rewards
- cumulative rewards

### Validator economics and revenue

- Jito tip revenue
- Harmonic revenue
- base revenue per block
- total revenue per produced block
- revenue per leader slot
- revenue normalized by activated stake
- revenue split by base fees, tips, and sidecar/auction sources when available

### Stake and topology

- stake activated / deactivated this epoch
- stake share trend
- optional datacenter / ISP / region enrichment

## Recommended order

1. Improve the watched-validator table and outlier panels with current metrics.
2. Add freshness and scrape reliability metrics.
3. Add node-local operator health metrics.
4. Add epoch progress and timing metrics.
5. Add rewards, balances, validator economics, and optional topology enrichment.
