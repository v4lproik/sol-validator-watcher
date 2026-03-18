set shell := ["bash", "-euo", "pipefail", "-c"]

default:
  @just --list

doctor:
  command -v cargo >/dev/null 2>&1
  command -v just >/dev/null 2>&1
  cargo --version
  just --version

doctor-surfpool:
  command -v surfpool >/dev/null 2>&1
  surfpool --version

fmt:
  cargo fmt

check:
  cargo check

clippy:
  cargo clippy --all-targets --all-features -- -D warnings

test:
  cargo test

test-surfpool:
  if ! command -v surfpool >/dev/null 2>&1; then echo "surfpool CLI not found. Install it with: curl -sL https://run.surfpool.run/ | bash" >&2; exit 1; fi
  SURFPOOL_E2E=1 cargo test surfpool_smoke_test_collects_snapshot_and_renders_metrics -- --nocapture

test-surfpool-all:
  if ! command -v surfpool >/dev/null 2>&1; then echo "surfpool CLI not found. Install it with: curl -sL https://run.surfpool.run/ | bash" >&2; exit 1; fi
  SURFPOOL_E2E=1 cargo test

build:
  cargo build

build-release:
  cargo build --release

run config="etc/config.local.yaml":
  cargo run -- --config '{{config}}'

run-release config="etc/config.local.yaml":
  cargo run --release -- --config '{{config}}'

docker-build tag="sol-validator-watcher:local":
  docker build -t '{{tag}}' .

clean:
  cargo clean

ci:
  just fmt
  just clippy
  just test

ci-full:
  just fmt
  just clippy
  just test
  just test-surfpool
