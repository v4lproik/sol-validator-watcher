# Operator Observability Roadmap

The current dashboards are useful, but they still lean toward monitoring overview instead of operator decision support.

## Improve now with current metrics

These changes do not require new exporter metrics.

### Overview dashboard

- Keep the top row focused on operator state.
  - watcher health
  - RPC health
  - websocket health
  - metrics freshness
- Make the watched-validator table the main drill-down panel.
  - desired columns: `status`, `stake`, `vote efficiency`, `last vote lag`, `root lag`, `produced`, `missed`, `skip rate`, `commission`, `future leader slots`
- Add explicit outlier panels.
  - highest vote lag
  - highest root lag
  - most missed slots
  - lowest vote efficiency
- Prefer short trend panels over large gauges for watched-vs-network comparisons.

### Breakdown dashboard

- Prefer ranked bars and tables over pie charts when the goal is action.
- Keep worst scopes first.
  - highest skip rate
  - highest delinquent stake ratio
  - lowest vote efficiency
  - most future leader slots
- Add stake share and validator share tables per scope.

### Usage guidance

- Use a concrete `Network` selection for day-to-day operations.
- Treat `scope:all-network` as the default health view.
- Treat watched validators and custom scopes as investigation views.

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
