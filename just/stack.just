set shell := ["bash", "-euo", "pipefail", "-c"]
set working-directory := '..'

_compose target action:
  #!/usr/bin/env bash
  set -euo pipefail

  target={{quote(target)}}
  action={{quote(action)}}
  files=(-f docker/compose.yaml)

  case "${target}" in
    surfpool) files+=(-f docker/compose.surfpool.yaml) ;;
    validator) files+=(-f docker/compose.validator.yaml) ;;
    devnet) files+=(-f docker/compose.devnet.yaml) ;;
    mainnet) files+=(-f docker/compose.mainnet.yaml) ;;
    all)
      files+=(
        -f docker/compose.surfpool.yaml
        -f docker/compose.validator.yaml
        -f docker/compose.devnet.yaml
        -f docker/compose.mainnet.yaml
      )
      ;;
    *)
      echo "stack target must be surfpool, validator, devnet, mainnet, or all: ${target}" >&2
      exit 2
      ;;
  esac

  export RUST_VERSION="$(mise config get tools.rust.version)"

  case "${action}" in
    config) docker compose "${files[@]}" config ;;
    up) docker compose "${files[@]}" up -d --build ;;
    logs) docker compose "${files[@]}" logs -f ;;
    ps) docker compose "${files[@]}" ps ;;
    down) docker compose "${files[@]}" down --remove-orphans ;;
    *)
      echo "unsupported stack action: ${action}" >&2
      exit 2
      ;;
  esac

config target:
  just stack _compose {{quote(target)}} config

up target:
  just stack _compose {{quote(target)}} up

logs target:
  just stack _compose {{quote(target)}} logs

ps target:
  just stack _compose {{quote(target)}} ps

reload service:
  service={{quote(service)}}; case "${service}" in prometheus) curl -fsS -X POST http://localhost:9090/-/reload ;; *) echo "reload service must be prometheus: ${service}" >&2; exit 2 ;; esac

down:
  just stack _compose all down
