# RPC Collection and Provider Behavior

## Scope

Use this resource for Solana JSON-RPC calls, connector ordering, fallback,
provider capability checks, request batching, source provenance, health probes,
and economics collection.

## Ownership and Boundaries

- `collector.rs` owns JSON-RPC request/response types, HTTP status handling,
  method calls, fallback selection, snapshot source records, batching, and
  aggregation inputs.
- `main.rs` owns runtime connector construction and dependency health checks.
- `config.rs` owns connector kind, URL, timeout, commitment, and ordering
  validation.
- `websocket.rs` owns websocket reachability only. Do not infer RPC method
  support from websocket success.
- `metrics.rs` publishes which connector supplied each collected metric family.

## Invariants

### Decode the protocol before classifying it

- Treat transport failure, HTTP status, invalid JSON, JSON-RPC error, missing
  result, and a valid empty result as distinct outcomes.
- Parse a valid JSON-RPC error envelope even when the HTTP status is non-success.
  Structured error code and data outrank human-readable text.
- A missing required `result` is an error. A JSON `null` is optional only for a
  method whose Solana contract permits it.
- Include the RPC method in boundary context without logging credentials or a
  secret-bearing URL.

### Keep connector selection deterministic

- Connector declaration order is the current precedence contract. The first
  connector that can successfully serve a metric wins; a failure may advance
  to the next configured connector.
- Required and optional collection are separate policies. Required collection
  exhausts candidates and returns an error. Optional collection may exhaust
  candidates and return absence, but each failed attempt remains observable.
- Preserve deterministic ordering on ties when priority or capability policy is
  introduced. Do not depend on hash-map iteration order.
- Health, collection, provenance, logs, and tests must name the same connector
  and metric semantics.
- Record source provenance only after a command succeeds. Do not attribute a
  fallback value to a connector that failed.

### Keep related requests on the correct source

- Multi-call operations whose second request depends on the first source's
  retention or capability must stay on that connector for one attempt. Reward
  epoch block production is the concrete example: retention probing and the
  ranged block-production call form one candidate attempt.
- Independent metric families may resolve through different connectors.
- Economics mode is fail-closed for its required methods and retained ledger
  range. Do not export plausible partial economics as complete data.

### Bound provider work

- Keep block-production slot ranges and inflation-reward address batches within
  their production constants.
- Merge batches with saturating arithmetic where provider totals can approach
  integer limits.
- Avoid duplicate prerequisite RPCs inside one collection cycle unless the
  second read intentionally checks freshness.
- Timeouts come from validated connector configuration. Do not add an unbounded
  request or retry loop.

### Classify fallback deliberately

- A provider error is not automatically retryable or evidence that every
  provider lacks the method.
- Do not classify by broad substring outside the narrow adapter that has no
  structured alternative.
- Fallback to another connector and retrying the same connector are different
  policies. This repository currently implements connector fallback, not an
  implicit retry/backoff subsystem.
- Follow [Error handling](error-handling.md) when an error is ignored, mapped to
  absence, or used to continue.

## Review Gates

1. What exact Solana method and wire result are being modeled?
2. Are HTTP, JSON parsing, JSON-RPC error, missing result, null, and empty result
   distinguished?
3. Is the metric required or optional, and who owns that decision?
4. Which connector wins, why, and what makes selection deterministic?
5. Do dependent calls remain on one candidate connector?
6. Is source provenance emitted only for successful data?
7. Are request count, range, address count, timeout, and response size bounded?
8. Does economics mode still fail closed for every required capability?
9. Would logs and `sol_metric_source_info` reveal the actual selected source?

## Prohibited Shortcuts

- Do not add a second Solana RPC client or bypass the established request
  envelope and commitment handling.
- Do not turn every provider error into an unsupported-method classification.
- Do not return default block production, rewards, or schedules while claiming
  the source succeeded.
- Do not retry forever, sleep inside the collector, or hide backoff in a helper.
- Do not copy provider limits into tests or a second module.

## Boundary Proof

- Pure range, merge, and aggregation policy: unit tests.
- HTTP status, JSON-RPC envelope, batching, connector fallback, and provenance:
  the local Axum mock RPC boundary.
- Real validator compatibility: `just test-surfpool`.
- Runtime source selection: run the watcher against controlled connectors and
  inspect the rendered `sol_metric_source_info` series.
