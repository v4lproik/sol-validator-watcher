# Error Handling

## Scope

Use this resource for every error conversion, context layer, fallback, ignored
result, default-on-error path, retry decision, cleanup failure, and background
task failure.

## Error Ownership

- The adapter that understands an external protocol classifies its failures.
  Callers decide whether that classified outcome is required, optional, or a
  candidate for connector fallback.
- `anyhow` carries causal chains across application boundaries. Add context that
  identifies the failed operation and role; do not replace the source error
  with a generic string.
- Task supervision owns background errors and process shutdown. Collection code
  owns method/connector failures. Config owns field/path validation.
- Logs supplement returned errors; logging an error does not make swallowing it
  correct.

## Invariants

- Distinguish transport, HTTP, invalid payload, JSON-RPC error, missing result,
  unsupported capability, valid absence, invalid config, timeout, cancellation,
  and shutdown.
- Required operations exhaust their documented fallback chain and then fail
  with every attempted connector's identity and causal error preserved in
  fallback order. Optional operations may return absence only when callers and
  metrics preserve that meaning.
- A default value is never a generic error handler. Use it only when the domain
  explicitly defines that value for absence and exposes availability elsewhere.
- Preserve the first causal process/task error while still attempting orderly
  shutdown. Later cleanup errors remain visible without replacing the cause.
- `Result::ok`, `unwrap_or_default`, ignored `let _ =`, and catch-all branches
  require review: state which error class is intentionally discarded and why
  the resulting state is safe.
- Cancellation is not provider failure. Shutdown must not trigger fallback,
  retries, or false dependency-down alerts.
- Do not retry implicitly. Retry count, deadline, backoff, idempotency, and
  observability must be owned together before a retry loop exists.
- Never include credentials, secret query parameters, private keys, or full
  environment values in context or logs.

## Connector Fallback

- Log failed attempts with bounded connector identity, method/metric, and the
  causal error.
- Preserve deterministic candidate order. When a required route is exhausted,
  return every attempted connector's bounded identity and causal error; do not
  discard earlier failures after logging them.
- A connector that returns a valid empty result succeeded; a connector that
  failed must not receive source attribution.
- An unsupported optional method and a transient request failure can both
  advance to another connector, but they are not the same diagnostic state.
- A required command's prerequisite RPC calls remain required. Do not convert
  their transport, HTTP, payload, or JSON-RPC failures into absence.

## Review Gates

1. What exact error classes can enter this branch?
2. Which module has enough information to classify them?
3. Is the operation required, optional, fallback-eligible, retryable, or
   cancelled, and are those decisions separate?
4. Does the returned error preserve its source and add actionable boundary
   context?
5. If an error is suppressed, how is absence/failure observable to callers and
   operators?
6. Can a default value or zero look like successful fresh data?
7. Does cleanup preserve the first causal failure?
8. Could logs expose a credential or unbounded provider text?
9. What test forces this failure and proves the caller-visible outcome?

## Prohibited Shortcuts

- Do not match broad human-readable substrings when a structured status or
  JSON-RPC code exists.
- Do not use `unwrap`, `expect`, panic, an empty collection, zero, or `false` to
  suppress a reachable runtime failure.
- Do not log-and-continue without an explicit optional/fallback contract.
- Do not detach cleanup, ignore join errors, or convert cancellation into an
  ordinary failure.
- Do not add retry/backoff around a non-idempotent or unbounded operation.
- A required post-startup collection failure may continue only at the
  idempotent whole-poll boundary: reset readiness, expose the failed round and
  affected modes, preserve the last successful snapshot, and wait with bounded
  backoff. Startup/configuration contract failures remain fatal.
- Suppress a watcher-state publication failure only when process-wide shutdown
  was already requested and the sibling actor won the shutdown race. Without
  that cancellation state, actor loss remains a causal task failure.

## Boundary Proof

Exercise each classification at the narrow real boundary: controlled HTTP
status, malformed JSON, JSON-RPC error, null/missing result, timeout, connector
fallback, invalid config, task panic/error, and shutdown cancellation. Assert the
observable return, selected source, metric, file state, or process exit rather
than the internal helper call.
