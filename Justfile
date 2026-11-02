set shell := ["bash", "-euo", "pipefail", "-c"]

default:
  @just --list

ensure-rust-components:
  mise exec -- rustup component add rustfmt clippy llvm-tools

doctor: ensure-rust-components
  command -v mise >/dev/null 2>&1
  command -v just >/dev/null 2>&1
  mise exec -- cargo --version
  mise exec -- cargo fmt --version
  mise exec -- cargo clippy --version
  mise exec -- cargo llvm-cov --version
  just --version

doctor-docker:
  command -v docker >/dev/null 2>&1
  docker --version
  docker compose version

doctor-surfpool:
  command -v surfpool >/dev/null 2>&1
  surfpool --version

fmt: ensure-rust-components
  mise exec -- cargo fmt

check:
  mise exec -- cargo check

clippy: ensure-rust-components
  mise exec -- cargo clippy --all-targets --all-features -- -D warnings

test:
  mise exec -- cargo test

verify:
  just fmt
  just clippy
  just build
  just test

test-surfpool:
  if ! command -v surfpool >/dev/null 2>&1; then echo "surfpool CLI not found. Install it with: curl -sL https://run.surfpool.run/ | bash" >&2; exit 1; fi
  SURFPOOL_E2E=1 mise exec -- cargo test surfpool_smoke_test_collects_snapshot_and_renders_metrics -- --nocapture

test-surfpool-all:
  if ! command -v surfpool >/dev/null 2>&1; then echo "surfpool CLI not found. Install it with: curl -sL https://run.surfpool.run/ | bash" >&2; exit 1; fi
  SURFPOOL_E2E=1 mise exec -- cargo test

coverage: ensure-rust-components
  mise exec -- cargo llvm-cov --workspace --all-features --summary-only --ignore-filename-regex '(^|/)src/(main|surfpool_tests)\.rs$'

coverage-html: ensure-rust-components
  mise exec -- cargo llvm-cov --workspace --all-features --html --output-dir coverage --ignore-filename-regex '(^|/)src/(main|surfpool_tests)\.rs$'

coverage-lcov: ensure-rust-components
  mise exec -- cargo llvm-cov --workspace --all-features --lcov --output-path lcov.info --ignore-filename-regex '(^|/)src/(main|surfpool_tests)\.rs$'

coverage-ci threshold="80": ensure-rust-components
  mise exec -- cargo llvm-cov --workspace --all-features --summary-only --fail-under-lines {{threshold}} --ignore-filename-regex '(^|/)src/(main|surfpool_tests)\.rs$'

site-build: coverage-html
  bash ./scripts/build-pages-site.sh

build:
  mise exec -- cargo build

build-release:
  mise exec -- cargo build --release

run config="etc/config.local.yaml":
  set -a; \
  [ ! -f .env ] || source .env; \
  set +a; \
  mise exec -- cargo run -- --config '{{config}}'

run-release config="etc/config.local.yaml":
  set -a; \
  [ ! -f .env ] || source .env; \
  set +a; \
  mise exec -- cargo run --release -- --config '{{config}}'

docker-build tag="sol-validator-watcher:local":
  docker build --build-arg "RUST_VERSION=$(mise config get tools.rust.version)" -t '{{tag}}' .

stack-config-surfpool:
  RUST_VERSION="$(mise config get tools.rust.version)" docker compose -f compose.yaml -f compose.surfpool.yaml config

stack-config-validator:
  RUST_VERSION="$(mise config get tools.rust.version)" docker compose -f compose.yaml -f compose.validator.yaml config

stack-config-devnet:
  RUST_VERSION="$(mise config get tools.rust.version)" docker compose -f compose.yaml -f compose.devnet.yaml config

stack-config-mainnet:
  RUST_VERSION="$(mise config get tools.rust.version)" docker compose -f compose.yaml -f compose.mainnet.yaml config

stack-config-all:
  RUST_VERSION="$(mise config get tools.rust.version)" docker compose -f compose.yaml -f compose.surfpool.yaml -f compose.validator.yaml -f compose.devnet.yaml -f compose.mainnet.yaml config

stack-up-surfpool:
  RUST_VERSION="$(mise config get tools.rust.version)" docker compose -f compose.yaml -f compose.surfpool.yaml up -d --build

stack-up-validator:
  RUST_VERSION="$(mise config get tools.rust.version)" docker compose -f compose.yaml -f compose.validator.yaml up -d --build

stack-up-devnet:
  RUST_VERSION="$(mise config get tools.rust.version)" docker compose -f compose.yaml -f compose.devnet.yaml up -d --build

stack-up-mainnet:
  RUST_VERSION="$(mise config get tools.rust.version)" docker compose -f compose.yaml -f compose.mainnet.yaml up -d --build

stack-up-all:
  RUST_VERSION="$(mise config get tools.rust.version)" docker compose -f compose.yaml -f compose.surfpool.yaml -f compose.validator.yaml -f compose.devnet.yaml -f compose.mainnet.yaml up -d --build

stack-reload-prometheus:
  curl -fsS -X POST http://localhost:9090/-/reload

stack-logs-surfpool:
  RUST_VERSION="$(mise config get tools.rust.version)" docker compose -f compose.yaml -f compose.surfpool.yaml logs -f

stack-logs-validator:
  RUST_VERSION="$(mise config get tools.rust.version)" docker compose -f compose.yaml -f compose.validator.yaml logs -f

stack-logs-devnet:
  RUST_VERSION="$(mise config get tools.rust.version)" docker compose -f compose.yaml -f compose.devnet.yaml logs -f

stack-logs-mainnet:
  RUST_VERSION="$(mise config get tools.rust.version)" docker compose -f compose.yaml -f compose.mainnet.yaml logs -f

stack-logs-all:
  RUST_VERSION="$(mise config get tools.rust.version)" docker compose -f compose.yaml -f compose.surfpool.yaml -f compose.validator.yaml -f compose.devnet.yaml -f compose.mainnet.yaml logs -f

stack-ps-surfpool:
  RUST_VERSION="$(mise config get tools.rust.version)" docker compose -f compose.yaml -f compose.surfpool.yaml ps

stack-ps-validator:
  RUST_VERSION="$(mise config get tools.rust.version)" docker compose -f compose.yaml -f compose.validator.yaml ps

stack-ps-devnet:
  RUST_VERSION="$(mise config get tools.rust.version)" docker compose -f compose.yaml -f compose.devnet.yaml ps

stack-ps-mainnet:
  RUST_VERSION="$(mise config get tools.rust.version)" docker compose -f compose.yaml -f compose.mainnet.yaml ps

stack-ps-all:
  RUST_VERSION="$(mise config get tools.rust.version)" docker compose -f compose.yaml -f compose.surfpool.yaml -f compose.validator.yaml -f compose.devnet.yaml -f compose.mainnet.yaml ps

stack-down:
  RUST_VERSION="$(mise config get tools.rust.version)" docker compose -f compose.yaml -f compose.surfpool.yaml -f compose.validator.yaml -f compose.devnet.yaml -f compose.mainnet.yaml down --remove-orphans

clean:
  mise exec -- cargo clean

ci:
  just verify

ci-full:
  just fmt
  just clippy
  just test
  just coverage-ci
  just test-surfpool
