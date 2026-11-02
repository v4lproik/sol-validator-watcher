# Rust Code Quality

## Scope

Use this resource for Rust APIs, ownership, type design, async code, serde
models, module boundaries, visibility, allocations, comments, and cleanup.
Specialized RPC, runtime, metrics, and error rules remain in their owning
resources.

## Ownership and Module Boundaries

- `main.rs` is the composition root: CLI parsing, runtime wiring, HTTP serving,
  reload orchestration, and process lifecycle.
- `collector.rs` owns Solana RPC transport, wire decoding, snapshot collection,
  and validator aggregation until an intentional module extraction moves a
  complete responsibility.
- `config.rs` owns parsing, normalization, defaults, environment expansion, and
  validation.
- `metrics.rs` owns Prometheus registration, update/reset behavior, rendering,
  labels, and unit conversion.
- `tasks.rs` owns task supervision and cooperative shutdown; `websocket.rs`
  owns websocket probing; `labels.rs` owns scope assignment.
- New code belongs with the invariant it changes. Do not add a generic `utils`,
  `helpers`, or second client module because the current owner is large.
- Keep visibility as narrow as the real caller boundary. Do not make a type or
  field public to simplify a test.

## Invariants

### Model states explicitly

- Prefer enums and focused structs over related booleans, sentinel strings, or
  partially initialized records.
- Use `Option<T>` only when absence is valid. Use `Result<T, E>` when the caller
  must distinguish failure.
- Keep units in names or types: lamports, slots, epochs, seconds, percentages,
  and SOL are not interchangeable numbers.
- Closed enums require intentional handling for every variant. Do not use a
  wildcard branch that silently accepts a new policy state.
- Serde wire structs model the external payload. Convert at the boundary before
  applying domain policy when the wire and domain invariants differ.

### Make ownership and allocation deliberate

- Borrow data when the callee does not need ownership. Move owned values once
  into the final snapshot instead of cloning to satisfy temporary APIs.
- Avoid per-poll allocation, repeated sorting, copied connector metadata, and
  reconstructed HTTP clients when the data changes only at configuration load.
- Use iterator chains when they remain readable and do not allocate an
  intermediate collection. Use local mutation for bounded accumulation when it
  avoids repeated allocation and exposes the algorithm.
- Do not box futures or introduce trait objects when a finite, statically
  dispatched command set expresses the contract. Dynamic dispatch must solve a
  current runtime-extensibility requirement.
- Batch and bound external requests. The production constant owns each provider
  limit; tests use that owner instead of copying the number.

### Sequence async effects explicitly

- Await or return every future. Detached work is allowed only when a supervisor
  owns its failure and shutdown.
- Use `tokio::join!` only for operations that are independent in ordering,
  failure, cancellation, and mutable state.
- Never hold a synchronous lock guard across `.await`.
- Futures spawned by `TaskManager` must be `Send + 'static`; do not weaken that
  contract to simplify an abstraction.
- Blocking filesystem or process work in a hot async path requires a deliberate
  boundary; do not add it invisibly to polling.

### Keep the main path readable

- Use guard clauses for invalid, disabled, or terminal states.
- Keep one abstraction level per function. Orchestration names steps; adapters
  perform I/O; pure functions own calculations and precedence.
- Extract code at a policy or effect boundary, not at an arbitrary line count.
  Avoid one-line wrappers and helper-on-helper layers.
- Names expose capability and unit. Avoid `data`, `value`, `handle`, `process`,
  or `manager` without a domain qualifier.
- Comments explain a safety property, provider constraint, or counterintuitive
  decision. Delete comments that only restate syntax.

### Preserve failure information

- Production code must not use `unwrap`, `expect`, `panic!`, `todo!`, or
  `unimplemented!` for reachable input or runtime states.
- Add `anyhow::Context` at a boundary where it identifies the failed operation,
  endpoint role, path, or config field. Preserve the original source error.
- Follow [Error handling](error-handling.md) for every fallback or suppressed
  error.

## Review Gates

1. Which module owns this policy and did the change create a second owner?
2. Which invalid states are prevented by the type, and which are rejected at
   runtime?
3. Are units and absence explicit at every public boundary?
4. Is each allocation, clone, boxed future, lock, and sort necessary at its
   execution frequency?
5. Are concurrent operations genuinely independent?
6. Can the happy path be read top-to-bottom without opening generic helpers?
7. Does every error retain its cause and useful boundary context?
8. Did the change remove obsolete branches, imports, tests, aliases, and
   comments made unreachable by the new path?

## Prohibited Shortcuts

- Do not cast or deserialize untrusted data directly into a domain assumption.
- Do not add a compatibility alias, duplicate policy map, or speculative
  abstraction for a hypothetical caller.
- Do not expose private internals solely for tests.
- Do not suppress Clippy globally. A targeted allow must state the Rust contract
  that requires it.
- Do not trade a visible error for an empty collection, zero, or `false` unless
  that fallback is the documented domain result and is observable.

## Boundary Proof

Use a pure unit test for a pure decision. Use the mock RPC boundary when serde,
HTTP status, JSON-RPC envelopes, or fallback behavior matters. Use an executable
smoke for runtime wiring. The compiler, formatter, and Clippy are supporting
evidence, not behavioral proof.
