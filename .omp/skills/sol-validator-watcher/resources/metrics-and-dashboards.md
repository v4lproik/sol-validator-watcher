# Metrics and Dashboards

## Scope

Use this resource for Prometheus metric descriptors, labels, units, snapshot
updates, source provenance, Grafana dashboards and provisioning, alerts, and
operator-facing semantics.

## Ownership and Contracts

- `metrics/registration.rs` owns metric construction and registration;
  `metrics/mod.rs` owns labels, updates, resets, rendering, and unit conversion.
- `collector::Snapshot` is the input contract. Collection code does not write
  Prometheus collectors directly.
- Dashboard JSON under `docker/grafana/dashboards/` consumes the exported metric
  contract. Provisioning under `docker/grafana/provisioning/` owns local loading.
- `docker/prometheus/prometheus.yml` owns local scraping.

## Metric Invariants

- Metric names, label names, and units are public operational APIs. A rename or
  label-set change requires every query, panel, test, and documentation consumer
  in the same change.
- Keep label cardinality bounded by configured networks, scopes, validators,
  connectors, statuses, and metric families. Never label with slot, epoch,
  error text, URL, request ID, or other unbounded values.
- Convert lamports to SOL and fractions to percentages in one owning function.
  Names and help text must match the exported unit.
- Reset snapshot-owned vectors before applying a new snapshot so removed
  validators and scopes do not survive as stale series. Reset command-source
  selections at the start of each command cycle so health and collection
  sources can coexist for that cycle.
- Connector inventory, declared provider features, command routes, and
  selected-command-source metrics answer different questions. Route and source
  metrics expose the connector declaration as `solana_rpc_compatibility`, the
  command requirement as `required_solana_rpc_compatibility`, plus independent
  provider, transport, and optional required-feature labels. Do not infer
  successful execution from a configured route or connector.
- Watcher readiness, completed rounds, the bounded clean-round streak, failed
  RPC attempts, HTTP response classes, and exact 200/403/429/500 counts are
  separate metrics. Readiness is zero until five failure-free completed rounds
  and must not be inferred from process liveness or a configured route.
- Reset connector-attributed WebSocket probe failure counters when a new
  configuration is accepted, initialize configured connectors at zero, and
  increment every failed fallback attempt rather than only route exhaustion.
- A zero is valid only when the domain value is truly zero. Missing, unsupported,
  stale, or failed data must not become a healthy-looking zero without an
  explicit availability/source signal.
- Node-local identity, version, feature set, vote-set classification, health,
  catch-up distance, and block height are snapshot-owned dynamic series. Reset
  them together. A healthy node has catch-up distance zero; an unhealthy node
  without structured distance data has no distance series, not zero.
- Jito MEV reward, commission, and reward-epoch series are snapshot-owned and
  exist only when economics mode, a valid provider-API route, and a settled
  target reward all exist. Missing settlement or route failure remains no data,
  never a zero reward.
- Help text states operator meaning, not implementation details.

## Dashboard Invariants

- A panel query must use labels and units exactly as exported. Verify template
  variables against every query that consumes them.
- Prefer operationally actionable ordering and breakdowns. Network, watched,
  custom scope, and source provenance must remain distinguishable.
- Dashboard changes must preserve datasource/provisioning compatibility with the
  repository Compose stack.
- No-data, zero, stale, and dependency-down states are different. Panels and
  alerts must not collapse them into one reassuring display.
- Alert rules under `docker/prometheus/rules/` gate snapshot-dependent
  conditions on data newer than the greater of 120 seconds or four configured
  poll intervals. Target-down and snapshot-stale alerts remain independent so
  missing data cannot look healthy. Do not alert on root lag alone; Solana roots
  routinely trail the current slot.
- Node-behind and node-not-voting alerts are snapshot-dependent and use the
  same freshness gate. In the overview's node-local row, `NOT CONFIGURED`
  means no `node_status` route is declared; `UNAVAILABLE` means a declared
  route supplied no node-health series. When node health exists without a
  structured catch-up distance, that panel remains `No data`. Healthy, behind,
  current-voting, and non-current-voting states remain distinct, and every
  multi-network result identifies its network.
- The repository provisions alert evaluation only. Notification routing remains
  an external Alertmanager responsibility.
- Keep overview, consensus, leadership, and economics dashboards separate.
  Mode dashboards consume only their mode's command health, routes, sources,
  and domain metrics.
- Keep generated/public Pages output out of source edits; the site build owns it.

## Review Gates

1. What operator question does the metric or panel answer?
2. Are name, type, labels, help, and unit mutually consistent?
3. Is every label finite and operationally useful?
4. What happens when a validator, scope, or connector disappears next poll?
5. Can missing or failed data look like a real zero?
6. Which dashboard queries, docs, and render assertions consume this contract?
7. Does source provenance identify the connector that actually succeeded?
8. Does the local Prometheus/Grafana stack load the changed artifact?

## Prohibited Shortcuts

- Do not add high-cardinality labels for convenience.
- Do not update one dashboard copy while leaving provisioning or the source JSON
  stale.
- Do not test metric registration with existence-only assertions when rendered
  labels and values are the contract.
- Do not retain stale dynamic series or use a default value to hide collection
  failure.
- Do not manually edit generated `public/` coverage/site output.

## Boundary Proof

- Unit conversion and label helpers: focused unit tests.
- Snapshot update/reset/provenance: render the Prometheus text and assert stable
  series, labels, values, and stale-series removal.
- Prometheus configuration and alerts: run `just stack check` to parse the
  loaded configuration and execute the permanent rule tests.
- Dashboard/provisioning changes: parse the JSON, run the affected
  `just stack config <target>` command, and inspect the real panel against local
  data.
- Pages changes: run `just site build` and inspect the generated surface without
  committing `public/`.
