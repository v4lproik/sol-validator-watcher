# Porting Code to Rust

## Scope

Use this resource when translating or rewriting C, C++, Python, Go,
JavaScript, or TypeScript behavior into this Rust application. Port the
observable contract, not the source language's structure.

## Understand Before Implementing

1. Read the original code, tests, callers, configuration, and failure handling.
2. List public inputs, outputs, ordering, tie-breaking, empty/boundary behavior,
   integer limits, side effects, and every failure outcome.
3. Separate required behavior from source-language artifacts such as manual
   memory management, out-parameters, exceptions, global mutable state,
   inheritance, duck typing, and fixed buffers.
4. Preserve unexplained quirks and identify them in the PR rather than silently
   fixing them during the port.
5. Choose the narrow repository owner before adding files. Ported collection,
   config, metrics, task, and websocket behavior belongs in the existing owner.

## Pin Behavior First

- Port or write characterization tests before replacing the implementation.
- Keep a traceable mapping from each original test/behavior to its Rust proof in
  the PR description; do not create a permanent porting document unless the
  user requests one.
- Use deterministic inputs and include ordering, empty input, exact limits,
  malformed input, and failure cases that callers observe.
- Differential testing is useful for realistic and boundary inputs. Do not build
  a compatibility runtime for exotic inputs the application never accepts.

## Translate Concepts, Not Syntax

| Source concept | Rust shape |
| --- | --- |
| Null or missing value | `Option<T>` |
| Exception, error code, `(value, error)` | `Result<T, E>` with application context |
| Closed magic strings or integer flags | Enum or focused options type |
| Record/dictionary/plain object | Struct with named fields |
| Out-parameters | Returned tuple or struct |
| Manual allocation/free | Owned value and `Drop`; explicit fallible close when needed |
| Pointer plus count/list | Slice or owned `Vec<T>` |
| Insertion-ordered map output | `Vec` or ordered map, not an unordered `HashMap` |
| Unbounded integer/JavaScript number | Deliberately sized integer or `f64` with checked conversion |
| Global mutable state | State owned by the runtime/module that enforces its invariant |
| Inheritance/base class | Composition, enum, or a trait required by real generic use |

- Use `&str`, `&[T]`, `&Path`, `Duration`, iterators, and standard conversion
  traits where they express the actual contract.
- Replace source-language constructors/factories with direct construction,
  `new`, `try_new`, `From`/`TryFrom`, or `FromStr` only when those semantics fit.
- Keep application APIs concrete. Do not introduce `dyn Any`, broad generic
  wrappers, service/factory hierarchies, or dependency injection to imitate the
  original language.
- Preserve deterministic sort stability and tie-breaking when order is visible.
- Preserve fixed limits only when they are provider or product behavior; remove
  limits that existed solely because of a source-language buffer.

## Memory, Concurrency, and FFI

- Prefer ownership and borrowing over `Rc<RefCell<_>>` or `Arc<Mutex<_>>` ports
  of mutable object graphs.
- Choose Tokio tasks/channels only when the original behavior is asynchronous in
  this runtime. Do not translate every thread/goroutine into a detached task.
- Do not write `unsafe` for a pure rewrite. If native FFI is required, isolate it
  in the smallest module, use `#[repr(C)]` and sized types, define ownership of
  pointers and buffers, reject null, prevent panics from crossing the boundary,
  and provide a safe API with explicit `// SAFETY:` reasoning.
- Allocation and deallocation stay on the same side of an FFI boundary.

## Keep the Port Proportionate

- Use the Rust standard library instead of reimplementing another language's
  string, collection, parse, or exception runtime.
- Keep the original algorithm unless it has a demonstrated correctness or
  realistic complexity problem. Name any deliberate algorithm change.
- Add only the public surface callers need plus coherent standard traits.
- Do not add builders, custom iterators, macros, or new crates without a current
  call-site requirement.
- Every new dependency must justify compile time, binary size, maintenance, and
  supply-chain cost while preserving this repository's exact-version policy.

## Verification

- Run the original test suite when available and the mapped Rust tests on the
  same representative inputs.
- Run `just ci verify` on the final revision.
- Smoke the real repository boundary: RPC fixture, watcher CLI, rendered metric,
  task shutdown, Compose config, or Surfpool as applicable.
- Record API mapping, test mapping, preserved quirks, deliberate differences,
  and exact commands/results in the PR description.

## Review Gates

1. Is every observable source behavior mapped to a Rust type, result, and test?
2. Which original constructs are incidental and intentionally absent?
3. Did ordering, tie-breaking, numeric limits, case/whitespace, and empty input
   remain compatible where callers rely on them?
4. Does Rust own resources and errors instead of simulating manual lifecycle or
   exceptions?
5. Is the API concrete and proportionate to current callers?
6. Are any behavior differences explicit and approved?
7. Does the real watcher boundary prove the port, not only unit tests?

## Prohibited Shortcuts

- Do not transliterate line by line.
- Do not silently fix ambiguous or quirky behavior while claiming compatibility.
- Do not replace typed errors with strings, magic integers, or panics.
- Do not use unordered collections where output order is part of the contract.
- Do not add unsafe/raw pointers unless an actual FFI boundary requires them.
- Do not treat compilation as proof that the port matches behavior.
