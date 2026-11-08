set shell := ["bash", "-euo", "pipefail", "-c"]
set working-directory := '..'

doctor:
  command -v docker >/dev/null 2>&1
  docker --version
  docker compose version

build tag="sol-validator-watcher:local":
  docker build --file docker/Dockerfile --build-arg "RUST_VERSION=$(mise config get tools.rust.version)" --build-arg "SOL_WATCHER_BUILD_REVISION=$(git rev-parse HEAD)" -t {{quote(tag)}} .

smoke image="sol-validator-watcher:local" expected_revision="":
  scripts/smoke-container.sh {{quote(image)}} {{quote(expected_revision)}}
