# Testing and Verification

## Scope

Use this resource to choose the smallest proof that can catch a realistic bug,
then complete the repository's required local gates.

## Local Gates

- Run `just verify` on the final revision before every commit or push. It owns
  formatting, all-target/all-feature Clippy with warnings denied, build, and the
  default tests.
- Run `just coverage-ci` when coverage logic or thresholds change. Record the
  threshold and result.
- Run `just test-surfpool` when real validator/RPC compatibility is part of the
  claim. `SURFPOOL_E2E` is opt-in; a skipped default test is not E2E evidence.
- Run `just site-build` for Pages or coverage publishing changes.
- Run `actionlint -color` for GitHub Actions changes.
- Run affected `just stack-config-*` recipes for Compose changes before starting
  any stack.
- A verification result becomes stale when a tested file changes. Rerun the
  affected gate before push.

## Choose the Smallest Valid Layer

- Pure calculation, normalization, precedence, aggregation, range, and state
  decisions: unit tests with direct values.
- Serde, HTTP status, JSON-RPC envelope, batching, fallback, and connector source
  behavior: local Axum mock RPC tests using the production client.
- Task supervision: tests that exercise completion, error, panic, cancellation,
  and join behavior through `TaskManager`.
- Prometheus contracts: render real metrics and assert stable names, labels,
  values, source selection, and stale-series removal.
- Runtime wiring: launch the actual binary against controlled local endpoints
  and fetch the real HTTP surface.
- Solana validator compatibility: Surfpool E2E.
- Compose, Grafana, Prometheus, Pages, and containers: validate or run the actual
  affected boundary, not a source-text substitute.

## Test Value

- Every permanent test defends a plausible consumer-visible regression. The
  original defect or policy violation must make it fail.
- Assert returned values, decoded errors, selected connector, rendered metric,
  file state, process exit, or another observable result.
- Test exact boundaries and rejected cases owned by the change. Include empty,
  maximum batch/range, missing optional data, all-connectors-failed, and invalid
  config cases when reachable.
- Fixtures own their setup and cleanup. Bind local servers to ephemeral ports;
  do not depend on live providers, execution order, shared files, or another
  test's process.
- Use observable barriers and deadlines for async tests. Avoid timing luck and
  arbitrary sleeps.
- Import production constants for provider limits and thresholds rather than
  copying their numeric values into tests.
- Keep one canonical test per behavior and layer. A second layer is justified
  only when it exercises a boundary mocks cannot prove.

## Smoke Proof

A behavioral change is not complete until the changed surface runs:

- CLI/runtime: start the watcher with representative config, observe one poll,
  fetch `/healthz` and `/metrics`, and stop cleanly.
- RPC fallback/source selection: control each endpoint response and assert the
  selected connector in `sol_metric_source_info`.
- Dashboard: load the actual local panel with representative data.
- Workflow or script: execute the harmless validation/build path and inspect its
  artifact or output.

Throwaway smoke fixtures stay outside the repository and are removed afterward.

## Performance Evidence

- Optimization claims need a representative release-mode measurement and the
  same workload before and after.
- Benchmark setup must not dominate the operation being measured.
- If durable benchmarks are added, expose them through a `Justfile` recipe so
  contributors use the pinned toolchain.
- Network latency does not excuse avoidable allocation, repeated sorting, or
  rebuilt clients in the polling path.

## Review Gates

1. What public behavior changes and which layer owns it?
2. Would the plausible regression fail this test?
3. Is a smaller layer sufficient, or does a real boundary change the result?
4. Are accepted, rejected, empty, boundary, fallback, and shutdown outcomes
   covered where reachable?
5. Is time, I/O, port allocation, and cleanup deterministic?
6. Which real smoke demonstrates the end-to-end behavior?
7. Which extra local gates apply beyond `just verify`?
8. Is the exact revision unchanged since those gates passed?

## Prohibited Shortcuts

- Do not assert source text, private helper calls, import shape, mere existence,
  or mock echoes when an outcome is observable.
- Do not add a test that only proves Rust, Tokio, Axum, Prometheus, serde, or a
  mock framework behaves as documented.
- Do not replace a real serde/HTTP/RPC boundary with a hand-written lookalike in
  a test claiming to prove that boundary.
- Do not treat a skipped Surfpool test, a dispatched CI run, or compilation alone
  as successful behavioral verification.
- Do not push before local gates pass.
