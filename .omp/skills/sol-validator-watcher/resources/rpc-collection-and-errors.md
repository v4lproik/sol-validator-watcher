# RPC Collection and Provider Behavior

## Scope

Use this resource for Solana JSON-RPC calls, typed mode commands, explicit
command routes, connector fallback, request batching, source provenance, health
probes, and economics collection.

## Ownership and Boundaries

- `collector.rs` owns JSON-RPC request/response types, HTTP status handling,
  method implementations, snapshot source records, batching, and aggregation
  inputs.
- `collector/command.rs` maps typed commands to RPC operations and outputs.
- `mode.rs` owns stable mode and command identifiers.
- `source.rs` owns connector client construction, compiled command routes,
  required/optional fallback execution, and source selection.
- `main.rs` owns dependency guard policy and transactional source-chain reload.
- `config.rs` owns connector transport settings and exact per-command route
  validation.
- `websocket.rs` owns websocket reachability only. Do not infer RPC method
  support from websocket success.
- `metrics.rs` publishes configured routes and the connector that supplied each
  command's latest result.

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

### Keep command routing deterministic

- Each `modes.<mode>.commands.<command>` array is the complete precedence
  contract for that command. The first connector that succeeds wins; a failure
  advances only within that declared route.
- Connector declaration order is inventory, not fallback policy. Do not add
  numeric priorities, capability overrides, hidden sorting, or global
  connector-order fallback.
- Required and optional execution are separate policies. Required commands
  exhaust their route and return the last causal error. Optional commands may
  exhaust their route and return absence, but every failed attempt is logged.
- Do not depend on hash-map iteration order.
- Health, collection, provenance, logs, tests, and dashboards must use the same
  mode, command, connector, and precedence semantics.
- Record source provenance only after a command succeeds. Do not attribute an
  empty/default value to a connector that failed.

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
3. Is the command required or optional, and which mode owns that policy?
4. What exact route is configured, which connector wins, and what makes that
   selection deterministic?
5. Do dependent calls remain on one candidate connector?
6. Is source provenance emitted only for successful data?
7. Are request count, range, address count, timeout, and response size bounded?
8. Does economics mode still fail closed for every required command?
9. Would logs and `sol_mode_command_source_info` reveal the actual source?

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
- Real validator compatibility: `just surfpool test`.
- Runtime source selection: run the watcher against controlled connectors and
  inspect `sol_mode_command_route_info` and `sol_mode_command_source_info`.
