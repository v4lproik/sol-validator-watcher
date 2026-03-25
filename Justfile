set shell := ["bash", "-euo", "pipefail", "-c"]

default:
  @just --list

doctor:
  command -v mise >/dev/null 2>&1
  command -v cargo >/dev/null 2>&1
  command -v just >/dev/null 2>&1
  mise --version
  cargo --version
  mise exec -- cargo llvm-cov --version
  just --version

doctor-docker:
  command -v docker >/dev/null 2>&1
  docker --version
  docker compose version

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

coverage:
  mise exec -- cargo llvm-cov --workspace --all-features --summary-only --ignore-filename-regex '(^|/)src/(main|surfpool_tests)\.rs$'

coverage-html:
  mise exec -- cargo llvm-cov --workspace --all-features --html --output-dir coverage --ignore-filename-regex '(^|/)src/(main|surfpool_tests)\.rs$'

coverage-lcov:
  mise exec -- cargo llvm-cov --workspace --all-features --lcov --output-path lcov.info --ignore-filename-regex '(^|/)src/(main|surfpool_tests)\.rs$'

coverage-ci threshold="80":
  mise exec -- cargo llvm-cov --workspace --all-features --summary-only --fail-under-lines {{threshold}} --ignore-filename-regex '(^|/)src/(main|surfpool_tests)\.rs$'

build:
  cargo build

build-release:
  cargo build --release

run config="etc/config.local.yaml":
  set -a; \
  [ ! -f .env ] || source .env; \
  set +a; \
  cargo run -- --config '{{config}}'

run-release config="etc/config.local.yaml":
  set -a; \
  [ ! -f .env ] || source .env; \
  set +a; \
  cargo run --release -- --config '{{config}}'

docker-build tag="sol-validator-watcher:local":
  docker build -t '{{tag}}' .

stack-config-surfpool:
  docker compose -f compose.yaml -f compose.surfpool.yaml config

stack-config-validator:
  docker compose -f compose.yaml -f compose.validator.yaml config

stack-config-all:
  docker compose -f compose.yaml -f compose.surfpool.yaml -f compose.validator.yaml config

stack-up-surfpool:
  docker compose -f compose.yaml -f compose.surfpool.yaml up -d --build

stack-up-validator:
  docker compose -f compose.yaml -f compose.validator.yaml up -d --build

stack-up-all:
  docker compose -f compose.yaml -f compose.surfpool.yaml -f compose.validator.yaml up -d --build

stack-logs-surfpool:
  docker compose -f compose.yaml -f compose.surfpool.yaml logs -f

stack-logs-validator:
  docker compose -f compose.yaml -f compose.validator.yaml logs -f

stack-logs-all:
  docker compose -f compose.yaml -f compose.surfpool.yaml -f compose.validator.yaml logs -f

stack-ps-surfpool:
  docker compose -f compose.yaml -f compose.surfpool.yaml ps

stack-ps-validator:
  docker compose -f compose.yaml -f compose.validator.yaml ps

stack-ps-all:
  docker compose -f compose.yaml -f compose.surfpool.yaml -f compose.validator.yaml ps

stack-down:
  docker compose -f compose.yaml -f compose.surfpool.yaml -f compose.validator.yaml down --remove-orphans

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
  just coverage-ci
  just test-surfpool
