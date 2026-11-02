# Runtime, Configuration, and Shutdown

## Scope

Use this resource for CLI startup, configuration files and environment
expansion, runtime reload, task supervision, HTTP endpoints, websocket probes,
liveness files, signals, and shutdown.

## Ownership and Boundaries

- `config.rs` parses each self-contained TOML profile, expands environment
  placeholders, normalizes values, and validates connectors plus every built-in
  mode's command routes. No other module reads raw environment-backed policy.
- `source.rs` constructs RPC and WebSocket clients once and compiles command
  routes into immutable connector index chains.
- `main.rs` owns transactional reload, the polling loop, dependency guards, and
  the `/metrics` and `/healthz` surfaces.
- `tasks.rs` owns task registration, first-failure handling, shutdown broadcast,
  and join errors.
- `websocket.rs` owns websocket connect/probe behavior.

## Configuration Invariants

- Expand `${VAR}` and `${VAR:-default}` before deserialization. Missing required
  variables fail with the config field/path context; never log resolved secrets.
- Keep connectors inline in the profile. Each connector has one stable
  `provider` name plus an explicit `features` array, which may be empty. The
  current provider feature is `historical-data`; `rpc` and `websocket` are
  transports, not features.
- Each `ModeId` owns its finite source requirements. Health permits RPC and
  WebSocket; consensus and leadership require RPC; economics requires RPC plus
  `historical-data`. A command selects one requirement from its mode, and its
  configured route supplies the ordered providers.
- Each `modes.<mode>.commands.<command>` array is that command's exact priority
  and fallback order; connector declaration order is not execution policy.
- Treat each connector's `rpc_url` and `ws_url` as its transport configuration.
  Do not duplicate those transports in `features` or add transport toggles.
- Normalize names, URLs, commitments, labels, and public keys once, then validate
  the normalized value.
- Reject unknown fields, missing providers or feature arrays, duplicate
  features, zero ports/timeouts/intervals, duplicate connector names,
  unsupported commitments, invalid websocket schemes, missing required routes,
  unknown or duplicate route entries, RPC routes without RPC transport, routes
  missing an optional provider feature required by the mode, and impossible
  economics mode/target combinations before client construction.
- Optional WebSocket routes may contain connectors whose environment-expanded
  `ws_url` is empty; expose them as unavailable and skip them at execution.
- Removed split-config and single-connection fields stay unsupported. Do not
  add aliases or a second migration path.
- Config reload is transactional: load and validate the candidate, build every
  changed runtime dependency, then replace the last known-good state. A failed
  reload keeps the prior state and remains observable.
- A changed metrics port requires restart because the listener is already
  bound. Do not pretend a reload moved it.

## Runtime and Shutdown Invariants

- Every long-running task is spawned through `TaskManager`. A task that exits
  unexpectedly, returns an error, or panics initiates shutdown and reaches the
  process result.
- Keep HTTP serving, watcher polling, and signal handling independently owned,
  but coordinated by the same shutdown channel.
- Do not spawn detached work whose error or lifetime outlives `TaskManager`.
- Shutdown waits for supervised tasks to finish. Cancellation must interrupt
  polling sleeps without waiting for the full interval.
- Dependency guards run before collection. The `health.rpc_health` route is
  fail-closed; `health.ws_health` is probed only through its explicitly named
  connectors and remains optional when no named connector has `ws_url`.
- Update the liveness file only after a complete snapshot has updated metrics.
  A process that is looping but failing collection is not live by this contract.
- `/healthz` currently proves the HTTP process is serving. Do not silently turn
  it into dependency readiness without updating callers, docs, tests, and
  operational expectations.

## Reload Review Gates

1. Which config fields changed, and must the compiled `SourceChain` rebuild?
2. Can any fallible step mutate live state before the candidate and its complete
   source chain are valid?
3. Which fields cannot reload and require a restart warning?
4. Does a failed candidate preserve every prior runtime dependency and flag?
5. Are economics capability checks invalidated when their inputs change?

## Task Review Gates

1. Who owns this future from spawn through shutdown?
2. How do success, unexpected completion, error, panic, cancellation, and join
   failure reach `TaskManager`?
3. Can shutdown interrupt every wait and I/O boundary?
4. Does cleanup preserve the first causal error?
5. Does the liveness/health surface still mean what operators think it means?

## Prohibited Shortcuts

- Do not read `std::env` in feature modules for configuration already owned by
  `Config`.
- Do not partially apply a reload or replace the old state before client
  construction succeeds.
- Do not swallow task errors, detach a `tokio::spawn`, or treat an unexpected
  task exit as successful shutdown.
- Do not update liveness before collection or use `/healthz` as proof that RPC
  data is fresh.
- Do not add wall-clock sleeps to coordinate tests; use task signals, local
  listeners, file state, or bounded polling.

## Boundary Proof

Use config unit tests for strict parsing/validation, every committed profile,
canonical provider features, transport/feature separation, and exact
command-route order. Use task tests for completion/error/panic/shutdown
outcomes. Smoke the binary with controlled RPC endpoints, fetch `/healthz` and
`/metrics`, trigger shutdown, and observe a clean exit when runtime wiring
changes.
