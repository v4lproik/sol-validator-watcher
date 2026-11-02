# Metrics and Dashboards

## Scope

Use this resource for Prometheus metric descriptors, labels, units, snapshot
updates, source provenance, Grafana dashboards and provisioning, alerts, and
operator-facing semantics.

## Ownership and Contracts

- `metrics.rs` is the single owner of metric registration, labels, updates,
  resets, rendering, and unit conversion.
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
- Connector inventory, command routes, and selected-command-source metrics
  answer different questions. Do not infer successful execution from a
  configured route or connector.
- A zero is valid only when the domain value is truly zero. Missing, unsupported,
  stale, or failed data must not become a healthy-looking zero without an
  explicit availability/source signal.
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
- Dashboard/provisioning changes: parse the JSON, run the affected
  `just stack config <target>` command, and inspect the real panel against local
  data.
- Pages changes: run `just site build` and inspect the generated surface without
  committing `public/`.
