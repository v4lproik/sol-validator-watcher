# Operator Observability Roadmap

The dashboards are organized by execution mode, but several panels can still provide stronger operator decision support.

## Improve now with current metrics

These changes do not require new exporter metrics.

### Overview dashboard

- Keep the top row focused on operator state.
  - process health
  - five-round readiness admission and clean-poll streak
  - RPC and websocket health
  - failed RPC attempts, HTTP response classes, and readiness-critical status codes
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
- Every mode dashboard must expose its enabled state, configured command routes
  with declared and required standard Solana RPC compatibility, latest
  successful command sources, and source health.
- Mode status cards use four states: `DISABLED` for a disabled mode, `PENDING`
  until an enabled mode completes once, `DOWN` when its latest execution
  failed, and `UP` when its latest execution succeeded.

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
- Vote-credit sample, total, and efficiency series are absent when no
  current-epoch sample is available; missing credits are not reported as zero.
- `/healthz` is process liveness. `/readyz` and `sol_watcher_ready` require five
  consecutive required polling rounds to complete without a failed RPC attempt
  or non-2xx response.
- Use `sol_rpc_request_failure_count` for the aggregate process total,
  `sol_rpc_request_failures_total` for connector/method failures,
  `sol_rpc_http_response_class_count` for received 2xx/3xx/4xx/5xx responses,
  and `sol_rpc_http_response_status_count` for exact 200, 403, 429, and 500
  counts. The structured `watcher state` log reports 30-second deltas.

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

Node-local operator health is exposed through `sol_node_info`,
`sol_node_health`, `sol_node_catchup_distance_slots`, and
`sol_node_block_height`. A configured `node_status` command route resolves one
operator-owned RPC connector, preserves structured `getHealth` catch-up
distance, and joins its active identity to the current and delinquent vote
account sets. Missing route data remains absent instead of rendering as zero.

Snapshot and transport reliability are exposed through
`sol_last_successful_snapshot_timestamp_seconds`,
`sol_snapshot_duration_seconds`, `sol_snapshot_failures_total`,
`sol_rpc_request_duration_seconds`, `sol_rpc_request_failures_total`,
`sol_ws_probe_failures_total`, and `sol_watcher_poll_interval_seconds`. The
overview dashboard derives snapshot age from the last successful timestamp and
shows cumulative HTTP, RPC, and connector-attributed WebSocket failures.
Prometheus evaluates target, readiness, stale-snapshot, RPC- and
WebSocket-failure, validator-health, upcoming-leader, and VAT-funding alerts.
Snapshot-dependent conditions require data newer than the greater of 120
seconds or four configured poll intervals. Alertmanager notification routing
remains an external deployment responsibility.

## Metrics the exporter should add

These are the operator signals that would most improve the dashboards.

### Freshness and reliability

- `sol_snapshot_timestamp_seconds` when an upstream observation timestamp is
  available

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

1. Add finalized revenue components and configurable operating costs.
2. Add stake and topology history.
