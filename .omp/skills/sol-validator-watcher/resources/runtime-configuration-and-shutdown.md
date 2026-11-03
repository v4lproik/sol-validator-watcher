# Runtime, Configuration, and Shutdown

## Scope

Use this resource for CLI startup, configuration files and environment
expansion, runtime reload, task supervision, HTTP endpoints, websocket probes,
liveness files, signals, and shutdown.

## Ownership and Boundaries

- `config/mod.rs` parses each self-contained TOML profile, expands environment
  placeholders, normalizes values, and validates connectors plus every built-in
  mode's command routes. No other module reads raw environment-backed policy.
- `source.rs` constructs RPC and WebSocket clients once, compiles command routes
  into immutable connector index chains, and verifies declared RPC capabilities.
- `runtime/state.rs` owns transactional reload; `runtime/poller.rs` owns startup
  capability verification, polling, dependency guards, and liveness updates;
  `runtime/http.rs` owns `/metrics`, `/healthz`, and `/readyz`.
- `watcher_state/actor.rs` is the single writer for readiness, completed rounds,
  clean streaks, and bounded RPC response telemetry.
- `runtime/app.rs` wires all supervised process tasks.
- `tasks.rs` owns task registration, first-failure handling, shutdown broadcast,
  and join errors.
- `websocket.rs` owns websocket connect/probe behavior.

## Configuration Invariants

- Expand `${VAR}` and `${VAR:-default}` before deserialization. Missing required
  variables fail with the config field/path context; never log resolved secrets.
- Default `metrics_host` to `127.0.0.1`; non-loopback HTTP binding must be
  explicit in a validated profile.
- Keep connectors inline in the profile. Each connector has one stable
  `provider` name plus an explicit `features` array, which may be empty.
  Provider names are provenance, not capabilities.
- A connector with `rpc_url` must declare
  `solana_rpc_compatibility = "core"` or
  `solana_rpc_compatibility = "extended"`. A connector without `rpc_url` must
  not declare standard Solana RPC compatibility. `extended` satisfies `core`;
  provider features currently require `extended`.
- The compatibility profile describes only standard Solana JSON-RPC methods
  consumed by this watcher. It is not the provider's complete API catalog.
  Provider-specific APIs and transports require their own typed command,
  capability, endpoint, and verification when the watcher starts consuming
  them.
- The current provider feature is `historical-data`; `rpc` and `websocket` are
  transports, not features. Historical data is independent of compatibility.
- Core commands are RPC health, epoch info, vote accounts, and current/next
  leader schedules. Recent performance samples, current block production,
  account balances, and rent exemption require extended compatibility.
  Prior-epoch rewards additionally require `historical-data`.
- Each `modes.<mode>.commands.<command>` array is that command's exact priority
  and fallback order; connector declaration order is not execution policy.
  Optional extended routes may be empty in a core-only profile.
- Treat each connector's `rpc_url` and `ws_url` as its transport configuration.
  Do not duplicate those transports in `features` or add transport toggles.
- Normalize names, URLs, commitments, labels, and public keys once, then validate
  the normalized value.
- Reject unknown fields, missing providers or feature arrays, missing or
  misplaced Solana RPC compatibility declarations, duplicate features, zero
  ports/timeouts/intervals, duplicate connector names, unsupported commitments,
  invalid websocket schemes, missing required routes, unknown or duplicate route
  entries, and routes whose connector lacks the required transport, Solana RPC
  compatibility, or provider feature before client construction.
- Optional WebSocket routes may contain connectors whose environment-expanded
  `ws_url` is empty; expose them as unavailable and skip them at execution.
- Removed split-config and single-connection fields stay unsupported. Do not
  add aliases or a second migration path.
- Config reload is transactional: load and validate the candidate, build every
  changed runtime dependency, verify declared RPC capabilities, then replace the
  last known-good state. A failed reload keeps the prior state and remains
  observable.
- A changed metrics host or port requires restart because the listener is
  already bound. Do not pretend a reload moved it.

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
- Verify every declared standard Solana RPC compatibility profile and provider
  feature before dependency health or collection. Startup verification failure
  is fatal; reload verification failure preserves the last-known-good runtime.
- After startup verification, treat required RPC and collection failures as
  failed whole polling rounds. Mark affected modes down, reset readiness through
  the watcher-state actor, retain the last successful snapshot, and retry after
  exponential 1, 2, 4, 8, 16, then 30-second delays. A successful round resets
  the backoff. Do not retry individual methods inside the collector.
- Update the liveness file only after a complete snapshot has updated metrics.
  A watcher that is retrying failed collection remains process-healthy through
  `/healthz`, but does not advance this data-liveness timestamp.
- `/healthz` proves only that the HTTP process is serving. `/readyz` admits the
  current configuration only after five consecutive required polling rounds
  complete without a failed RPC attempt. A failed attempt, failed whole poll, or
  accepted configuration reload resets the admission streak.
- The `healthcheck` CLI command probes `/healthz`, not `/readyz`, so container
  health tracks the serving process without restarting on recoverable
  collection failures.
- The watcher-state actor is the sole writer for readiness, completed rounds,
  the clean-round streak, their Prometheus samples, and the 30-second state
  report. Successful and failed poll completions plus accepted-reload events are
  acknowledged only after the actor publishes a coherent snapshot for HTTP
  readers.
- A watcher-state transition failure remains fatal unless process-wide shutdown
  was already requested. During cooperative shutdown, the actor may stop before
  the poller publishes its final transition; treat only that ordering race as
  normal task completion.
- Report bounded failed-attempt totals and RPC response-class counts plus exact
  200, 403, 429, and 500 counts every 30 seconds without logging response bodies
  or endpoint URLs.

## Reload Review Gates

1. Which config fields changed, and must the compiled `SourceChain` rebuild?
2. Can any fallible step mutate live state before the candidate, its complete
   source chain, and its RPC capability probes succeed?
3. Which fields cannot reload and require a restart warning?
4. Does a failed candidate preserve every prior runtime dependency and flag?
5. Are connector and economics capability checks invalidated when their inputs
   change?

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
canonical Solana RPC compatibility/provider-feature declarations, transport
separation, and exact command-route order. Exercise capability probes and reload
rejection at a controlled HTTP/JSON-RPC boundary with valid method-specific
success responses.
Use task tests for completion/error/panic/shutdown outcomes. Smoke the binary
through five controlled polling rounds, verify `/healthz`, `/readyz`, and
`/metrics`, force a post-startup RPC failure, and prove that health remains up,
readiness resets, the successful-snapshot timestamp remains unchanged, bounded
backoff delays another round, and five new clean rounds restore readiness.
Trigger shutdown during a wait and observe a clean exit when runtime wiring
changes.
When the HTTP bind contract or container health check changes, also confirm the
actual bound address, execute the healthcheck command against the running
binary, and retain the failure/recovery smoke above.
