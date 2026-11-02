set shell := ["bash", "-euo", "pipefail", "-c"]
set working-directory := '..'

doctor:
  command -v docker >/dev/null 2>&1
  docker --version
  docker compose version

build tag="sol-validator-watcher:local":
  docker build --file docker/Dockerfile --build-arg "RUST_VERSION=$(mise config get tools.rust.version)" -t {{quote(tag)}} .
