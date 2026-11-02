---
name: sol-validator-watcher
description: >-
  Repository-specific Rust engineering invariants and verification gates for
  sol-validator-watcher. Use when changing Rust code, Solana RPC collection,
  runtime lifecycle, configuration, Prometheus/Grafana, tests, local stacks,
  GitHub Actions, Pages, containers, or repository workflows.
---

# Sol Validator Watcher Engineering

This skill is a routing index. Identify the changed behavior, then read only the
matching resources. Read every matching resource when work crosses categories.

## Workflow

1. Identify the observable behavior and every boundary it crosses: caller,
   Rust module, RPC or websocket, configuration, metric, task, test, container,
   and workflow.
2. Read each component-index row whose trigger matches. Do not open unrelated
   resources.
3. Trace the current success, failure, fallback, and shutdown paths before
   editing.
4. Reuse the existing owner and `just` command. Do not create a second client,
   policy map, config path, fallback convention, or ad hoc workflow.
5. Verify the narrow invariant and then smoke the real boundary.
6. Run `just verify` on the final revision before every commit or push. Run all
   additional category gates listed in `AGENTS.md`.

## Component Index

| Component | Read when changing |
| --- | --- |
| [Rust best practices](../rust-best-practices/SKILL.md) and [repository Rust rules](resources/rust-code-quality.md) | Writing, refactoring, or debugging Rust APIs, ownership, lifetimes, enums, serde models, async sequencing, allocations, comments, visibility, or dead code |
| [Rust review](../rust-review/SKILL.md) and [repository review rules](resources/rust-review.md) | Reviewing a Rust file, module, crate, diff, or pull request |
| [Rust porting](../rust-port/SKILL.md) and [repository porting rules](resources/rust-porting.md) | Translating or rewriting C, C++, Python, Go, JavaScript, or TypeScript behavior in Rust |
| [RPC collection and errors](resources/rpc-collection-and-errors.md) | JSON-RPC methods, connector order, fallback, batching, provider health, source provenance, economics checks, or wire decoding |
| [Runtime, configuration, and shutdown](resources/runtime-configuration-and-shutdown.md) | Startup, config load/reload, task supervision, HTTP endpoints, websocket probes, liveness, signals, or environment expansion |
| [Metrics and dashboards](resources/metrics-and-dashboards.md) | Prometheus names/labels/units, resets, source metrics, Grafana dashboards, provisioning, alerts, or operator semantics |
| [Testing and verification](resources/testing-and-verification.md) | Test scope, fixtures, mock RPC, Surfpool, smoke tests, coverage, CI evidence, or performance proof |
| [Local stacks and Surfpool](resources/local-stacks-and-surfpool.md) | Docker, Compose overlays, validator/Surfpool lifecycle, env templates, Prometheus wiring, image builds, or teardown |
| [CI, publishing, and workflows](resources/ci-publishing-and-workflows.md) | `Justfile`, GitHub Actions, coverage artifacts, Pages, containers, scripts, command changes, or release checks |
| [Error handling](resources/error-handling.md) | Any fallback, ignored result, default-on-error path, retry decision, error translation, cleanup failure, or background task failure |

## Cross-Component Rules

- Deserialize and validate external data at the owning boundary; pass typed
  values inward.
- Keep one owner for each policy, unit conversion, label set, RPC limit,
  connector order, timeout, and shutdown decision.
- Required, optional, unavailable, and failed are distinct states. Do not hide
  one behind an empty collection, zero, or generic success.
- Runtime behavior, source provenance, metrics, dashboards, and tests must
  describe the same semantics.
- Every long-running future has an owner, a surfaced failure path, and shutdown
  behavior.
- Configuration changes are validated before they replace the last known-good
  runtime state.
- CI confirms a locally verified revision; it is not the first execution of a
  repository check.

## Extending This Skill

1. Update the resource that owns the changed invariant.
2. Add a resource only when no existing category owns the behavior.
3. Record durable rules, prohibited shortcuts, and observable proof; do not
   copy pull-request narration.
4. Keep command names and defaults in `AGENTS.md` and the live `Justfile`, not in
   multiple resources.
5. Remove stale guidance in the same change that removes the underlying path.
