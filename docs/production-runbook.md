# Production Runbook

This runbook operates the supported single-host Mainnet deployment in
[`docker/compose.production.yaml`](../docker/compose.production.yaml). The stack
runs one watcher profile, Prometheus, Grafana, and Alertmanager. It does not run
a validator or expose public ports.

## Production contract

- Deploy only semantically versioned, digest-pinned references such as
  `ghcr.io/v4lproik/sol-validator-watcher:vMAJOR.MINOR.PATCH@sha256:<digest>`.
  Mutable aliases such as `latest` and digest-only references are rejected.
- Run one watcher replica per profile. A second active replica duplicates RPC
  traffic and alert series.
- Keep `8003`, `9090`, `3001`, and `9093` bound to loopback. Use an SSH tunnel or
  a private authenticated proxy for remote access.
- Store RPC URLs, Grafana credentials, and the Alertmanager webhook outside the
  repository in files mode `0600`.
- Keep the configured RPC response limit at or below the available watcher
  memory. The committed default is 32 MiB and the production watcher limit is
  512 MiB.
- Treat `/healthz` as process liveness and `/readyz` as snapshot availability.
  Readiness requires five consecutive complete polling rounds; a backup-sourced
  complete snapshot remains ready but reports degraded source redundancy.

## Prepare

1. Install Docker Engine with Compose v2, `mise`, `just`, `curl`, and `gh`.
2. Copy the templates outside the repository and restrict their permissions:

   ```bash
   cp .env.production.example /etc/sol-validator-watcher/production.env
   cp .env.production.watcher.example /etc/sol-validator-watcher/watcher.env
   install -m 0600 /dev/null /etc/sol-validator-watcher/grafana-admin-password
   install -m 0600 /dev/null /etc/sol-validator-watcher/alertmanager-webhook-url
   chmod 0600 /etc/sol-validator-watcher/production.env \
     /etc/sol-validator-watcher/watcher.env
   ```

3. Set `SOL_WATCHER_IMAGE` to the release version bound to its immutable image
   digest, fill the Mainnet RPC URLs, and write the Grafana password and complete
   alert receiver URL to their secret files. Never put credentials on a command line.
4. Verify the image attestation:

   ```bash
   gh attestation verify \
     oci://ghcr.io/v4lproik/sol-validator-watcher:vMAJOR.MINOR.PATCH@sha256:<digest> \
     --repo v4lproik/sol-validator-watcher \
     --signer-workflow v4lproik/sol-validator-watcher/.github/workflows/container.yml
   ```

5. Validate the complete rendered deployment and monitoring rules:

   ```bash
   just production check /etc/sol-validator-watcher/production.env
   ```

## Deploy

Use a canary before replacing the production watcher:

```bash
just production canary \
  /etc/sol-validator-watcher/production.env \
  ghcr.io/v4lproik/sol-validator-watcher:vMAJOR.MINOR.PATCH@sha256:<candidate-digest>
```

The canary uses the production profile and secrets but does not publish ports or
write production volumes. It must complete five successful polls, expose the
expected immutable build revision, and stop cleanly.

Roll out the candidate only after the canary passes:

```bash
just production rollout \
  /etc/sol-validator-watcher/production.env \
  ghcr.io/v4lproik/sol-validator-watcher:vMAJOR.MINOR.PATCH@sha256:<candidate-digest>
```

The rollout deliberately repeats the canary so it never relies on stale
verification. It stores the previous environment as `production.env.rollback`,
starts the versioned, digest-pinned stack, and requires both `/healthz` and
`/readyz` to succeed. A failed rollout restores the prior reference and revalidates readiness.

## Verify

```bash
just production ps /etc/sol-validator-watcher/production.env
curl -fsS http://127.0.0.1:8003/healthz
curl -fsS http://127.0.0.1:8003/readyz
curl -fsS http://127.0.0.1:9090/-/ready
curl -fsS http://127.0.0.1:3001/api/health
curl -fsS http://127.0.0.1:9093/-/ready
```

Inspect these watcher series before declaring the deployment healthy:

- `sol_watcher_build_info`: expected version and full revision
- `sol_config_info`: expected SHA-256 configuration fingerprint
- `sol_config_reload_total`: no unexpected rejected reloads
- `sol_mode_command_fallback_active`: zero unless a documented provider failure
  is active
- `sol_rpc_block_fee_cache_accesses_total`: block-fee cache hit/miss state when
  economics collection is enabled
- `sol_rpc_block_fee_methods_requested_total`: bounded provider method load

Open Grafana at <http://127.0.0.1:3001> and confirm every enabled mode, connector
route, selected source, and current alert state. Deliver a synthetic notification
before launch and after receiver changes:

```bash
just production test-alert-delivery
```

## Routine operation

```bash
just production logs /etc/sol-validator-watcher/production.env
just production reload-prometheus /etc/sol-validator-watcher/production.env
just production down /etc/sol-validator-watcher/production.env
```

Prometheus retains 30 days or 10 GiB, whichever limit is reached first. Docker
rotates each service log at 10 MiB with five files. Monitor host disk, memory,
CPU, file descriptors, and NTP outside this stack.

Configuration files are watched transactionally. A valid changed profile is
accepted as one source-chain revision; an invalid or capability-incompatible
change increments the rejected-reload counter and keeps the last-known-good
runtime. Investigate rejections instead of restarting around them.

## Alert response

| Alert | First checks | Action |
| --- | --- | --- |
| `SolValidatorWatcherTargetDown` | container status, `/healthz`, watcher logs | Restore the process or last-known-good image; do not suppress the target. |
| `SolValidatorWatcherNotReady` | `/readyz`, failed RPC counters, mode status | Identify the failing required command or provider. Readiness recovers after five complete polls. |
| `SolValidatorWatcherSourceFailoverActive` | `sol_source_failover_status`, transition counters, source logs | Restore the affected primary connector; successful backup snapshots remain available while redundancy is degraded. |
| `SolValidatorWatcherConfigReloadRejected` | watcher logs, `sol_config_info`, profile diff | Revert or correct the profile; the prior runtime remains active. |
| `SolValidatorWatcherFallbackActive` | route and source metrics, selected provider health | A recent snapshot used a non-primary route; correlate with the source-circuit alert to distinguish an active cooldown from a one-round fallback. |
| `SolValidatorWatcherBlockFeeCacheIneffective` | cache hit/miss and requested-method counters | Check epoch/slot churn and provider behavior before increasing request budgets. |
| `SolValidatorWatcherSnapshotStale` | latest successful snapshot, RPC status, host clock | Restore source access; stale data must not be treated as current. |

`sol_source_failover_status` uses `0` for primary, `1` for open cooldown, and
`2` for an admitted recovery probe. An empty `command` label identifies the
shared connector/source-requirement circuit; a populated label identifies the
isolated command circuit.

Critical alerts inhibit warning alerts for the same deployment and network. The
committed Alertmanager receiver reads its endpoint from a secret file, groups
related alerts for 30 seconds, and sends resolved notifications.

## Roll back

```bash
just production rollback /etc/sol-validator-watcher/production.env
```

Rollback requires a retained `.rollback` environment containing exactly one
immutable `SOL_WATCHER_IMAGE` entry. It restores the prior environment and
restarts the complete stack. If rollback fails, the failed environment is
preserved as `.failed` and the prior environment file is restored for manual
recovery.

## Backup and restore

The source of truth for watcher and dashboard configuration is Git. Back up the
Prometheus and Grafana Docker volumes only when historical monitoring data or
operator-created Grafana state must be retained.

1. Stop the stack with `just production down` before a filesystem-level volume
   snapshot.
2. Snapshot `sol-validator-watcher-prometheus-data` and
   `sol-validator-watcher-grafana-data` with the host's volume or block-storage
   tooling.
3. Record the deployed image digest, Git revision, configuration fingerprint,
   production environment, and watcher secret file as one recovery set.
4. Restore volumes only to compatible Prometheus and Grafana image versions,
   run `just production check`, start the stack, and verify all health endpoints
   and the newest retained sample time.

Alertmanager state is disposable; its routing configuration is committed and
its receiver URL is supplied from the secret file.

## Release

After a reviewed commit passes required CI, security, and pull-request
exact-image checks, create an annotated `vMAJOR.MINOR.PATCH` tag. The container
workflow publishes a versioned multi-architecture image bound to its immutable
digest, smokes that pushed digest, scans both published architectures, generates
its SBOM and provenance attestation, verifies the attestation, and creates
GitHub-generated release notes containing the immutable image reference.
Published releases are repository-immutable: their tags and assets cannot be
changed or deleted. Keep the previous versioned digest through the observation
window; never roll back to a mutable tag.
