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
- Mode status cards use four states: `DISABLED` for a disabled mode, `PENDING`
  until an enabled mode completes once, `DOWN` when its latest execution
  failed, and `UP` when its latest execution succeeded.

### Usage guidance

- Use a concrete `Network` selection for day-to-day operations.
- Treat `scope:all-network` as the default health view.
- Treat watched validators and custom scopes as investigation views.
- `ACTIVE` means Solana currently lists the vote account in its current set.
  `DELINQUENT` means the vote account is not keeping up with cluster voting.
  These states do not probe a validator's private RPC process; use vote and root
  lag for additional liveness context.
- Vote-credit sample, total, and efficiency series are absent when no
  current-epoch sample is available; missing credits are not reported as zero.

## Current operator timing and funding signals

The exporter and economics dashboard now publish:

- `sol_epoch_progress_pct`, `sol_epoch_slots_remaining`,
  `sol_average_slot_time_seconds`, and
  `sol_estimated_epoch_end_unixtime`
- `sol_watched_validator_next_leader_slot`,
  `sol_watched_validator_slots_until_next_leader`, and
  `sol_watched_validator_seconds_until_next_leader`
- economics-target identity and vote-account balances, vote-account rent
  exemption, VAT reserve, complete funded VAT epochs, and next-epoch VAT
  funding status

Timing estimates use the latest `getRecentPerformanceSamples` result. Missing
performance samples or leader schedules produce no estimate series rather than
a healthy-looking zero. VAT runway is the current vote-account balance above
rent exemption divided by the current 0.8 SOL epoch charge; it excludes future
rewards and transfers. If an RPC omits the vote account's data size, account
balances remain available while rent and VAT series are omitted.

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

### Remaining rewards and balances

- reserve wallet balance
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

1. Improve freshness and scrape reliability metrics.
2. Add node-local operator health metrics.
3. Add finalized revenue components and configurable operating costs.
4. Add stake and topology history.
