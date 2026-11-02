# Rust Review

## Scope

Use this resource when reviewing a Rust file, module, crate, diff, or pull
request. Review the requested surface and the contracts it can break; do not
rewrite code unless the user asks for a fix.

This repository is an application crate. Review against the Rust version
pinned in `mise.toml`, and favor concrete application types and `anyhow`
boundary context over speculative public library abstractions. If a change
introduces a library API, apply the stricter public API rules in
[Rust code quality](rust-code-quality.md).

## Establish the Review Boundary

1. Read the requested diff or files and the direct callers, tests, config,
   metrics, and operational consumers affected by the change.
2. Read `Cargo.toml`, `mise.toml`, and the matching repository resources before
   recommending language features, lints, commands, or architecture.
3. Distinguish application-internal items from exported library contracts.
4. Review changed behavior and adjacent breakage, not unrelated legacy style.
5. Preserve deliberate quirks unless the review can show a correctness,
   maintainability, security, or operational cost.

## Tool Evidence

- For a code change being prepared for handoff, `just ci verify` is the
  canonical gate.
- For a read-only review, do not run the mutating `just rust fmt`. Use available
  non-mutating evidence (`just rust clippy`, `just rust build`,
  `just rust test`, existing CI) and state exactly what ran.
- Use `just coverage check` only when coverage policy or the requested review
  scope requires it. Use `just surfpool test` only for real validator compatibility.
- Do not paste raw repeated compiler or Clippy output. Group one cause into one
  finding and cite the tool once.

## Review Order

### 1. Correctness and soundness

- Reachable panics, unchecked indexing, integer overflow, lossy casts, invalid
  serde assumptions, and safe APIs that can violate an invariant.
- `Eq`, `Ord`, and `Hash` implementations that compare different fields or use
  inconsistent semantics.
- Nondeterministic `HashMap` iteration leaking into output, connector ordering,
  metrics, or tests.
- Lock guards held across `.await`, user callbacks under a lock, detached tasks,
  and shutdown paths that can strand work.
- `unsafe` without a narrow soundness boundary and `// SAFETY:` justification.

### 2. Error handling

- `unwrap`, `expect`, panic, empty/default data, ignored results, or broad
  substring matching on reachable external failures.
- Lost causal errors, missing path/method/connector context, and errors logged
  then silently converted to success.
- Confusion among required, optional, unsupported, failed, cancelled, and stale.
- Read [Error handling](error-handling.md) and
  [RPC collection and errors](rpc-collection-and-errors.md) when applicable.

### 3. API and type design

- Borrowed parameters should use `&str`, `&[T]`, `&Path`, or `&T`, not
  `&String`, `&Vec<T>`, `&PathBuf`, or `&Box<T>`.
- Functions that store data take ownership instead of borrowing then cloning.
- Shared/public boolean parameters become an enum or separate named operation.
- Fields and items use the narrowest visibility; tests do not force production
  internals public.
- Domain units, valid states, absence, and fallibility are represented in types.
- Public types derive the coherent standard traits their semantics support.
  Hand-written ordering and equality contracts agree.
- `as_`, `to_`, and `into_` names match borrowing, allocation, and consumption.

### 4. Idiom and maintainability

- Clones used only to appease the borrow checker instead of shortening a borrow,
  moving with `take`/`replace`, or separating ownership.
- Traits, generics, trait objects, macros, builders, or helper modules without a
  current requirement.
- Iterator chains that obscure control flow and loops that merely reimplement a
  clear iterator operation.
- String concatenation instead of `format!`, `write!`, or `push_str`.
- Global lint suppression, unexplained exceptions, glob imports, and dead
  compatibility paths.

### 5. Documentation and tests

- Missing failure, panic, safety, side-effect, or unit semantics that signatures
  cannot express.
- Comments that narrate syntax or no longer match behavior.
- Tests of private wiring, source text, mock echoes, existence, or constants
  instead of observable behavior.
- Missing boundary proof for serde, HTTP/JSON-RPC, task lifecycle, metrics,
  runtime wiring, or Surfpool compatibility.

## Report Format

```markdown
# Rust review: <scope>

<Application/library assumption, overall verdict, and commands or CI examined.>

## Must fix
### 1. <Finding> (`path:line`)
**Rule:** <named repository invariant or Clippy/Rust rule>
**Impact:** <concrete failure or cost>
**Fix:** <smallest safe change, with focused code when useful>

## Should fix
...

## Consider
...

## What is already good
- <specific behavior or design to preserve>
```

- **Must fix:** correctness bug, soundness issue, reachable panic, security or
  operational failure, or a public contract mistake that will be costly to
  reverse.
- **Should fix:** code works but carries a concrete maintenance, error quality,
  testing, or performance cost.
- **Consider:** proportionate polish with a stated trade-off.

Every finding cites `path:line`, the violated rule, the consequence, and the
smallest fix. Group repeats. Do not pad the report or bury real defects under
style notes. Always name two to four concrete strengths.

## Prohibited Review Behavior

- Do not invent a defect without a reachable path and repository evidence.
- Do not demand library-grade generality from application-internal code.
- Do not recommend language features newer than the version pinned in `mise.toml`.
- Do not run formatters or modify code during a read-only review.
- Do not repeat resolved findings or turn personal style into a blocker.
