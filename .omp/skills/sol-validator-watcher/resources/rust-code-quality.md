# Rust Code Quality

## Scope

Read [Beautiful Rust best practices](../../rust-best-practices/SKILL.md) first,
then use this repository-specific resource whenever writing, refactoring,
debugging, or explaining Rust: APIs, ownership, types, async code, serde
models, modules, dependencies, allocations, unsafe code, documentation, and
cleanup. For explicit reviews, also read [Beautiful Rust review](../../rust-review/SKILL.md)
and [repository review rules](rust-review.md). For translations from another
language, also read [Beautiful Rust porting](../../rust-port/SKILL.md) and
[repository porting rules](rust-porting.md).

Repository-specific Rust version, commands, module ownership, and verification
rules in this resource take precedence when generic upstream advice differs.

This repository is an application crate. `mise.toml` is the single source for
the supported Rust version. Prefer concrete types, direct control flow, and
`anyhow` context inside the application. Do not introduce library-grade
genericity without a real cross-crate consumer. If a library API is added, its
public contracts require stricter evolution, documentation, and error-type discipline.

## Ownership and Module Boundaries

- `main.rs` is the composition root: CLI parsing, runtime wiring, HTTP serving,
  reload orchestration, and process lifecycle.
- `collector.rs` owns Solana RPC transport, wire decoding, snapshot collection,
  and validator aggregation until an intentional extraction moves a complete
  responsibility.
- `config.rs` owns parsing, normalization, defaults, environment expansion, and
  validation.
- `metrics.rs` owns Prometheus registration, update/reset behavior, rendering,
  labels, and unit conversion.
- `tasks.rs` owns task supervision and cooperative shutdown; `websocket.rs`
  owns websocket probing; `labels.rs` owns scope assignment.
- New code belongs with the invariant it changes. Do not add a generic `utils`,
  `helpers`, `manager`, or second client module because the current owner is
  large.
- Keep visibility as narrow as the real caller boundary: private, then
  `pub(super)` or `pub(crate)`, and only then `pub`. Tests do not justify wider
  production visibility.

## Write Rust-Shaped Code

- Model absence with `Option`, failure with `Result`, resources with ownership
  and `Drop`, and finite states with enums. Do not reproduce exceptions,
  inheritance, null checks, out-parameters, flag integers, or factories from
  another language.
- Make invalid combinations unrepresentable where practical. Prefer a
  data-carrying enum or focused struct over related booleans and optional fields
  whose valid combinations exist only in comments.
- Keep units in names or types: lamports, SOL, slots, epochs, seconds,
  durations, and percentages are not interchangeable numbers.
- Use the strongest standard type that matches the contract: `Path`/`PathBuf`
  for paths, `Duration` for spans, slices for borrowed sequences, and checked or
  saturating arithmetic where external values can reach a boundary.
- Serde structs model the external wire shape. Convert once before applying
  stronger domain invariants when wire and domain states differ.
- Preserve deterministic ordering. Do not let `HashMap` iteration define
  connector precedence, rendered output, metrics, or test results.

## Ownership and Borrowing

- Prefer unique ownership. `Arc`, `Mutex`, and interior mutability are design
  choices, not borrow-checker escape hatches.
- Borrow the most general useful shape: `&str` rather than `&String`, `&[T]`
  rather than `&Vec<T>`, `&Path` rather than `&PathBuf`, and `&T` rather than
  `&Box<T>`.
- If a function stores or consumes a value, take `T` by value instead of taking
  `&T` and cloning internally. If it reads, take `&T`; if it mutates the
  caller-owned value, take `&mut T`.
- Do not clone merely to end a borrow. First shorten the borrow with a block or
  intermediate, split disjoint state, or move with `mem::take`,
  `mem::replace`, or `Option::take`.
- Clone small immutable data deliberately. Use `Arc::clone(&value)` so shared
  ownership is visible at the call site.
- Prefer owned data in long-lived structs. Add lifetimes to stored structs only
  when borrowing is part of the real model, not to avoid one allocation.
- Keep `Arc`, `MutexGuard`, `Box`, and `RefCell` out of public signatures when a
  borrowed or owned domain value expresses the contract.

## Types and API Design

### Naming and call-site clarity

- Types and enum variants use `UpperCamelCase`; functions, modules, and locals
  use `snake_case`; constants use `SCREAMING_SNAKE_CASE`.
- Acronyms read as words: `RpcClient`, `HttpStatus`, `JsonRpcError`.
- Getter methods omit `get_`: `name()`, `name_mut()`, `take_name()`.
- Prefix conversions by cost and ownership: `as_` borrows cheaply, `to_`
  creates or converts an owned value, and `into_` consumes `self`.
- Shared or public boolean parameters are prohibited when the call site would
  read like `run(true)`. Use a two-state enum or separate named methods. A
  private helper with nearby obvious call sites may use a boolean.
- Avoid vague suffixes such as `Manager`, `Factory`, `Helper`, and `Util`.
  Name the capability or owned policy.

### Domain types and construction

- Use a newtype when two values share a representation but not a domain
  meaning. Invariant-bearing newtypes use a fallible constructor, `TryFrom`, or
  `FromStr`; do not provide infallible `From<Raw>` when validation can fail.
- Prefer named fields over tuples except for a single-field newtype or very
  local pair.
- `new()` and `Default` agree when both exist. A fallible constructor is
  `try_new`; conversion uses `From`/`TryFrom`; text parsing uses `FromStr`.
- Group four or more loose constructor parameters into semantic types. Add a
  builder only when many optional fields, cross-field validation, or side
  effects make direct construction unclear.
- Implement `From` or `TryFrom`, not `Into` or `TryInto` directly. Avoid `as`
  for conversions that can truncate, wrap, or lose sign.

### Traits and public contracts

- Derive coherent common traits when their semantics are valid: `Debug`,
  `Clone`, small-value `Copy`, `PartialEq`/`Eq`, `Ord`, and `Hash`.
- Hand-written `Eq`, `Ord`, and `Hash` must use the same fields and semantics.
  Mismatches corrupt map/set/sort behavior.
- Implement `Display` for user-facing values and dedicated library errors.
  Secret-bearing types use redacted `Debug`.
- Define a trait only for real generic programming, dynamic dispatch, or an
  extension trait. Do not create traits merely to group methods.
- Prefer concrete types, then static generics, then `dyn Trait` for a current
  heterogeneous/runtime requirement. Do not box futures when a finite static
  command set works.
- Keep required trait methods minimal and derive defaults from them.
- `Deref` is for smart pointers and owning collection-like wrappers, not
  inheritance or method forwarding.
- Crate-crossing public enums/structs require an explicit evolution decision;
  use `#[non_exhaustive]` when downstream exhaustive construction or matching
  would prevent compatible growth.

## Errors and Panics

- Use `Option` for normal absence, `Result` for reachable failure, and panic
  only for an internal invariant violation that indicates a bug.
- Production paths must not use `unwrap`, `expect`, indexing, or panic for
  external input, config, network, filesystem, time, or task outcomes.
- Pattern-match instead of check-then-unwrap.
- An intentional invariant panic or assertion includes the relevant values and
  a reason. A function returning `Result` should not also panic on ordinary
  inputs.
- Application boundaries use `anyhow::Context` to name the operation, method,
  connector role, or path while preserving the source error.
- If this crate exposes library APIs, public error types implement
  `std::error::Error`, `Display`, `Debug`, and preserve sources; do not expose
  `anyhow::Error`, `String`, `()`, or an unstructured boxed error.
- Destructors do not block, panic, or report fallible teardown. Provide an
  explicit consuming close/shutdown operation when callers must observe it.
- Read [Error handling](error-handling.md) for every fallback, ignored result,
  default-on-error path, retry, or cleanup failure.

## Everyday Idioms

- Use a short iterator pipeline when it reads top-to-bottom. Keep a `for` loop
  when the body has multiple effects, early exits, or clearer mutable
  accumulation. Do not use `.for_each` merely to avoid `for`.
- Use `Option`/`Result` combinators when they clarify one transformation; use
  `match` or `let ... else` when branching policy matters.
- Use `format!`, `write!`, and `push_str` rather than chained string `+`.
- Add explicit types where `.parse()`, `.collect()`, or `.into()` would obscure
  the intended result.
- Bind cleanup guards to a named variable. `let _ = guard` drops immediately.
- Import types/traits for repeated use and qualify free functions by module.
  Avoid wildcard imports outside tightly scoped test or enum matching code.
- Structured runtime diagnostics use `tracing`, not `println!` or `dbg!`.
- A reused limit, timeout, range, or batch size has one production constant.
  Tests import that owner rather than restating the literal.

## Async and Concurrency

- Await or return every future. Detached work is permitted only when a
  supervisor owns its failure and shutdown.
- Futures spawned by `TaskManager` remain `Send + 'static`. Do not weaken that
  contract to simplify an abstraction.
- Use `tokio::join!` only for operations independent in ordering, failure,
  cancellation, and mutable state.
- Never hold a synchronous lock or `RefCell` borrow across `.await`, call a
  callback under a lock, or return a lock guard from an API.
- Prefer channels for independently owned tasks. When state must be shared, keep
  linked invariants under one lock and make the critical section short.
- Use `async fn` for ordinary async functions. An explicit
  `fn -> impl Future + Send` is appropriate in traits or when the `Send` bound or
  pre-future setup is part of the contract.
- Move sustained CPU or blocking work out of the async executor or use the
  appropriate Tokio blocking boundary.

## Unsafe Code

- This repository currently needs no unsafe application code. A first unsafe
  block requires a concrete FFI or measured performance need and a narrow safe
  wrapper.
- Every unsafe operation is inside an explicit unsafe block with a `// SAFETY:`
  explanation of the upheld invariant. Every unsafe function documents caller
  obligations under `# Safety`.
- Do not use `transmute` to reinterpret enums or lifetimes, and do not add
  `unsafe impl Send/Sync` to silence the compiler.
- FFI types use `#[repr(C)]`, fixed-width integers, explicit ownership, null
  checks, same-side allocation/free, and panic containment at the boundary.
- Unsafe changes require adversarial tests and Miri or an equivalent soundness
  check in addition to repository verification.

## Modules, Dependencies, and Lints

- Organize modules by domain responsibility, not `types`, `traits`, or
  `helpers`. Move a complete owner; do not create a re-export/barrel layer or
  duplicate public path.
- Every dependency costs compile time, binary size, maintenance, and
  supply-chain risk. Prefer the standard library and existing crates.
- Preserve the repository's exact dependency-version policy and committed
  `Cargo.lock` unless dependency policy is the explicit change.
- New features are additive and describe capability. Do not use a feature to
  remove public behavior or hide production invariants.
- Do not recommend or commit language or library features newer than the Rust
  version pinned in `mise.toml` without updating the pin and verifying the
  migration.
- `just clippy` is the lint contract. Fix lints rather than suppressing them.
  When a narrow exception is real, prefer `#[expect(lint, reason = \"...\")]`
  so stale suppressions become visible. Do not add `#![deny(warnings)]` to
  source; CI already applies `-D warnings`.
- A broader lint-policy change must be its own reviewable change with cleanup,
  `just verify`, and no unexplained global allows.

## Documentation and Comments

- Document what a signature cannot express: invariants, units, failure modes,
  side effects, ownership transfer, complexity, and provider constraints.
- Public library functions returning `Result` document `# Errors`; reachable
  panics use `# Panics`; unsafe APIs use `# Safety`.
- Copyable examples use `?` instead of `unwrap`.
- Comments explain why, safety, external constraints, or an intentional
  workaround. They do not narrate syntax or preserve a design journal.
- Update or delete stale comments in the same change as behavior.

## Performance Order

1. Make the API correct and usable.
2. Measure the representative release workload.
3. Remove repeated I/O, client construction, sorting, allocation, and copying.
4. Reserve or reuse bounded buffers and batch external operations.
5. Consider specialized collections, hashers, allocators, or unsafe code only
   with evidence that earlier steps are insufficient.

Do not claim a speedup without before/after evidence on the same workload.

## Review Gates

1. Which module owns this policy and did the change create a second owner?
2. Which invalid states are prevented by types, and which are rejected at
   runtime?
3. Are borrowing, ownership, visibility, units, and absence explicit?
4. Does every `Clone`, allocation, box, lock, sort, and conversion have a
   reason at its execution frequency?
5. Are standard trait implementations coherent?
6. Are concurrent operations genuinely independent and supervised?
7. Does every error retain its cause and useful boundary context?
8. Is the API concrete and proportionate to this application?
9. Does the code use only features supported by the Rust version pinned in
   `mise.toml` and the existing dependency policy?
10. Did the change remove obsolete branches, tests, aliases, and comments?

## Prohibited Shortcuts

- Do not clone, box, add shared mutability, or widen visibility merely to satisfy
  the compiler.
- Do not add a compatibility alias, duplicate policy map, prelude, wildcard
  re-export, or speculative generic abstraction.
- Do not suppress Clippy globally or add an unexplained lint exception.
- Do not trade a visible error for an empty collection, zero, `false`, or a
  default object unless that is the explicit domain contract and remains
  observable.
- Do not introduce unsafe code or a dependency before proving the current
  requirement cannot be met safely with existing tools.

## Boundary Proof

Use a pure unit test for a pure decision. Use the mock RPC boundary when serde,
HTTP status, JSON-RPC envelopes, batching, or fallback matters. Use task tests
for concurrency/shutdown and an executable smoke for runtime wiring. The
compiler, rustfmt, and Clippy are supporting evidence, not behavioral proof.
