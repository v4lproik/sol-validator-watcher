#!/usr/bin/env bash
set -euo pipefail

env_file=${1:?usage: canary-production.sh <production-env-file> <candidate-image:vMAJOR.MINOR.PATCH@sha256:digest>}
candidate=${2:?usage: canary-production.sh <production-env-file> <candidate-image:vMAJOR.MINOR.PATCH@sha256:digest>}
if [[ ! "$candidate" =~ ^[^[:space:]@]+:v?[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z][0-9A-Za-z.-]*)?@sha256:[0-9a-f]{64}$ ]]; then
  echo "canary image must include a semantic version tag and immutable sha256 digest: $candidate" >&2
  exit 1
fi
[[ -f "$env_file" ]] || {
  echo "production environment file not found: $env_file" >&2
  exit 1
}

metrics_port=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')
project=sol-validator-watcher-canary
compose=(docker compose --project-name "$project" --env-file "$env_file" -f docker/compose.production.yaml)
cleanup() {
  status=$?
  if [[ "$status" -ne 0 ]]; then
    SOL_WATCHER_IMAGE="$candidate" WATCHER_MAINNET_METRICS_PORT="$metrics_port" \
      "${compose[@]}" logs --no-color watcher-mainnet >&2 || true
  fi
  SOL_WATCHER_IMAGE="$candidate" WATCHER_MAINNET_METRICS_PORT="$metrics_port" \
    "${compose[@]}" down --remove-orphans >/dev/null 2>&1 || true
  return "$status"
}
trap cleanup EXIT

SOL_WATCHER_IMAGE="$candidate" WATCHER_MAINNET_METRICS_PORT="$metrics_port" \
  "${compose[@]}" up -d --pull always --no-build --wait watcher-mainnet

curl -fsS --retry 90 --retry-all-errors --retry-delay 3 --retry-max-time 300 \
  "http://127.0.0.1:${metrics_port}/readyz" >/dev/null
curl -fsS "http://127.0.0.1:${metrics_port}/metrics" | python3 -c '
import re
import sys

metrics = sys.stdin.read()

def metric_value(name):
    match = re.search(rf"^{name}\{{[^}}]*\}} ([0-9]+)$", metrics, re.MULTILINE)
    if match is None:
        raise SystemExit(f"canary metrics omitted {name}")
    return int(match.group(1))

if metric_value("sol_watcher_ready") != 1:
    raise SystemExit("canary readiness metric did not report admission")
if metric_value("sol_watcher_successful_poll_streak") < 5:
    raise SystemExit("canary successful-poll streak did not reach five")
if "sol_watcher_build_info{" not in metrics or "revision=\"unknown\"" in metrics:
    raise SystemExit("canary image omitted an immutable build revision")

'

container_id=$(SOL_WATCHER_IMAGE="$candidate" WATCHER_MAINNET_METRICS_PORT="$metrics_port" \
  "${compose[@]}" ps -q watcher-mainnet)
SOL_WATCHER_IMAGE="$candidate" WATCHER_MAINNET_METRICS_PORT="$metrics_port" \
  "${compose[@]}" stop --timeout 45 watcher-mainnet >/dev/null
exit_code=$(docker inspect "$container_id" --format '{{.State.ExitCode}}')
[[ "$exit_code" == 0 ]] || {
  echo "canary image did not stop cleanly: exit $exit_code" >&2
  exit 1
}

echo "Canary image became ready with an immutable build revision and stopped cleanly"
