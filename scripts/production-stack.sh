#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "usage: $0 <config|up|down|ps|logs|ready|reload-prometheus> <env-file> [args...]" >&2
  exit 2
}

[[ $# -ge 2 ]] || usage
action=$1
env_file=$2
shift 2

[[ -f "$env_file" ]] || {
  echo "production environment file not found: $env_file" >&2
  exit 1
}

compose=(docker compose --env-file "$env_file" -f docker/compose.production.yaml)

validate_images() {
  local image
  local found=false
  while IFS= read -r image; do
    [[ -n "$image" ]] || continue
    found=true
    if [[ ! "$image" =~ ^[^[:space:]]+@sha256:[0-9a-f]{64}$ ]]; then
      echo "production image is not pinned by sha256 digest: $image" >&2
      exit 1
    fi
  done < <("${compose[@]}" config --images)
  [[ "$found" == true ]] || {
    echo "production Compose config resolved no images" >&2
    exit 1
  }
}

case "$action" in
  config)
    validate_images
    exec "${compose[@]}" config "$@"
    ;;
  up)
    validate_images
    exec "${compose[@]}" up -d --pull always --no-build --wait "$@"
    ;;
  down)
    exec "${compose[@]}" down "$@"
    ;;
  ps)
    exec "${compose[@]}" ps "$@"
    ;;
  logs)
    exec "${compose[@]}" logs "$@"
    ;;
  ready)
    address=$("${compose[@]}" port watcher-mainnet 8000)
    port=${address##*:}
    curl -fsS --retry 30 --retry-all-errors --retry-delay 2 --retry-max-time 120 \
      "http://127.0.0.1:${port}/readyz" >/dev/null
    ;;
  reload-prometheus)
    exec "${compose[@]}" exec -T prometheus \
      wget --post-data='' -qO- http://127.0.0.1:9090/-/reload
    ;;
  *)
    usage
    ;;
esac
