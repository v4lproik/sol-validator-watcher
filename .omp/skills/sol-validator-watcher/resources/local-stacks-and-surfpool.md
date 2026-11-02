# Local Stacks and Surfpool

## Scope

Use this resource for Dockerfiles, Compose base/overlay files, validator and
Surfpool lifecycle, env/config templates, local Prometheus/Grafana wiring,
image builds, stack inspection, and teardown.

## Ownership and Boundaries

- `docker/compose.yaml` owns the shared observability and watcher base.
- `docker/compose.surfpool.yaml`, `docker/compose.validator.yaml`,
  `docker/compose.devnet.yaml`, and `docker/compose.mainnet.yaml` are explicit
  overlays for distinct environments.
- `docker/Dockerfile` and `docker/Dockerfile.dockerignore` own the image build.
- `config/` contains committed templates and connector configuration;
  credentials stay in `.env` or exported environment variables.
- `deploy/prometheus/` and `deploy/grafana/` own local observability
  provisioning.
- The root `Justfile` and modules under `just/` expose the supported Compose
  file combinations. Do not rebuild those combinations from memory.

## Invariants

- Run `just stack config <target>` before a changed stack is started. Rendered
  config must resolve files, environment interpolation, networks, ports,
  volumes, and dependencies without mutation.
- Start stacks only when the behavior requires a live boundary. Use the
  narrowest `just stack up <target>` profile and inspect it with matching
  `just stack ps <target>` and `just stack logs <target>` commands.
- `just stack down` is destructive process control. Do not run it merely to
  inspect syntax, and do not add hidden volume deletion to it.
- Surfpool and local validator state are ephemeral. Tests own startup, readiness,
  and cleanup; do not assume a previous ledger or deployment remains valid.
- Readiness must prove the endpoint and method the watcher needs, not only that a
  container process exists.
- Keep container paths and config mount paths aligned with the image working
  directory and CLI defaults.
- Keep host and container port roles explicit. Avoid adding an overlapping
  default port across overlays.
- Never commit `.env`, RPC credentials, websocket tokens, private validator
  material, or rendered secret values.

## Surfpool Rules

- Verify the installed CLI with `just surfpool doctor` before an opt-in E2E run.
- Use `just surfpool test` for the focused watcher smoke and
  `just surfpool test all` only when the broader E2E scope is justified.
- A default test run that prints a Surfpool skip is not proof of validator
  compatibility.
- The E2E harness must use bounded startup polling, report the last observed
  failure, and clean up its process and temporary files on every exit path.

## Docker and Compose Review Gates

1. Which base and overlay files form the intended stack?
2. Which env/config files are mounted, and are all paths valid in the container?
3. Which service owns readiness and startup order?
4. Can a missing secret, endpoint, or file fail clearly without printing its
   value?
5. Are ports and volumes isolated from other profiles?
6. What harmless `just stack config <target>` command proves the wiring?
7. If a live start is required, what observable watcher/Prometheus/Grafana or
   validator result proves it?
8. How is the stack stopped without deleting unrelated state?

## Prohibited Shortcuts

- Do not invoke ad hoc Docker Compose file combinations when a `just` recipe
  owns the profile.
- Do not make a stack recipe silently delete volumes, ledgers, coverage, or
  configuration.
- Do not use live Devnet/Mainnet RPCs as deterministic unit or integration test
  fixtures.
- Do not log environment contents or embed secrets in Compose, committed TOML,
  command arguments, or test output.
- Do not claim a container build proves runtime health; exercise the changed
  endpoint or collection path.

## Boundary Proof

- Compose-only change: matching `just stack config <target>` output.
- Image/build-context change: `just container build` plus a representative
  container invocation.
- Surfpool/validator lifecycle change: doctor, focused opt-in E2E, and observed
  metrics.
- Prometheus/Grafana wiring: start the narrow stack, confirm targets/data source,
  and inspect the changed metric or panel.
