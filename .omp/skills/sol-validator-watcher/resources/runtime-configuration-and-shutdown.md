# Runtime, Configuration, and Shutdown

## Scope

Use this resource for CLI startup, configuration files and environment
expansion, runtime reload, task supervision, HTTP endpoints, websocket probes,
liveness files, signals, and shutdown.

## Ownership and Boundaries

- `config.rs` parses, expands, normalizes, validates, and resolves connector
  files. No other module reads raw environment-backed config policy.
- `main.rs` builds runtime clients from validated config, owns reload, runs the
  polling loop, and exposes `/metrics` and `/healthz`.
- `tasks.rs` owns task registration, first-failure handling, shutdown broadcast,
  and join errors.
- `websocket.rs` owns websocket connect/probe behavior.

## Configuration Invariants

- Expand `${VAR}` and `${VAR:-default}` before deserialization. Missing required
  variables fail with the config field/path context; never log resolved secrets.
- Resolve companion connector TOML paths relative to the watcher config, not the
  process working directory.
- Normalize names, URLs, commitments, labels, and public keys once, then validate
  the normalized value.
- Reject zero ports/timeouts/intervals, duplicate connector names, unsupported
  commitments, invalid websocket schemes, and impossible economics mode/target
  combinations before client construction.
- Legacy single-connection fields remain one compatibility owner. Do not add a
  second migration path or allow legacy and connector-stack semantics to drift.
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
- Dependency guards run before collection. RPC health failure is fatal; websocket
  behavior follows the configured transport requirement.
- Update the liveness file only after a complete snapshot has updated metrics.
  A process that is looping but failing collection is not live by this contract.
- `/healthz` currently proves the HTTP process is serving. Do not silently turn
  it into dependency readiness without updating callers, docs, tests, and
  operational expectations.

## Reload Review Gates

1. Which config fields changed, and which runtime clients/orderings must rebuild?
2. Can any fallible step mutate live state before the candidate is fully valid?
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

Use config unit tests for parsing/validation and relative paths. Use task tests
for completion/error/panic/shutdown outcomes. Smoke the binary with a controlled
RPC endpoint, fetch `/healthz` and `/metrics`, trigger shutdown, and observe a
clean exit when runtime wiring changes.
